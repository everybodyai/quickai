//! A conversation with the model that runs, in a tab of the dock: the quickest way to try a model, with nothing to
//! install and no browser. It speaks the OpenAI chat API that llama-server and Ollama both serve, on this machine's
//! own address (the public port, so the live traffic shows it too when that is on; a tunnel's, for a model that runs on
//! another computer), and the reply is shown as it streams in, with how fast it came.
//!
//! What is the person's to set: instructions (a system message), the temperature and the length of a reply, each left
//! to the model unless set. The last reply can be asked for again and the last message changed and sent again. Nothing
//! is kept unless "Keep chats" is on (WP4.28): then each conversation is a file of this computer (`chats.rs`), listed
//! beside the conversation, opened, deleted and exported as Markdown.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use eframe::egui;

use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::chats::{self, ChatSummary, SavedChat, SavedTurn};
use crate::events::ServerState;

/// Where the height the chat's box took when it was last drawn is kept (egui's data), for the room the page leaves it
/// and for how high the log under the page may be dragged.
pub(super) const CHAT_BOX_HEIGHT: &str = "chat_box_height";

/// One message of the conversation, the pictures sent with it, and for a reply, how fast it came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub from_person: bool,
    pub text: String,
    pub stats: Option<Stats>,
    pub pictures: Vec<Picture>,
    /// For a reply, the tools the model asked to use (C204), and what came of each.
    pub calls: Vec<ToolCall>,
}

impl Turn {
    pub fn said(from_person: bool, text: impl Into<String>) -> Turn {
        Turn {
            from_person,
            text: text.into(),
            stats: None,
            pictures: Vec::new(),
            calls: Vec::new(),
        }
    }
}

/// A tool the model asked to use in a reply (C204): the name it gave it (and the server and tool that is), with what, and
/// what came of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The server and its tool; the server is empty for a name the model was not offered.
    pub server: String,
    pub tool: String,
    /// What the model gave it, as JSON text.
    pub arguments: String,
    pub state: CallState,
    /// What it was given and gave back is shown under it.
    pub open: bool,
    /// The ticket of its run, once it runs.
    pub ticket: Option<u64>,
}

/// What came of a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallState {
    /// It waits for the person to allow it, or not.
    Asking,
    Running,
    /// It ran: what it gave back.
    Done(String),
    /// It could not run, or said it failed: why.
    Failed(String),
    /// The person did not allow it.
    Denied,
}

impl ToolCall {
    /// It ran, failed or was not allowed: the model can be told.
    pub fn settled(&self) -> bool {
        matches!(
            self.state,
            CallState::Done(_) | CallState::Failed(_) | CallState::Denied
        )
    }

    /// What the model is told came of it.
    pub fn answer(&self) -> String {
        match &self.state {
            CallState::Done(text) => text.clone(),
            CallState::Failed(why) => format!("The tool failed: {why}"),
            CallState::Denied => "The person did not allow this tool to run.".to_string(),
            CallState::Asking | CallState::Running => "The tool did not run.".to_string(),
        }
    }
}

/// A tool call as the stream gave it: its id, the name of the tool, and its arguments as JSON text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskedCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A picture sent with a message: its name, its kind (`image/png`) and its bytes in base64.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub mime: String,
    pub base64: String,
}

/// The largest picture and text file a message takes.
const MOST_PICTURE: u64 = 12 * 1024 * 1024;
const MOST_TEXT: u64 = 256 * 1024;

/// The kinds of picture a message takes, by ending.
fn picture_kind(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let ending = lower.rsplit_once('.')?.1;
    Some(match ending {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    })
}

/// Bytes as base64 (RFC 4648, with padding).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What a file given to a message is: a picture to send with it, or text to put in it (as a block under its name).
#[derive(Debug, PartialEq, Eq)]
pub enum Given {
    Picture(Picture),
    Text(String),
}

/// Reads a file the person gave a message, or says why it cannot go with it.
pub fn read_given(path: &std::path::Path) -> Result<Given, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let size = std::fs::metadata(path)
        .map_err(|e| format!("Could not read {name}: {e}"))?
        .len();
    if let Some(mime) = picture_kind(&name) {
        if size > MOST_PICTURE {
            return Err(format!("{name} is too big to send (12 MB at most)."));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read {name}: {e}"))?;
        return Ok(Given::Picture(Picture {
            name,
            mime: mime.to_string(),
            base64: base64(&bytes),
        }));
    }
    if size > MOST_TEXT {
        return Err(format!(
            "{name} is too long to put in a message (256 KB of text at most)."
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {name}: {e}"))?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Given::Text(format!(
            "{name}:\n```\n{}\n```",
            text.trim_end()
        ))),
        Err(_) => Err(format!(
            "{name} is not text or a picture (PNG, JPEG, WebP, GIF): it cannot go with a message."
        )),
    }
}

/// How a reply came: the pieces of it the engine sent (about a token each), the time to the first, and the whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub tokens: usize,
    pub first_ms: u64,
    pub total_ms: u64,
}

impl Stats {
    /// "120 tokens · 34.2 tokens/s · first word in 0.4 s" (the rate is of the time after the first word).
    pub fn line(&self) -> String {
        let streaming_ms = self.total_ms.saturating_sub(self.first_ms).max(1);
        let rate = (self.tokens.saturating_sub(1)) as f64 * 1000.0 / streaming_ms as f64;
        format!(
            "{} tokens · {rate:.1} tokens/s · first word in {:.1} s",
            self.tokens,
            self.first_ms as f64 / 1000.0
        )
    }
}

/// What the reply that is coming says, as it comes.
#[derive(Debug, PartialEq, Eq)]
pub enum ChatEvent {
    Text(String),
    /// A piece of the model's thinking came (`reasoning_content`, or Ollama's `reasoning`): it is not shown, but the
    /// reply is on its way.
    Thinking,
    /// The reply asks to use tools (C204): they come before its end.
    Calls(Vec<AskedCall>),
    /// Something to say under the conversation: the engine did not take the tools, and the reply came without them.
    Notice(String),
    Done,
    Failed(String),
}

/// What a reply is asked with besides the conversation; `None` leaves it to the model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub instructions: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

/// The conversation of the tab, and the reply that is coming.
#[derive(Default)]
pub struct Chat {
    pub turns: Vec<Turn>,
    pub input: String,
    reply: Option<Receiver<ChatEvent>>,
    cancel: Option<Arc<AtomicBool>>,
    /// When the reply that is coming was asked for, and when its first piece came.
    started: Option<Instant>,
    first: Option<Instant>,
    pieces: usize,
    /// When the model began to think before its reply, while it has said no word of it yet.
    thinking: Option<Instant>,
    /// Why the last reply did not come, in the engine's words when it gave some.
    pub error: Option<String>,
    /// The tab has been opened since the application started: the dot that points to it is not drawn again.
    pub opened: bool,
    /// What the model is told before the conversation (a system message), when the person wrote something.
    pub instructions: String,
    /// The temperature and the longest reply, when the person set them.
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// The window of the chat's options is open (C208: a window of its own, as Settings and Help are).
    pub show_instructions: bool,
    /// The conversation is kept on this computer (the person's choice, in the settings), under this id once it has
    /// a message.
    pub keep: bool,
    pub saved_id: Option<String>,
    /// The kept conversations, as the list beside the conversation shows them.
    pub kept: Vec<ChatSummary>,
    /// The pictures that go with the next message.
    pub pictures: Vec<Picture>,
    /// The tools offered with the reply that is coming (C204): the name the model knows each by, to its server and tool.
    pub tool_names: BTreeMap<String, (String, String)>,
    /// How many replies in a row asked for tools since the person's last message.
    pub tool_rounds: u32,
    /// The calls of the last reply wait to be settled: then the model is asked to go on.
    pub tools_waiting: bool,
    /// The message waits for the MCP servers to start before the reply is asked for.
    pub waiting_for_tools: bool,
    /// Said under the conversation (the engine did not take the tools).
    pub notice: Option<String>,
}

impl Chat {
    pub fn replying(&self) -> bool {
        self.reply.is_some()
    }

    /// A new conversation that keeps what is the person's and not the engine's: the options, whether chats are kept,
    /// and that the tab was found.
    pub fn carried_over(&mut self) -> Chat {
        Chat {
            opened: self.opened,
            instructions: std::mem::take(&mut self.instructions),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            show_instructions: self.show_instructions,
            keep: self.keep,
            kept: std::mem::take(&mut self.kept),
            ..Chat::default()
        }
    }

