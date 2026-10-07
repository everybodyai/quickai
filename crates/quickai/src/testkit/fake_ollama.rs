//! A stand-in for Ollama's HTTP side: the endpoints QuickAI uses, answering the way the recordings
//! of a real Ollama say it does (`recorded.rs`; the contract test in `p3/contract.rs` replays them
//! against this and compares shapes), and misbehaving on purpose when a test asks: slow loads,
//! failing loads, a pull that fails after it started (with status 200, as the real one does), a
//! reply cut off in the middle, a daemon that disappears, a server on the port that is not Ollama.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::mini_http::{MiniServer, Request, Response};

#[derive(Clone, Debug)]
pub struct Model {
    /// As Ollama lists it: with a tag (`bonsai-q1:latest`).
    pub name: String,
    pub size: u64,
    pub digest: String,
    pub capabilities: Vec<String>,
}

/// What answers on the port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    Ollama,
    /// A server whose `/api/version` says `{"status":"ok"}`, as llama-server's `/health` does.
    LlamaServer,
    /// A web server: 200 and HTML for every path.
    WebServer,
}

#[derive(Clone, Debug)]
pub struct PullScript {
    /// `(digest, total bytes)` per layer, pulled one after the other.
    pub layers: Vec<(String, u64)>,
    /// Progress lines per layer that carry `completed` (the real stream also has lines with only
    /// `total`, before any bytes moved, and ends a layer without a last `completed`).
    pub steps: usize,
    /// The sentence of an `{"error": ...}` line sent after the manifest (status stays 200).
    pub fail: Option<String>,
    /// Hang up after this many lines instead of finishing.
    pub cut_after_lines: Option<usize>,
}

