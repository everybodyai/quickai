//! Group H of the test plan: `http_call@1`, one request to the engine and its reply read whole or
//! as a stream (work package 3.3). The engine is the fake of `testkit`, which answers the way the
//! recordings of a real Ollama say it does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use quickai_core::plan::Endpoint;
use serde_json::{json, Value};

use crate::jobs::JobEvent;
use crate::prims::engine_models::{list_models, read_cache, spawn_pull, ListSpec, PullSpec};
use crate::prims::http_call::{call, stream, Call, CallError, Framing, Reply};
use crate::testkit::fake_ollama::FakeOllama;
use crate::testkit::mini_http::MiniServer;

fn endpoint(port: u16) -> Endpoint {
    Endpoint {
        host: "127.0.0.1".into(),
        port: Some(port),
    }
}

fn json_call(method: &str, path: &str, body: Option<Value>) -> Call {
    Call {
        method: method.into(),
        path: path.into(),
        body,
        framing: Framing::Json,
        timeout: Duration::from_secs(5),
    }
}

fn stream_call(method: &str, path: &str, body: Value, framing: Framing) -> Call {
    Call {
        method: method.into(),
        path: path.into(),
        body: Some(body),
        framing,
        timeout: Duration::from_secs(5),
    }
}

/// Everything a stream delivered, and how it ended.
fn collect(fake: &FakeOllama, c: &Call) -> (Vec<Value>, Result<(), CallError>) {
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let outcome = stream(&endpoint(fake.port()), c, &cancel, &mut |e| events.push(e));
    (events, outcome)
}

#[test]
fn p3_h02_a_call_sends_what_the_document_says_and_says_who_is_asking() {
    let fake = FakeOllama::start();
    fake.add_model("qwen3:4b", 1);
    let reply = call(
        &endpoint(fake.port()),
        &json_call(
            "POST",
            "/api/generate",
            Some(json!({"model": "qwen3:4b", "keep_alive": -1})),
        ),
    )
    .expect("the call succeeds");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body["done_reason"], "load");
    let seen = &fake.requests()[0];
    assert_eq!(
        (seen.method.as_str(), seen.path.as_str()),
        ("POST", "/api/generate")
    );
    assert_eq!(
        seen.body,
        Some(json!({"model": "qwen3:4b", "keep_alive": -1})),
        "the number stays a number"
    );
    assert_eq!(seen.header("content-type"), Some("application/json"));
    assert_eq!(seen.header("user-agent"), Some("quickai"));
}

#[test]
fn p3_h03_an_error_status_carries_the_engines_own_sentence_when_it_gave_one() {
    let fake = FakeOllama::start();
    let e = call(
        &endpoint(fake.port()),
        &json_call("POST", "/api/show", Some(json!({"model": "ghost"}))),
    )
    .expect_err("a 404 is an error");
    assert_eq!(
        e,
        CallError::Http {
            status: 404,
            message: "model 'ghost:latest' not found".into()
        }
    );
    // no sentence of the engine's: the status says it
    let e = call(
        &endpoint(fake.port()),
        &json_call("GET", "/no/such/route", None),
    )
    .unwrap_err();
    assert!(
        matches!(e, CallError::Http { status: 404, ref message } if message.contains("404")),
        "{e:?}"
    );
    fake.behave(|b| {
        b.fail_next.insert("/api/version".into(), (500, 1));
    });
    let e = call(
        &endpoint(fake.port()),
        &json_call("GET", "/api/version", None),
    )
    .unwrap_err();
    assert!(matches!(e, CallError::Http { status: 500, .. }), "{e:?}");
}

#[test]
fn p3_h04_an_empty_reply_is_null_a_delete_may_carry_a_body_and_text_is_not_json() {
    let fake = FakeOllama::start();
    fake.add_model("old:1b", 1);
    let reply = call(
        &endpoint(fake.port()),
        &json_call("DELETE", "/api/delete", Some(json!({"model": "old:1b"}))),
    )
    .unwrap();
    assert_eq!(
        reply,
        Reply {
            status: 200,
            body: Value::Null
        }
    );
    assert!(fake.model_names().is_empty());
    assert_eq!(fake.requests()[0].body, Some(json!({"model": "old:1b"})));
    // `GET /` answers plain text: asked for as JSON that is a failure, not a crash
    let e = call(&endpoint(fake.port()), &json_call("GET", "/", None)).unwrap_err();
    assert!(matches!(e, CallError::Failed(_)), "{e:?}");
}