    /// Stops the reply that is coming; what came of it stays. A reply that had said nothing yet is not kept as a turn.
    pub fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        if self.reply.take().is_some()
            && self
                .turns
                .last()
                .is_some_and(|t| !t.from_person && t.text.is_empty() && t.calls.is_empty())
        {
            self.turns.pop();
        }
        self.thinking = None;
        self.waiting_for_tools = false;
        self.tools_waiting = false;
        // a call that waits for the person, or runs, is left as it is: nothing is sent of it
        for call in self
            .turns
            .iter_mut()
            .flat_map(|t| t.calls.iter_mut())
            .filter(|c| !c.settled())
        {
            call.state = CallState::Denied;
        }
    }

    fn options(&self) -> Options {
        Options {
            instructions: Some(self.instructions.clone()).filter(|i| !i.trim().is_empty()),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
        }
    }

    /// Starts a new conversation: the one there was stays kept when chats are.
    pub fn new_conversation(&mut self) {
        self.stop();
        self.turns.clear();
        self.error = None;
        self.saved_id = None;
        self.notice = None;
        self.tool_rounds = 0;
    }
}

/// Asks the engine on `port` for the reply to `turns`, streamed, on a thread of its own.
#[cfg(test)]
pub fn spawn_reply(
    port: u16,
    model: String,
    options: Options,
    turns: Vec<Turn>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Receiver<ChatEvent>, Arc<AtomicBool>) {
    spawn_reply_with_tools(port, model, options, turns, None, wake)
}

/// `spawn_reply`, offering the model `tools` (the request's `tools`, C204). An engine that refuses the request with them
/// is asked again without them, and that is said.
pub fn spawn_reply_with_tools(
    port: u16,
    model: String,
    options: Options,
    turns: Vec<Turn>,
    tools: Option<serde_json::Value>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Receiver<ChatEvent>, Arc<AtomicBool>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let send = |event: ChatEvent| {
            let _ = tx.send(event);
            wake();
        };
        let said = |piece: Piece| {
            send(match piece {
                Piece::Text(text) => ChatEvent::Text(text),
                Piece::Thinking => ChatEvent::Thinking,
            })
        };
        let mut outcome =
            stream_reply(port, &model, &options, &turns, tools.as_ref(), &stop, &said);
        if tools.is_some() {
            if let Err(failure) = &outcome {
                if failure.refused {
                    send(ChatEvent::Notice(format!(
                        "The engine did not take the tools ({}): this reply is without them.",
                        failure.why
                    )));
                    outcome = stream_reply(port, &model, &options, &turns, None, &stop, &said);
                }
            }
        }
        match outcome {
            Ok(calls) => {
                if !calls.is_empty() {
                    send(ChatEvent::Calls(calls));
                }
                send(ChatEvent::Done);
            }
            Err(failure) => send(ChatEvent::Failed(failure.why)),
        }
    });
    (rx, cancel)
}

/// The body of the request: the model, the instructions as the system's word, the conversation, and what the
/// person set (and nothing else, so that the model's own defaults stand).
#[cfg(test)]
pub fn request_body(model: &str, options: &Options, turns: &[Turn]) -> serde_json::Value {
    request_body_with(model, options, turns, None)
}

/// `request_body`, with the tools offered (C204). A reply that asked for tools is said as it was asked, followed by what
/// came of each call.
pub fn request_body_with(
    model: &str,
    options: &Options,
    turns: &[Turn],
    tools: Option<&serde_json::Value>,
) -> serde_json::Value {
    let system = options
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
        .map(|i| serde_json::json!({"role": "system", "content": i}));
    let messages: Vec<serde_json::Value> = system
        .into_iter()
        .chain(turns.iter().flat_map(|t| {
            let role = if t.from_person { "user" } else { "assistant" };
            if !t.from_person && !t.calls.is_empty() {
                // the reply as it asked for the tools, then what came of each, in the order asked
                let asked: Vec<serde_json::Value> = t
                    .calls
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "id": c.id,
                            "type": "function",
                            "function": {"name": c.name, "arguments": c.arguments},
                        })
                    })
                    .collect();
                let mut said = vec![serde_json::json!({"role": "assistant", "content": t.text, "tool_calls": asked})];
                said.extend(t.calls.iter().map(|c| {
                    serde_json::json!({"role": "tool", "tool_call_id": c.id, "content": c.answer()})
                }));
                return said;
            }
            if t.pictures.is_empty() {
                return vec![serde_json::json!({"role": role, "content": t.text})];
            }
            // the words and the pictures, as the OpenAI chat API takes them (llama-server and Ollama both do)
            let mut content = vec![serde_json::json!({"type": "text", "text": t.text})];
            content.extend(t.pictures.iter().map(|p| {
                serde_json::json!({
                    "type": "image_url",
                    "image_url": {"url": format!("data:{};base64,{}", p.mime, p.base64)},
                })
            }));
            vec![serde_json::json!({"role": role, "content": content})]
        }))
        .collect();
    let mut body = serde_json::json!({"model": model, "messages": messages, "stream": true});
    if let Some(tools) = tools {
        body["tools"] = tools.clone();
    }
    if let Some(t) = options.temperature {
        body["temperature"] = serde_json::json!((f64::from(t) * 100.0).round() / 100.0);
    }
    if let Some(n) = options.max_tokens {
        body["max_tokens"] = serde_json::json!(n);
    }
    body
}

/// What a line of the stream brings: words of the reply, or a piece of the thinking before it.
enum Piece {
    Text(String),
    Thinking,
}

/// The request and its stream of events (`data: {...}` lines, then `data: [DONE]`). An `error` the engine sends,
/// in the body of a refusal or in the stream, is the failure's sentence. A model that thinks first sends its thinking
/// apart from its reply (llama-server as `reasoning_content`, Ollama as `reasoning`): that it thinks is said, so the
/// window can show the reply is on its way.
/// Why a reply did not come: the words, and whether the engine refused the request before any of it came.
struct Failure {
    why: String,
    refused: bool,
}

impl Failure {
    fn on_the_way(why: String) -> Failure {
        Failure {
            why,
            refused: false,
        }
    }
}

fn stream_reply(
    port: u16,
    model: &str,
    options: &Options,
    turns: &[Turn],
    tools: Option<&serde_json::Value>,
    stop: &AtomicBool,
    said: &dyn Fn(Piece),
) -> Result<Vec<AskedCall>, Failure> {
    let body = request_body_with(model, options, turns, tools);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .into();
    let response = agent
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| Failure::on_the_way(format!("could not reach the server: {e}")))?;
    let status = response.status().as_u16();
    let reader = response.into_body().into_reader();
    if !(200..300).contains(&status) {
        let said: serde_json::Value =
            serde_json::from_reader(reader).unwrap_or(serde_json::Value::Null);
        return Err(Failure {
            why: error_words(&said).unwrap_or_else(|| format!("the server answered {status}")),
            refused: true,
        });
    }
    // the tool calls, as they come in pieces: by their index, the id, the name and the arguments so far
    let mut calls: Vec<AskedCall> = Vec::new();
    for line in BufReader::new(reader).lines() {
        if stop.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        let line = line.map_err(|e| Failure::on_the_way(format!("the reply stopped: {e}")))?;
        let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(why) = error_words(&event) {
            return Err(Failure::on_the_way(why));
        }
        gather_calls(&event, &mut calls);
        let delta = |key: &str| {
            event
                .pointer(&format!("/choices/0/delta/{key}"))
                .and_then(serde_json::Value::as_str)
                .filter(|p| !p.is_empty())
        };
        if delta("reasoning_content").or(delta("reasoning")).is_some() {
            said(Piece::Thinking);
        }
        if let Some(piece) = delta("content") {
            said(Piece::Text(piece.to_string()));
        }
        if event
            .pointer("/choices/0/finish_reason")
            .is_some_and(|r| !r.is_null())
        {
            break;
        }
    }
    for (i, call) in calls.iter_mut().enumerate() {
        if call.id.is_empty() {
            call.id = format!("call_{i}");
        }
        if call.arguments.trim().is_empty() {
            call.arguments = "{}".to_string();
        }
    }
    Ok(calls.into_iter().filter(|c| !c.name.is_empty()).collect())
}

