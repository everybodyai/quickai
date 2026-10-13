//! Groups H, E and K of the Phase 3 test plan that need no process and no network: reading a
//! streamed reply, adding up a model pull, the user's environment lines, keeping the packaging's
//! loader environment out of a child, and reading what was typed into the model box
//! (docs/phase3-test-plan.md). They are written against the interface stubs in `quickai-core`
//! (`stream`, `kvlines`, `loaderenv`, `registry::input`) and are ignored until the work package
//! that implements each lands; run with `cargo test -p quickai-core -- --ignored p3_`.

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::kvlines;
use quickai_core::loaderenv::{self, EnvChange};
use quickai_core::registry::{InputAction, KnownPrimitives, Registry};
use quickai_core::stream::{
    LineError, NdjsonSplitter, ProgressMap, PullTracker, PullUpdate, SseEvent, SseSplitter,
};
use serde_json::{json, Value};

fn recorded(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quickai-runtime/tests/fixtures/ollama/recorded")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn all_lines(bytes: &[u8], chunk: usize) -> Vec<Result<Value, LineError>> {
    let mut splitter = NdjsonSplitter::new();
    let mut out = Vec::new();
    for piece in bytes.chunks(chunk.max(1)) {
        out.extend(splitter.push(piece));
    }
    out.extend(splitter.finish());
    out
}

// ---- JSON pointers ---------------------------------------------------------------------------------

/// The documents name where a number or a text is in a reply by JSON pointer (`/digest`,
/// `/models/0/name`, `/capabilities`). `serde_json` implements RFC 6901, and these are the corners the
/// engine documents rely on, so a change in the library cannot change what a document means.
#[test]
fn p3_h01_the_json_pointers_of_the_documents_resolve_as_rfc_6901_says() {
    let reply = json!({
        "models": [{"name": "qwen3:4b", "size": 2500000000u64}, {"name": "bonsai-q1"}],
        "a/b": 1, "m~n": 2, "": 3, "list": ["x"], "nothing": null
    });
    assert_eq!(
        reply.pointer(""),
        Some(&reply),
        "the empty pointer is the whole document"
    );
    assert_eq!(reply.pointer("/models/0/name"), Some(&json!("qwen3:4b")));
    assert_eq!(reply.pointer("/models/1/name"), Some(&json!("bonsai-q1")));
    assert_eq!(
        reply.pointer("/a~1b"),
        Some(&json!(1)),
        "~1 is a slash in a key"
    );
    assert_eq!(
        reply.pointer("/m~0n"),
        Some(&json!(2)),
        "~0 is a tilde in a key"
    );
    assert_eq!(reply.pointer("/"), Some(&json!(3)), "the empty key");
    assert_eq!(
        reply.pointer("/nothing"),
        Some(&Value::Null),
        "null is a value, not an absence"
    );
    // what is not there is not there: no panic, no guess
    for missing in [
        "/models/2",
        "/models/-",
        "/models/01",
        "/models/x",
        "/nope",
        "/list/0/y",
        "models",
        "/models/0/name/x",
    ] {
        assert_eq!(reply.pointer(missing), None, "{missing}");
    }
}

// ---- NDJSON ----------------------------------------------------------------------------------------

#[test]
fn p3_h05a_lines_come_out_whole_and_in_order_however_the_bytes_arrive() {
    let bytes = recorded("pull_real.body");
    let whole = all_lines(&bytes, bytes.len());
    assert_eq!(whole.len(), 139, "the recorded pull has 139 lines");
    assert!(whole.iter().all(Result::is_ok));
    for chunk in [1, 2, 3, 7, 64, 1000, 4096] {
        assert_eq!(
            all_lines(&bytes, chunk),
            whole,
            "chunks of {chunk} bytes gave another result"
        );
    }
}

#[test]
fn p3_h05b_crlf_blank_lines_and_a_last_line_without_a_newline() {
    let mut s = NdjsonSplitter::new();
    assert_eq!(
        s.push(b"{\"a\":1}\r\n\r\n{\"a\":2}\n\n"),
        vec![Ok(json!({"a": 1})), Ok(json!({"a": 2}))]
    );
    // a line with no newline yet is not a line yet
    assert!(s.push(b"{\"a\":3}").is_empty());
    assert_eq!(s.finish(), vec![Ok(json!({"a": 3}))]);
    // and nothing is left over for a second finish
    assert!(s.finish().is_empty());
    assert!(NdjsonSplitter::new().finish().is_empty());
}