impl Default for PullScript {
    fn default() -> Self {
        PullScript {
            layers: vec![
                ("797b70c4edf8".into(), 45_949_216),
                ("c71d239df917".into(), 11_357),
                ("85011998c600".into(), 16),
                ("548455b72658".into(), 407),
            ],
            steps: 6,
            fail: None,
            cut_after_lines: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Behave {
    pub identity: Identity,
    pub version: String,
    /// How long a load (a generate with no prompt) takes.
    pub load_delay: Duration,
    /// Answer a load with this status and `{"error": message}`.
    pub load_failure: Option<(u16, String)>,
    pub unload_delay: Duration,
    pub unload_failure: Option<(u16, String)>,
    /// A pause between the lines of any stream.
    pub line_pause: Duration,
    pub pull: PullScript,
    /// Hang up after this many lines of a chat or generate stream.
    pub cut_chat_after_lines: Option<usize>,
    /// The text a chat or generate answers with.
    pub reply: String,
    /// `path -> (status, times)`: answer this many requests to the path with the status first.
    pub fail_next: BTreeMap<String, (u16, usize)>,
    /// `path -> times`: hang up on this many requests to the path before answering.
    pub hang_up_next: BTreeMap<String, usize>,
    /// Write each line of a stream in two pieces with a pause between them, so a reader sees
    /// a line cut in the middle.
    pub split_lines: bool,
    /// How many lines of streams have been written (shared by every clone of this struct, so a
    /// test can ask after the fact whether the fake stopped writing when its client left).
    pub lines_sent: Arc<AtomicUsize>,
}

impl Default for Behave {
    fn default() -> Self {
        Behave {
            identity: Identity::Ollama,
            version: "0.35.1".into(),
            load_delay: Duration::ZERO,
            load_failure: None,
            unload_delay: Duration::ZERO,
            unload_failure: None,
            line_pause: Duration::ZERO,
            pull: PullScript::default(),
            cut_chat_after_lines: None,
            reply: "Hello! I'm Bonsai, your AI assistant.".into(),
            fail_next: BTreeMap::new(),
            hang_up_next: BTreeMap::new(),
            split_lines: false,
            lines_sent: Arc::new(AtomicUsize::new(0)),
        }
    }
}

/// One request the fake received.
#[derive(Clone, Debug)]
pub struct Logged {
    pub method: String,
    pub path: String,
    pub body: Option<Value>,
    pub headers: Vec<(String, String)>,
    pub at: Instant,
}

impl Logged {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

type Hook = Arc<dyn Fn(&Logged) + Send + Sync>;

#[derive(Default)]
struct State {
    hook: Option<Hook>,
    models: Vec<Model>,
    /// The loaded model's name, and whether it stays for ever (`keep_alive: -1`).
    loaded: Option<(String, bool)>,
    log: Vec<Logged>,
    behave: Behave,
    next_stamp: u64,
}

pub struct FakeOllama {
    server: MiniServer,
    state: Arc<Mutex<State>>,
}

/// `qwen3:4b` stays, `qwen3` becomes `qwen3:latest`.
pub fn with_tag(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("{name}:latest")
    }
}

fn digest_of(name: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    let mut out = String::new();
    for round in 0..4u64 {
        for b in name.bytes().chain(round.to_le_bytes()) {
            h ^= u64::from(b);
            h = h.wrapping_mul(1099511628211);
        }
        out.push_str(&format!("{h:016x}"));
    }
    out
}

impl FakeOllama {
    pub fn start() -> FakeOllama {
        Self::start_on(None).expect("a free port")
    }

    /// On a given port; `Err` when it is taken.
    pub fn start_on(port: Option<u16>) -> std::io::Result<FakeOllama> {
        let state: Arc<Mutex<State>> = Arc::default();
        let shared = state.clone();
        let server = MiniServer::start(port, move |req, resp| handle(&shared, req, resp))?;
        Ok(FakeOllama { server, state })
    }

    pub fn port(&self) -> u16 {
        self.server.port()
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port())
    }

    /// Adds a model the way a finished pull does.
    pub fn add_model(&self, name: &str, size: u64) -> &Self {
        let name = with_tag(name);
        let mut state = self.state.lock().unwrap();
        let capabilities = vec!["tools".to_string(), "thinking".into(), "completion".into()];
        state.models.retain(|m| m.name != name);
        state.models.push(Model {
            digest: digest_of(&name),
            name,
            size,
            capabilities,
        });
        self
    }

    /// Calls `hook` with every request as it arrives, before it is answered (to look at the world
    /// at that moment: is the daemon's process still alive when it is asked to unload?).
    pub fn on_request(&self, hook: impl Fn(&Logged) + Send + Sync + 'static) -> &Self {
        self.state.lock().unwrap().hook = Some(Arc::new(hook));
        self
    }

    pub fn behave(&self, change: impl FnOnce(&mut Behave)) -> &Self {
        change(&mut self.state.lock().unwrap().behave);
        self
    }

    /// Stops answering: the port is closed, as if the daemon had died.
    pub fn go_down(&self) {
        self.server.shutdown();
    }

    pub fn requests(&self) -> Vec<Logged> {
        self.state.lock().unwrap().log.clone()
    }

    /// The requests to one path, in order.
    pub fn requests_to(&self, path: &str) -> Vec<Logged> {
        self.requests()
            .into_iter()
            .filter(|r| r.path.split('?').next() == Some(path))
            .collect()
    }

    /// Lines of streamed replies written so far.
    pub fn lines_sent(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .behave
            .lines_sent
            .load(Ordering::SeqCst)
    }

    pub fn loaded(&self) -> Option<String> {
        self.state.lock().unwrap().loaded.clone().map(|(n, _)| n)
    }

    pub fn model_names(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .models
            .iter()
            .map(|m| m.name.clone())
            .collect()
    }
}

// ---- the handler --------------------------------------------------------------------------------

const JSON_UTF8: &str = "application/json; charset=utf-8";
const NDJSON: &str = "application/x-ndjson";

fn stamp(state: &mut State) -> String {
    state.next_stamp += 1;
    format!(
        "2026-10-06T14:03:{:02}.{:09}Z",
        state.next_stamp % 60,
        state.next_stamp
    )
}

fn error(resp: &mut Response, status: u16, message: &str) {
    resp.json(status, &json!({ "error": message }), JSON_UTF8);
}

fn handle(shared: &Arc<Mutex<State>>, req: &Request, resp: &mut Response) {
    let route = req.route().to_string();
    let body = req.json();
    let behave;
    let hook;
    {
        let mut state = shared.lock().unwrap();
        let logged = Logged {
            method: req.method.clone(),
            path: req.path.clone(),
            body: body.clone(),
            headers: req.headers.clone(),
            at: Instant::now(),
        };
        state.log.push(logged);
        hook = state
            .hook
            .clone()
            .map(|h| (h, state.log.last().unwrap().clone()));
        // faults first: they stand in for something going wrong before the engine answers
        if let Some(left) = state.behave.hang_up_next.get_mut(&route) {
            if *left > 0 {
                *left -= 1;
                drop(state);
                resp.abort();
                return;
            }
        }
        if let Some((status, times)) = state.behave.fail_next.get_mut(&route) {
            if *times > 0 {
                *times -= 1;
                let status = *status;
                drop(state);
                error(resp, status, "injected failure");
                return;
            }
        }
        behave = state.behave.clone();
    }
    if let Some((hook, logged)) = hook {
        hook(&logged);
    }

    match behave.identity {
        Identity::WebServer => {
            resp.status(200).header("Content-Type", "text/html");
            resp.send(b"<html><body>hello</body></html>");
            return;
        }
        Identity::LlamaServer => {
            match route.as_str() {
                "/api/version" | "/health" => resp.json(
                    200,
                    &json!({"status": "ok"}),
                    "application/json; charset=utf-8",
                ),
                _ => {
                    resp.status(404).header("Content-Type", "text/plain");
                    resp.send(b"404 page not found");
                }
            }
            return;
        }
        Identity::Ollama => {}
    }

    match (req.method.as_str(), route.as_str()) {
        ("GET" | "HEAD", "/") => {
            resp.status(200)
                .header("Content-Type", "text/plain; charset=utf-8");
            resp.send(b"Ollama is running");
        }
        ("GET", "/api/version") => {
            resp.json(200, &json!({ "version": behave.version }), JSON_UTF8);
        }
        ("GET", "/api/tags") => tags(shared, resp),
        ("GET", "/api/ps") => ps(shared, resp),
        ("GET", "/v1/models") => v1_models(shared, resp),
        ("POST", "/api/show") => show(shared, body, resp),
        ("POST", "/api/generate") => generate(shared, &behave, body, resp),
        ("POST", "/api/chat") => chat(shared, &behave, body, resp),
        ("POST", "/v1/chat/completions") => v1_chat(shared, &behave, body, resp),
        ("POST", "/api/pull") => pull(shared, &behave, body, resp),
        ("DELETE", "/api/delete") => delete(shared, body, resp),
        _ => {
            resp.status(404).header("Content-Type", "text/plain");
            resp.send(b"404 page not found");
        }
    }
}

fn find<'a>(state: &'a State, name: &str) -> Option<&'a Model> {
    let wanted = with_tag(name);
    state.models.iter().find(|m| m.name == wanted)
}