/// The pieces of tool calls a line of the stream brings (`choices[0].delta.tool_calls`), put with what came before by
/// their index: the id and the name come once, the arguments in pieces (or whole, as an object, from some engines).
fn gather_calls(event: &serde_json::Value, calls: &mut Vec<AskedCall>) {
    let Some(pieces) = event
        .pointer("/choices/0/delta/tool_calls")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    for (n, piece) in pieces.iter().enumerate() {
        let index = piece
            .get("index")
            .and_then(serde_json::Value::as_u64)
            .map_or(n, |i| i as usize);
        while calls.len() <= index {
            calls.push(AskedCall {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
        }
        let call = &mut calls[index];
        if let Some(id) = piece.get("id").and_then(serde_json::Value::as_str) {
            if !id.is_empty() {
                call.id = id.to_string();
            }
        }
        if let Some(name) = piece
            .pointer("/function/name")
            .and_then(serde_json::Value::as_str)
        {
            call.name.push_str(name);
        }
        match piece.pointer("/function/arguments") {
            Some(serde_json::Value::String(more)) => call.arguments.push_str(more),
            Some(whole @ serde_json::Value::Object(_)) => call.arguments = whole.to_string(),
            _ => {}
        }
    }
}

/// The parts of a message: text, and code between lines of three backticks (```` ```rust ````), which is set in
/// monospace on a ground of its own. A fence that is not closed yet (a reply that is still coming) is code to its end.
pub fn parts(text: &str) -> Vec<(bool, String)> {
    let mut parts: Vec<(bool, String)> = Vec::new();
    let mut code = false;
    let mut current = String::new();
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            if !current.is_empty() {
                parts.push((code, std::mem::take(&mut current)));
            }
            code = !code;
            continue;
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        parts.push((code, current));
    }
    parts
        .into_iter()
        .map(|(code, text)| (code, text.trim_end_matches('\n').to_string()))
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

/// A line of text as Markdown has it: a heading (and its level), an item of a list (and its mark), a row of a table, a
/// quote, or a plain line.
#[derive(Debug, PartialEq, Eq)]
pub enum Line<'a> {
    Heading(usize, &'a str),
    Item(String, &'a str),
    Table(&'a str),
    Quote(&'a str),
    Plain(&'a str),
}

pub fn line_kind(line: &str) -> Line<'_> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
        return Line::Heading(hashes, trimmed[hashes..].trim());
    }
    for mark in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(mark) {
            return Line::Item("•".into(), rest);
        }
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 4 && trimmed[digits..].starts_with(". ") {
        return Line::Item(trimmed[..=digits].to_string(), &trimmed[digits + 2..]);
    }
    if trimmed.starts_with('|') {
        return Line::Table(line);
    }
    if let Some(rest) = trimmed.strip_prefix("> ") {
        return Line::Quote(rest);
    }
    Line::Plain(line)
}

/// The spans of a line: plain words, **strong** words and `code`, in order.
pub fn spans(text: &str) -> Vec<(Span, String)> {
    let mut out: Vec<(Span, String)> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next_code = rest.find('`');
        let next_bold = rest.find("**");
        match (next_code, next_bold) {
            (Some(c), b) if b.is_none_or(|b| c < b) => {
                if let Some(end) = rest[c + 1..].find('`') {
                    if c > 0 {
                        out.push((Span::Plain, rest[..c].to_string()));
                    }
                    out.push((Span::Code, rest[c + 1..c + 1 + end].to_string()));
                    rest = &rest[c + 2 + end..];
                    continue;
                }
                out.push((Span::Plain, rest.to_string()));
                break;
            }
            (_, Some(b)) => {
                if let Some(end) = rest[b + 2..].find("**") {
                    if b > 0 {
                        out.push((Span::Plain, rest[..b].to_string()));
                    }
                    out.push((Span::Strong, rest[b + 2..b + 2 + end].to_string()));
                    rest = &rest[b + 4 + end..];
                    continue;
                }
                out.push((Span::Plain, rest.to_string()));
                break;
            }
            _ => {
                out.push((Span::Plain, rest.to_string()));
                break;
            }
        }
    }
    out
}

/// What a span of a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Plain,
    Strong,
    Code,
}

/// One line of text with its spans, in the size and colour given.
fn show_spans(ui: &mut egui::Ui, text: &str, size: Option<f32>, strong: bool) {
    let p = theme::palette();
    let mut job = egui::text::LayoutJob::default();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    for (span, words) in spans(text) {
        let mut font = match span {
            Span::Code => mono.clone(),
            _ => body.clone(),
        };
        if let Some(size) = size {
            font.size = size;
        }
        let mut format = egui::TextFormat {
            font_id: font,
            color: if strong || span == Span::Strong {
                p.text
            } else {
                p.body
            },
            ..Default::default()
        };
        if span == Span::Code {
            format.background = p.log;
        }
        if strong || span == Span::Strong {
            // a stronger weight of the same face, as the panels' titles have
            format.font_id.family = theme::semibold(ui);
        }
        job.append(&words, 0.0, format);
    }
    job.wrap.max_width = ui.available_width();
    // wrapped whatever the layout it is in: a row (an item of a list, a quote) would let it run on out of the bubble
    // and widen the page
    ui.add(egui::Label::new(job).selectable(true).wrap());
}

/// One message: its lines as Markdown has them, and its code blocks in monospace.
fn show_message(ui: &mut egui::Ui, text: &str) {
    let p = theme::palette();
    for (code, part) in parts(text) {
        if code {
            egui::Frame::new()
                .fill(p.log)
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(part).monospace().color(p.text))
                            .selectable(true)
                            .wrap(),
                    );
                });
            continue;
        }
        for line in part.lines() {
            match line_kind(line) {
                Line::Heading(level, words) => {
                    ui.add_space(2.0);
                    let size = match level {
                        1 => 19.0,
                        2 => 17.0,
                        _ => 15.0,
                    };
                    show_spans(ui, words, Some(size), true);
                }
                Line::Item(mark, words) => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(mark).color(p.muted));
                        show_spans(ui, words, None, false);
                    });
                }
                Line::Table(row) => {
                    // a row of a table, its cells where the person can read them across
                    if !row.trim().trim_matches(['|', '-', ':', ' ']).is_empty() {
                        ui.label(egui::RichText::new(row.trim()).monospace().color(p.body));
                    }
                }
                Line::Quote(words) => {
                    ui.horizontal_top(|ui| {
                        ui.label(egui::RichText::new("▎").color(p.muted));
                        show_spans(ui, words, None, false);
                    });
                }
                Line::Plain(words) if words.trim().is_empty() => {
                    ui.add_space(4.0);
                }
                Line::Plain(words) => show_spans(ui, words, None, false),
            }
        }
    }
}