#[test]
fn p3_h07_each_event_of_a_stream_is_handed_over_as_it_arrives_not_all_at_the_end() {
    let fake = FakeOllama::start();
    fake.add_model("m", 1);
    fake.behave(|b| b.line_pause = Duration::from_millis(120));
    let arrivals = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now();
    let cancel = AtomicBool::new(false);
    let seen = arrivals.clone();
    stream(
        &endpoint(fake.port()),
        &stream_call(
            "POST",
            "/api/chat",
            json!({"model": "m", "messages": []}),
            Framing::Ndjson,
        ),
        &cancel,
        &mut |e| seen.lock().unwrap().push((started.elapsed(), e)),
    )
    .expect("the stream ends cleanly");
    let arrivals = arrivals.lock().unwrap();
    assert!(arrivals.len() >= 6);
    assert_eq!(arrivals.last().unwrap().1["done"], true);
    let (first, last) = (arrivals[0].0, arrivals.last().unwrap().0);
    assert!(
        first < last / 3,
        "the first event came at {first:?} of {last:?}: the reply was buffered"
    );
}

#[test]
fn p3_h08_an_error_line_in_a_stream_that_started_with_200_fails_the_call_after_the_events_before_it(
) {
    let fake = FakeOllama::start();
    fake.behave(|b| b.pull.fail = Some("pull model manifest: file does not exist".into()));
    let (events, outcome) = collect(
        &fake,
        &stream_call(
            "POST",
            "/api/pull",
            json!({"model": "ghost"}),
            Framing::Ndjson,
        ),
    );
    assert_eq!(
        events,
        [json!({"status": "pulling manifest"})],
        "what came before the error was delivered"
    );
    assert_eq!(
        outcome,
        Err(CallError::Failed(
            "pull model manifest: file does not exist".into()
        ))
    );
}

#[test]
fn p3_h09_a_stream_that_is_cut_off_is_an_error_not_a_quiet_end() {
    let fake = FakeOllama::start();
    fake.add_model("m", 1);
    fake.behave(|b| b.cut_chat_after_lines = Some(3));
    let (events, outcome) = collect(
        &fake,
        &stream_call(
            "POST",
            "/api/chat",
            json!({"model": "m", "messages": []}),
            Framing::Ndjson,
        ),
    );
    assert_eq!(events.len(), 3);
    assert!(
        matches!(
            outcome,
            Err(CallError::Failed(_)) | Err(CallError::Unreachable(_))
        ),
        "{outcome:?}"
    );
}

