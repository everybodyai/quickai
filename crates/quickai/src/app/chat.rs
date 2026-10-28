//! A conversation with the model that runs, in a tab of the dock: the quickest way to try a model, with nothing to
//! install and no browser. It speaks the OpenAI chat API that llama-server and Ollama both serve, on this machine's
//! own address (the public port, so the live traffic shows it too when that is on; a tunnel's, for a model that runs on
//! another computer), and the reply is shown as it streams in, with how fast it came.
//!
//! What is the person's to set: instructions (a system message), the temperature and the length of a reply, each left
//! to the model unless set. The last reply can be asked for again and the last message changed and sent again. Nothing
//! is kept unless "Keep chats" is on (WP4.28): then each conversation is a file of this computer (`chats.rs`), listed
//! beside the conversation, opened, deleted and exported as Markdown.

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

/// One message of the conversation, the pictures sent with it, and for a reply, how fast it came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub from_person: bool,
    pub text: String,
    pub stats: Option<Stats>,
    pub pictures: Vec<Picture>,
}

impl Turn {
    pub fn said(from_person: bool, text: impl Into<String>) -> Turn {
        Turn {
            from_person,
            text: text.into(),
            stats: None,
            pictures: Vec::new(),
        }
    }
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
    /// Why the last reply did not come, in the engine's words when it gave some.
    pub error: Option<String>,
    /// The tab has been opened since the application started: the dot that points to it is not drawn again.
    pub opened: bool,
    /// What the model is told before the conversation (a system message), when the person wrote something.
    pub instructions: String,
    /// The temperature and the longest reply, when the person set them.
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// The box of the options is open.
    pub show_instructions: bool,
    /// The conversation is kept on this computer (the person's choice, in the settings), under this id once it has
    /// a message.
    pub keep: bool,
    pub saved_id: Option<String>,
    /// The kept conversations, as the list beside the conversation shows them.
    pub kept: Vec<ChatSummary>,
    /// The pictures that go with the next message.
    pub pictures: Vec<Picture>,
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

    /// Stops the reply that is coming; what came of it stays.
    pub fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        self.reply = None;
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
    }
}

/// Asks the engine on `port` for the reply to `turns`, streamed, on a thread of its own.
pub fn spawn_reply(
    port: u16,
    model: String,
    options: Options,
    turns: Vec<Turn>,
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
        match stream_reply(port, &model, &options, &turns, &stop, &|text| {
            send(ChatEvent::Text(text))
        }) {
            Ok(()) => send(ChatEvent::Done),
            Err(why) => send(ChatEvent::Failed(why)),
        }
    });
    (rx, cancel)
}

/// The body of the request: the model, the instructions as the system's word, the conversation, and what the
/// person set (and nothing else, so that the model's own defaults stand).
pub fn request_body(model: &str, options: &Options, turns: &[Turn]) -> serde_json::Value {
    let system = options
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
        .map(|i| serde_json::json!({"role": "system", "content": i}));
    let messages: Vec<serde_json::Value> = system
        .into_iter()
        .chain(turns.iter().map(|t| {
            let role = if t.from_person { "user" } else { "assistant" };
            if t.pictures.is_empty() {
                return serde_json::json!({"role": role, "content": t.text});
            }
            // the words and the pictures, as the OpenAI chat API takes them (llama-server and Ollama both do)
            let mut content = vec![serde_json::json!({"type": "text", "text": t.text})];
            content.extend(t.pictures.iter().map(|p| {
                serde_json::json!({
                    "type": "image_url",
                    "image_url": {"url": format!("data:{};base64,{}", p.mime, p.base64)},
                })
            }));
            serde_json::json!({"role": role, "content": content})
        }))
        .collect();
    let mut body = serde_json::json!({"model": model, "messages": messages, "stream": true});
    if let Some(t) = options.temperature {
        body["temperature"] = serde_json::json!((f64::from(t) * 100.0).round() / 100.0);
    }
    if let Some(n) = options.max_tokens {
        body["max_tokens"] = serde_json::json!(n);
    }
    body
}

