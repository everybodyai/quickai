//! The models of an engine that keeps its own (work package 3.13, docs/phase3-test-plan.md group M):
//! the `[models]` section is read and checked when the document loads, its calls come out with the
//! templates filled in, and names that mean one model are one model.

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::decode::Framing;
use quickai_core::models::{
    canonical_name, find_listed, list_call, needs_server, pull_call, remove_call, same_model,
};
use quickai_core::plan::{plan, HostFacts, ModelInfo, Selection};
use quickai_core::registry::{KnownPrimitives, Registry};
use quickai_core::stream::ProgressMap;
use serde_json::json;

const BASE: &str = r#"schema = 1
kind = "engine"
id = "acme"
requires = { primitives = ["spawn@1", "probe.http@1", "locate.setting@1"] }

[engine]
name = "Acme"

[provides]
apis = ["acme.talk"]
default_port = 9000

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["serve"]

[launch.ready]
probe = { use = "http", path = "/health", expect = 200 }

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"

MODELS
"#;

/// The section as the Ollama document writes it.
const MODELS: &str = r#"[models]
needs_server = true
list = { call = { method = "GET", path = "/api/tags" }, items = "/models", map = { id = "/name", display = "/name", size_bytes = "/size", digest = "/digest", capabilities = "/capabilities" }, cache = "${data}/acme/catalog-cache.json" }
pull = { call = { method = "POST", path = "/api/pull", body = { model = "${ref}" }, stream = "ndjson" }, progress = { key = "/digest", completed = "/completed", total = "/total", status = "/status" } }
remove = { call = { method = "DELETE", path = "/api/delete", body = { model = "${ref}" } } }
capabilities = { call = { method = "POST", path = "/api/show", body = { model = "${ref}" } }, pointer = "/capabilities" }
input_rules = [
  { when = 'matches(input, "^[a-z0-9._/-]+(:[A-Za-z0-9._-]+)?$")', do = "pull", ref = "${input}" },
  { do = "reject", message = "That does not look like a model name." },
]
"#;

fn registry(models: &str) -> Registry {
    let text = BASE.replace("MODELS", models);
    let source = MemorySource::new(&[("embedded:engines/acme.toml", &text)]);
    Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1"]),
    )
}

fn facts() -> HostFacts {
    HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        data_dir: "/home/me/.quickai".into(),
        ..HostFacts::default()
    }
}

#[test]
fn p3_m04_qwen3_and_qwen3_latest_are_the_same_model_whichever_way_they_are_written() {
    for (a, b) in [
        ("qwen3", "qwen3:latest"),
        ("library/qwen3", "qwen3"),
        ("library/qwen3:latest", "qwen3:latest"),
        (
            "hf.co/Qwen/Qwen3-4B-GGUF",
            "hf.co/Qwen/Qwen3-4B-GGUF:latest",
        ),
        ("localhost:5000/team/m", "localhost:5000/team/m:latest"),
    ] {
        assert!(same_model(a, b), "{a} and {b}");
    }
    for (a, b) in [
        ("qwen3", "qwen3:4b"),
        ("hf.co/a/b:Q4_K_M", "hf.co/a/b:Q8_0"),
        ("localhost:5000/m", "localhost:5001/m"),
    ] {
        assert!(!same_model(a, b), "{a} and {b}");
    }
    // the picker, the remembered model and the warm-up all go through the listing: what the engine
    // calls it is what is asked for
    let listed = ["bonsai-q1:latest", "qwen3:4b"];
    let remembered = "bonsai-q1";
    let at = find_listed(listed, remembered).expect("it is there, under its full name");
    assert_eq!(listed[at], "bonsai-q1:latest");
    assert_eq!(canonical_name(remembered), "bonsai-q1:latest");
    assert_eq!(
        find_listed(listed, "qwen3"),
        None,
        "qwen3 is qwen3:latest, which is not listed"
    );
}