fn model_name_of(body: &Option<Value>) -> Option<String> {
    body.as_ref()?
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn tags(shared: &Arc<Mutex<State>>, resp: &mut Response) {
    let state = shared.lock().unwrap();
    // newest first, as the real listing is
    let models: Vec<Value> = state
        .models
        .iter()
        .rev()
        .map(|m| {
            json!({
                "name": m.name,
                "model": m.name,
                "modified_at": "2026-10-06T16:03:44.836786343+02:00",
                "size": m.size,
                "digest": m.digest,
                "details": {
                    "parent_model": "",
                    "format": "gguf",
                    "family": "qwen3",
                    "families": ["qwen3"],
                    "parameter_size": "1.7B",
                    "quantization_level": "Q1_0",
                    "context_length": 32768,
                    "embedding_length": 2048
                },
                "capabilities": m.capabilities,
            })
        })
        .collect();
    resp.json(200, &json!({ "models": models }), JSON_UTF8);
}

fn ps(shared: &Arc<Mutex<State>>, resp: &mut Response) {
    let state = shared.lock().unwrap();
    let models: Vec<Value> = state
        .loaded
        .iter()
        .filter_map(|(name, forever)| find(&state, name).map(|m| (m, *forever)))
        .map(|(m, forever)| {
            json!({
                "name": m.name,
                "model": m.name,
                "size": m.size,
                "digest": m.digest,
                "details": {
                    "parent_model": "",
                    "format": "gguf",
                    "family": "qwen3",
                    "families": ["qwen3"],
                    "parameter_size": "1.7B",
                    "quantization_level": "Q1_0"
                },
                "expires_at": if forever { "2319-01-16T14:51:03.311311116+01:00" } else { "2026-10-06T14:08:46.456333284Z" },
                "size_vram": m.size,
                "context_length": 4096
            })
        })
        .collect();
    resp.json(200, &json!({ "models": models }), JSON_UTF8);
}

fn v1_models(shared: &Arc<Mutex<State>>, resp: &mut Response) {
    let state = shared.lock().unwrap();
    let data: Vec<Value> = state
        .models
        .iter()
        .map(|m| json!({"id": m.name, "object": "model", "created": 1791295424u64, "owned_by": "library"}))
        .collect();
    resp.json(
        200,
        &json!({"object": "list", "data": data}),
        "application/json",
    );
}

fn show(shared: &Arc<Mutex<State>>, body: Option<Value>, resp: &mut Response) {
    let state = shared.lock().unwrap();
    let Some(name) = model_name_of(&body) else {
        return error(resp, 400, "model is required");
    };
    let Some(m) = find(&state, &name) else {
        return error(resp, 404, &format!("model '{}' not found", with_tag(&name)));
    };
    resp.json(
        200,
        &json!({
            "modelfile": format!("# Modelfile generated by \"ollama show\"\nFROM {}\n", m.name),
            "template": "{{ .Prompt }}",
            "details": {
                "parent_model": "", "format": "gguf", "family": "qwen3", "families": ["qwen3"],
                "parameter_size": "1.7B", "quantization_level": "Q1_0"
            },
            "model_info": { "general.architecture": "qwen3", "general.parameter_count": 1_700_000_000u64 },
            "capabilities": m.capabilities,
            "modified_at": "2026-10-06T16:03:44.836786343+02:00"
        }),
        JSON_UTF8,
    );
}

fn delete(shared: &Arc<Mutex<State>>, body: Option<Value>, resp: &mut Response) {
    let mut state = shared.lock().unwrap();
    let Some(name) = model_name_of(&body) else {
        return error(resp, 400, "model is required");
    };
    let wanted = with_tag(&name);
    if find(&state, &name).is_none() {
        return error(resp, 404, &format!("model '{name}' not found"));
    }
    state.models.retain(|m| m.name != wanted);
    if state.loaded.as_ref().is_some_and(|(n, _)| *n == wanted) {
        state.loaded = None;
    }
    resp.status(200);
    resp.send(b"");
}

/// A keep-alive of 0 (number or text) means unload; a negative one means keep for ever.
fn keep_alive_of(body: &Value) -> Option<i64> {
    match body.get("keep_alive")? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) if s == "0" || s == "0s" => Some(0),
        Value::String(s) if s.starts_with('-') => Some(-1),
        _ => Some(300),
    }
}