/// The request and its stream of events (`data: {...}` lines, then `data: [DONE]`). An `error` the engine sends,
/// in the body of a refusal or in the stream, is the failure's sentence.
fn stream_reply(
    port: u16,
    model: &str,
    options: &Options,
    turns: &[Turn],
    stop: &AtomicBool,
    text: &dyn Fn(String),
) -> Result<(), String> {
    let body = request_body(model, options, turns);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .into();
    let response = agent
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| format!("could not reach the server: {e}"))?;
    let status = response.status().as_u16();
    let reader = response.into_body().into_reader();
    if !(200..300).contains(&status) {
        let said: serde_json::Value =
            serde_json::from_reader(reader).unwrap_or(serde_json::Value::Null);
        return Err(error_words(&said).unwrap_or_else(|| format!("the server answered {status}")));
    }
    for line in BufReader::new(reader).lines() {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let line = line.map_err(|e| format!("the reply stopped: {e}"))?;
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
            return Err(why);
        }
        if let Some(piece) = event
            .pointer("/choices/0/delta/content")
            .and_then(serde_json::Value::as_str)
            .filter(|p| !p.is_empty())
        {
            text(piece.to_string());
        }
        if event
            .pointer("/choices/0/finish_reason")
            .is_some_and(|r| !r.is_null())
        {
            break;
        }
    }
    Ok(())
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
    /// Whether the engine in use can be talked to here: it serves the OpenAI chat API.
    pub(super) fn can_chat(&self) -> bool {
        self.engine_doc()
            .is_some_and(|e| e.doc.provides.apis.iter().any(|a| a == "openai.chat"))
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
            self.chat.error = Some(format!(
                "Start {} with a model to chat with it.",
                self.server_name()
            ));
            return;
        }
        self.chat.input.clear();
        let mut turn = Turn::said(true, text);
        turn.pictures = std::mem::take(&mut self.chat.pictures);
        self.chat.turns.push(turn);
        self.ask_for_reply(ctx);
    }

    /// Where a reply is asked for: the port of the run, and the model that is loaded.
    fn chat_target(&self) -> Option<(u16, String)> {
        let port = self
            .session
            .port()
            .filter(|_| self.session.state() == ServerState::Running)?;
        Some((port, self.session.model_name()?.to_string()))
    }

    /// Asks for the reply to the conversation as it stands.
    fn ask_for_reply(&mut self, ctx: &egui::Context) {
        let Some((port, model)) = self.chat_target() else {
            return;
        };
        self.chat.error = None;
        let ctx = ctx.clone();
        let (rx, cancel) = spawn_reply(
            port,
            model,
            self.chat.options(),
            self.chat.turns.clone(),
            move || ctx.request_repaint(),
        );
        self.chat.turns.push(Turn::said(false, ""));
        self.chat.reply = Some(rx);
        self.chat.cancel = Some(cancel);
        self.chat.started = Some(Instant::now());
        self.chat.first = None;
        self.chat.pieces = 0;
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
        if self.chat.reply.is_some() && self.session.state() != ServerState::Running {
            self.chat.stop();
        }
        let Some(rx) = &self.chat.reply else {
            return;
        };
        let mut ended = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                ChatEvent::Text(piece) => {
                    if self.chat.first.is_none() {
                        self.chat.first = Some(Instant::now());
                    }
                    self.chat.pieces += 1;
                    if let Some(last) = self.chat.turns.last_mut().filter(|t| !t.from_person) {
                        last.text.push_str(&piece);
                    }
                }
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
            let stats = match (self.chat.started, self.chat.first) {
                (Some(started), Some(first)) => Some(Stats {
                    tokens: self.chat.pieces,
                    first_ms: first.duration_since(started).as_millis() as u64,
                    total_ms: started.elapsed().as_millis() as u64,
                }),
                _ => None,
            };
            match self.chat.turns.last_mut() {
                // a reply that said nothing is not kept as a turn
                Some(last) if !last.from_person && last.text.is_empty() => {
                    self.chat.turns.pop();
                }
                Some(last) if !last.from_person => last.stats = stats,
                _ => {}
            }
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
            model: self.session.model_name().unwrap_or_default().to_string(),
            engine: self.engine_name(),
            runner: self.remote_in_use().map(|r| r.name.clone()),
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
            model: self.session.model_name().unwrap_or_default().to_string(),
            engine: self.engine_name(),
            runner: self.remote_in_use().map(|r| r.name.clone()),
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

    /// What is beside the tabs while the chat is open: New chat, and a menu of the rest (Keep chats, Options, Export).
    /// Drawn in a layout that runs from right to left.
    pub(super) fn show_chat_actions(&mut self, ui: &mut egui::Ui) {
        let more = icons::icon_button(ui, Icon::More, egui::vec2(28.0, 24.0), true, false)
            .on_hover_text("Keep chats, options, export");
        let mut export = false;
        let mut keep = self.chat.keep;
        let mut keep_changed = false;
        let mut options = false;
        egui::Popup::menu(&more)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(200.0);
                keep_changed = ui
                    .checkbox(&mut keep, "Keep chats")
                    .on_hover_text(
                        "Keep each conversation on this computer (in QuickAI's own folder), listed beside it. Off, \
                         nothing is kept. Nothing leaves this computer either way.",
                    )
                    .changed();
                let set = !self.chat.instructions.trim().is_empty()
                    || self.chat.temperature.is_some()
                    || self.chat.max_tokens.is_some();
                if ui
                    .button(if set { "Options... •" } else { "Options..." })
                    .on_hover_text(
                        "Instructions the model reads before every message, how free its answers are, and how long \
                         they may be. Each is left to the model unless set.",
                    )
                    .clicked()
                {
                    options = true;
                    ui.close();
                }
                if ui
                    .add_enabled(
                        !self.chat.turns.is_empty() && !self.chat.replying(),
                        egui::Button::new("Export..."),
                    )
                    .on_hover_text("Save this conversation as a Markdown file.")
                    .clicked()
                {
                    export = true;
                    ui.close();
                }
            });
        if icons::labelled_button(
            ui,
            Role::Quiet,
            !self.chat.turns.is_empty() && !self.chat.replying(),
            Icon::Plus,
            "New chat",
            24.0,
        )
        .on_hover_text(if self.chat.keep {
            "Start a new conversation; this one stays in the list."
        } else {
            "Start a new conversation. This one is not kept."
        })
        .clicked()
        {
            self.chat.new_conversation();
        }
        if keep_changed {
            self.set_keep_chats(keep);
        }
        if options {
            self.chat.show_instructions = !self.chat.show_instructions;
        }
        if export {
            self.export_chat();
        }
    }

    fn show_chat_conversation(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let running = self.session.state() == ServerState::Running;
        let model = self.session.model_name().map(str::to_string);
        let server = self.server_name();
        if self.chat.show_instructions {
            ui.add_space(6.0);
            self.show_chat_options(ui);
        }
        // with nothing said yet, what to do is said in the middle of the room the conversation will have
        let empty = self.chat.turns.is_empty() && self.chat.error.is_none();
        let empty_words = match (&model, running) {
            (Some(model), true) => match self.remote_in_use() {
                Some(remote) => format!("Ask {model} anything. It runs on {}.", remote.name),
                None => format!("Ask {model} anything."),
            },
            (None, true) => {
                format!("{server} runs with no model loaded: choose one, then Load model.")
            }
            _ => match self.models.get(self.selected) {
                Some(m) => format!("Press the play button to chat with {}.", m.display),
                None => format!("Start {server} with a model to chat with it here."),
            },
        };
        let box_height = 64.0;
        let room = (ui.available_height() - box_height).max(40.0);
        let mut regenerate = false;
        let mut edit = false;
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
                        "…".to_string()
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
                        // the model's is read as Markdown, at the left
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
                if let Some(why) = &self.chat.error {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(8.0);
                        ui.colored_label(p.bad, why);
                    });
                }
            });
        }
        if regenerate {
            self.regenerate_chat(ui.ctx());
        }
        if edit {
            self.edit_last_message();
        }
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
        let ready = running && model.is_some();
        egui::Frame::new()
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
                    attach = icons::icon_button(ui, Icon::Clip, egui::vec2(30.0, 28.0), ready, false)
                        .on_hover_text(
                            "Add a picture (PNG, JPEG, WebP, GIF) for a model that sees them, or a text file to put \
                             in the message.",
                        )
                        .clicked();
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
    fn show_chat_options(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::multiline(&mut self.chat.instructions)
                    .desired_rows(2)
                    .desired_width(ui.available_width() - 8.0)
                    .hint_text("Instructions, for example: You are a helpful assistant. Answer briefly, in English."),
            );
        });
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let mut own_temperature = self.chat.temperature.is_none();
            if ui
                .checkbox(&mut own_temperature, "The model's own temperature")
                .on_hover_text(
                    "Lower is steadier and more literal; higher is freer and more varied.",
                )
                .changed()
            {
                self.chat.temperature = (!own_temperature).then_some(0.7);
            }
            if let Some(t) = self.chat.temperature.as_mut() {
                ui.add(
                    egui::Slider::new(t, 0.0..=2.0)
                        .step_by(0.05)
                        .text("temperature"),
                );
            }
            ui.add_space(12.0);
            let mut no_limit = self.chat.max_tokens.is_none();
            if ui
                .checkbox(&mut no_limit, "No length limit")
                .on_hover_text(
                    "The longest a reply may be, in tokens (about three quarters of a word each).",
                )
                .changed()
            {
                self.chat.max_tokens = (!no_limit).then_some(512);
            }
            if let Some(n) = self.chat.max_tokens.as_mut() {
                ui.add(
                    egui::DragValue::new(n)
                        .range(16..=32768)
                        .speed(16.0)
                        .suffix(" tokens"),
                );
            }
        });
    }
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
}