#[test]
fn p3_h10_cancelling_ends_the_call_at_once_and_the_engine_stops_writing() {
    let fake = FakeOllama::start();
    fake.behave(|b| {
        b.line_pause = Duration::from_millis(40);
        b.pull.steps = 40;
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let port = fake.port();
    let worker = std::thread::spawn(move || {
        let mut n = 0;
        let outcome = stream(
            &endpoint(port),
            &stream_call(
                "POST",
                "/api/pull",
                json!({"model": "big"}),
                Framing::Ndjson,
            ),
            &flag,
            &mut |_| n += 1,
        );
        (n, outcome)
    });
    crate::testkit::wait_until(Duration::from_secs(5), "the first lines", || {
        fake.lines_sent() >= 3
    });
    let cancelled = Instant::now();
    cancel.store(true, Ordering::SeqCst);
    let (n, outcome) = worker.join().unwrap();
    assert!(
        cancelled.elapsed() < Duration::from_millis(600),
        "took {:?} to give up",
        cancelled.elapsed()
    );
    assert_eq!(outcome, Err(CallError::Cancelled));
    assert!(n >= 3);
    // the engine notices that its client has gone and stops
    let sent = fake.lines_sent();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        fake.lines_sent() <= sent + 2,
        "the engine went on writing to a client that had gone"
    );
}

#[test]
fn p3_h11_a_call_the_engine_never_answers_times_out() {
    let fake = FakeOllama::start();
    fake.add_model("slow", 1);
    fake.behave(|b| b.load_delay = Duration::from_secs(5));
    let mut c = json_call(
        "POST",
        "/api/generate",
        Some(json!({"model": "slow", "keep_alive": -1})),
    );
    c.timeout = Duration::from_millis(300);
    let started = Instant::now();
    assert_eq!(call(&endpoint(fake.port()), &c), Err(CallError::TimedOut));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    // and a stream that goes quiet for longer than the timeout is the same
    fake.behave(|b| {
        b.line_pause = Duration::from_secs(3);
        b.load_delay = Duration::ZERO;
    });
    let mut c = stream_call(
        "POST",
        "/api/chat",
        json!({"model": "slow", "messages": []}),
        Framing::Ndjson,
    );
    c.timeout = Duration::from_millis(400);
    let (_, outcome) = collect(&fake, &c);
    assert_eq!(outcome, Err(CallError::TimedOut));
}

#[test]
fn p3_h12_nothing_can_be_aimed_anywhere_but_at_the_engine_on_this_machine() {
    let fake = FakeOllama::start();
    for path in [
        "http://evil.example/x",
        "//evil.example/x",
        "api/version",
        "/api/../etc/passwd",
        "/api//version",
        "/a b",
        "/a\r\nHost: evil",
        "",
    ] {
        let e = call(&endpoint(fake.port()), &json_call("GET", path, None)).unwrap_err();
        assert!(matches!(e, CallError::Refused(_)), "{path:?} gave {e:?}");
    }
    let remote = Endpoint {
        host: "203.0.113.7".into(),
        port: Some(11434),
    };
    assert!(matches!(
        call(&remote, &json_call("GET", "/api/version", None)),
        Err(CallError::Refused(_))
    ));
    assert!(fake.requests().is_empty(), "a refused call was sent anyway");
}

#[test]
fn p3_h13_a_redirect_is_not_followed_wherever_it_points() {
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = hits.clone();
    let elsewhere = MiniServer::start(None, move |_, r| {
        counted.fetch_add(1, Ordering::SeqCst);
        r.send(b"{\"secret\":true}");
    })
    .unwrap();
    let target = format!("http://127.0.0.1:{}/secret", elsewhere.port());
    let redirecting = MiniServer::start(None, move |_, r| {
        r.status(302).header("Location", &target);
        r.send(b"");
    })
    .unwrap();
    let outcome = call(
        &endpoint(redirecting.port()),
        &json_call("GET", "/api/version", None),
    );
    assert!(
        matches!(outcome, Err(CallError::Http { status: 302, .. })),
        "{outcome:?}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the redirect was followed to another server"
    );
}

#[test]
fn p3_h16_server_sent_events_arrive_as_json_and_the_done_marker_ends_the_stream() {
    let fake = FakeOllama::start();
    fake.add_model("m", 1);
    let (events, outcome) = collect(
        &fake,
        &stream_call(
            "POST",
            "/v1/chat/completions",
            json!({"model": "m", "stream": true, "messages": []}),
            Framing::Sse,
        ),
    );
    assert_eq!(outcome, Ok(()));
    assert!(events.len() > 5);
    assert!(
        events
            .iter()
            .all(|e| e["object"] == "chat.completion.chunk"),
        "[DONE] is not an event: {events:?}"
    );
    assert_eq!(
        events.last().unwrap()["choices"][0]["finish_reason"],
        "stop"
    );
}

// ---- the model list and the pull (H15, H17) ---------------------------------------------------------

#[test]
fn p3_h15_the_engines_models_become_rows_in_the_order_given_and_the_list_is_kept_for_when_it_is_down(
) {
    let dir = crate::testkit::scratch("h15");
    let cache = dir.join("catalog-cache.json");
    let fake = FakeOllama::start();
    // an empty list is a list
    let spec = ListSpec::ollama(Some(cache.clone()));
    let listing = list_models(&endpoint(fake.port()), &spec).unwrap();
    assert_eq!((listing.rows.len(), listing.stale), (0, false));
    assert!(listing.warnings.is_empty());
    assert_eq!(
        read_cache(&cache),
        Some(vec![]),
        "an empty list is kept as one"
    );

    // rows keep what the engine said, in its order
    fake.add_model("qwen3:4b", 2_500_000_000)
        .add_model("bonsai-q1", 248_302_385);
    let listing = list_models(&endpoint(fake.port()), &spec).unwrap();
    // in the order the engine's own answer has them, whatever order they were added in
    let raw = call(&endpoint(fake.port()), &json_call("GET", "/api/tags", None)).unwrap();
    let asked: Vec<String> = raw.body["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_string())
        .collect();
    let ids: Vec<String> = listing.rows.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids, asked);
    assert_eq!(ids.len(), 2);

    let first = listing
        .rows
        .iter()
        .find(|r| r.id.starts_with("qwen3"))
        .expect("the model that was added");
    assert_eq!(first.size_bytes, Some(2_500_000_000));
    assert!(
        first.digest.as_deref().is_some_and(|d| d.len() == 64),
        "{first:?}"
    );
    assert_eq!(first.capabilities, ["tools", "thinking", "completion"]);
    assert_eq!(first.display, first.id);

    // a row with no name is skipped, with a word about it, and the rest still come
    let odd = MiniServer::start(None, |_, r| {
        r.json(
            200,
            &json!({"models": [
                {"name": "a:1b", "size": 1, "digest": "d1"},
                {"size": 2},
                {"name": "", "size": 3},
                {"name": "b:2b", "capabilities": ["vision"]}
            ]}),
            "application/json",
        );
    })
    .unwrap();
    let listing = list_models(&endpoint(odd.port()), &ListSpec::ollama(None)).unwrap();
    let ids: Vec<&str> = listing.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["a:1b", "b:2b"]);
    assert_eq!(listing.warnings.len(), 2, "{:?}", listing.warnings);
    assert!(
        listing.warnings[0].contains("Entry 2"),
        "{:?}",
        listing.warnings
    );
    assert_eq!(listing.rows[1].capabilities, ["vision"]);
    assert_eq!(listing.rows[1].size_bytes, None);

    // the engine is not answering: what was kept is given back, marked
    let kept = read_cache(&cache).expect("the last answer was kept");
    assert_eq!(kept.len(), 2);
    fake.go_down();
    let down = list_models(&endpoint(fake.port()), &spec).unwrap();
    assert!(down.stale, "a list from the cache must say so");
    assert_eq!(down.rows, kept);
    assert!(
        down.warnings.iter().any(|w| w.contains("last time")),
        "{:?}",
        down.warnings
    );
    // with nothing kept there is only the failure
    assert!(matches!(
        list_models(
            &endpoint(fake.port()),
            &ListSpec::ollama(Some(dir.join("none.json")))
        ),
        Err(CallError::Unreachable(_))
    ));
    // a cache that is not ours or is damaged is ignored, not trusted
    for bad in [
        "not json",
        "{}",
        "{\"version\": 2, \"rows\": []}",
        "{\"version\": 1, \"rows\": [{\"id\": 5}]}",
        "[]",
    ] {
        std::fs::write(&cache, bad).unwrap();
        assert_eq!(read_cache(&cache), None, "{bad}");
        assert!(list_models(&endpoint(fake.port()), &spec).is_err(), "{bad}");
    }
    // the cache is written whole: no half-written temporary is left beside it
    let fake = FakeOllama::start();
    list_models(&endpoint(fake.port()), &spec).unwrap();
    let names: Vec<String> = std::fs::read_dir(&*dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

/// Runs a pull job to its end, collecting what it said and the fractions it reported.
fn run_pull(fake: &FakeOllama, name: &str) -> (Result<(), String>, Vec<String>, Vec<f32>) {
    let (mut job, _handle) = spawn_pull(
        endpoint(fake.port()),
        PullSpec::ollama(),
        name.to_string(),
        || {},
    );
    let (mut said, mut fractions) = (Vec::new(), Vec::new());
    let deadline = Instant::now() + Duration::from_secs(30);
    let outcome = loop {
        let polled = job.poll();
        said.extend(job.take_statuses());
        if let Some(f) = job.fraction() {
            if fractions.last() != Some(&f) {
                fractions.push(f);
            }
        }
        if let Some(outcome) = polled {
            break outcome;
        }
        assert!(Instant::now() < deadline, "the pull never ended");
        std::thread::sleep(Duration::from_millis(2));
    };
    (outcome, said, fractions)
}

#[test]
fn p3_h17_a_pull_is_a_job_whose_progress_follows_the_tracker_and_whose_words_reach_the_log() {
    let fake = FakeOllama::start();
    fake.behave(|b| {
        b.line_pause = Duration::from_millis(8);
        b.pull.steps = 6;
    });
    let (outcome, said, fractions) = run_pull(&fake, "qwen3:4b");
    assert_eq!(outcome, Ok(()));
    assert!(said.contains(&"pulling manifest".to_string()), "{said:?}");
    assert!(said.contains(&"success".to_string()), "{said:?}");
    assert!(
        said.iter()
            .filter(|l| l.starts_with("pulling ") && *l != "pulling manifest")
            .count()
            >= 1,
        "the layers are said: {said:?}"
    );
    assert!(
        said.iter().collect::<std::collections::BTreeSet<_>>().len() == said.len(),
        "a line was said twice: {said:?}"
    );
    assert!(fractions.windows(2).all(|w| w[0] <= w[1]), "{fractions:?}");
    assert_eq!(fractions.last(), Some(&1.0), "{fractions:?}");
    // the model is there when the job is done, which is when the list is asked again
    assert!(fake.model_names().iter().any(|n| n.starts_with("qwen3:4b")));
    let listed = list_models(&endpoint(fake.port()), &ListSpec::ollama(None)).unwrap();
    assert!(listed.rows.iter().any(|r| r.id.starts_with("qwen3:4b")));

    // the engine's own sentence, from inside the 200
    fake.behave(|b| b.pull.fail = Some("pull model manifest: file does not exist".into()));
    let (outcome, said, _) = run_pull(&fake, "ghost");
    assert_eq!(
        outcome,
        Err("pull model manifest: file does not exist".to_string())
    );
    assert_eq!(said, ["pulling manifest"]);

    // a stream that stops short is not a model
    fake.behave(|b| {
        b.pull.fail = None;
        b.pull.cut_after_lines = Some(3);
    });
    let (outcome, _, _) = run_pull(&fake, "cut");
    assert!(
        outcome.as_ref().is_err_and(|m| !m.is_empty()),
        "{outcome:?}"
    );
    assert!(!fake.model_names().iter().any(|n| n.starts_with("cut")));
    // nothing to reach
    let (mut job, _) = spawn_pull(
        Endpoint {
            host: "127.0.0.1".into(),
            port: Some(crate::testkit::free_port()),
        },
        PullSpec::ollama(),
        "x".into(),
        || {},
    );
    let outcome = job.wait(Duration::from_secs(10)).unwrap();
    assert!(outcome.is_err());
}

#[test]
fn p3_h17b_cancelling_a_pull_closes_the_connection_and_the_job_says_so() {
    let fake = FakeOllama::start();
    fake.behave(|b| {
        b.line_pause = Duration::from_millis(40);
        b.pull.steps = 60;
    });
    let (mut job, handle) = spawn_pull(
        endpoint(fake.port()),
        PullSpec::ollama(),
        "big".into(),
        || {},
    );
    crate::testkit::wait_until(Duration::from_secs(5), "the pull to be under way", || {
        fake.lines_sent() >= 4
    });
    let cancelled = Instant::now();
    handle.cancel();
    let outcome = job.wait(Duration::from_secs(5)).expect("the job ended");
    assert!(
        cancelled.elapsed() < Duration::from_secs(1),
        "{:?}",
        cancelled.elapsed()
    );
    assert_eq!(outcome, Err("The pull was cancelled.".to_string()));
    let sent = fake.lines_sent();
    std::thread::sleep(Duration::from_millis(400));
    assert!(fake.lines_sent() <= sent + 2, "the engine went on");
    assert!(!fake.model_names().iter().any(|n| n.starts_with("big")));
    let _ = JobEvent::<()>::Fraction(0.0);
}