fn load(
    shared: &Arc<Mutex<State>>,
    behave: &Behave,
    name: &str,
    forever: bool,
) -> Result<(), (u16, String)> {
    std::thread::sleep(behave.load_delay);
    if let Some((status, message)) = &behave.load_failure {
        return Err((*status, message.clone()));
    }
    let mut state = shared.lock().unwrap();
    if find(&state, name).is_none() {
        return Err((404, format!("model '{name}' not found")));
    }
    state.loaded = Some((with_tag(name), forever));
    Ok(())
}

fn words(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split(' ').collect();
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i + 1 < parts.len() {
                format!("{p} ")
            } else {
                p.to_string()
            }
        })
        .collect()
}

/// Writes `lines` as a stream; `true` when every one was sent and the stream was ended properly
/// (not cut on purpose, and the client did not hang up).
fn write_lines(
    resp: &mut Response,
    lines: &[Value],
    pause: Duration,
    cut_after: Option<usize>,
    content_type: &str,
    split: bool,
    sent: &AtomicUsize,
) -> bool {
    resp.status(200).header("Content-Type", content_type);
    resp.begin_stream();
    for (i, line) in lines.iter().enumerate() {
        if cut_after.is_some_and(|n| i >= n) {
            resp.abort();
            return false;
        }
        let mut text = line.to_string();
        text.push('\n');
        if split {
            let at = text.len() / 2;
            if resp.chunk(&text.as_bytes()[..at]).is_err() {
                return false;
            }
            std::thread::sleep(pause.max(Duration::from_millis(15)));
            if resp.chunk(&text.as_bytes()[at..]).is_err() {
                return false;
            }
        } else if resp.chunk(text.as_bytes()).is_err() {
            return false;
        }
        sent.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(pause);
    }
    resp.end_stream();
    true
}

