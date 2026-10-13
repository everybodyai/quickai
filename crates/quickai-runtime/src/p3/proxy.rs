//! Group D of the test plan: what the live-traffic proxy makes of Ollama's own API, and that it
//! still reads the OpenAI shape the way it always has (work package 3.6). The proxy sits in front
//! of the fake Ollama exactly as it sits in front of a real engine.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver};
use serde_json::json;

use crate::events::{Event, TrafficEvent};
use crate::proxy::{spawn, ProxyHandle};
use crate::testkit::fake_ollama::FakeOllama;
use crate::testkit::free_port;
use crate::testkit::http::post_json;

const REPLY: &str = "Hello! I'm Bonsai, your AI assistant.";

struct Rig {
    fake: FakeOllama,
    public: u16,
    handle: ProxyHandle,
    rx: Receiver<Event>,
}

impl Rig {
    fn new() -> Rig {
        let fake = FakeOllama::start();
        fake.add_model("m", 1);
        let (tx, rx) = unbounded();
        // a port found by binding 0 can still be held when the proxy binds it: ask for another
        let (public, handle) = (0..5)
            .find_map(|_| {
                let port = free_port();
                spawn(
                    port,
                    fake.port(),
                    crate::testkit::decoders(),
                    tx.clone(),
                    Arc::new(|| {}),
                )
                .ok()
                .map(|h| (port, h))
            })
            .expect("five free ports in a row were all taken");
        Rig {
            fake,
            public,
            handle,
            rx,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.public)
    }

    /// The traffic events until one that ends a conversation has arrived (or time runs out).
    fn events(&self) -> Vec<TrafficEvent> {
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Event::Traffic(t)) = self.rx.recv_timeout(Duration::from_millis(100)) {
                let ends = matches!(
                    t,
                    TrafficEvent::ResponseDone { .. } | TrafficEvent::ResponseError { .. }
                );
                out.push(t);
                if ends {
                    break;
                }
            }
        }
        out
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.handle.stop();
    }
}

fn request_of(events: &[TrafficEvent]) -> Option<(&str, &str, &Option<String>, &Vec<String>)> {
    events.iter().find_map(|e| match e {
        TrafficEvent::Request {
            method,
            path,
            prompt,
            attachments,
            ..
        } => Some((method.as_str(), path.as_str(), prompt, attachments)),
        _ => None,
    })
}

fn streamed_text(events: &[TrafficEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            TrafficEvent::ResponseDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn done_text(events: &[TrafficEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        TrafficEvent::ResponseDone { full_text, .. } => full_text.clone(),
        _ => None,
    })
}

/// A streamed conversation ends in a `ResponseDone` with no full text (the deltas carried it), as
/// an OpenAI stream always has; a reply that came whole carries it in the `ResponseDone`.
fn has_done(events: &[TrafficEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, TrafficEvent::ResponseDone { .. }))
}

#[test]
fn p3_d01_an_openai_chat_through_the_proxy_is_read_as_it_always_was() {
    let rig = Rig::new();
    let reply = post_json(
        &rig.url("/v1/chat/completions"),
        &json!({"model": "m", "stream": true, "messages": [{"role": "user", "content": "hello"}]}),
    );
    assert_eq!(reply.status, 200);
    assert!(
        reply.body.ends_with("data: [DONE]\n\n"),
        "the stream was altered: {:?}",
        reply.body
    );
    let events = rig.events();
    let (method, path, prompt, attachments) = request_of(&events).expect("a Request event");
    assert_eq!((method, path), ("POST", "/v1/chat/completions"));
    assert_eq!(prompt.as_deref(), Some("hello"));
    assert!(attachments.is_empty());
    assert_eq!(streamed_text(&events), REPLY);
    assert!(has_done(&events));
}

#[test]
fn p3_d02_ollamas_native_chat_is_read_like_an_openai_chat() {
    let rig = Rig::new();
    let reply = post_json(
        &rig.url("/api/chat"),
        &json!({"model": "m", "messages": [{"role": "system", "content": "be brief"}, {"role": "user", "content": "hello"}]}),
    );
    assert_eq!(reply.status, 200);
    let events = rig.events();
    let (_, path, prompt, _) =
        request_of(&events).expect("no Request event: /api/chat is not recognised");
    assert_eq!(path, "/api/chat");
    assert_eq!(
        prompt.as_deref(),
        Some("hello"),
        "the last message that is not the system's"
    );
    assert_eq!(streamed_text(&events), REPLY);
    assert!(
        has_done(&events),
        "the conversation was never closed: {events:?}"
    );
}