#[test]
fn p3_h05c_a_line_that_is_not_json_is_an_error_with_its_number_and_the_others_still_come() {
    let out = all_lines(
        b"{\"a\":1}\nthis is not json\n\n{\"a\":2}\n{broken\n{\"a\":3}\n",
        5,
    );
    assert_eq!(out.len(), 5);
    assert_eq!(out[0], Ok(json!({"a": 1})));
    assert!(matches!(&out[1], Err(LineError { line: 2, .. })));
    assert_eq!(out[2], Ok(json!({"a": 2})));
    assert!(
        matches!(&out[3], Err(LineError { line: 5, .. })),
        "blank lines count in the numbering: {:?}",
        out[3]
    );
    assert_eq!(out[4], Ok(json!({"a": 3})));
}

#[test]
fn p3_h05d_a_multibyte_character_cut_by_a_chunk_boundary_is_not_damaged() {
    let line = "{\"t\":\"héllo ✓ 日本語 🦙\"}\n";
    let out = all_lines(line.as_bytes(), 1);
    assert_eq!(out, vec![Ok(json!({"t": "héllo ✓ 日本語 🦙"}))]);
}

#[test]
fn p3_h05e_a_very_long_line_is_read_in_reasonable_time() {
    let big = "x".repeat(2 * 1024 * 1024);
    let line = format!("{{\"t\":\"{big}\"}}\n");
    let started = std::time::Instant::now();
    let out = all_lines(line.as_bytes(), 4096);
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].as_ref().unwrap()["t"].as_str().unwrap().len(),
        big.len()
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

// ---- SSE -------------------------------------------------------------------------------------------

fn sse(bytes: &[u8], chunk: usize) -> Vec<SseEvent> {
    let mut s = SseSplitter::new();
    let mut out = Vec::new();
    for piece in bytes.chunks(chunk.max(1)) {
        out.extend(s.push(piece));
    }
    out.extend(s.finish());
    out
}

#[test]
fn p3_h05f_the_recorded_openai_stream_is_the_same_events_however_it_is_chunked() {
    let bytes = recorded("v1_chat_stream.body");
    let whole = sse(&bytes, bytes.len());
    assert_eq!(
        whole.len(),
        14,
        "13 chunks and [DONE]: the file has 28 lines, each event being a `data:` line and a blank one"
    );
    assert_eq!(whole.last().unwrap().data, "[DONE]");
    assert!(whole[..13]
        .iter()
        .all(|e| serde_json::from_str::<Value>(&e.data).is_ok()));
    for chunk in [1, 5, 33, 1000] {
        assert_eq!(sse(&bytes, chunk), whole, "chunks of {chunk} bytes");
    }
}

#[test]
fn p3_h05g_sse_names_comments_multiline_data_crlf_and_an_unfinished_last_event() {
    let out = sse(
        b": a comment\r\nevent: delta\r\ndata: one\r\ndata: two\r\n\r\ndata: three\n\ndata: unfinished",
        3,
    );
    assert_eq!(
        out,
        vec![
            SseEvent {
                event: Some("delta".into()),
                data: "one\ntwo".into()
            },
            SseEvent {
                event: None,
                data: "three".into()
            },
            SseEvent {
                event: None,
                data: "unfinished".into()
            },
        ]
    );
}

// ---- adding up a pull ------------------------------------------------------------------------------

fn tracker() -> PullTracker {
    PullTracker::new(ProgressMap::ollama_pull())
}

fn feed_all(t: &mut PullTracker, events: &[Value]) -> Vec<PullUpdate> {
    events.iter().flat_map(|e| t.feed(e)).collect()
}