fn generate(shared: &Arc<Mutex<State>>, behave: &Behave, body: Option<Value>, resp: &mut Response) {
    let Some(body) = body else {
        return error(resp, 400, "unexpected EOF");
    };
    let Some(name) = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return error(resp, 400, "model is required");
    };
    {
        let state = shared.lock().unwrap();
        if find(&state, &name).is_none() {
            return error(resp, 404, &format!("model '{name}' not found"));
        }
    }
    let keep = keep_alive_of(&body);
    let has_prompt = body
        .get("prompt")
        .and_then(Value::as_str)
        .is_some_and(|p| !p.is_empty());
    let created = stamp(&mut shared.lock().unwrap());

    if !has_prompt {
        if keep == Some(0) {
            std::thread::sleep(behave.unload_delay);
            if let Some((status, message)) = &behave.unload_failure {
                return error(resp, *status, message);
            }
            shared.lock().unwrap().loaded = None;
            return resp.json(
                200,
                &json!({"model": name, "created_at": created, "response": "", "done": true, "done_reason": "unload"}),
                JSON_UTF8,
            );
        }
        return match load(shared, behave, &name, keep.is_some_and(|k| k < 0)) {
            Ok(()) => resp.json(
                200,
                &json!({"model": name, "created_at": created, "response": "", "done": true, "done_reason": "load"}),
                JSON_UTF8,
            ),
            Err((status, message)) => error(resp, status, &message),
        };
    }

    if shared.lock().unwrap().loaded.is_none() {
        if let Err((status, message)) = load(shared, behave, &name, false) {
            return error(resp, status, &message);
        }
    }
    let stats = |extra: Value| {
        let mut done = json!({
            "model": name, "created_at": created, "response": "", "done": true, "done_reason": "stop",
            "context": [151644, 872, 198], "total_duration": 158822797u64, "load_duration": 644019u64,
            "prompt_eval_count": 15, "prompt_eval_cached_count": 0, "prompt_eval_duration": 111150000u64,
            "eval_count": 12, "eval_duration": 39733000u64
        });
        if let (Some(d), Some(e)) = (done.as_object_mut(), extra.as_object()) {
            d.extend(e.clone());
        }
        done
    };
    if body.get("stream").and_then(Value::as_bool) == Some(false) {
        return resp.json(200, &stats(json!({"response": behave.reply})), JSON_UTF8);
    }
    let mut lines: Vec<Value> = words(&behave.reply)
        .into_iter()
        .map(|w| json!({"model": name, "created_at": created, "response": w, "done": false}))
        .collect();
    lines.push(stats(json!({})));
    write_lines(
        resp,
        &lines,
        behave.line_pause,
        behave.cut_chat_after_lines,
        NDJSON,
        behave.split_lines,
        &behave.lines_sent,
    );
}