/// The sentence of an `error` member: `{"error": "..."}` or `{"error": {"message": "..."}}`.
fn error_words(value: &serde_json::Value) -> Option<String> {
    let error = value.get("error")?;
    error
        .as_str()
        .or_else(|| error.get("message").and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

impl App {
    /// Whether the engine in use can be talked to here: it serves the OpenAI chat API. On a computer QuickAI set up, the
    /// chat is there until its run says it serves no API.
    pub(super) fn can_chat(&self) -> bool {
        if self.setup_in_use().is_some() {
            return self
                .setup_run()
                .is_none_or(|r| !r.running() || r.endpoint("api").is_some());
        }
        self.engine_doc()
            .is_some_and(|e| e.doc.provides.apis.iter().any(|a| a == "openai.chat"))
    }

    /// The chat talks to a run now: the one here, or the one on the computer QuickAI set up that is shown.
    pub(super) fn chat_running(&self) -> bool {
        if self.setup_in_use().is_some() {
            return self.setup_run().is_some_and(|r| r.running());
        }
        self.session.state() == ServerState::Running
    }

    /// The model the chat talks to: the one loaded here, or the one the run on the computer shown runs.
    fn chat_model(&self) -> Option<String> {
        if self.setup_in_use().is_some() {
            // as the list of that computer names it (`Qwen3.5 9B (Q4_K_M)`), not by its file
            return self
                .setup_run()
                .and_then(|r| r.chat_target())
                .map(|(_, model)| model)
                .filter(|m| !m.is_empty())
                .map(|m| self.model_there_name(&m));
        }
        self.session.model_name().map(str::to_string)
    }

    fn data_dir(&self) -> &Path {
        Path::new(&self.host_facts.data_dir)
    }

    /// Sends what is in the box, with the conversation so far, to the model that runs.
    pub(super) fn send_chat(&mut self, ctx: &egui::Context) {
        let text = self.chat.input.trim().to_string();
        if text.is_empty() || self.chat.replying() {
            return;
        }
        if self.chat_target().is_none() {
            self.chat.error = Some(match self.setup_in_use() {
                Some(spec) => format!(
                    "Start \"{}\" on {} to chat with it.",
                    self.selected_configuration_name(),
                    spec.name
                ),
                None => format!("Start {} with a model to chat with it.", self.server_name()),
            });
            return;
        }
        self.chat.input.clear();
        let mut turn = Turn::said(true, text);
        turn.pictures = std::mem::take(&mut self.chat.pictures);
        self.chat.turns.push(turn);
        self.chat.tool_rounds = 0;
        // the MCP servers that are on start with the first message, and the reply waits for them (C204)
        if !self.tools.started && self.mcp_list().iter().any(|s| !s.disabled) {
            self.start_tools(ctx);
        }
        if self.tools_starting() {
            self.chat.waiting_for_tools = true;
            self.chat.error = None;
        } else {
            self.ask_for_reply(ctx);
        }
    }

    /// Where a reply is asked for: the port of the run, and the model that is loaded. With a computer QuickAI set up
    /// shown, its run's API here, through the tunnel.
    fn chat_target(&self) -> Option<(u16, String)> {
        if self.setup_in_use().is_some() {
            return self.setup_run()?.chat_target();
        }
        let port = self
            .session
            .port()
            .filter(|_| self.session.state() == ServerState::Running)?;
        Some((port, self.session.model_name()?.to_string()))
    }

    /// Asks for the reply to the conversation as it stands, offering the tools of the MCP servers that are ready.
    pub(super) fn ask_for_reply(&mut self, ctx: &egui::Context) {
        let Some((port, model)) = self.chat_target() else {
            return;
        };
        self.chat.error = None;
        let (tools, names) = self.offered_tools();
        self.chat.tool_names = names;
        let ctx = ctx.clone();
        let (rx, cancel) = spawn_reply_with_tools(
            port,
            model,
            self.chat.options(),
            self.chat.turns.clone(),
            tools,
            move || ctx.request_repaint(),
        );
        self.chat.turns.push(Turn::said(false, ""));
        self.chat.reply = Some(rx);
        self.chat.cancel = Some(cancel);
        self.chat.started = Some(Instant::now());
        self.chat.first = None;
        self.chat.pieces = 0;
        self.chat.thinking = None;
    }

    /// A file the person gave the next message: a picture goes with it, text goes in it.
    pub(super) fn attach_to_message(&mut self, path: &Path) {
        match read_given(path) {
            Ok(Given::Picture(picture)) => self.chat.pictures.push(picture),
            Ok(Given::Text(block)) => {
                if !self.chat.input.trim().is_empty() {
                    self.chat.input.push_str("\n\n");
                }
                self.chat.input.push_str(&block);
            }
            Err(why) => self.chat.error = Some(why),
        }
    }

    /// Whether the model that runs says it sees pictures: its engine lists `vision`, or its file came with a projector.
    fn model_sees_pictures(&self) -> bool {
        let Some(name) = self.session.model_name() else {
            return false;
        };
        self.models
            .iter()
            .find(|m| m.display == name || m.filename == name)
            .is_some_and(|m| {
                m.note.split(", ").any(|w| w == "vision")
                    || m.companions
                        .iter()
                        .any(|c| c.kind == "mmproj" && c.exists())
            })
    }

    /// The last reply, asked for again.
    pub(super) fn regenerate_chat(&mut self, ctx: &egui::Context) {
        if self.chat.replying() || !self.chat.turns.last().is_some_and(|t| !t.from_person) {
            return;
        }
        self.chat.turns.pop();
        self.ask_for_reply(ctx);
    }

    /// The last message back in the box to be changed, and what came of it taken away.
    pub(super) fn edit_last_message(&mut self) {
        if self.chat.replying() {
            return;
        }
        if let Some(at) = self.chat.turns.iter().rposition(|t| t.from_person) {
            let turn = self.chat.turns.remove(at);
            self.chat.turns.truncate(at);
            self.chat.input = turn.text;
            self.chat.error = None;
        }
    }

    /// Each frame: what the reply has said since, and its end. A server that stopped ends it.
    pub(super) fn poll_chat(&mut self) {
        // the run the chat talks to stopped: the one here, or the one on the computer QuickAI set up (where nothing
        // runs here, so this computer's server being stopped says nothing of it)
        if self.chat.reply.is_some() && !self.chat_running() {
            self.chat.stop();
        }
        let Some(rx) = &self.chat.reply else {
            return;
        };
        let mut ended = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                ChatEvent::Thinking => {
                    if self.chat.first.is_none() && self.chat.thinking.is_none() {
                        self.chat.thinking = Some(Instant::now());
                    }
                }
                ChatEvent::Text(piece) => {
                    if self.chat.first.is_none() {
                        self.chat.first = Some(Instant::now());
                    }
                    self.chat.thinking = None;
                    self.chat.pieces += 1;
                    if let Some(last) = self.chat.turns.last_mut().filter(|t| !t.from_person) {
                        last.text.push_str(&piece);
                    }
                }
                ChatEvent::Calls(asked) => {
                    let calls: Vec<ToolCall> = asked
                        .into_iter()
                        .map(|a| {
                            let (server, tool) = self
                                .chat
                                .tool_names
                                .get(&a.name)
                                .cloned()
                                .unwrap_or_default();
                            ToolCall {
                                id: a.id,
                                name: a.name,
                                server,
                                tool,
                                arguments: a.arguments,
                                state: CallState::Asking,
                                open: false,
                                ticket: None,
                            }
                        })
                        .collect();
                    if let Some(last) = self.chat.turns.last_mut().filter(|t| !t.from_person) {
                        last.calls = calls;
                    }
                }
                ChatEvent::Notice(said) => self.chat.notice = Some(said),
                ChatEvent::Done => ended = true,
                ChatEvent::Failed(why) => {
                    self.chat.error = Some(why);
                    ended = true;
                }
            }
        }
        if ended {
            self.chat.reply = None;
            self.chat.cancel = None;
            // a model that only thought ran out of room before its answer: an empty reply would be dropped unsaid
            if let Some(since) = self.chat.thinking.take() {
                let nothing = self
                    .chat
                    .turns
                    .last()
                    .is_some_and(|t| !t.from_person && t.text.is_empty() && t.calls.is_empty());
                if nothing && self.chat.error.is_none() {
                    self.chat.error = Some(format!(
                        "It thought for {} s and stopped before it answered. Allow longer answers in Options… \
                         (⋯ at the right of the bar), then send again.",
                        since.elapsed().as_secs()
                    ));
                }
            }
            let stats = match (self.chat.started, self.chat.first) {
                (Some(started), Some(first)) => Some(Stats {
                    tokens: self.chat.pieces,
                    first_ms: first.duration_since(started).as_millis() as u64,
                    total_ms: started.elapsed().as_millis() as u64,
                }),
                _ => None,
            };
            match self.chat.turns.last_mut() {
                // a reply that said nothing (and asked for no tool) is not kept as a turn
                Some(last)
                    if !last.from_person && last.text.is_empty() && last.calls.is_empty() =>
                {
                    self.chat.turns.pop();
                }
                Some(last) if !last.from_person => last.stats = stats,
                _ => {}
            }
            // the tools it asked for: those allowed always run, the others wait for the person (C204)
            self.settle_new_calls();
            self.keep_conversation();
            // a first reply is the last step of Getting started, remembered once
            if self.chat.error.is_none()
                && self.chat.turns.last().is_some_and(|t| !t.from_person)
                && !self.stored.said_hello()
            {
                let mut stored = self.settings_now();
                stored.set_said_hello();
                self.write_settings(&stored);
                self.stored = stored;
            }
        }
    }

    // ---- keeping the conversations --------------------------------------------------------------------------

    /// Turns keeping chats on or off, and writes the choice down. Off deletes nothing: what was kept stays kept.
    pub(super) fn set_keep_chats(&mut self, keep: bool) {
        self.chat.keep = keep;
        let mut stored = self.settings_now();
        stored.set_keep_chats(keep);
        self.write_settings(&stored);
        self.stored = stored;
        if keep {
            self.keep_conversation();
        }
        self.reload_kept_chats();
    }

    pub(super) fn reload_kept_chats(&mut self) {
        self.chat.kept = if self.chat.keep {
            chats::list(self.data_dir())
        } else {
            Vec::new()
        };
    }

    /// Writes the conversation down when chats are kept and it has a message.
    fn keep_conversation(&mut self) {
        if !self.chat.keep || self.chat.turns.is_empty() {
            return;
        }
        let data = self.data_dir().to_path_buf();
        let id = self
            .chat
            .saved_id
            .clone()
            .unwrap_or_else(|| chats::new_id(&data));
        let turns: Vec<SavedTurn> = self
            .chat
            .turns
            .iter()
            .map(|t| SavedTurn {
                from_person: t.from_person,
                text: t.text.clone(),
            })
            .collect();
        let created = chats::load(&data, &id).map_or_else(chats::now, |c| c.created);
        let saved = SavedChat {
            title: chats::title_of(&turns),
            id: id.clone(),
            model: self.chat_model().unwrap_or_default(),
            engine: self.engine_name(),
            runner: self.runner_name(),
            instructions: self.chat.instructions.clone(),
            created,
            updated: chats::now(),
            turns,
        };
        match chats::save(&data, &saved) {
            Ok(()) => self.chat.saved_id = Some(id),
            Err(e) => self.push_log(format!("Could not keep the chat: {e}")),
        }
        self.reload_kept_chats();
    }

    /// Opens a kept conversation in place of the one there is.
    pub(super) fn open_kept_chat(&mut self, id: &str) {
        let Some(saved) = chats::load(self.data_dir(), id) else {
            return;
        };
        self.chat.stop();
        self.chat.turns = saved
            .turns
            .into_iter()
            .map(|t| Turn::said(t.from_person, t.text))
            .collect();
        self.chat.instructions = saved.instructions;
        self.chat.saved_id = Some(saved.id);
        self.chat.error = None;
    }

    pub(super) fn delete_kept_chat(&mut self, id: &str) {
        if let Err(e) = chats::delete(self.data_dir(), id) {
            self.push_log(format!("Could not delete the chat: {e}"));
        }
        if self.chat.saved_id.as_deref() == Some(id) {
            self.chat.new_conversation();
        }
        self.reload_kept_chats();
    }

    /// Writes the conversation as Markdown where the person says.
    fn export_chat(&mut self) {
        let turns: Vec<SavedTurn> = self
            .chat
            .turns
            .iter()
            .map(|t| SavedTurn {
                from_person: t.from_person,
                text: t.text.clone(),
            })
            .collect();
        let chat = SavedChat {
            id: String::new(),
            title: chats::title_of(&turns),
            model: self.chat_model().unwrap_or_default(),
            engine: self.engine_name(),
            runner: self.runner_name(),
            instructions: self.chat.instructions.clone(),
            created: 0,
            updated: 0,
            turns,
        };
        let name = format!("{}.md", crate::remote::slug(&chat.title));
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name(name)
            .save_file()
        {
            match std::fs::write(&path, chats::markdown(&chat)) {
                Ok(()) => self.push_log(format!("Chat exported to {}", path.display())),
                Err(e) => {
                    self.chat.error = Some(format!("Could not write {}: {e}", path.display()))
                }
            }
        }
    }

    // ---- what is drawn ------------------------------------------------------------------------------------

    /// The tab: the kept chats (when they are kept), the conversation, and under it the box and the button that sends
    /// it.
    pub(super) fn show_chat_tab(&mut self, ui: &mut egui::Ui) {
        if self.chat.keep && self.chat.kept.is_empty() && self.chat.saved_id.is_none() {
            self.reload_kept_chats();
        }
        if self.chat.keep {
            egui::Panel::left("chat_list")
                .resizable(true)
                .default_size(180.0)
                .size_range(140.0..=320.0)
                .frame(egui::Frame::new().fill(theme::palette().bar))
                .show(ui, |ui| self.show_kept_chats(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::palette().page))
            .show(ui, |ui| self.show_chat_conversation(ui));
    }

    fn show_kept_chats(&mut self, ui: &mut egui::Ui) {
        let mut open: Option<String> = None;
        let mut delete: Option<String> = None;
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("kept_chats")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.chat.kept.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(8.0);
                        ui.label(theme::muted("Chats are kept here once they have a reply."));
                    });
                }
                for c in &self.chat.kept {
                    let on = self.chat.saved_id.as_deref() == Some(c.id.as_str());
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        let row = ui
                            .add_enabled(
                                !self.chat.replying(),
                                egui::Button::selectable(on, c.title.as_str()).truncate(),
                            )
                            .on_hover_text(c.title.as_str());
                        if row.clicked() {
                            open = Some(c.id.clone());
                        }
                        row.context_menu(|ui| {
                            if ui.button("Delete").clicked() {
                                delete = Some(c.id.clone());
                                ui.close();
                            }
                        });
                    });
                }
            });
        if let Some(id) = open {
            self.open_kept_chat(&id);
        }
        if let Some(id) = delete {
            self.delete_kept_chat(&id);
        }
    }

    /// The chat's part of the bar's ⋯ menu (UI v3, C203): Keep chats, Options and Export. New chat is the bar's pencil.
    pub(super) fn chat_menu_items(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        // Keep chats: its picture, its words and a switch, the row as tall as the menu's items
        let mut keep = self.chat.keep;
        let row = ui
            .horizontal(|ui| {
                // laid out as the menu's other items are: the picture 8 points in, the words 10 after it
                ui.set_min_height(30.0);
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(8.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                super::icons::paint(ui.painter(), rect, super::icons::Icon::Document, p.body);
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Keep chats").color(p.body));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    theme::switch(ui, &mut keep, true)
                })
                .inner
            })
            .inner;
        if row
            .on_hover_text(
                "Keep each conversation on this computer (in QuickAI's own folder), listed beside it. Off, nothing is \
                 kept. Nothing leaves this computer either way.",
            )
            .changed()
        {
            self.set_keep_chats(keep);
        }
        let set = !self.chat.instructions.trim().is_empty()
            || self.chat.temperature.is_some()
            || self.chat.max_tokens.is_some();
        let options = super::icons::menu_item_keyed(
            ui,
            Some(super::icons::Icon::Sliders),
            if set { "Options… •" } else { "Options…" },
            "",
        )
        .on_hover_text(
            "Instructions the model reads before every message, how free its answers are, and how long they may be, in \
             a window of their own. Each is left to the model unless set.",
        );
        if options.clicked() {
            self.open_chat_options();
            ui.close();
        }
        let can_export = !self.chat.turns.is_empty() && !self.chat.replying();
        let export = super::icons::menu_item(
            ui,
            super::icons::Icon::Upload,
            "Export this chat…",
            can_export,
            false,
        )
        .on_hover_text("Save this conversation as a Markdown file.");
        if export.clicked() && can_export {
            ui.close();
            self.export_chat();
        }
    }

    /// Opens the window of the chat's options, or brings it to the front.
    pub(super) fn open_chat_options(&mut self) {
        self.raise_chat_options = self.chat.show_instructions;
        self.chat.show_instructions = true;
    }

    /// Whether a new chat can be begun: there is one to leave, and no reply is on its way.
    pub(super) fn can_begin_new_chat(&self) -> bool {
        !self.chat.turns.is_empty() && !self.chat.replying()
    }

    /// What the bar's New chat says when the pointer rests on it.
    pub(super) fn new_chat_hover(&self) -> &'static str {
        if self.chat.keep {
            "New chat: this one stays in the list."
        } else {
            "New chat. This one is not kept."
        }
    }

    /// Begins a new conversation.
    pub(super) fn begin_new_chat(&mut self) {
        self.chat.new_conversation();
    }

    fn show_chat_conversation(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let running = self.chat_running();
        let model = self.chat_model();
        let server = self.server_name();
        let remote_run = self.setup_in_use().map(|spec| {
            (
                spec.name.clone(),
                self.setup_run()
                    .map(|r| r.configuration.clone())
                    .filter(|c| !c.is_empty())
                    .unwrap_or_else(|| self.selected_configuration_name()),
            )
        });
        // with nothing said yet, what to do is said in the middle of the room the conversation will have
        let empty = self.chat.turns.is_empty() && self.chat.error.is_none();
        let empty_words = match (&model, running) {
            // a run on a computer QuickAI set up
            _ if remote_run.is_some() => {
                let (computer, configuration) = remote_run.clone().unwrap_or_default();
                match (&model, running) {
                    (Some(model), true) => {
                        format!("Ask {model} anything. It runs on {computer}.")
                    }
                    (None, true) => {
                        format!("Ask it anything. \"{configuration}\" runs on {computer}.")
                    }
                    _ => format!("The chat opens once \"{configuration}\" runs on {computer}."),
                }
            }
            (Some(model), true) => match self.remote_in_use() {
                Some(remote) => format!("Ask {model} anything. It runs on {}.", remote.name),
                None => format!("Ask {model} anything."),
            },
            (None, true) => {
                format!("{server} runs with no model loaded: choose one, then Load model.")
            }
            _ => match self.models.get(self.selected) {
                Some(m) => format!("Press Start to chat with {}.", m.display),
                None => format!("Start {server} with a model to chat with it here."),
            },
        };
        // the room under the conversation is what the box took the last time it was drawn, with the files to go with the
        // next message over it and the lines of a long message in it: a height of its own would let a taller box run
        // under the log
        let box_id = egui::Id::new(CHAT_BOX_HEIGHT);
        let box_height = ui.data(|d| d.get_temp::<f32>(box_id)).unwrap_or(64.0);
        let room = (ui.available_height() - box_height).max(40.0);
        let mut regenerate = false;
        let mut edit = false;
        // what was pressed on the card of a tool the model asked for: the turn, the call, and what
        let mut card: Option<(usize, usize, super::tools::CardAction)> = None;
        let asker = model.clone().unwrap_or_else(|| "The model".to_string());
        let last_reply = self.chat.turns.iter().rposition(|t| !t.from_person);
        let last_message = self.chat.turns.iter().rposition(|t| t.from_person);
        if empty {
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), room), egui::Sense::hover());
            let centre = rect.center();
            // the picture only where there is room for it over the words
            if room > 120.0 {
                icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        centre - egui::vec2(0.0, 22.0),
                        egui::vec2(34.0, 34.0),
                    ),
                    Icon::Chat,
                    p.faint,
                );
            }
            let galley = ui.painter().layout(
                empty_words,
                egui::TextStyle::Body.resolve(ui.style()),
                p.muted,
                (rect.width() - 40.0).max(100.0),
            );
            ui.painter().galley(
                egui::pos2(centre.x - galley.size().x / 2.0, centre.y + 6.0),
                galley,
                p.muted,
            );
        }
        // (with nothing said, no scroll area: it would keep a least height of its own and push the box out)
        let turns_area = (!empty).then(|| {
            egui::ScrollArea::vertical()
                .id_salt("chat_turns")
                .max_height(room)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
        });
        if let Some(area) = turns_area {
            area.show(ui, |ui| {
                ui.add_space(4.0);
                let width = ui.available_width();
                for (i, turn) in self.chat.turns.iter().enumerate() {
                    let typing =
                        !turn.from_person && self.chat.replying() && i + 1 == self.chat.turns.len();
                    let text = if typing && turn.text.is_empty() {
                        match self.chat.thinking {
                            // a model that thinks first: the reply is on its way, and for how long it has been
                            Some(since) => {
                                ui.ctx().request_repaint_after(Duration::from_secs(1));
                                format!("Thinking… {} s", since.elapsed().as_secs())
                            }
                            None => "…".to_string(),
                        }
                    } else {
                        turn.text.clone()
                    };
                    let bubble = egui::Frame::new()
                        .fill(if turn.from_person {
                            p.accent_soft
                        } else {
                            p.card
                        })
                        .stroke(egui::Stroke::new(1.0, p.line))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(10, 6));
                    let with = (!turn.pictures.is_empty()).then(|| {
                        let names: Vec<&str> =
                            turn.pictures.iter().map(|p| p.name.as_str()).collect();
                        format!("With: {}", names.join(", "))
                    });
                    if turn.from_person {
                        // what the person wrote is as they wrote it, at the right, in a bubble as wide as its words
                        // (measured: a row laid from right to left drew nothing, and a column took the whole width)
                        let wrap = width * 0.78 - 20.0;
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let mut inner = ui
                            .painter()
                            .layout(text.clone(), font.clone(), p.text, wrap)
                            .size()
                            .x;
                        if let Some(with) = &with {
                            inner = inner.max(
                                ui.painter()
                                    .layout(with.clone(), font, p.muted, wrap)
                                    .size()
                                    .x,
                            );
                        }
                        let inner = inner.ceil() + 1.0;
                        ui.horizontal(|ui| {
                            ui.add_space((width - inner - 20.0 - 10.0).max(0.0));
                            bubble.show(ui, |ui| {
                                ui.set_width(inner);
                                ui.vertical(|ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(text.as_str()).color(p.text),
                                        )
                                        .wrap(),
                                    );
                                    if let Some(with) = &with {
                                        ui.label(theme::muted(with.as_str()));
                                    }
                                });
                            });
                        });
                    } else {
                        // the model's is read as Markdown, at the left; a reply that only asked for tools has no bubble
                        if !text.is_empty() || turn.calls.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                bubble.show(ui, |ui| {
                                    ui.set_max_width(width * 0.78);
                                    ui.vertical(|ui| {
                                        show_message(ui, &text);
                                        if let Some(with) = &with {
                                            ui.label(theme::muted(with.as_str()));
                                        }
                                    });
                                });
                            });
                        }
                        // the tools it asked for (C204), each with what came of it
                        for (c, call) in turn.calls.iter().enumerate() {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                if let Some(action) =
                                    super::tools::tool_card(ui, call, &asker, width * 0.78)
                                {
                                    card = Some((i, c, action));
                                }
                            });
                        }
                    }
                    let idle = !self.chat.replying();
                    if !turn.from_person && !typing && !turn.text.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            if theme::button_sized(
                                ui,
                                Role::Quiet,
                                true,
                                "Copy",
                                egui::vec2(0.0, 18.0),
                            )
                            .on_hover_text("Copy this reply.")
                            .clicked()
                            {
                                ui.ctx().copy_text(turn.text.clone());
                            }
                            if Some(i) == last_reply
                                && idle
                                && theme::button_sized(
                                    ui,
                                    Role::Quiet,
                                    true,
                                    "Regenerate",
                                    egui::vec2(0.0, 18.0),
                                )
                                .on_hover_text("Ask for this reply again.")
                                .clicked()
                            {
                                regenerate = true;
                            }
                            if let Some(stats) = &turn.stats {
                                ui.label(theme::muted(stats.line()));
                            }
                        });
                    } else if turn.from_person && Some(i) == last_message && idle {
                        // a row of its own height: a row from right to left in what is left of the room is as high
                        // as all of it
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 20.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.add_space(8.0);
                                if theme::button_sized(
                                    ui,
                                    Role::Quiet,
                                    true,
                                    "Edit",
                                    egui::vec2(0.0, 18.0),
                                )
                                .on_hover_text("Change this message and send it again.")
                                .clicked()
                                {
                                    edit = true;
                                }
                            },
                        );
                    }
                    ui.add_space(6.0);
                }
                if self.chat.waiting_for_tools {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(theme::muted("Starting the MCP servers…"));
                    });
                }
                if let Some(why) = &self.chat.error {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(8.0);
                        ui.colored_label(p.bad, why);
                    });
                }
                if let Some(notice) = &self.chat.notice {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(notice).small().color(p.warn));
                    });
                }
            });
        }
        if let Some((turn, call, action)) = card {
            match action {
                super::tools::CardAction::Allow => self.allow_call(turn, call, false),
                super::tools::CardAction::Always => self.allow_call(turn, call, true),
                super::tools::CardAction::Deny => self.deny_call(turn, call),
                super::tools::CardAction::Toggle => {
                    if let Some(c) = self
                        .chat
                        .turns
                        .get_mut(turn)
                        .and_then(|t| t.calls.get_mut(call))
                    {
                        c.open = !c.open;
                    }
                }
            }
        }
        if regenerate {
            self.regenerate_chat(ui.ctx());
        }
        if edit {
            self.edit_last_message();
        }
        let box_top = ui.cursor().top();
        // the pictures that go with the next message, each with the way to take it off
        if !self.chat.pictures.is_empty() {
            let mut off: Option<usize> = None;
            ui.horizontal_wrapped(|ui| {
                ui.add_space(6.0);
                for (i, p) in self.chat.pictures.iter().enumerate() {
                    if ui
                        .small_button(format!("{} ×", p.name))
                        .on_hover_text("Take it off this message.")
                        .clicked()
                    {
                        off = Some(i);
                    }
                }
                if !self.model_sees_pictures() {
                    ui.label(
                        egui::RichText::new(
                            "This model may not see pictures: one that does says vision, or came with a projector.",
                        )
                        .small()
                        .color(theme::palette().warn),
                    );
                }
            });
            if let Some(i) = off {
                self.chat.pictures.remove(i);
            }
        }
        let mut attach = false;
        // a run on a computer QuickAI set up may not say its model (llama.cpp serves the one it was started with)
        let ready = running && (model.is_some() || remote_run.is_some());
        let frame = egui::Frame::new()
            .fill(p.well)
            .stroke(egui::Stroke::new(1.0, p.outline))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(6, 4))
            .outer_margin(egui::Margin {
                left: 16,
                right: 16,
                top: 6,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // the clip and the tools side by side, with no room between them (the user: "get it closer to
                    // the attachment icon - so it doesnt waste space")
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let clip = icons::icon_button(ui, Icon::Clip, egui::vec2(24.0, 28.0), ready, false)
                            .on_hover_text(
                                "Add a picture (PNG, JPEG, WebP, GIF) for a model that sees them, or a text file to \
                                 put in the message.",
                            );
                        super::help::mark_spot(ui, super::help::Spot::Clip, clip.rect);
                        attach = clip.clicked();
                        // the tools the model can use here: the MCP servers that are on (C204, C210)
                        self.show_tools_button(ui);
                    });
                    // Enter sends and Shift+Enter is a new line: the key is taken before the box sees it, so that it
                    // does not put a line break in the message wherever the cursor is
                    let input_id = egui::Id::new("chat_input");
                    let focused = ui.memory(|m| m.has_focus(input_id));
                    let enter = focused
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    let hint = match &model {
                        Some(model) if ready => format!("Message {model}"),
                        _ => "Start a model to chat with it".to_string(),
                    };
                    let field = ui.add_enabled(
                        ready,
                        egui::TextEdit::multiline(&mut self.chat.input)
                            .id(input_id)
                            .frame(egui::Frame::NONE)
                            .desired_rows(1)
                            .desired_width(ui.available_width() - 40.0)
                            .hint_text(hint),
                    );
                    if self.chat.replying() {
                        if icons::icon_button(ui, Icon::Close, egui::vec2(32.0, 28.0), true, true)
                            .on_hover_text("Stop the reply. What came of it stays.")
                            .clicked()
                        {
                            self.chat.stop();
                            self.poll_chat();
                        }
                    } else {
                        let can_send = ready && !self.chat.input.trim().is_empty();
                        let clicked =
                            icons::icon_button(ui, Icon::Send, egui::vec2(32.0, 28.0), can_send, can_send)
                                .on_hover_text("Send (Enter). Shift+Enter starts a new line.")
                                .clicked();
                        if can_send && (clicked || enter) {
                            self.send_chat(ui.ctx());
                            field.request_focus();
                        }
                    }
                });
            });
        // how high the box was, for the room the next frame leaves it; when it changed, this frame is drawn again before
        // it is shown, so that the box is never seen under the log
        let took = frame.response.rect.bottom() - box_top;
        if (took - box_height).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(box_id, took));
            ui.ctx().request_discard("the chat's box changed height");
        }
        // the box, without the room its frame keeps round it
        let r = frame.response.rect;
        super::help::mark_spot(
            ui,
            super::help::Spot::ChatBox,
            egui::Rect::from_min_max(
                r.min + egui::vec2(16.0, 6.0),
                r.max - egui::vec2(16.0, 10.0),
            ),
        );
        if attach {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(
                    "Pictures and text",
                    &[
                        "png", "jpg", "jpeg", "webp", "gif", "txt", "md", "csv", "json", "log",
                    ],
                )
                .add_filter("Any file", &["*"])
                .pick_file()
            {
                self.attach_to_message(&path);
            }
        }
    }

    /// The options of a reply: the instructions, the temperature and the length, each the model's own until set.
    /// The window of the chat's options (C208, `the mockups`): the instructions the model reads before
    /// every message, how free its answers are, and how long a reply may be; each left to the model until set. What is
    /// set is used from the next message on; Reset gives all three back to the model, Close closes it.
    pub(super) fn show_chat_options_window(&mut self, ctx: &egui::Context) {
        if !self.chat.show_instructions {
            return;
        }
        let id = chat_options_viewport();
        if std::mem::take(&mut self.raise_chat_options) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let mut close = false;
        let builder = theme::own_window("Chat options", [460.0, 440.0], [380.0, 360.0]);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class != egui::ViewportClass::EmbeddedWindow
                && ui.input(|i| i.viewport().close_requested())
            {
                close = true;
            }
            egui::Panel::bottom("chat_options_footer")
                .frame(theme::bar_frame(egui::Margin::symmetric(16, 10)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::muted("For this chat and the next ones.").small());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                close = true;
                            }
                            if ui
                                .button("Reset")
                                .on_hover_text("Leave all three to the model again.")
                                .clicked()
                            {
                                self.chat.instructions.clear();
                                self.chat.temperature = None;
                                self.chat.max_tokens = None;
                            }
                        });
                    });
                });
            egui::CentralPanel::default()
                .frame(theme::page_frame().inner_margin(egui::Margin::symmetric(18, 14)))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("chat_options_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.chat_options_body(ui));
                });
        });
        if close {
            self.chat.show_instructions = false;
        }
    }

    /// What the window of the chat's options has: three parts, each a name, a line on what it is, and its control.
    fn chat_options_body(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let head = |ui: &mut egui::Ui, name: &str, said: &str| {
            ui.label(
                egui::RichText::new(name)
                    .family(theme::semibold(ui))
                    .color(p.text),
            );
            if !said.is_empty() {
                ui.label(theme::muted(said).small());
            }
            ui.add_space(4.0);
        };
        head(
            ui,
            "Instructions",
            "What the model reads before every message.",
        );
        ui.add(
            egui::TextEdit::multiline(&mut self.chat.instructions)
                .desired_rows(3)
                .desired_width(ui.available_width())
                .hint_text("For example: You are a helpful assistant. Answer briefly, in English."),
        );
        ui.add_space(16.0);
        head(
            ui,
            "How free its answers are",
            "Lower is steadier and more literal; higher is freer and more varied.",
        );
        let mut own_temperature = self.chat.temperature.is_none();
        if ui
            .checkbox(&mut own_temperature, "The model's own")
            .changed()
        {
            self.chat.temperature = (!own_temperature).then_some(0.7);
        }
        ui.add_enabled_ui(self.chat.temperature.is_some(), |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::muted("steady").small());
                // the slider takes the row, but for its number and the word after it
                ui.spacing_mut().slider_width = (ui.available_width() - 110.0).max(100.0);
                let mut t = self.chat.temperature.unwrap_or(0.7);
                let slider = ui.add(
                    egui::Slider::new(&mut t, 0.0..=2.0)
                        .step_by(0.05)
                        .fixed_decimals(2)
                        .show_value(true),
                );
                ui.label(theme::muted("free").small());
                if slider.changed() {
                    self.chat.temperature = Some(t);
                }
            });
        });
        ui.add_space(16.0);
        head(
            ui,
            "How long a reply may be",
            "In tokens: about three quarters of a word each.",
        );
        ui.horizontal(|ui| {
            let mut no_limit = self.chat.max_tokens.is_none();
            if ui.checkbox(&mut no_limit, "No limit").changed() {
                self.chat.max_tokens = (!no_limit).then_some(512);
            }
            ui.add_space(12.0);
            ui.add_enabled_ui(self.chat.max_tokens.is_some(), |ui| {
                let mut n = self.chat.max_tokens.unwrap_or(512);
                let drag = ui.add(
                    egui::DragValue::new(&mut n)
                        .range(16..=32768)
                        .speed(16.0)
                        .suffix(" tokens"),
                );
                if drag.changed() {
                    self.chat.max_tokens = Some(n);
                }
            });
        });
    }
}