#[test]
fn p3_m06_the_models_section_loads_and_its_calls_are_filled_in() {
    let registry = registry(MODELS);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let engine = registry.engine("acme").expect("loaded");
    assert!(needs_server(engine));

    let list = list_call(engine, &facts())
        .expect("a list call")
        .expect("renders");
    assert_eq!(list.call.method, "GET");
    assert_eq!(list.call.path, "/api/tags");
    assert_eq!(list.call.body, None);
    assert_eq!(list.call.framing, Framing::Json);
    assert_eq!(
        (
            list.items.as_str(),
            list.id.as_str(),
            list.display.as_deref()
        ),
        ("/models", "/name", Some("/name"))
    );
    assert_eq!(list.size_bytes.as_deref(), Some("/size"));
    assert_eq!(list.digest.as_deref(), Some("/digest"));
    assert_eq!(list.capabilities.as_deref(), Some("/capabilities"));
    assert_eq!(
        list.cache.as_deref(),
        Some("/home/me/.quickai/acme/catalog-cache.json"),
        "the cache is where the document says, with the data directory filled in"
    );

    let pull = pull_call(engine, "hf.co/Qwen/Qwen3-4B-GGUF:Q4_K_M")
        .expect("a pull call")
        .expect("renders");
    assert_eq!(
        (pull.call.method.as_str(), pull.call.path.as_str()),
        ("POST", "/api/pull")
    );
    assert_eq!(
        pull.call.body,
        Some(json!({"model": "hf.co/Qwen/Qwen3-4B-GGUF:Q4_K_M"})),
        "the name is the one asked for, as it is"
    );
    assert_eq!(pull.call.framing, Framing::Ndjson);
    assert_eq!(
        pull.progress,
        ProgressMap::ollama_pull(),
        "the document's pointers, the usual error and end"
    );

    let remove = remove_call(engine, "bonsai-q1:latest")
        .expect("a remove call")
        .expect("renders");
    assert_eq!(
        (remove.method.as_str(), remove.path.as_str()),
        ("DELETE", "/api/delete")
    );
    assert_eq!(remove.body, Some(json!({"model": "bonsai-q1:latest"})));
    assert_eq!(remove.framing, Framing::Json);

    // an engine with no `[models]` has none of it
    let none = self::registry("");
    let engine = none.engine("acme").expect("loaded");
    assert!(!needs_server(engine));
    assert!(list_call(engine, &facts()).is_none());
    assert!(pull_call(engine, "x").is_none());
    assert!(remove_call(engine, "x").is_none());
}

/// `MODELS` with one thing changed, and the errors it gives.
fn errors_of(old: &str, new: &str) -> Vec<(Option<u32>, String)> {
    assert!(MODELS.contains(old), "the section has no `{old}`");
    let registry = registry(&MODELS.replace(old, new));
    registry
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.line, d.to_string()))
        .collect()
}

#[test]
fn p3_m06b_a_models_section_that_says_it_wrongly_is_reported_where_it_is_wrong() {
    let cases: [(&str, &str, &str); 9] = [
        (
            r#"items = "/models""#,
            r#"items = "models""#,
            "`models` is not a JSON pointer",
        ),
        (
            r#"id = "/name", display"#,
            r#"id = "name", display"#,
            "`name` is not a JSON pointer",
        ),
        (
            r#"stream = "ndjson""#,
            r#"stream = "chunks""#,
            "`chunks` is not a way to read a reply",
        ),
        (
            r#"path = "/api/delete""#,
            r#"path = "api/delete""#,
            "the path `api/delete` must start with `/`",
        ),
        (
            r#"method = "GET""#,
            r#"method = "FETCH""#,
            "`FETCH` is not a method a call uses",
        ),
        (
            r#"do = "pull", ref"#,
            r#"do = "download", ref"#,
            "`download` is not something a rule does",
        ),
        (
            r#"do = "pull", ref = "${input}" }"#,
            r#"do = "pull" }"#,
            "a `pull` rule says what to pull",
        ),
        (
            r#"message = "That does not look like a model name." }"#,
            r#"message = "  " }"#,
            "a `reject` rule says why",
        ),
        (
            r#"ref = "${input}""#,
            r#"ref = "${imput}""#,
            "unknown name `imput`",
        ),
    ];
    for (old, new, wanted) in cases {
        let found = errors_of(old, new);
        assert!(
            found.iter().any(|(_, text)| text.contains(wanted)),
            "`{new}` should be reported with `{wanted}`: {found:#?}"
        );
        assert!(
            found.iter().all(|(line, _)| line.is_some()),
            "every error has a line: {found:#?}"
        );
    }
    // the progress pointers are checked as well
    let found = errors_of(r#"status = "/status""#, r#"status = "status""#);
    assert!(
        found
            .iter()
            .any(|(_, t)| t.contains("`status` is not a JSON pointer")),
        "{found:#?}"
    );

    // a rule that comes after one that takes everything is never reached: a warning, not an error
    let text = MODELS.replace(
        "input_rules = [",
        "input_rules = [\n  { do = \"reject\", message = \"Nothing.\" },",
    );
    let registry = registry(&text);
    assert!(
        registry.engine("acme").is_some(),
        "{:#?}",
        registry.diagnostics
    );
    assert!(
        registry
            .diagnostics
            .iter()
            .any(|d| !d.is_error() && d.message.contains("this rule is never reached")),
        "{:#?}",
        registry.diagnostics
    );

    // a warm-up is read whole
    let doc = BASE.replace(
        "MODELS",
        "[[launch.warm]]\ncall = { path = \"/api/generate\", stream = \"ndjson\" }\n",
    );
    let source = MemorySource::new(&[("embedded:engines/acme.toml", &doc)]);
    let registry = Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1"]),
    );
    assert!(
        registry
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.message.contains("a warm-up or an unload is read whole")),
        "{:#?}",
        registry.diagnostics
    );
}