fn recorded_events(name: &str) -> Vec<Value> {
    String::from_utf8(recorded(name))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn p3_h06a_the_real_pull_never_goes_backwards_stays_below_one_until_success_and_ends_finished() {
    let events = recorded_events("pull_real.body");
    let mut t = tracker();
    let mut last = 0.0f32;
    let mut finished_at = None;
    for (i, e) in events.iter().enumerate() {
        for update in t.feed(e) {
            match update {
                PullUpdate::Progress(f) => {
                    assert!((0.0..=1.0).contains(&f), "line {i}: {f}");
                    assert!(
                        f >= last,
                        "line {i}: the progress went back from {last} to {f}"
                    );
                    last = f;
                    if finished_at.is_none() && i + 1 < events.len() {
                        assert!(f < 1.0, "line {i} of {}: 100% before success", events.len());
                    }
                }
                PullUpdate::Finished => finished_at = Some(i),
                PullUpdate::Failed(m) => panic!("line {i}: failed: {m}"),
                PullUpdate::Status(_) => {}
            }
        }
    }
    assert_eq!(
        finished_at,
        Some(events.len() - 1),
        "it finished on the `success` line, and not before"
    );
    assert!(last > 0.9, "the progress ended at {last}");
}

#[test]
fn p3_h06b_the_engines_own_words_go_to_the_log_once_per_step_and_once_per_layer() {
    let mut t = tracker();
    let statuses: Vec<String> = feed_all(&mut t, &recorded_events("pull_real.body"))
        .into_iter()
        .filter_map(|u| match u {
            PullUpdate::Status(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        [
            "pulling manifest",
            "pulling 797b70c4edf8",
            "pulling c71d239df917",
            "pulling 85011998c600",
            "pulling 548455b72658",
            "verifying sha256 digest",
            "writing manifest",
            "success",
        ],
        "one line per layer, not one per progress report (the pull has 139)"
    );
}

#[test]
fn p3_h06c_an_error_line_fails_the_pull_with_the_engines_sentence() {
    let mut t = tracker();
    let updates = feed_all(&mut t, &recorded_events("pull_unreachable.body"));
    assert_eq!(
        updates.first(),
        Some(&PullUpdate::Status("pulling manifest".into()))
    );
    match updates.last() {
        Some(PullUpdate::Failed(m)) => assert!(
            m.contains("registry.ollama.ai") && m.contains("connection refused"),
            "{m}"
        ),
        other => panic!("the last update is {other:?}"),
    }
    assert!(!updates.contains(&PullUpdate::Finished));
}

#[test]
fn p3_h06d_layers_pulled_side_by_side_add_up_and_a_missing_completed_counts_as_nothing() {
    let layer = |d: &str, total: u64, completed: Option<u64>| {
        let mut e = json!({"status": format!("pulling {d}"), "digest": format!("sha256:{d}"), "total": total});
        if let Some(c) = completed {
            e["completed"] = json!(c);
        }
        e
    };
    let mut t = tracker();
    assert_eq!(
        t.fraction(),
        None,
        "nothing is known before a layer says how big it is"
    );
    t.feed(&layer("a", 100, None));
    assert_eq!(t.fraction(), Some(0.0));
    t.feed(&layer("b", 300, Some(150)));
    assert_eq!(t.fraction(), Some(150.0 / 400.0));
    t.feed(&layer("a", 100, Some(100)));
    assert_eq!(t.fraction(), Some(250.0 / 400.0));
}

#[test]
fn p3_h06e_a_completed_above_its_total_is_clamped_and_a_total_that_grows_does_not_move_the_bar_back(
) {
    let mut t = tracker();
    t.feed(&json!({"status": "pulling a", "digest": "sha256:a", "total": 100, "completed": 150}));
    let f = t.fraction().unwrap();
    assert!(
        f > 0.9 && f < 1.0,
        "a layer cannot be more than complete, and nothing is done before `success`: {f}"
    );
    let mut t = tracker();
    t.feed(&json!({"status": "pulling a", "digest": "sha256:a", "total": 100, "completed": 90}));
    let before = t.fraction().unwrap();
    t.feed(&json!({"status": "pulling b", "digest": "sha256:b", "total": 900}));
    assert!(
        t.fraction().unwrap() >= before,
        "a layer that turned up late moved the bar back"
    );
}

#[test]
fn p3_h06f_a_model_that_is_already_there_goes_straight_to_the_end() {
    let mut t = tracker();
    let updates = feed_all(&mut t, &recorded_events("pull_again.body"));
    assert_eq!(updates.last(), Some(&PullUpdate::Finished));
    assert_eq!(t.fraction(), Some(1.0));
}

// ---- the user's environment lines ------------------------------------------------------------------

const PROTECTED: &[&str] = &["OLLAMA_HOST", "OLLAMA_MODELS", "OLLAMA_NO_CLOUD"];

#[test]
fn p3_e11a_lines_are_read_blank_lines_and_comments_are_skipped_and_the_value_keeps_its_equals_signs(
) {
    let out = kvlines::parse(
        "# a comment\r\nOLLAMA_DEBUG=1\n\n  OLLAMA_NUM_PARALLEL = 2  \nTOKEN=a=b=c\nEMPTY=\n",
        PROTECTED,
        false,
    );
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    assert_eq!(
        out.entries,
        [
            ("OLLAMA_DEBUG".to_string(), "1".to_string()),
            ("OLLAMA_NUM_PARALLEL".to_string(), "2".to_string()),
            ("TOKEN".to_string(), "a=b=c".to_string()),
            ("EMPTY".to_string(), String::new()),
        ]
    );
}

#[test]
fn p3_e11b_a_bad_line_is_a_problem_with_its_number_and_the_good_ones_still_count() {
    let out = kvlines::parse(
        "GOOD=1\nno equals sign\n1BAD=x\nA B=x\n=novalue\nALSO_GOOD=2",
        PROTECTED,
        false,
    );
    assert_eq!(
        out.entries,
        [
            ("GOOD".to_string(), "1".to_string()),
            ("ALSO_GOOD".to_string(), "2".to_string())
        ]
    );
    let lines: Vec<usize> = out.problems.iter().map(|p| p.line).collect();
    assert_eq!(lines, [2, 3, 4, 5]);
    assert!(out.problems.iter().all(|p| !p.message.is_empty()));
}

#[test]
fn p3_e11c_the_protected_names_are_refused_by_name_and_on_windows_in_any_case() {
    let text = "OLLAMA_HOST=0.0.0.0:80\nOLLAMA_MODELS=/x\nollama_no_cloud=0\nOLLAMA_DEBUG=1";
    let unix = kvlines::parse(text, PROTECTED, false);
    assert_eq!(
        unix.problems.iter().map(|p| p.line).collect::<Vec<_>>(),
        [1, 2],
        "names are case-sensitive on Unix"
    );
    assert!(unix.problems[0].message.contains("OLLAMA_HOST"));
    assert!(
        unix.entries.iter().any(|(k, _)| k == "ollama_no_cloud"),
        "a different name on Unix"
    );
    let windows = kvlines::parse(text, PROTECTED, true);
    assert_eq!(
        windows.problems.iter().map(|p| p.line).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        windows.entries,
        [("OLLAMA_DEBUG".to_string(), "1".to_string())]
    );
}

#[test]
fn p3_e11d_a_later_line_for_the_same_name_replaces_the_earlier_in_place() {
    let out = kvlines::parse("A=1\nB=2\nA=3", PROTECTED, false);
    assert_eq!(
        out.entries,
        [
            ("A".to_string(), "3".to_string()),
            ("B".to_string(), "2".to_string())
        ]
    );
}

// ---- the packaging's loader environment ------------------------------------------------------------

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: std::collections::BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name: &str| map.get(name).cloned()
}

#[test]
fn p3_k07a_an_appimages_directories_are_dropped_from_the_loader_path() {
    let changes = loaderenv::scrub(&env(&[
        ("APPDIR", "/tmp/.mount_quickaXYZ"),
        (
            "LD_LIBRARY_PATH",
            "/tmp/.mount_quickaXYZ/usr/lib:/usr/lib:/tmp/.mount_quickaXYZ:/opt/cuda/lib64",
        ),
    ]));
    assert_eq!(
        changes,
        [EnvChange {
            name: "LD_LIBRARY_PATH".into(),
            value: Some("/usr/lib:/opt/cuda/lib64".into())
        }]
    );
}

#[test]
fn p3_k07b_only_a_directory_inside_the_root_counts_not_one_that_merely_starts_like_it() {
    let changes = loaderenv::scrub(&env(&[
        ("APPDIR", "/tmp/.mount_x"),
        ("LD_LIBRARY_PATH", "/tmp/.mount_xyz/lib:/tmp/.mount_x/lib"),
    ]));
    assert_eq!(
        changes,
        [EnvChange {
            name: "LD_LIBRARY_PATH".into(),
            value: Some("/tmp/.mount_xyz/lib".into())
        }]
    );
}

#[test]
fn p3_k07c_a_snaps_directories_go_too_and_a_variable_with_nothing_left_is_removed() {
    let changes = loaderenv::scrub(&env(&[
        ("SNAP", "/snap/quickai/12"),
        (
            "LD_LIBRARY_PATH",
            "/snap/quickai/12/lib:/snap/quickai/12/usr/lib",
        ),
        ("LD_PRELOAD", "/snap/quickai/12/lib/libx.so"),
    ]));
    assert_eq!(
        changes,
        [
            EnvChange {
                name: "LD_LIBRARY_PATH".into(),
                value: None
            },
            EnvChange {
                name: "LD_PRELOAD".into(),
                value: None
            },
        ]
    );
}

#[test]
fn p3_k07d_with_no_appimage_and_no_snap_nothing_changes_which_is_how_llamacpp_stays_as_it_was() {
    assert!(loaderenv::scrub(&env(&[("LD_LIBRARY_PATH", "/usr/lib:/opt/x")])).is_empty());
    assert!(loaderenv::scrub(&env(&[])).is_empty());
    // the root is set but nothing in the variable lies under it
    assert!(loaderenv::scrub(&env(&[
        ("APPDIR", "/tmp/.mount_x"),
        ("LD_LIBRARY_PATH", "/usr/lib")
    ]))
    .is_empty());
    // an empty root must not match everything
    assert!(loaderenv::scrub(&env(&[("APPDIR", ""), ("LD_LIBRARY_PATH", "/usr/lib")])).is_empty());
}

// ---- the model box ---------------------------------------------------------------------------------

/// The rules of the Ollama document: first match wins.
const MODELS_DOC: &str = r#"schema = 1
kind = "engine"
id = "ollama_models"
requires = { primitives = ["spawn@1"] }

[engine]
name = "Ollama"

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "x"

[models]
needs_server = true
input_rules = [
  { when = 'matches(input, "^hf\.co/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(:[A-Za-z0-9._-]+)?$")', do = "pull", ref = "${input}" },
  { when = 'matches(input, "^https://huggingface\.co/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+/?$")', do = "pull", ref = "hf.co/${path_after_host(input)}" },
  { when = 'starts_with(input, "https://huggingface.co/")', do = "reject", message = "Paste the model's page (huggingface.co/user/repo), not a link to a file." },
  { when = "is_url(input)", do = "reject", message = "Ollama pulls by name. Use an hf.co/... reference, or switch the engine to llama.cpp." },
  { when = "is_file(input)", do = "reject", message = "Importing a local file is not supported for Ollama yet." },
  { when = 'matches(input, "^[a-z0-9._/-]+(:[A-Za-z0-9._-]+)?$")', do = "pull", ref = "${input}" },
  { do = "reject", message = "That does not look like a model name. Try qwen3:4b or hf.co/user/repo:Q4_K_M." },
]
"#;

fn models_registry() -> Registry {
    let source = MemorySource::new(&[("embedded:engines/ollama_models.toml", MODELS_DOC)]);
    Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &KnownPrimitives::new(&["spawn@1"]),
    )
}

fn classify(input: &str) -> InputAction {
    let registry = models_registry();
    let engine = registry
        .engine("ollama_models")
        .unwrap_or_else(|| panic!("{:#?}", registry.diagnostics));
    engine.classify_input(input, &|path| path.starts_with("/home/u/"))
}

fn pull(r: &str) -> InputAction {
    InputAction::Pull {
        reference: r.to_string(),
    }
}

#[test]
fn p3_m01_what_is_typed_into_the_model_box_is_pulled_or_refused_with_a_sentence() {
    assert_eq!(classify("qwen3:4b"), pull("qwen3:4b"));
    assert_eq!(classify("qwen3"), pull("qwen3"));
    assert_eq!(classify("library/qwen3:4b"), pull("library/qwen3:4b"));
    assert_eq!(
        classify("  qwen3:4b \n"),
        pull("qwen3:4b"),
        "spaces around it are not part of the name"
    );
    assert_eq!(
        classify("hf.co/Qwen/Qwen3-4B-GGUF:Q4_K_M"),
        pull("hf.co/Qwen/Qwen3-4B-GGUF:Q4_K_M")
    );
    assert_eq!(
        classify("hf.co/Qwen/Qwen3-4B-GGUF"),
        pull("hf.co/Qwen/Qwen3-4B-GGUF")
    );
    assert_eq!(
        classify("https://huggingface.co/Qwen/Qwen3-4B-GGUF"),
        pull("hf.co/Qwen/Qwen3-4B-GGUF")
    );
    assert_eq!(
        classify("https://huggingface.co/Qwen/Qwen3-4B-GGUF/"),
        pull("hf.co/Qwen/Qwen3-4B-GGUF")
    );
    assert_eq!(classify(""), InputAction::Nothing);
    assert_eq!(classify("   "), InputAction::Nothing);
}

#[test]
fn p3_m01b_a_link_to_a_file_a_local_file_and_nonsense_are_refused_in_the_documents_words() {
    let message = |input: &str| match classify(input) {
        InputAction::Reject { message } => message,
        other => panic!("{input:?} gave {other:?}"),
    };
    assert!(
        message("https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/x.gguf")
            .contains("model's page")
    );
    assert!(message("https://example.com/model.gguf").contains("pulls by name"));
    assert!(message("/home/u/models/m.gguf").contains("local file"));
    assert!(message("two words").contains("does not look like a model name"));
    assert!(
        message("Qwen3:4B").contains("does not look like a model name"),
        "library names are lower case"
    );
    assert!(message("qwen3:4b; rm -rf /").contains("does not look like a model name"));
    // the first rule that matches decides: a file path is not mistaken for a name
    assert!(message("/home/u/qwen3.gguf").contains("local file"));
}