#[test]
fn p3_d03_ollamas_generate_is_read_from_its_prompt_and_its_response_field() {
    let rig = Rig::new();
    post_json(
        &rig.url("/api/generate"),
        &json!({"model": "m", "prompt": "Say hello."}),
    );
    let events = rig.events();
    let (_, path, prompt, _) =
        request_of(&events).expect("no Request event: /api/generate is not recognised");
    assert_eq!(path, "/api/generate");
    assert_eq!(prompt.as_deref(), Some("Say hello."));
    assert_eq!(streamed_text(&events), REPLY);
    assert!(has_done(&events));
}

#[test]
fn p3_d03c_a_reply_that_comes_whole_carries_its_text_in_the_done_event() {
    let rig = Rig::new();
    post_json(
        &rig.url("/api/chat"),
        &json!({"model": "m", "stream": false, "messages": [{"role": "user", "content": "hello"}]}),
    );
    assert_eq!(
        done_text(&rig.events()).as_deref(),
        Some(REPLY),
        "from /message/content"
    );
    let rig = Rig::new();
    post_json(
        &rig.url("/api/generate"),
        &json!({"model": "m", "stream": false, "prompt": "hello"}),
    );
    assert_eq!(
        done_text(&rig.events()).as_deref(),
        Some(REPLY),
        "from /response"
    );
}

#[test]
fn p3_d03b_a_load_or_an_unload_is_not_shown_as_a_conversation() {
    // QuickAI's own warm-up and unload go through /api/generate with no prompt: they must not
    // appear in the Live tab as an empty chat
    let rig = Rig::new();
    post_json(
        &rig.url("/api/generate"),
        &json!({"model": "m", "keep_alive": -1}),
    );
    post_json(
        &rig.url("/api/generate"),
        &json!({"model": "m", "keep_alive": 0}),
    );
    std::thread::sleep(Duration::from_millis(300));
    let mut seen = Vec::new();
    while let Ok(Event::Traffic(t)) = rig.rx.try_recv() {
        seen.push(t);
    }
    assert!(
        seen.is_empty(),
        "a load or unload was shown as traffic: {seen:?}"
    );
}

#[test]
fn p3_d04_images_sent_to_ollama_are_listed_as_attachments() {
    let rig = Rig::new();
    let pixels = "iVBORw0KGgo".repeat(40); // base64 of about 330 bytes
    post_json(
        &rig.url("/api/chat"),
        &json!({"model": "m", "messages": [{"role": "user", "content": "what is this", "images": [pixels, "AAAA"]}]}),
    );
    let events = rig.events();
    let (_, _, prompt, attachments) = request_of(&events).expect("no Request event");
    assert_eq!(prompt.as_deref(), Some("what is this"));
    assert_eq!(attachments.len(), 2, "{attachments:?}");
    assert!(
        attachments
            .iter()
            .all(|a| a.to_lowercase().contains("image")),
        "{attachments:?}"
    );
}

#[test]
fn p3_d05_an_error_reply_and_an_error_in_the_stream_are_shown_as_errors() {
    let rig = Rig::new();
    post_json(
        &rig.url("/api/chat"),
        &json!({"model": "ghost", "messages": [{"role": "user", "content": "hi"}]}),
    );
    let events = rig.events();
    let error = events.iter().find_map(|e| match e {
        TrafficEvent::ResponseError { message, .. } => Some(message.clone()),
        _ => None,
    });
    assert!(error.is_some_and(|m| m.contains("not found")), "{events:?}");

    // a reply that stops half way through: the daemon died, or the model did
    let rig = Rig::new();
    rig.fake.behave(|b| b.cut_chat_after_lines = Some(3));
    let _ = crate::testkit::http::try_get(&rig.url("/api/version"));
    // A client of an engine that died mid-reply waits for its own timeout: the relay cannot reset
    // a connection (tiny_http offers no way to), and it does not pretend the reply was complete.
    // What the proxy owes is the Live tab, which closes the entry at once.
    let client = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .build(),
    );
    let _ = client
        .post(rig.url("/api/chat"))
        .header("Content-Type", "application/json")
        .send(json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]}).to_string())
        .and_then(|r| {
            let mut sink = Vec::new();
            std::io::Read::read_to_end(&mut r.into_body().into_reader(), &mut sink)?;
            Ok(())
        });
    let events = rig.events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            TrafficEvent::ResponseError { .. } | TrafficEvent::ResponseDone { .. }
        )),
        "the conversation was left open: {events:?}"
    );
    assert_eq!(
        streamed_text(&events),
        "Hello! I'm Bonsai, ",
        "what arrived before the cut (three words) is kept"
    );
}