#[test]
fn p3_m04b_the_warm_up_names_the_model_exactly_as_the_engine_lists_it() {
    let warm = "\n[[launch.warm]]\nwhen = \"model\"\ncall = { path = \"/api/generate\", body = { model = \"${model.id}\", keep_alive = -1 } }\n";
    let registry = registry(&format!("{MODELS}{warm}"));
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let facts = HostFacts {
        binary: Some("/opt/acme/acme".into()),
        ..facts()
    };
    // the person remembered `bonsai-q1`; the engine lists `bonsai-q1:latest`, and that is what the
    // picker holds and the plan is made with
    let listed = ["bonsai-q1:latest", "qwen3:4b"];
    let at = find_listed(listed, "bonsai-q1").expect("found by what it means");
    let model = ModelInfo {
        id: listed[at].to_string(),
        display: listed[at].to_string(),
        path: String::new(),
        size_bytes: 248_302_385,
    };
    let planned = plan(&registry, &Selection::new("acme").with_model(model), &facts)
        .unwrap_or_else(|e| panic!("{e:#?}"));
    assert_eq!(planned.units[0].warm.len(), 1);
    assert_eq!(
        planned.units[0].warm[0].body,
        json!({"model": "bonsai-q1:latest", "keep_alive": -1})
    );
    // with no model selected there is nothing to warm (management mode)
    let none = plan(&registry, &Selection::new("acme"), &facts).unwrap();
    assert!(none.units[0].warm.is_empty());
}

/// The `[models]` section of the Ollama example, up to the next section of its own.
fn example_models_section() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/examples/engines/ollama.toml");
    let text = std::fs::read_to_string(&path).expect("the Ollama example");
    let from = text
        .find("\n[models]\n")
        .expect("the example has a [models] section")
        + 1;
    let rest = &text[from..];
    let end = rest[1..].find("\n[").map(|i| i + 1).unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn p3_m06d_the_ollama_examples_models_section_is_read_with_nothing_ignored() {
    let section = example_models_section();
    assert!(section.contains("input_rules"), "{section}");
    let registry = registry(&section);
    assert!(
        registry.diagnostics.is_empty(),
        "the typed schema must cover what the example says: {:#?}",
        registry.diagnostics
    );
    let engine = registry.engine("acme").expect("loaded");
    // and the rules do what the model box test of the plan says (M01), on the document's own words
    let classify = |input: &str| engine.classify_input(input, &|p| p.starts_with("/home/u/"));
    assert_eq!(
        classify("qwen3:4b"),
        quickai_core::registry::InputAction::Pull {
            reference: "qwen3:4b".into()
        }
    );
    assert_eq!(
        classify("https://huggingface.co/Qwen/Qwen3-4B-GGUF/"),
        quickai_core::registry::InputAction::Pull {
            reference: "hf.co/Qwen/Qwen3-4B-GGUF".into()
        }
    );
    assert!(matches!(
        classify("/home/u/m.gguf"),
        quickai_core::registry::InputAction::Reject { .. }
    ));
}