fn chat(shared: &Arc<Mutex<State>>, behave: &Behave, body: Option<Value>, resp: &mut Response) {
    let Some(body) = body else {
        return error(resp, 400, "unexpected EOF");
    };
    let Some(name) = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return error(resp, 400, "model is required");
    };
    {
        let state = shared.lock().unwrap();
        if find(&state, &name).is_none() {
            return error(resp, 404, &format!("model '{name}' not found"));
        }
    }
    if shared.lock().unwrap().loaded.is_none() {
        if let Err((status, message)) = load(shared, behave, &name, false) {
            return error(resp, status, &message);
        }
    }
    let created = stamp(&mut shared.lock().unwrap());
    let stats = json!({
        "total_duration": 67290584u64, "load_duration": 603880u64, "prompt_eval_count": 15,
        "prompt_eval_cached_count": 14, "prompt_eval_duration": 13400000u64, "eval_count": 12,
        "eval_duration": 46370000u64
    });
    let done_line = |content: &str| {
        let mut line = json!({
            "model": name, "created_at": created,
            "message": {"role": "assistant", "content": content},
            "done": true, "done_reason": "stop"
        });
        line.as_object_mut()
            .unwrap()
            .extend(stats.as_object().unwrap().clone());
        line
    };
    if body.get("stream").and_then(Value::as_bool) == Some(false) {
        return resp.json(200, &done_line(&behave.reply), JSON_UTF8);
    }
    let mut lines: Vec<Value> = words(&behave.reply)
        .into_iter()
        .map(|w| {
            json!({"model": name, "created_at": created,
                   "message": {"role": "assistant", "content": w}, "done": false})
        })
        .collect();
    lines.push(done_line(""));
    write_lines(
        resp,
        &lines,
        behave.line_pause,
        behave.cut_chat_after_lines,
        NDJSON,
        behave.split_lines,
        &behave.lines_sent,
    );
}

fn v1_chat(shared: &Arc<Mutex<State>>, behave: &Behave, body: Option<Value>, resp: &mut Response) {
    let Some(body) = body else {
        return error(resp, 400, "unexpected EOF");
    };
    let Some(name) = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return error(resp, 400, "model is required");
    };
    {
        let state = shared.lock().unwrap();
        if find(&state, &name).is_none() {
            return resp.json(
                404,
                &json!({"error": {"message": format!("model '{name}' not found"), "type": "api_error", "param": null, "code": null}}),
                "application/json",
            );
        }
    }
    if shared.lock().unwrap().loaded.is_none() {
        let _ = load(shared, behave, &name, false);
    }
    let created = 1_791_295_427u64;
    if body.get("stream").and_then(Value::as_bool) != Some(true) {
        return resp.json(
            200,
            &json!({
                "id": "chatcmpl-539", "object": "chat.completion", "created": created, "model": name,
                "system_fingerprint": "fp_ollama",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": behave.reply}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 15, "prompt_tokens_details": {"cached_tokens": 14}, "completion_tokens": 12, "total_tokens": 27}
            }),
            "application/json",
        );
    }
    resp.status(200).header("Content-Type", "text/event-stream");
    resp.begin_stream();
    for (i, w) in words(&behave.reply).into_iter().enumerate() {
        let delta = if i == 0 {
            json!({"role": "assistant", "content": w})
        } else {
            json!({"content": w})
        };
        let event = json!({
            "id": "chatcmpl-235", "object": "chat.completion.chunk", "created": created, "model": name,
            "system_fingerprint": "fp_ollama",
            "choices": [{"index": 0, "delta": delta, "finish_reason": null}]
        });
        if resp.chunk(format!("data: {event}\n\n").as_bytes()).is_err() {
            return;
        }
        std::thread::sleep(behave.line_pause);
    }
    let last = json!({
        "id": "chatcmpl-235", "object": "chat.completion.chunk", "created": created, "model": name,
        "system_fingerprint": "fp_ollama",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    });
    let _ = resp.chunk(format!("data: {last}\n\n").as_bytes());
    let _ = resp.chunk(b"data: [DONE]\n\n");
    resp.end_stream();
}