/// Which window the chat's options are.
pub(super) fn chat_options_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_chat_options")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::fake_ollama::FakeOllama;

    fn reply_of(
        port: u16,
        model: &str,
        options: Options,
        turns: Vec<Turn>,
    ) -> (String, Option<String>) {
        let (rx, _cancel) = spawn_reply(port, model.to_string(), options, turns, || {});
        let (mut text, mut failed) = (String::new(), None);
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).expect("an answer") {
                ChatEvent::Text(piece) => text.push_str(&piece),
                ChatEvent::Thinking | ChatEvent::Calls(_) | ChatEvent::Notice(_) => {}
                ChatEvent::Done => break,
                ChatEvent::Failed(why) => {
                    failed = Some(why);
                    break;
                }
            }
        }
        (text, failed)
    }

    #[test]
    fn a_reply_is_streamed_from_the_chat_api_of_the_engine_with_the_conversation_so_far() {
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 2_500_000_000);
        let turns = vec![
            Turn::said(true, "Hello"),
            Turn::said(false, "Hi."),
            Turn::said(true, "Say more"),
        ];
        let (text, failed) = reply_of(fake.port(), "qwen3:4b", Options::default(), turns);
        assert_eq!(failed, None);
        assert!(!text.is_empty(), "nothing came");
        let asked = fake.requests_to("/v1/chat/completions");
        assert_eq!(asked.len(), 1);
        let body = asked[0].body.clone().unwrap();
        assert_eq!(body["model"], "qwen3:4b");
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["messages"],
            serde_json::json!([
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi."},
                {"role": "user", "content": "Say more"},
            ])
        );
        assert!(
            body.get("temperature").is_none() && body.get("max_tokens").is_none(),
            "the model's own"
        );
    }

    #[test]
    fn what_the_person_set_is_in_the_request_and_nothing_else() {
        let hello = [Turn::said(true, "Hello")];
        let options = Options {
            instructions: Some("  Answer in French.  ".into()),
            temperature: Some(0.333),
            max_tokens: Some(256),
        };
        let body = request_body("m", &options, &hello);
        assert_eq!(
            body["messages"],
            serde_json::json!([
                {"role": "system", "content": "Answer in French."},
                {"role": "user", "content": "Hello"},
            ])
        );
        assert_eq!(body["temperature"], serde_json::json!(0.33));
        assert_eq!(body["max_tokens"], 256);
        let plain = request_body("m", &Options::default(), &hello);
        assert_eq!(
            plain["messages"],
            serde_json::json!([{"role": "user", "content": "Hello"}])
        );
        assert!(plain.get("temperature").is_none());
    }

    #[test]
    fn base64_is_what_the_standard_says() {
        // RFC 4648, section 10
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), coded, "{plain}");
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn a_picture_goes_with_the_message_and_a_text_file_goes_in_it() {
        let dir = crate::testkit::scratch("chat-attach");
        std::fs::write(dir.join("dot.png"), [0x89, b'P', b'N', b'G']).unwrap();
        std::fs::write(dir.join("notes.md"), "# Notes\nbuy milk\n").unwrap();
        std::fs::write(dir.join("blob.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let Ok(Given::Picture(picture)) = read_given(&dir.join("dot.png")) else {
            panic!("a PNG is a picture");
        };
        assert_eq!(
            (picture.name.as_str(), picture.mime.as_str()),
            ("dot.png", "image/png")
        );
        assert_eq!(picture.base64, base64(&[0x89, b'P', b'N', b'G']));
        assert_eq!(
            read_given(&dir.join("notes.md")),
            Ok(Given::Text("notes.md:\n```\n# Notes\nbuy milk\n```".into()))
        );
        assert!(
            read_given(&dir.join("blob.bin")).is_err_and(|w| w.contains("not text or a picture"))
        );

        // the request: the words and the picture as parts of one message
        let mut turn = Turn::said(true, "What is this?");
        turn.pictures.push(picture);
        let body = request_body("m", &Options::default(), &[turn]);
        assert_eq!(
            body["messages"][0]["content"],
            serde_json::json!([
                {"type": "text", "text": "What is this?"},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", base64(&[0x89, b'P', b'N', b'G']))}},
            ])
        );
    }

    #[test]
    fn code_between_fences_is_a_part_of_its_own_and_a_fence_still_open_is_code_to_its_end() {
        let said = "Here it is:\n```python\ndef f(s):\n    return s[::-1]\n```\nThat is all.";
        assert_eq!(
            parts(said),
            [
                (false, "Here it is:".to_string()),
                (true, "def f(s):\n    return s[::-1]".to_string()),
                (false, "That is all.".to_string()),
            ]
        );
        assert_eq!(
            parts("Look:\n```\nlet x = 1;"),
            [
                (false, "Look:".to_string()),
                (true, "let x = 1;".to_string())
            ]
        );
        assert_eq!(parts("plain words"), [(false, "plain words".to_string())]);
        assert!(parts("").is_empty());
    }

    #[test]
    fn markdown_lines_and_spans_are_read_as_a_person_writes_them() {
        assert_eq!(line_kind("## Steps"), Line::Heading(2, "Steps"));
        assert_eq!(line_kind("#hashtag"), Line::Plain("#hashtag"));
        assert_eq!(line_kind("- milk"), Line::Item("•".into(), "milk"));
        assert_eq!(line_kind("  * eggs"), Line::Item("•".into(), "eggs"));
        assert_eq!(
            line_kind("12. twelfth"),
            Line::Item("12.".into(), "twelfth")
        );
        assert_eq!(line_kind("| a | b |"), Line::Table("| a | b |"));
        assert_eq!(line_kind("> quoted"), Line::Quote("quoted"));
        assert_eq!(
            spans("Use **bold** and `code` here"),
            [
                (Span::Plain, "Use ".to_string()),
                (Span::Strong, "bold".to_string()),
                (Span::Plain, " and ".to_string()),
                (Span::Code, "code".to_string()),
                (Span::Plain, " here".to_string()),
            ]
        );
        // a mark that is not closed is a plain character
        assert_eq!(
            spans("2 ** 3 and a `tick"),
            [(Span::Plain, "2 ** 3 and a `tick".to_string())]
        );
    }

    #[test]
    fn the_speed_of_a_reply_is_said_from_its_first_word() {
        let stats = Stats {
            tokens: 101,
            first_ms: 500,
            total_ms: 5500,
        };
        assert_eq!(
            stats.line(),
            "101 tokens · 20.0 tokens/s · first word in 0.5 s"
        );
    }

    #[test]
    fn a_refusal_of_the_engine_is_said_in_its_words_and_a_server_that_is_not_there_is_said_too() {
        let fake = FakeOllama::start();
        let hello = || vec![Turn::said(true, "Hello")];
        let (_, failed) = reply_of(fake.port(), "nosuch:1b", Options::default(), hello());
        assert_eq!(failed.as_deref(), Some("model 'nosuch:1b' not found"));
        let port = fake.port();
        drop(fake);
        let (_, failed) = reply_of(port, "qwen3:4b", Options::default(), hello());
        assert!(
            failed.is_some_and(|w| w.starts_with("could not reach the server")),
            "a server that is gone was not said"
        );
    }

    /// C204: a reply that asked for tools goes back to the model as it was asked, each call followed by what came of it
    /// (what the tool gave back, why it failed, or that the person did not allow it), with the tools offered again.
    #[test]
    fn a_reply_that_asked_for_tools_is_sent_back_with_what_came_of_each_call() {
        let call = |id: &str, state: CallState| ToolCall {
            id: id.to_string(),
            name: "files_read".to_string(),
            server: "files".to_string(),
            tool: "read".to_string(),
            arguments: "{\"path\":\"a.txt\"}".to_string(),
            state,
            open: false,
            ticket: None,
        };
        let mut reply = Turn::said(false, "");
        reply.calls = vec![
            call("call_1", CallState::Done("text of a".to_string())),
            call("call_2", CallState::Denied),
            call("call_3", CallState::Failed("no such file".to_string())),
        ];
        let turns = vec![Turn::said(true, "read a.txt"), reply];
        let tools = serde_json::json!([{"type": "function", "function": {"name": "files_read"}}]);
        let body = request_body_with("m", &Options::default(), &turns, Some(&tools));
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 5, "{messages:?}");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["name"],
            "files_read"
        );
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a.txt\"}"
        );
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_1");
        assert_eq!(messages[2]["content"], "text of a");
        assert_eq!(
            messages[3]["content"],
            "The person did not allow this tool to run."
        );
        assert_eq!(messages[4]["content"], "The tool failed: no such file");
        assert_eq!(body["tools"], tools);
        // and without tools, the body has none
        assert!(request_body("m", &Options::default(), &turns[..1])
            .get("tools")
            .is_none());
    }

    /// C204: the calls of a streamed reply come in pieces, put together by their index; an engine that gives the
    /// arguments as an object (and not as text) is read too.
    #[test]
    fn tool_calls_that_come_in_pieces_are_put_together_by_their_index() {
        let mut calls = Vec::new();
        for event in [
            serde_json::json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c1", "function": {"name": "files_", "arguments": ""}}]}}]}),
            serde_json::json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"name": "read", "arguments": "{\"pa"}}]}}]}),
            serde_json::json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "th\":\"a\"}"}}]}}]}),
            serde_json::json!({"choices": [{"delta": {"tool_calls": [{"index": 1, "id": "c2", "function": {"name": "time_now", "arguments": {"tz": "UTC"}}}]}}]}),
            serde_json::json!({"choices": [{"delta": {"content": "no call here"}}]}),
        ] {
            gather_calls(&event, &mut calls);
        }
        assert_eq!(
            calls,
            vec![
                AskedCall {
                    id: "c1".to_string(),
                    name: "files_read".to_string(),
                    arguments: "{\"path\":\"a\"}".to_string(),
                },
                AskedCall {
                    id: "c2".to_string(),
                    name: "time_now".to_string(),
                    arguments: "{\"tz\":\"UTC\"}".to_string(),
                },
            ]
        );
    }

    /// C204: a call is settled once it ran, failed or was not allowed, and what the model is told says which.
    #[test]
    fn what_the_model_is_told_of_a_call_says_what_came_of_it() {
        let mut call = ToolCall {
            id: "c".to_string(),
            name: "n".to_string(),
            server: "s".to_string(),
            tool: "t".to_string(),
            arguments: "{}".to_string(),
            state: CallState::Asking,
            open: false,
            ticket: None,
        };
        assert!(!call.settled());
        call.state = CallState::Running;
        assert!(!call.settled());
        call.state = CallState::Done("ok".to_string());
        assert!(call.settled());
        assert_eq!(call.answer(), "ok");
    }
}