#[test]
fn p3_d06_lines_cut_in_the_middle_by_the_network_are_put_back_together() {
    let rig = Rig::new();
    rig.fake.behave(|b| b.split_lines = true);
    let reply = post_json(
        &rig.url("/api/chat"),
        &json!({"model": "m", "messages": [{"role": "user", "content": "hello"}]}),
    );
    assert_eq!(reply.status, 200);
    let events = rig.events();
    assert_eq!(
        streamed_text(&events),
        REPLY,
        "a line split across two chunks was lost"
    );
}

#[test]
fn p3_d07_the_stream_reaches_the_client_exactly_as_the_engine_sent_it() {
    // relaying is byte for byte, whether or not the proxy understands what passes
    let rig = Rig::new();
    let reply = post_json(
        &rig.url("/api/chat"),
        &json!({"model": "m", "messages": [{"role": "user", "content": "hello"}]}),
    );
    assert_eq!(reply.status, 200);
    let lines: Vec<serde_json::Value> = reply
        .body
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(lines.len() > 5);
    assert_eq!(lines.last().unwrap()["done"], true);
    let text: String = lines
        .iter()
        .filter_map(|l| l["message"]["content"].as_str())
        .collect();
    assert_eq!(text, REPLY);
    // and what the engine was sent is what the client sent
    let seen = rig.fake.requests_to("/api/chat");
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].body.as_ref().unwrap()["messages"][0]["content"],
        "hello"
    );
}

/// Finding F1 of the GUI suite (TC-LIVE-008): a streamed reply was held back until the engine had
/// finished (tiny_http fills a kilobyte before it sends a chunk), which defeats the point of a chat that
/// is written as it is thought, and Ollama streams by default.
#[test]
fn p3_d10_a_reply_that_streams_reaches_the_client_as_it_arrives() {
    use std::io::Read;
    let rig = Rig::new();
    rig.fake
        .behave(|b| b.line_pause = Duration::from_millis(400));
    let started = Instant::now();
    let response = ureq::post(rig.url("/api/chat"))
        .header("Content-Type", "application/json")
        .send(json!({"model": "m", "messages": [{"role": "user", "content": "hello"}]}).to_string())
        .unwrap();
    let mut reader = response.into_body().into_reader();
    let mut first = [0u8; 64];
    let n = reader.read(&mut first).unwrap();
    let first_at = started.elapsed();
    assert!(n > 0);
    assert!(
        first[..n].starts_with(b"{"),
        "the start of the first line of the reply: {:?}",
        String::from_utf8_lossy(&first[..n])
    );
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    let all_at = started.elapsed();
    assert!(
        all_at > Duration::from_millis(2000),
        "the engine took {all_at:?}: it was not streaming slowly enough to tell"
    );
    assert!(
        first_at < Duration::from_millis(1500),
        "the first line came after {first_at:?}, with the reply over after {all_at:?}: it was held back"
    );
    let lines: Vec<&str> = rest.lines().collect();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines.last().unwrap()).unwrap()["done"],
        true,
        "and the reply is whole"
    );
}