fn pull(shared: &Arc<Mutex<State>>, behave: &Behave, body: Option<Value>, resp: &mut Response) {
    let Some(name) = model_name_of(&body).filter(|n| !n.trim().is_empty()) else {
        return error(resp, 400, "invalid model name");
    };
    let script = &behave.pull;
    let mut lines: Vec<Value> = vec![json!({"status": "pulling manifest"})];
    if let Some(message) = &script.fail {
        lines.push(json!({ "error": message }));
        write_lines(
            resp,
            &lines,
            behave.line_pause,
            script.cut_after_lines,
            NDJSON,
            behave.split_lines,
            &behave.lines_sent,
        );
        return;
    }
    let already_there = find(&shared.lock().unwrap(), &name).is_some();
    for (short, total) in &script.layers {
        let digest = format!("sha256:{short}{}", "0".repeat(52));
        let status = format!("pulling {short}");
        if already_there {
            // what is in the store is not fetched again: each layer is reported complete at once
            lines.push(
                json!({"status": status, "digest": digest, "total": total, "completed": total}),
            );
            continue;
        }
        // the real stream says how big a layer is a few times before any byte has moved
        for _ in 0..3 {
            lines.push(json!({"status": status, "digest": digest, "total": total}));
        }
        for step in 1..=script.steps {
            let completed = total * step as u64 / (script.steps as u64 + 1);
            lines.push(
                json!({"status": status, "digest": digest, "total": total, "completed": completed}),
            );
        }
        // and ends the layer on a line with no `completed` at all
        lines.push(json!({"status": status, "digest": digest, "total": total}));
    }
    lines.push(json!({"status": "verifying sha256 digest"}));
    lines.push(json!({"status": "writing manifest"}));
    lines.push(json!({"status": "success"}));
    // a model is in the store only if the client stayed to the end: a pull that was cancelled
    // (the connection closed) leaves nothing, as a real one does
    let finished = write_lines(
        resp,
        &lines,
        behave.line_pause,
        script.cut_after_lines,
        NDJSON,
        behave.split_lines,
        &behave.lines_sent,
    );
    if finished {
        let size: u64 = script.layers.iter().map(|(_, t)| t).sum();
        let mut state = shared.lock().unwrap();
        let tagged = with_tag(&name);
        state.models.retain(|m| m.name != tagged);
        state.models.push(Model {
            digest: digest_of(&tagged),
            name: tagged,
            size,
            capabilities: vec!["completion".into()],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::http::{get, post_json, try_get};
    use std::io::Read;

    #[test]
    fn it_says_what_a_real_ollama_says_on_the_basics() {
        let fake = FakeOllama::start();
        let r = get(&fake.base());
        assert_eq!((r.status, r.body.as_str()), (200, "Ollama is running"));
        let r = get(&format!("{}/api/version", fake.base()));
        assert_eq!(
            (r.status, r.body.as_str()),
            (200, r#"{"version":"0.35.1"}"#)
        );
        assert_eq!(
            get(&format!("{}/api/tags", fake.base())).body,
            r#"{"models":[]}"#
        );
        assert_eq!(get(&format!("{}/nope", fake.base())).status, 404);
        assert_eq!(fake.requests().len(), 4);
    }

    #[test]
    fn a_model_can_be_loaded_listed_as_running_and_unloaded() {
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 2_500_000_000);
        let url = format!("{}/api/generate", fake.base());
        let r = post_json(&url, &json!({"model": "qwen3:4b", "keep_alive": -1}));
        assert_eq!(r.status, 200);
        assert_eq!(r.json()["done_reason"], "load");
        assert_eq!(fake.loaded().as_deref(), Some("qwen3:4b"));
        let ps = get(&format!("{}/api/ps", fake.base())).body;
        assert!(ps.contains("qwen3:4b") && ps.contains("2319-"), "{ps}");
        let r = post_json(&url, &json!({"model": "qwen3:4b", "keep_alive": 0}));
        assert!(r.body.contains(r#""done_reason":"unload""#), "{}", r.body);
        assert_eq!(fake.loaded(), None);
        let r = post_json(&url, &json!({"model": "nope"}));
        assert_eq!(
            (r.status, r.body.as_str()),
            (404, r#"{"error":"model 'nope' not found"}"#)
        );
    }

    #[test]
    fn a_chat_streams_lines_that_end_in_done() {
        let fake = FakeOllama::start();
        fake.add_model("m", 1);
        let r = post_json(
            &format!("{}/api/chat", fake.base()),
            &json!({"model":"m","messages":[{"role":"user","content":"hi"}]}),
        );
        assert_eq!(r.status, 200);
        let lines: Vec<Value> = r
            .body
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(lines.len() > 2);
        assert_eq!(lines.last().unwrap()["done"], true);
        let text: String = lines
            .iter()
            .filter_map(|l| l["message"]["content"].as_str())
            .collect();
        assert_eq!(text, "Hello! I'm Bonsai, your AI assistant.");
    }

    #[test]
    fn a_pull_streams_progress_then_adds_the_model_and_a_failed_one_says_so_in_a_200() {
        let fake = FakeOllama::start();
        let url = format!("{}/api/pull", fake.base());
        let r = post_json(&url, &json!({"model": "qwen3:4b"}));
        assert_eq!(r.status, 200);
        assert!(r.body.lines().next().unwrap().contains("pulling manifest"));
        assert!(r.body.lines().last().unwrap().contains("success"));
        assert_eq!(fake.model_names(), ["qwen3:4b"]);

        fake.behave(|b| b.pull.fail = Some("pull model manifest: file does not exist".into()));
        let r = post_json(&url, &json!({"model": "nope"}));
        assert_eq!(
            r.status, 200,
            "a real pull error arrives inside a 200 stream"
        );
        assert!(
            r.body
                .lines()
                .last()
                .unwrap()
                .contains(r#""error":"pull model manifest"#),
            "{}",
            r.body
        );
        assert_eq!(post_json(&url, &json!({"model": ""})).status, 400);
    }

    #[test]
    fn it_can_be_made_to_hang_up_fail_pose_as_something_else_and_go_away() {
        let fake = FakeOllama::start();
        fake.behave(|b| {
            b.fail_next.insert("/api/version".into(), (500, 1));
            b.hang_up_next.insert("/api/tags".into(), 1);
        });
        let v = format!("{}/api/version", fake.base());
        assert_eq!(get(&v).status, 500);
        assert_eq!(
            get(&v).status,
            200,
            "only the first request was made to fail"
        );
        assert!(try_get(&format!("{}/api/tags", fake.base())).is_err());

        fake.behave(|b| b.identity = Identity::LlamaServer);
        assert_eq!(get(&v).body, r#"{"status":"ok"}"#);
        fake.behave(|b| b.identity = Identity::WebServer);
        assert!(get(&v).body.contains("<html>"));

        let port = fake.port();
        fake.go_down();
        crate::testkit::wait_until(Duration::from_secs(3), "the port to close", || {
            std::net::TcpStream::connect(("127.0.0.1", port)).is_err()
        });
    }

    #[test]
    fn a_cut_stream_is_an_error_to_the_reader() {
        let fake = FakeOllama::start();
        fake.add_model("m", 1);
        fake.behave(|b| b.cut_chat_after_lines = Some(2));
        let r = ureq::post(format!("{}/api/chat", fake.base()))
            .header("Content-Type", "application/json")
            .send(json!({"model":"m","messages":[]}).to_string())
            .unwrap();
        let mut text = String::new();
        let outcome = r.into_body().into_reader().read_to_string(&mut text);
        assert!(outcome.is_err(), "{text:?}");
        assert_eq!(text.lines().count(), 2);
    }
}