/// An API that only a document defines, on a server of a made-up engine: the proxy needs no change
/// to read it, because how it is read is the vocabulary's to say.
#[test]
fn p3_d08e_the_proxy_reads_an_api_that_only_a_document_defines() {
    use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
    use quickai_core::registry::Registry;

    const VOCAB: &str = r#"schema = 1
kind = "vocab"
id = "acme"
requires = { primitives = ["decode@1"] }

[[api]]
id = "acme.talk"
paths = ["/v2/talk"]
capabilities = ["chat"]

[api.decode]
framing = ["ndjson", "json"]
delta = ["/piece"]
final = ["/answer"]
error = ["/problem"]

[api.decode.chat]
messages = "/turns"
role = "/who"
content = "/say"
skip_roles = ["rules"]
"#;
    let source = MemorySource::new(&[("embedded:vocab/acme.toml", VOCAB)]);
    let registry = Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &crate::prims::known(),
    );
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let decoders = registry.decoders_for(&["acme.talk".to_string()]);
    assert_eq!(decoders.len(), 1);

    let server = crate::testkit::mini_http::MiniServer::start(None, |req, resp| {
        if req.route() == "/v2/talk" {
            resp.status(200)
                .header("Content-Type", "application/x-ndjson");
            resp.begin_stream();
            for piece in ["Hel", "lo ", "there"] {
                let _ = resp.chunk(format!("{{\"piece\":\"{piece}\"}}\n").as_bytes());
            }
            resp.end_stream();
        } else {
            resp.json(404, &json!({"problem": "nothing here"}), "application/json");
        }
    })
    .expect("a server");
    let (tx, rx) = unbounded();
    let (public, handle) = (0..5)
        .find_map(|_| {
            let port = free_port();
            spawn(
                port,
                server.port(),
                decoders.clone(),
                tx.clone(),
                Arc::new(|| {}),
            )
            .ok()
            .map(|h| (port, h))
        })
        .expect("five free ports in a row were all taken");

    let reply = post_json(
        &format!("http://127.0.0.1:{public}/v2/talk"),
        &json!({"turns": [{"who": "person", "say": "hi"}, {"who": "rules", "say": "be brief"}]}),
    );
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.body.lines().count(),
        3,
        "the stream reached the client whole: {:?}",
        reply.body
    );

    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Event::Traffic(t)) = rx.recv_timeout(Duration::from_millis(100)) {
            let ends = matches!(t, TrafficEvent::ResponseDone { .. });
            events.push(t);
            if ends {
                break;
            }
        }
    }
    let (_, path, prompt, _) = request_of(&events).expect("a Request event");
    assert_eq!(path, "/v2/talk");
    assert_eq!(
        prompt.as_deref(),
        Some("hi"),
        "the last turn that is not the rules'"
    );
    assert_eq!(streamed_text(&events), "Hello there");
    assert!(has_done(&events));

    // what no document claims is relayed and shown nowhere
    let other = post_json(
        &format!("http://127.0.0.1:{public}/v2/other"),
        &json!({"x": 1}),
    );
    assert_eq!(
        other.status, 404,
        "an error status reaches the client as it is"
    );
    assert!(other.body.contains("nothing here"), "{}", other.body);
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        rx.try_recv().is_err(),
        "a path that no document claims was shown"
    );
    handle.stop();
}

#[test]
fn p3_d09_an_error_status_reaches_the_client_as_it_is_and_is_shown_as_an_error() {
    // the engine's own 404 with its own sentence, not a 502 from the proxy (TC-LIVE-017)
    let rig = Rig::new();
    let reply = post_json(
        &rig.url("/api/chat"),
        &json!({"model": "ghost", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert_eq!(reply.status, 404);
    assert!(
        reply.body.contains("model 'ghost' not found"),
        "the engine's sentence was lost: {}",
        reply.body
    );
    // and an OpenAI-shaped error object is read the same way
    let reply = post_json(
        &rig.url("/v1/chat/completions"),
        &json!({"model": "ghost", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert_eq!(reply.status, 404);
    // both conversations end in an error that says what the engine said
    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && events
            .iter()
            .filter(|e| matches!(e, TrafficEvent::ResponseError { .. }))
            .count()
            < 2
    {
        if let Ok(Event::Traffic(t)) = rig.rx.recv_timeout(Duration::from_millis(100)) {
            events.push(t);
        }
    }
    let errors: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            TrafficEvent::ResponseError { message, .. } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(errors.len(), 2, "{events:?}");
    assert!(
        errors.iter().all(|m| m.contains("model 'ghost' not found")),
        "{errors:?}"
    );
}
