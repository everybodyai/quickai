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
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use eframe::egui;

use super::given;
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
    /// For a reply, what the model thought before it (`reasoning_content`, Ollama's `reasoning`), and for how many
    /// seconds: shown folded above it when Show thinking is on, and never sent back to the model.
    pub thought: String,
    pub thought_secs: Option<u64>,
    /// For a reply, the person stopped it while it was written (C224): what it wrote stays, said to be stopped.
    pub stopped: bool,
}

impl Turn {
    pub fn said(from_person: bool, text: impl Into<String>) -> Turn {
        Turn {
            from_person,
            text: text.into(),
            stats: None,
            pictures: Vec::new(),
            calls: Vec::new(),
            thought: String::new(),
            thought_secs: None,
            stopped: false,
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

/// A picture sent with a message: its name, its kind (`image/png`) and its bytes in base64, and the small picture of it
/// the chat shows (C226).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub mime: String,
    pub base64: String,
    pub preview: Preview,
}

/// The small picture of a picture, once it is in the window's memory (`given::preview_of`); not part of what the
/// picture is: two pictures are the same whatever their previews.
#[derive(Clone, Default)]
pub struct Preview(pub Option<egui::TextureHandle>);

impl PartialEq for Preview {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Preview {}

impl std::fmt::Debug for Preview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "Preview"
        } else {
            "no preview"
        })
    }
}

/// The largest picture and text file a message takes; the text is all the files put in the box, with what is typed.
pub(super) const MOST_PICTURE: u64 = 12 * 1024 * 1024;
pub(super) const MOST_TEXT: u64 = 256 * 1024;

/// The kinds of picture a message takes, by ending.
pub(super) fn picture_kind(name: &str) -> Option<&'static str> {
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

/// Base64 (RFC 4648) as the bytes it stands for; `None` when it is not base64.
#[cfg(test)]
pub(super) fn base64_decoded(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digits: Vec<u32> = text
        .trim_end_matches('=')
        .bytes()
        .map(|b| ALPHABET.iter().position(|a| *a == b).map(|d| d as u32))
        .collect::<Option<_>>()?;
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, d)| n | d << (18 - 6 * i));
        out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    Some(out)
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
    /// A piece of the model's thinking came (`reasoning_content`, or Ollama's `reasoning`): it is kept apart from the
    /// reply, shown folded above it when Show thinking is on, and says the reply is on its way.
    Thinking(String),
    /// The reply asks to use tools (C204): they come before its end.
    Calls(Vec<AskedCall>),
    /// Something to say under the conversation: the engine did not take the tools, and the reply came without them.
    Notice(String),
    /// Why the engine ended the reply (`finish_reason`: `stop`, `length`, `tool_calls`), when it said.
    Finished(String),
    /// What the request took, as the engine counted it (`usage`): the conversation as it was sent, and the reply
    /// (with its thinking).
    Usage {
        prompt: u64,
        completion: u64,
    },
    Done,
    Failed(String),
    /// The engine refused the request because it does not fit in the context (llama.cpp's `exceed_context_size_error`),
    /// with the length of the context when it said it.
    TooLong {
        why: String,
        context: Option<u64>,
    },
}

/// How a server of the OpenAI chat API is told what that API has no word for (whether the model thinks), and asked
/// how long its context is: by the engine it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
    /// llama.cpp's server: `chat_template_kwargs`, `/props`.
    LlamaCpp,
    /// Ollama: `reasoning_effort`, `/api/ps` and `/api/show`.
    Ollama,
    /// Another server of the API: the API's own words only.
    #[default]
    Other,
}

/// What a reply is asked with besides the conversation; `None` leaves it to the model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub instructions: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// What the model reads in place of the first turns once they were summarized (Auto-compact), after the
    /// instructions.
    pub summary: Option<String>,
    /// Whether the model is told to think (`None`: as it runs), and how its engine is told.
    pub thinking: Option<bool>,
    pub dialect: Dialect,
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
    /// Why the engine ended the reply that is coming (`finish_reason`), once it has said.
    finish: Option<String>,
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
    /// What was given to the next message and is being read (`given.rs`, C226): picked with the paper clip, pasted or
    /// dropped on the window.
    pub(super) reading: Vec<super::given::Reading>,
    /// Said over the box of what was given and could not go: refused, or past what a message takes.
    pub given_notes: Vec<String>,
    /// How many pictures were pasted in this conversation, to name the next (Pasted picture 1, 2, ...).
    pub pasted: usize,
    /// The V of Ctrl+V: egui-winit takes its press as the paste and gives egui only its release, so a release of V that
    /// was not seen pressed is a paste (`v_down`: a "v" being typed), which gave text when `v_pasted`.
    v_down: bool,
    v_pasted: bool,
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
    /// What the person sent, the oldest first, for Up and Down in the box to bring back (a new conversation keeps it).
    pub sent: Vec<String>,
    /// While the box shows one of `sent` brought back with Up: which, counted from the newest (0).
    pub recalled: Option<usize>,
    /// What the model reads in place of the first turns once they were summarized to make room (Auto-compact).
    pub summary: Option<super::compact::Summary>,
    /// The summary being written, while it is (`compact.rs`).
    pub(super) summarizing: Option<super::compact::Summarizing>,
    /// The summary is shown under the line where the model's view begins, and what the person types in its place while
    /// they edit it.
    pub summary_open: bool,
    pub summary_edit: Option<String>,
    /// What the last request took of the context, as the engine counted it (`compact::Used`).
    pub(super) used: Option<super::compact::Used>,
    /// The characters of the request that is coming, and what the engine counted of it, for `used`.
    pub(super) asked_chars: usize,
    pub(super) asked_tokens: Option<u64>,
    /// What the run the chat talks to says of itself (its engine, its context, whether its model thinks), for the run
    /// it was asked of, and the asking while it goes on (`compact::probe`).
    pub(super) run_facts: Option<super::compact::RunFacts>,
    pub(super) probing: Option<super::compact::Probing>,
    /// The last reply was cut off (`finish_reason` `length`): Continue goes on with it. By the context, and not by the
    /// reply limit, when `cut_by_context`: Continue then needs earlier messages to summarize.
    pub cut_off: bool,
    pub cut_by_context: bool,
    /// The reply that is coming goes on with the last one (Continue) instead of being one of its own.
    pub(super) continuing: bool,
    /// The conversation takes most of the context while Auto-compact is off: said under it, with the tokens it takes and
    /// the context's.
    pub room_note: Option<(u64, u64)>,
    /// The engine refused the request as too long: it is summarized and asked again, once for each message.
    pub(super) too_long: bool,
    pub(super) retried: bool,
}

/// The most messages Up and Down in the box go back through.
const MOST_SENT: usize = 200;

/// Up (`up`) or Down in the chat's box, as a terminal's history of what the person sent (`sent`, the oldest first): Up
/// brings back the message before when the box is empty, or when it shows one brought back (`recalled`, from the newest)
/// with the cursor (`cursor`, in characters) on its first line; Down the one after, and past the newest an empty box. What
/// the box then shows, and which it is; `None` when the key is the box's own, to move the cursor in what is typed.
pub(super) fn recall(
    sent: &[String],
    recalled: Option<usize>,
    input: &str,
    cursor: usize,
    up: bool,
) -> Option<(Option<usize>, String)> {
    let newest = |k: usize| sent.len().checked_sub(k + 1).map(|i| sent[i].clone());
    // a message brought back and then changed is the person's draft: the keys are its own
    let shown = recalled.filter(|k| newest(*k).as_deref() == Some(input));
    let before: String = input.chars().take(cursor).collect();
    let after: String = input.chars().skip(cursor).collect();
    if up {
        let next = match shown {
            Some(k) if !before.contains('\n') => k + 1,
            None if input.is_empty() => 0,
            _ => return None,
        };
        newest(next).map(|text| (Some(next), text))
    } else {
        match shown {
            Some(_) if after.contains('\n') => None,
            Some(0) => Some((None, String::new())),
            Some(k) => newest(k - 1).map(|text| (Some(k - 1), text)),
            None => None,
        }
    }
}

impl Chat {
    /// A reply is coming, or the summary that makes room for one is being written: the chat waits.
    pub fn replying(&self) -> bool {
        self.reply.is_some() || self.summarizing.is_some()
    }

    /// The first turn the model reads whole: the ones before it are in the summary.
    pub fn summarized_upto(&self) -> usize {
        self.summary
            .as_ref()
            .map_or(0, |s| s.upto.min(self.turns.len()))
    }

    /// A new conversation that keeps what is the person's and not the engine's: the options, whether chats are kept,
    /// that the tab was found, and what they sent (for Up in the box).
    pub fn carried_over(&mut self) -> Chat {
        Chat {
            opened: self.opened,
            instructions: std::mem::take(&mut self.instructions),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            show_instructions: self.show_instructions,
            keep: self.keep,
            kept: std::mem::take(&mut self.kept),
            sent: std::mem::take(&mut self.sent),
            // what the run said of itself is the run's, not the conversation's
            run_facts: self.run_facts.take(),
            probing: self.probing.take(),
            ..Chat::default()
        }
    }

    /// Stops the reply that is coming; what came of it stays. A reply that had said nothing yet is not kept as a turn.
    /// A summary being written is given up, and the conversation is as it was.
    pub fn stop(&mut self) {
        if let Some(summarizing) = self.summarizing.take() {
            summarizing.cancel();
        }
        let continued = std::mem::take(&mut self.continuing);
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        let stats = self.stats_now();
        if self.reply.take().is_some() {
            match self.turns.last_mut() {
                Some(last)
                    if !last.from_person && last.text.is_empty() && last.calls.is_empty() =>
                {
                    self.turns.pop();
                }
                // what it wrote stays, said to be stopped; Continue goes on with it (C224)
                Some(last) if !last.from_person && !last.text.is_empty() => {
                    last.stopped = true;
                    last.stats = match (last.stats.filter(|_| continued), stats) {
                        (Some(before), Some(now)) => Some(Stats {
                            tokens: before.tokens + now.tokens,
                            first_ms: before.first_ms,
                            total_ms: before.total_ms + now.total_ms,
                        }),
                        (before, now) => now.or(before),
                    };
                    self.cut_off = true;
                    self.cut_by_context = false;
                    // said under it too, where a narrow window has no room for it in the line of its buttons
                    self.notice =
                        Some("The answer was stopped. Continue goes on with it.".to_string());
                }
                _ => {}
            }
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

    /// How the reply that is coming came so far: its pieces, the time to the first, and the whole.
    fn stats_now(&self) -> Option<Stats> {
        match (self.started, self.first) {
            (Some(started), Some(first)) => Some(Stats {
                tokens: self.pieces,
                first_ms: first.duration_since(started).as_millis() as u64,
                total_ms: started.elapsed().as_millis() as u64,
            }),
            _ => None,
        }
    }

    /// What the person set for the replies; what the run is told besides is added by `App::reply_options`.
    pub(super) fn options(&self) -> Options {
        Options {
            instructions: Some(self.instructions.clone()).filter(|i| !i.trim().is_empty()),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            summary: self.summary.as_ref().map(|s| s.text.clone()),
            ..Options::default()
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
        self.forget_room();
    }

    /// What was known of the conversation's room in the context, for another conversation: its summary, what its last
    /// request took, that its last reply was cut off, and what was said of it.
    pub fn forget_room(&mut self) {
        self.summary = None;
        self.summary_open = false;
        self.summary_edit = None;
        self.used = None;
        self.cut_off = false;
        self.cut_by_context = false;
        self.room_note = None;
        self.too_long = false;
        self.retried = false;
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
/// is asked again without them, and that is said; one that refuses it as too long for its context is not. An error that
/// can pass is asked again after a wait first (`stream_reply_retried`).
pub fn spawn_reply_with_tools(
    port: u16,
    model: String,
    options: Options,
    turns: Vec<Turn>,
    tools: Option<serde_json::Value>,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Receiver<ChatEvent>, Arc<AtomicBool>) {
    spawn_request(wake, move |stop, send| {
        let body = request_body_with(&model, &options, &turns, tools.as_ref());
        let mut outcome = stream_reply_retried(port, &body, stop, send);
        if tools.is_some() {
            if let Err(failure) = &outcome {
                if failure.refused && failure.too_long.is_none() {
                    send(ChatEvent::Notice(format!(
                        "The engine did not take the tools ({}): this reply is without them.",
                        failure.why
                    )));
                    let body = request_body_with(&model, &options, &turns, None);
                    outcome = stream_reply_retried(port, &body, stop, send);
                }
            }
        }
        outcome
    })
}

/// How long the chat waits before it asks again after an error that can pass (C225): the engine could not be reached,
/// was busy (429) or failed on its side (5xx), before any of the reply came. Twice, as the OpenAI and Anthropic SDKs do;
/// then the error is said. Any other error is said at once: the same request would be refused the same way.
#[cfg(not(test))]
const RETRY_WAITS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];
#[cfg(test)]
const RETRY_WAITS: [Duration; 2] = [Duration::from_millis(20), Duration::from_millis(40)];

/// `stream_reply`, asked again after each of `RETRY_WAITS` while it fails in a way that can pass. A stop during a wait
/// ends it, as a stop during the reply does.
pub(super) fn stream_reply_retried(
    port: u16,
    body: &serde_json::Value,
    stop: &AtomicBool,
    said: &dyn Fn(ChatEvent),
) -> Result<Vec<AskedCall>, Failure> {
    let mut waits = RETRY_WAITS.iter();
    loop {
        let outcome = stream_reply(port, body, stop, said);
        let Err(failure) = &outcome else {
            return outcome;
        };
        let Some(wait) = waits.next().filter(|_| failure.may_pass) else {
            return outcome;
        };
        let until = Instant::now() + *wait;
        while Instant::now() < until {
            if stop.load(Ordering::SeqCst) {
                return Ok(Vec::new());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Runs `ask` on a thread of its own, what it says sent as events as it comes: then the tools it asked for and its end,
/// or why it failed (a request too long for the context said as such).
pub(super) fn spawn_request(
    wake: impl Fn() + Send + Sync + 'static,
    ask: impl FnOnce(&AtomicBool, &dyn Fn(ChatEvent)) -> Result<Vec<AskedCall>, Failure>
        + Send
        + 'static,
) -> (Receiver<ChatEvent>, Arc<AtomicBool>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let send = |event: ChatEvent| {
            let _ = tx.send(event);
            wake();
        };
        match ask(&stop, &send) {
            Ok(calls) => {
                if !calls.is_empty() {
                    send(ChatEvent::Calls(calls));
                }
                send(ChatEvent::Done);
            }
            Err(Failure {
                why,
                too_long: Some(context),
                ..
            }) => send(ChatEvent::TooLong { why, context }),
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

/// The arguments of a call, when they are what a tool takes: a JSON object (C225). A reply that the context or the reply
/// limit cut off ends in the middle of them, and a model can write them wrong.
pub fn call_arguments(text: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .filter(serde_json::Value::is_object)
}

/// `request_body`, with the tools offered (C204). A reply that asked for tools is said as it was asked, followed by what
/// came of each call. Arguments that are not a JSON object go back as `{}`: llama.cpp reads those of every call of the
/// conversation as JSON for templates such as Qwen's, and refuses the whole request when one is not (C225); what came
/// of the call tells the model why it did not run.
pub fn request_body_with(
    model: &str,
    options: &Options,
    turns: &[Turn],
    tools: Option<&serde_json::Value>,
) -> serde_json::Value {
    let system = system_text(options.instructions.as_deref(), options.summary.as_deref())
        .map(|text| serde_json::json!({"role": "system", "content": text}));
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
                        let arguments = match call_arguments(&c.arguments) {
                            Some(_) => c.arguments.as_str(),
                            None => "{}",
                        };
                        serde_json::json!({
                            "id": c.id,
                            "type": "function",
                            "function": {"name": c.name, "arguments": arguments},
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
    // what the request takes of the context, counted by the engine and said after the reply (`usage`)
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if let Some(tools) = tools {
        body["tools"] = tools.clone();
    }
    if let Some(t) = options.temperature {
        body["temperature"] = serde_json::json!((f64::from(t) * 100.0).round() / 100.0);
    }
    if let Some(n) = options.max_tokens {
        body["max_tokens"] = serde_json::json!(n);
    }
    thinking_words(&mut body, options.dialect, options.thinking);
    body
}

/// What the model reads before the conversation: the person's instructions, then the summary it reads in place of the
/// first turns (Auto-compact), introduced as such.
pub(super) fn system_text(instructions: Option<&str>, summary: Option<&str>) -> Option<String> {
    let instructions = instructions.map(str::trim).filter(|i| !i.is_empty());
    let summary = summary
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("{SUMMARY_LEAD}\n\n{s}"));
    match (instructions, summary) {
        (Some(i), Some(s)) => Some(format!("{i}\n\n{s}")),
        (Some(i), None) => Some(i.to_string()),
        (None, s) => s,
    }
}

/// What the model is told of the summary it reads in place of the first messages.
const SUMMARY_LEAD: &str =
    "The earlier part of this conversation was summarized to make room. This summary stands for those messages:";

/// Tells the engine whether the model is to think, in its own words: llama.cpp's chat templates read `enable_thinking`,
/// Ollama takes `reasoning_effort` `none` for no thinking (and thinks as the model does without it). Another server is
/// told nothing, nor is any while the person has not chosen.
pub(super) fn thinking_words(
    body: &mut serde_json::Value,
    dialect: Dialect,
    thinking: Option<bool>,
) {
    match (dialect, thinking) {
        (Dialect::LlamaCpp, Some(on)) => {
            body["chat_template_kwargs"] = serde_json::json!({"enable_thinking": on});
        }
        (Dialect::Ollama, Some(false)) => body["reasoning_effort"] = serde_json::json!("none"),
        _ => {}
    }
}

/// A context size as the panel says it: `4K` for 4096, the number itself when it is not a whole number of K.
pub(super) fn context_words(context: i64) -> String {
    if context >= 1024 && context % 1024 == 0 {
        format!("{}K", context / 1024)
    } else {
        context.to_string()
    }
}

/// What the chat says when a model thought and stopped before it answered, by why the engine ended the reply
/// (`finish_reason`): `length` with a reply limit is that limit, which Options raises; `length` with none is the context
/// (the room the conversation and the thinking share), which How it runs makes longer, as a new chat makes room in it;
/// anything else is the model's own end.
pub(super) fn stopped_thinking_words(
    seconds: u64,
    finish: Option<&str>,
    limit: Option<u32>,
    context: Option<i64>,
) -> String {
    match (finish, limit) {
        (Some("length"), Some(limit)) => format!(
            "It thought for {seconds} s and reached the reply limit ({limit} tokens) before it answered. Allow longer \
             answers in Options… (⋯ at the right of the bar), then send again."
        ),
        (Some("length"), None) => {
            let size = context
                .map(|c| format!(" ({} tokens)", context_words(c)))
                .unwrap_or_default();
            format!(
                "It thought for {seconds} s and ran out of context{size} before it answered: the conversation and its \
                 thinking fill it. Choose a longer context in How it runs, or start a new chat, then send again."
            )
        }
        _ => format!(
            "It thought for {seconds} s and stopped before it answered. Send it again, or ask in other words."
        ),
    }
}

/// What the chat says under an answer the engine cut off (`finish_reason` `length`): the reply limit the person set, or
/// the context, which Continue makes room in when Auto-compact is on (`compacts`). Continue is the button under it.
pub(super) fn cut_off_words(limit: Option<u32>, context: Option<i64>, compacts: bool) -> String {
    match limit {
        Some(limit) => format!(
            "The answer stopped at the reply limit ({limit} tokens). Continue goes on with it; Options… (⋯ at the \
             right of the bar) allows longer answers."
        ),
        None => {
            let size = context
                .map(|c| format!(" ({} tokens)", context_words(c)))
                .unwrap_or_default();
            if compacts {
                format!(
                    "The answer stopped when the context{size} was full. Continue summarizes earlier messages to make \
                     room, and goes on with it."
                )
            } else {
                format!(
                    "The answer stopped when the context{size} was full. Turn on Auto-compact in How it runs to make \
                     room, choose a longer context, or start a new chat."
                )
            }
        }
    }
}

/// What the chat says under a reply the engine cut off while the model asked for a tool (C225): the call is not whole,
/// so it did not run, and the chat stopped. With Auto-compact on, the next message makes room first.
pub(super) fn cut_call_words(limit: Option<u32>, context: Option<i64>, compacts: bool) -> String {
    match limit {
        Some(limit) => format!(
            "The model was asking for a tool when the reply reached its limit ({limit} tokens), so the tool did not \
             run. Options… (⋯ at the right of the bar) allows longer answers; then send a message to go on."
        ),
        None => {
            let size = context
                .map(|c| format!(" ({} tokens)", context_words(c)))
                .unwrap_or_default();
            if compacts {
                format!(
                    "The model was asking for a tool when the context{size} was full, so the tool did not run. Send \
                     a message to go on: Auto-compact summarizes earlier messages first to make room."
                )
            } else {
                format!(
                    "The model was asking for a tool when the context{size} was full, so the tool did not run. Turn \
                     on Auto-compact in How it runs to make room, choose a longer context, or start a new chat."
                )
            }
        }
    }
}

/// What a call that did not run says it came to, when the reply was cut off before the call was whole (C225).
pub(super) const CALL_CUT_OFF: &str =
    "The reply was cut off before this call was complete, so the tool did not run.";
/// … and when its arguments are not a JSON object, as a tool takes them.
pub(super) const CALL_NOT_JSON: &str =
    "The arguments of this call are not a valid JSON object, so the tool did not run.";

/// Why a request did not come to its end: the words, whether the engine refused it before any of it came, and whether
/// that is because it does not fit in the context (with the context's length, when the engine said it).
pub(super) struct Failure {
    pub why: String,
    pub refused: bool,
    pub too_long: Option<Option<u64>>,
    /// Before any of the reply came, the engine could not be reached, was busy or failed on its side: asked again a
    /// little later, the same request may be answered (C225).
    pub may_pass: bool,
}

impl Failure {
    fn on_the_way(why: String) -> Failure {
        Failure {
            why,
            refused: false,
            too_long: None,
            may_pass: false,
        }
    }
}

/// Whether an `error` says the request does not fit in the context, and the context's length when it says it
/// (llama.cpp: `exceed_context_size_error` with `n_ctx`; OpenAI's API: `context_length_exceeded`; others in words).
pub(super) fn too_long(value: &serde_json::Value) -> Option<Option<u64>> {
    let error = value.get("error")?;
    let said = |key: &str| {
        error
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    };
    let words = error_words(value).unwrap_or_default().to_lowercase();
    let is = said("type") == "exceed_context_size_error"
        || said("code") == "context_length_exceeded"
        || words.contains("exceeds the available context size")
        || words.contains("maximum context length")
        || words.contains("context length exceeded");
    is.then(|| error.get("n_ctx").and_then(serde_json::Value::as_u64))
}

/// The request and its stream of events (`data: {...}` lines, then `data: [DONE]`). An `error` the engine sends,
/// in the body of a refusal or in the stream, is the failure's sentence. A model that thinks first sends its thinking
/// apart from its reply (llama-server as `reasoning_content`, Ollama as `reasoning`): it is said as it comes, so that the
/// window shows the reply is on its way (and, when asked, what it thought). After the reply's end the engine says what
/// the request took of the context (`usage`), when it counts it.
pub(super) fn stream_reply(
    port: u16,
    body: &serde_json::Value,
    stop: &AtomicBool,
    said: &dyn Fn(ChatEvent),
) -> Result<Vec<AskedCall>, Failure> {
    // a request the stop closes at once, whatever the engine is doing (C224)
    let answer = match super::stoppable::post(port, "/v1/chat/completions", &body.to_string(), stop)
    {
        Ok(answer) => answer,
        Err(_) if stop.load(Ordering::SeqCst) => return Ok(Vec::new()),
        Err(e) => {
            return Err(Failure {
                may_pass: true,
                ..Failure::on_the_way(format!("could not reach the server: {e}"))
            })
        }
    };
    let status = answer.status;
    let reader = answer.body;
    if !(200..300).contains(&status) {
        let answer: serde_json::Value =
            serde_json::from_reader(reader).unwrap_or(serde_json::Value::Null);
        let too_long = too_long(&answer);
        return Err(Failure {
            why: error_words(&answer).unwrap_or_else(|| format!("the server answered {status}")),
            refused: true,
            may_pass: too_long.is_none() && (status == 429 || status >= 500),
            too_long,
        });
    }
    // the tool calls, as they come in pieces: by their index, the id, the name and the arguments so far
    let mut calls: Vec<AskedCall> = Vec::new();
    // the reply ended (`finish_reason`): only what it took of the context is still to come
    let mut finished = false;
    for line in reader.lines() {
        if stop.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        let line = match line {
            Ok(line) => line,
            Err(_) if stop.load(Ordering::SeqCst) => return Ok(Vec::new()),
            Err(e) => return Err(Failure::on_the_way(format!("the reply stopped: {e}"))),
        };
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
            return Err(Failure {
                why,
                refused: false,
                too_long: too_long(&event),
                may_pass: false,
            });
        }
        let counted = event.get("usage").and_then(|u| {
            Some((
                u.get("prompt_tokens")?.as_u64()?,
                u.get("completion_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            ))
        });
        if let Some((prompt, completion)) = counted {
            said(ChatEvent::Usage { prompt, completion });
        }
        if finished {
            if counted.is_some() {
                break;
            }
            continue;
        }
        gather_calls(&event, &mut calls);
        let delta = |key: &str| {
            event
                .pointer(&format!("/choices/0/delta/{key}"))
                .and_then(serde_json::Value::as_str)
                .filter(|p| !p.is_empty())
        };
        if let Some(thought) = delta("reasoning_content").or(delta("reasoning")) {
            said(ChatEvent::Thinking(thought.to_string()));
        }
        if let Some(piece) = delta("content") {
            said(ChatEvent::Text(piece.to_string()));
        }
        if let Some(why) = event
            .pointer("/choices/0/finish_reason")
            .filter(|r| !r.is_null())
        {
            said(ChatEvent::Finished(
                why.as_str().unwrap_or_default().to_string(),
            ));
            finished = true;
            if counted.is_some() {
                break;
            }
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

/// One line of text with its spans, in the size and colour given; the part of it that is `lit` (the sentence being
/// read aloud, C216) has a background.
fn show_spans(ui: &mut egui::Ui, text: &str, size: Option<f32>, strong: bool, lit: Option<&str>) {
    let p = theme::palette();
    let mut job = egui::text::LayoutJob::default();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let pieces = spans(text);
    // where the sentence is in the words of the line, in bytes of them all
    let plain: String = pieces.iter().map(|(_, w)| w.as_str()).collect();
    let lit_range = lit.and_then(|sentence| lit_range(&plain, sentence));
    let mut at = 0;
    for (span, words) in pieces {
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
        let end = at + words.len();
        match lit_range.filter(|(from, to)| *from < end && *to > at) {
            Some((from, to)) => {
                // the span cut where the sentence begins and ends in it, its middle lit
                let a = from.saturating_sub(at).min(words.len());
                let b = (to - at).min(words.len());
                let (a, b) = (floor_char(&words, a), floor_char(&words, b));
                if a > 0 {
                    job.append(&words[..a], 0.0, format.clone());
                }
                let mut lit_format = format.clone();
                lit_format.background = p.accent_text.gamma_multiply(0.22);
                lit_format.color = p.text;
                job.append(&words[a..b], 0.0, lit_format);
                if b < words.len() {
                    job.append(&words[b..], 0.0, format);
                }
            }
            None => job.append(&words, 0.0, format),
        }
        at = end;
    }
    job.wrap.max_width = ui.available_width();
    // wrapped whatever the layout it is in: a row (an item of a list, a quote) would let it run on out of the bubble
    // and widen the page
    ui.add(egui::Label::new(job).selectable(true).wrap());
}

/// Where `sentence` (as it is read aloud: without the marks of Markdown) is in `plain` (the words of a line), as byte
/// offsets; the start of a sentence that goes on to the next line lights to the end of this one.
fn lit_range(plain: &str, sentence: &str) -> Option<(usize, usize)> {
    // its last mark is not looked for: a piece of a sentence cut at a dash ends in a comma the answer does not have
    let sentence = sentence.trim().trim_end_matches(['.', ',', ';', ':']);
    if sentence.is_empty() {
        return None;
    }
    if let Some(at) = plain.find(sentence) {
        return Some((at, at + sentence.len()));
    }
    // the first words of it, at least a few
    let head: String = sentence.chars().take(40).collect();
    let head = head.trim_end();
    (head.chars().count() >= 12)
        .then(|| plain.find(head))
        .flatten()
        .map(|at| (at, plain.len()))
}

/// The largest char boundary of `text` at or before `at`.
fn floor_char(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// What was pressed on the summary's line, or under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SummaryAction {
    Toggle,
    Copy,
    Edit,
    Save,
    Cancel,
}

/// The line where the model's view begins (C223): how many messages above it the summary stands for, and Show summary;
/// opened, the summary (what the model reads in their place) with Copy and Edit, or its box with Save and Cancel while
/// it is edited.
fn show_summary_line(
    ui: &mut egui::Ui,
    summary: &super::compact::Summary,
    open: bool,
    edit: Option<&mut String>,
    width: f32,
) -> Option<SummaryAction> {
    let p = theme::palette();
    let mut action = None;
    // the words that fit the column: shorter ones where it is narrow (a list of kept chats beside it, the panel open)
    let n = summary.upto;
    let messages = if n == 1 {
        "1 earlier message".to_string()
    } else {
        format!("{n} earlier messages")
    };
    let (show, hide) = ("Show summary", "Hide summary");
    let choices = [
        (format!("{messages} summarized to make room"), show, hide),
        (format!("{messages} summarized"), show, hide),
        (format!("{n} summarized"), "Show", "Hide"),
    ];
    let font = egui::FontId::proportional(12.0);
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), p.muted)
            .size()
            .x
    };
    let fitting = choices
        .iter()
        .find(|(said, show, hide)| {
            18.0 + measure(said) + 16.0 + measure(if open { hide } else { show }) <= width - 24.0
        })
        .unwrap_or(&choices[2]);
    let said = fitting.0.clone();
    let link = if open { fitting.2 } else { fitting.1 };
    let said_width = measure(&said);
    let link_width = measure(link);
    let middle = 18.0 + said_width + 16.0 + link_width;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
    response.on_hover_text(
        "Above this line the model reads a summary in place of the messages; below it, every word. The model wrote the \
         summary itself, so it may leave out a detail: open it to read or correct it.",
    );
    let left = (rect.center().x - middle / 2.0).max(rect.left() + 8.0);
    let y = rect.center().y;
    let stroke = egui::Stroke::new(1.0, p.line);
    if left - 10.0 > rect.left() + 8.0 {
        ui.painter()
            .hline((rect.left() + 8.0)..=(left - 10.0), y, stroke);
    }
    let end = left + middle + 10.0;
    if end < rect.right() - 8.0 {
        ui.painter().hline(end..=(rect.right() - 8.0), y, stroke);
    }
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(egui::pos2(left + 6.5, y), egui::vec2(13.0, 13.0)),
        Icon::Fold,
        p.muted,
    );
    let mut x = left + 18.0;
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        &said,
        font.clone(),
        p.muted,
    );
    x += said_width + 5.0;
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        "·",
        font.clone(),
        p.faint,
    );
    x += 11.0;
    let link_rect = egui::Rect::from_min_size(egui::pos2(x, y - 9.0), egui::vec2(link_width, 18.0));
    let pressed = ui.interact(
        link_rect,
        ui.id().with("summary_link"),
        egui::Sense::click(),
    );
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        link,
        font,
        if pressed.hovered() {
            p.text
        } else {
            p.accent_text
        },
    );
    if pressed.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if pressed.clicked() {
        action = Some(SummaryAction::Toggle);
    }
    if !open {
        return action;
    }
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(p.bar)
            .stroke(egui::Stroke::new(1.0, p.line))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                // as wide as the column leaves it (the room at the left, the frame's margins), and one thing under another
                ui.set_width((width - 46.0).max(120.0));
                ui.vertical(|ui| {
                    let counted = match (summary.before, summary.after) {
                        (Some(before), Some(after)) => format!(
                            " (about {} tokens, now {})",
                            super::compact::tokens_words(before),
                            super::compact::tokens_words(after)
                        ),
                        _ => String::new(),
                    };
                    ui.add(
                        egui::Label::new(
                            theme::muted(format!(
                                "What the model reads in place of the messages above{counted}:"
                            ))
                            .small(),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                    match edit {
                        Some(text) => {
                            ui.add(
                                egui::TextEdit::multiline(text)
                                    .desired_rows(6)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.horizontal(|ui| {
                                if theme::button_sized(
                                    ui,
                                    Role::Normal,
                                    true,
                                    "Save",
                                    egui::vec2(0.0, 20.0),
                                )
                                .on_hover_text("The model reads it so from the next message on.")
                                .clicked()
                                {
                                    action = Some(SummaryAction::Save);
                                }
                                if theme::button_sized(
                                    ui,
                                    Role::Quiet,
                                    true,
                                    "Cancel",
                                    egui::vec2(0.0, 20.0),
                                )
                                .clicked()
                                {
                                    action = Some(SummaryAction::Cancel);
                                }
                            });
                        }
                        None => {
                            show_message(ui, &summary.text, None);
                            ui.horizontal(|ui| {
                                if theme::button_sized(
                                    ui,
                                    Role::Quiet,
                                    true,
                                    "Copy",
                                    egui::vec2(0.0, 18.0),
                                )
                                .on_hover_text("Copy the summary.")
                                .clicked()
                                {
                                    action = Some(SummaryAction::Copy);
                                }
                                if theme::button_sized(
                                    ui,
                                    Role::Quiet,
                                    true,
                                    "Edit",
                                    egui::vec2(0.0, 18.0),
                                )
                                .on_hover_text(
                                    "Correct the summary: the model reads it as you leave it.",
                                )
                                .clicked()
                                {
                                    action = Some(SummaryAction::Edit);
                                }
                            });
                        }
                    }
                });
            });
    });
    ui.add_space(4.0);
    action
}

/// What a model thought before its answer, folded above it (C223): its head (`Thought for 14 s`, or how long it has been
/// thinking), which opens it; opened, the thought in small, faint words beside a line.
fn show_thought(ui: &mut egui::Ui, turn: usize, head: &str, thought: &str) {
    let p = theme::palette();
    let id = egui::Id::new(("thought_open", turn));
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    // a row of its own id: its words change every second while the model thinks, and a click must still reach it
    let font = egui::FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(head.to_string(), font, p.muted);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(18.0 + galley.size().x + 4.0, 20.0),
        egui::Sense::hover(),
    );
    let row = ui
        .interact(
            rect,
            egui::Id::new(("thought_fold", turn)),
            egui::Sense::click(),
        )
        .on_hover_text(if open {
            "Hide what it thought."
        } else {
            "Show what it thought before this answer."
        });
    let color = if row.hovered() { p.text } else { p.muted };
    let icon = if open {
        Icon::Chevron
    } else {
        Icon::ChevronRight
    };
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 6.0, rect.center().y),
            egui::vec2(11.0, 11.0),
        ),
        icon,
        color,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 18.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if row.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if row.clicked() {
        ui.data_mut(|d| d.insert_temp(id, !open));
    }
    if open {
        let top = ui.cursor().top();
        let shown = ui
            .horizontal(|ui| {
                ui.add_space(10.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(thought.trim())
                            .small()
                            .italics()
                            .color(p.faint),
                    )
                    .wrap(),
                );
            })
            .response
            .rect;
        ui.painter().vline(
            shown.left() + 3.0,
            top..=shown.bottom(),
            egui::Stroke::new(2.0, p.line),
        );
    }
    ui.add_space(4.0);
}

/// What goes between an answer that stopped and what Continue brings: nothing where either side has the space or the
/// mark itself, a new paragraph where the model begins anew (a capital, a list, a heading), else a space.
pub(super) fn joint(before: &str, after: &str) -> &'static str {
    let ended = before.chars().last();
    let begins = after.chars().next();
    match (ended, begins) {
        (None, _) | (_, None) => "",
        (Some(e), _) if e.is_whitespace() => "",
        (_, Some(b)) if b.is_whitespace() || ",.;:!?)»”’".contains(b) => "",
        (_, Some(b)) if b.is_uppercase() || "#*-•|>".contains(b) || b.is_ascii_digit() => "\n\n",
        _ => " ",
    }
}

/// A note under the conversation that informs: a grey (i), then its words, their lines under the first.
fn info_note(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.add_space(8.0);
        info_mark(ui);
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 8.0).max(80.0));
            ui.add(egui::Label::new(theme::muted(text).small()).wrap());
        });
    });
}

/// The stop of an answer, in the box where the arrow is while the answer is written (C224): a round button with a square
/// in it, as chat applications have it (light on the dark theme, dark on the light one).
fn stop_button(ui: &mut egui::Ui) -> egui::Response {
    let p = theme::palette();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 28.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let round = if response.hovered() {
            p.text
        } else {
            p.text.gamma_multiply(0.86)
        };
        ui.painter().circle_filled(rect.center(), 12.5, round);
        ui.painter().rect_filled(
            egui::Rect::from_center_size(rect.center(), egui::vec2(9.0, 9.0)),
            2.0,
            p.well,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A grey (i) before a note that informs.
fn info_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    icons::paint(
        ui.painter(),
        rect.shrink(1.0),
        Icon::Info,
        theme::palette().faint,
    );
}

/// A note over the box that informs, small and grey after a grey (i), in a row with what goes with the message; the
/// pointer on either says `more`.
fn given_note(ui: &mut egui::Ui, text: &str, more: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (rect, mark) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        icons::paint(
            ui.painter(),
            rect.shrink(1.0),
            Icon::Info,
            theme::palette().faint,
        );
        mark.on_hover_text(more);
        ui.add(egui::Label::new(theme::muted(text).small()).wrap())
            .on_hover_text(more);
    });
}

/// The middle square of a picture of `size`, as a tile shows it.
fn middle_square(size: [usize; 2]) -> egui::Rect {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let (dx, dy) = if w > h {
        ((1.0 - h / w) / 2.0, 0.0)
    } else {
        (0.0, (1.0 - w / h) / 2.0)
    };
    egui::Rect::from_min_max(egui::pos2(dx, dy), egui::pos2(1.0 - dx, 1.0 - dy))
}

/// A picture that goes with the next message (C226, B): a square of its middle, its name when the pointer is on it,
/// and a round × in its corner that takes it off (true when clicked).
fn picture_tile(ui: &mut egui::Ui, picture: &Picture, at: usize) -> bool {
    let p = theme::palette();
    let (rect, tile) = ui.allocate_exact_size(egui::vec2(60.0, 60.0), egui::Sense::hover());
    match &picture.preview.0 {
        Some(texture) => {
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(texture))
                .uv(middle_square(texture.size()))
                .corner_radius(7)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 7.0, p.card);
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(rect.center(), egui::vec2(20.0, 20.0)),
                Icon::Image,
                p.muted,
            );
        }
    }
    ui.painter().rect_stroke(
        rect,
        7.0,
        egui::Stroke::new(1.0, p.line),
        egui::StrokeKind::Inside,
    );
    let spot = egui::Rect::from_center_size(
        rect.right_top() + egui::vec2(-11.0, 11.0),
        egui::vec2(17.0, 17.0),
    );
    let close = ui.interact(spot, ui.id().with(("given off", at)), egui::Sense::click());
    ui.painter().circle_filled(
        spot.center(),
        8.5,
        p.well
            .gamma_multiply(if close.hovered() { 1.0 } else { 0.85 }),
    );
    let arm = 3.0;
    let stroke = egui::Stroke::new(1.4, p.text);
    let c = spot.center();
    ui.painter().line_segment(
        [c + egui::vec2(-arm, -arm), c + egui::vec2(arm, arm)],
        stroke,
    );
    ui.painter().line_segment(
        [c + egui::vec2(arm, -arm), c + egui::vec2(-arm, arm)],
        stroke,
    );
    if close.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else {
        tile.on_hover_text(&picture.name);
    }
    close.on_hover_text("Take it off this message.").clicked()
}

/// Over the chat while files are dragged over the window (C226, D1): it darkens under a dashed frame that says what a
/// drop does; with no model running, that one must run first.
fn paint_drop_cover(ctx: &egui::Context, rect: egui::Rect, ready: bool) {
    let p = theme::palette();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("drop cover"),
    ));
    painter.rect_filled(rect, 0.0, p.page.gamma_multiply(0.9));
    let zone = rect.shrink(12.0);
    let corners = [
        zone.left_top(),
        zone.right_top(),
        zone.right_bottom(),
        zone.left_bottom(),
        zone.left_top(),
    ];
    let color = if ready { p.accent_text } else { p.faint };
    painter.extend(egui::Shape::dashed_line(
        &corners,
        egui::Stroke::new(2.0, color),
        8.0,
        6.0,
    ));
    let middle = zone.center();
    icons::paint(
        &painter,
        egui::Rect::from_center_size(middle - egui::vec2(0.0, 34.0), egui::vec2(34.0, 34.0)),
        Icon::Download,
        color,
    );
    let (big, small) = if ready {
        (
            "Drop to add to your message",
            "Pictures go with it; text files go into it.",
        )
    } else {
        ("Start a model to add files to a message", "")
    };
    painter.text(
        middle + egui::vec2(0.0, 6.0),
        egui::Align2::CENTER_CENTER,
        big,
        egui::FontId::proportional(17.0),
        p.text,
    );
    painter.text(
        middle + egui::vec2(0.0, 30.0),
        egui::Align2::CENTER_CENTER,
        small,
        egui::FontId::proportional(12.5),
        p.muted,
    );
}

/// One message: its lines as Markdown has them, and its code blocks in monospace; the sentence being read aloud lit.
fn show_message(ui: &mut egui::Ui, text: &str, lit: Option<&str>) {
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
                    show_spans(ui, words, Some(size), true, lit);
                }
                Line::Item(mark, words) => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(mark).color(p.muted));
                        show_spans(ui, words, None, false, lit);
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
                        show_spans(ui, words, None, false, lit);
                    });
                }
                Line::Plain(words) if words.trim().is_empty() => {
                    ui.add_space(4.0);
                }
                Line::Plain(words) => show_spans(ui, words, None, false, lit),
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

/// A turn as a chat keeps it: who said it, the words, and for a reply what the model thought.
fn saved_turn(t: &Turn) -> SavedTurn {
    SavedTurn {
        from_person: t.from_person,
        text: t.text.clone(),
        thought: t.thought.clone(),
    }
}

impl App {
    /// Whether there is an answer to stop (C224): from the message sent (the MCP servers starting, a summary that makes
    /// room) to the answer's end, and while its tools run before the next reply.
    pub(super) fn chat_busy(&self) -> bool {
        self.chat.replying() || self.chat.tools_waiting || self.chat.waiting_for_tools
    }

    /// Esc stops the answer (C224) while the chat is what the window shows and Esc closes nothing else there (a menu, the
    /// tour of Help). Whether there was one to stop.
    pub(super) fn escape_answer(&mut self, ctx: &egui::Context) -> bool {
        let shown = self.view == super::MainView::Model || self.setup_in_use().is_some();
        if !self.chat_busy() || !shown || egui::Popup::is_any_open(ctx) || self.help.tour.is_some()
        {
            return false;
        }
        self.chat.stop();
        self.poll_chat();
        true
    }

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
    pub(super) fn chat_model(&self) -> Option<String> {
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
        // for Up in the box: once, however many times in a row it was sent
        if self.chat.sent.last() != Some(&text) {
            self.chat.sent.push(text.clone());
            if self.chat.sent.len() > MOST_SENT {
                self.chat.sent.remove(0);
            }
        }
        self.chat.recalled = None;
        let mut turn = Turn::said(true, text);
        turn.pictures = std::mem::take(&mut self.chat.pictures);
        self.chat.given_notes.clear();
        self.chat.turns.push(turn);
        self.chat.tool_rounds = 0;
        // a message the engine refuses as too long is summarized and asked again, once
        self.chat.retried = false;
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
    pub(super) fn chat_target(&self) -> Option<(u16, String)> {
        if self.setup_in_use().is_some() {
            return self.setup_run()?.chat_target();
        }
        let port = self
            .session
            .port()
            .filter(|_| self.session.state() == ServerState::Running)?;
        Some((port, self.session.model_name()?.to_string()))
    }

    /// Asks for the reply to the conversation as it stands, offering the tools of the MCP servers that are ready. With
    /// Auto-compact on, a conversation that would leave the reply too little room in the context is summarized first,
    /// and the reply asked for once the summary came (`compact.rs`). The model reads the turns after the summary whole;
    /// Continue asks it to go on with its last reply, which the words that come are added to.
    pub(super) fn ask_for_reply(&mut self, ctx: &egui::Context) {
        let Some((port, model)) = self.chat_target() else {
            return;
        };
        self.chat.error = None;
        if self.stored.auto_compact() && self.needs_room() && self.start_summary(ctx, true) {
            return;
        }
        let (tools, names) = self.offered_tools();
        self.chat.tool_names = names;
        let options = self.reply_options();
        let mut turns = self.chat.turns[self.chat.summarized_upto()..].to_vec();
        if self.chat.continuing {
            turns.push(Turn::said(true, super::compact::CONTINUE_ASK));
            if let Some(last) = self.chat.turns.last_mut().filter(|t| !t.from_person) {
                last.stopped = false;
            }
        }
        self.chat.asked_chars = super::compact::request_chars(&options, &turns);
        self.chat.asked_tokens = None;
        let ctx = ctx.clone();
        let (rx, cancel) = spawn_reply_with_tools(port, model, options, turns, tools, move || {
            ctx.request_repaint()
        });
        if !self.chat.continuing {
            self.chat.turns.push(Turn::said(false, ""));
        }
        self.chat.cut_off = false;
        self.chat.room_note = None;
        self.chat.reply = Some(rx);
        self.chat.cancel = Some(cancel);
        self.chat.started = Some(Instant::now());
        self.chat.first = None;
        self.chat.pieces = 0;
        self.chat.thinking = None;
        self.chat.finish = None;
        // what was said under the last reply (cut off, tools not taken) is about that reply
        self.chat.notice = None;
    }

    /// How much more the next message takes: pictures, and text in the box.
    fn given_room(&self) -> given::Room {
        given::Room {
            pictures: given::MOST_PICTURES.saturating_sub(self.chat.pictures.len()),
            text: (MOST_TEXT as usize).saturating_sub(self.chat.input.len()),
        }
    }

    /// Reads what the person gave the next message (C226), away from the window's thread: it is taken as it is ready.
    pub(super) fn give(&mut self, source: given::Source, ctx: &egui::Context) {
        let room = self.given_room();
        self.chat.reading.push(given::start(source, room, ctx));
    }

    /// What was given and is read by now: a picture goes with the next message, with its preview; text goes in the box.
    /// What a message no longer takes (another reading may have filled it meanwhile) is counted, and said.
    pub(super) fn take_given(&mut self, ctx: &egui::Context) {
        let mut taken = Vec::new();
        self.chat.reading.retain(|reading| loop {
            match reading.rx.try_recv() {
                Ok(t) => taken.push(t),
                Err(crossbeam_channel::TryRecvError::Empty) => break true,
                Err(crossbeam_channel::TryRecvError::Disconnected) => break false,
            }
        });
        let mut left = 0;
        for t in taken {
            match t {
                given::Taken::Picture(mut picture, preview) => {
                    if self.chat.pictures.len() >= given::MOST_PICTURES {
                        left += 1;
                        continue;
                    }
                    if picture.name.is_empty() {
                        self.chat.pasted += 1;
                        picture.name = format!("Pasted picture {}", self.chat.pasted);
                    }
                    picture.preview = Preview(preview.map(|image| {
                        ctx.load_texture(
                            format!("given {}", picture.name),
                            image,
                            egui::TextureOptions::LINEAR,
                        )
                    }));
                    self.chat.pictures.push(picture);
                }
                given::Taken::Text(block) => {
                    if self.chat.input.len() + block.len() + 2 > MOST_TEXT as usize {
                        let name = block.split_once(":\n").map_or("The file", |(n, _)| n);
                        self.chat.given_notes.push(format!(
                            "{name} would make the message longer than 256 KB of text."
                        ));
                        continue;
                    }
                    if !self.chat.input.trim().is_empty() {
                        self.chat.input.push_str("\n\n");
                    }
                    self.chat.input.push_str(&block);
                }
                given::Taken::Refused(why) => self.chat.given_notes.push(why),
                given::Taken::Left(n) => left += n,
            }
        }
        if left > 0 {
            self.chat.given_notes.push(format!(
                "{left} more {} not added: a message takes {} pictures and 256 KB of text at most.",
                if left == 1 { "file was" } else { "files were" },
                given::MOST_PICTURES
            ));
        }
    }

    /// Ctrl+V and files dropped on the window, for the next message (C226), while a model runs. A paste of text is the
    /// box's own, unless it is what a file manager pastes for copied files (the files are then taken), or longer than a
    /// message takes. A paste that gave no text (egui sees only the release of its V) may be a picture or copied files:
    /// the clipboard is read for them, unless another field has the keys.
    fn take_paste_and_drop(&mut self, ui: &mut egui::Ui, ready: bool) {
        let input_id = egui::Id::new("chat_input");
        let focused = ui.memory(|m| m.focused());
        let elsewhere = focused.is_some_and(|id| id != input_id);
        let mut texts = Vec::new();
        let mut released = false;
        ui.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::Key {
                        key: egui::Key::V,
                        pressed,
                        ..
                    } => {
                        if *pressed {
                            self.chat.v_down = true;
                        } else {
                            released |= !self.chat.v_down && !self.chat.v_pasted;
                            self.chat.v_down = false;
                            self.chat.v_pasted = false;
                        }
                    }
                    egui::Event::Paste(text) => {
                        self.chat.v_pasted = true;
                        texts.push(text.clone());
                    }
                    _ => {}
                }
            }
        });
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !ready {
            return;
        }
        if !dropped.is_empty() {
            self.give(given::Source::Files(dropped), ui.ctx());
        }
        if focused == Some(input_id) {
            for text in texts {
                let files = given::files_behind(&text);
                let too_long =
                    files.is_none() && self.chat.input.len() + text.len() > MOST_TEXT as usize;
                if files.is_none() && !too_long {
                    continue;
                }
                ui.input_mut(|i| {
                    i.events
                        .retain(|e| !matches!(e, egui::Event::Paste(t) if *t == text))
                });
                match files {
                    Some(files) => self.give(given::Source::Files(files), ui.ctx()),
                    None => self.chat.given_notes.push(
                        "The text on the clipboard is too long to paste: a message takes 256 KB of text at most."
                            .into(),
                    ),
                }
            }
        }
        if released && !elsewhere {
            self.give(given::Source::Clipboard, ui.ctx());
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
                    || (self.projector_on()
                        && m.companions
                            .iter()
                            .any(|c| c.kind == "mmproj" && c.exists()))
            })
    }

    /// The last reply, asked for again.
    pub(super) fn regenerate_chat(&mut self, ctx: &egui::Context) {
        if self.chat.replying() || !self.chat.turns.last().is_some_and(|t| !t.from_person) {
            return;
        }
        self.chat.turns.pop();
        self.chat.retried = false;
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
            self.chat.cut_off = false;
            // a summary of turns that are gone stands for nothing (the last message is never in one)
            if self
                .chat
                .summary
                .as_ref()
                .is_some_and(|s| s.upto > self.chat.turns.len())
            {
                self.chat.summary = None;
            }
        }
    }

    /// Each frame: what the reply has said since, and its end. A server that stopped ends it.
    pub(super) fn poll_chat(&mut self) {
        // the run the chat talks to stopped: the one here, or the one on the computer QuickAI set up (where nothing
        // runs here, so this computer's server being stopped says nothing of it)
        if self.chat.replying() && !self.chat_running() {
            self.chat.stop();
        }
        let Some(rx) = &self.chat.reply else {
            return;
        };
        let mut ended = false;
        // a request the engine refused as too long, and the length of the context when it said it
        let mut too_long: Option<Option<u64>> = None;
        while let Ok(event) = rx.try_recv() {
            match event {
                ChatEvent::Thinking(piece) => {
                    if self.chat.first.is_none() && self.chat.thinking.is_none() {
                        self.chat.thinking = Some(Instant::now());
                    }
                    if let Some(last) = self.chat.turns.last_mut().filter(|t| !t.from_person) {
                        last.thought.push_str(&piece);
                    }
                }
                ChatEvent::Text(piece) => {
                    if self.chat.first.is_none() {
                        self.chat.first = Some(Instant::now());
                    }
                    let thought_for = self.chat.thinking.take().map(|t| t.elapsed().as_secs());
                    let first_of_more = self.chat.continuing && self.chat.pieces == 0;
                    self.chat.pieces += 1;
                    if let Some(last) = self.chat.turns.last_mut().filter(|t| !t.from_person) {
                        if thought_for.is_some() {
                            last.thought_secs = thought_for;
                        }
                        // what Continue brings goes on from where the answer stopped
                        if first_of_more {
                            last.text.push_str(joint(&last.text, &piece));
                        }
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
                ChatEvent::Finished(why) => self.chat.finish = Some(why),
                ChatEvent::Usage { prompt, .. } => self.chat.asked_tokens = Some(prompt),
                ChatEvent::Done => ended = true,
                ChatEvent::Failed(why) => {
                    self.chat.error = Some(why);
                    ended = true;
                }
                ChatEvent::TooLong { context, .. } => {
                    too_long = Some(context);
                    ended = true;
                }
            }
        }
        if !ended {
            return;
        }
        self.chat.reply = None;
        self.chat.cancel = None;
        let continued = std::mem::take(&mut self.chat.continuing);
        // a model that only thought stopped before its answer: an empty reply would be dropped unsaid, so why it
        // stopped is said, by what the engine gave as the reason (the limit the person set, or the context filled)
        let finish = self.chat.finish.take();
        let limit = self.chat.max_tokens;
        let context = self.chat_context();
        let words_of = context.and_then(|c| i64::try_from(c).ok());
        let compacts = self.stored.auto_compact();
        let nothing = self
            .chat
            .turns
            .last()
            .is_some_and(|t| !t.from_person && t.text.is_empty() && t.calls.is_empty());
        if let Some(said_context) = too_long {
            // the engine said how long its context is: it is what the next request is reckoned against
            if let Some(n) = said_context {
                self.note_context(n);
            }
            if compacts && !self.chat.retried {
                // made room for and asked again, once (`poll_capabilities`): as it was asked, Continue too
                self.chat.too_long = true;
                self.chat.continuing = continued;
            } else {
                self.chat.error = Some(super::compact::too_long_words(context, compacts));
            }
        }
        let cut = self.chat.error.is_none() && finish.as_deref() == Some("length");
        // a reply cut off while it asked for tools: the calls are not whole, and do not run (C225)
        let calls_cut = cut
            && self
                .chat
                .turns
                .last()
                .is_some_and(|t| !t.from_person && !t.calls.is_empty());
        if let Some(since) = self.chat.thinking.take() {
            let secs = since.elapsed().as_secs();
            if let Some(last) = self.chat.turns.last_mut().filter(|t| !t.from_person) {
                last.thought_secs = Some(secs);
            }
            if nothing && self.chat.error.is_none() && !self.chat.too_long {
                self.chat.error = Some(stopped_thinking_words(
                    secs,
                    finish.as_deref(),
                    limit,
                    words_of,
                ));
            }
        }
        if calls_cut {
            // the chat stops there, and says why and what makes room (a model that thought, then asked, is cut off too)
            self.chat.notice = Some(cut_call_words(limit, words_of, compacts));
        } else if !nothing && cut {
            // an answer that was cut off says so, and why; Continue goes on with it
            self.chat.cut_off = true;
            self.chat.cut_by_context = limit.is_none();
            self.chat.notice = Some(cut_off_words(limit, words_of, compacts));
        }
        // what the request took of the context, as the engine counted it, with the words of the reply
        if let Some(prompt) = self
            .chat
            .asked_tokens
            .take()
            .filter(|_| self.chat.error.is_none())
        {
            let used =
                super::compact::Used::counted(prompt, self.chat.pieces, self.chat.asked_chars);
            // (an answer the full context cut off says it already)
            if !compacts && !(self.chat.cut_off && self.chat.cut_by_context) {
                self.chat.room_note = context
                    .filter(|c| used.tokens as f64 >= *c as f64 * super::compact::NEARLY_FULL)
                    .map(|c| (used.tokens, c));
            }
            self.chat.used = Some(used);
        }
        let stats = self.chat.stats_now();
        match self.chat.turns.last_mut() {
            // a reply that said nothing (and asked for no tool) is not kept as a turn
            Some(last) if !last.from_person && last.text.is_empty() && last.calls.is_empty() => {
                self.chat.turns.pop();
            }
            // what Continue added is counted with what came before it
            Some(last) if !last.from_person && continued => {
                last.stats = match (last.stats, stats) {
                    (Some(before), Some(now)) => Some(Stats {
                        tokens: before.tokens + now.tokens,
                        first_ms: before.first_ms,
                        total_ms: before.total_ms + now.total_ms,
                    }),
                    (before, now) => now.or(before),
                };
            }
            Some(last) if !last.from_person => last.stats = stats,
            _ => {}
        }
        // the tools it asked for: those allowed always run, the others wait for the person (C204)
        self.settle_new_calls(calls_cut);
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
    pub(super) fn keep_conversation(&mut self) {
        if !self.chat.keep || self.chat.turns.is_empty() {
            return;
        }
        let data = self.data_dir().to_path_buf();
        let id = self
            .chat
            .saved_id
            .clone()
            .unwrap_or_else(|| chats::new_id(&data));
        let turns: Vec<SavedTurn> = self.chat.turns.iter().map(saved_turn).collect();
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
            summary: self.saved_summary(),
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
        self.chat.forget_room();
        self.chat.turns = saved
            .turns
            .into_iter()
            .map(|t| Turn {
                thought: t.thought,
                ..Turn::said(t.from_person, t.text)
            })
            .collect();
        // the summary goes on standing for the turns it was made of
        self.chat.summary = saved
            .summary
            .filter(|s| s.upto > 0 && s.upto < self.chat.turns.len())
            .map(|s| super::compact::Summary {
                text: s.text,
                upto: s.upto,
                before: None,
                after: None,
            });
        self.chat.instructions = saved.instructions;
        self.chat.saved_id = Some(saved.id);
        self.chat.error = None;
    }

    /// The summary as it is kept with the chat.
    fn saved_summary(&self) -> Option<chats::SavedSummary> {
        self.chat.summary.as_ref().map(|s| chats::SavedSummary {
            text: s.text.clone(),
            upto: s.upto,
        })
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
        let turns: Vec<SavedTurn> = self.chat.turns.iter().map(saved_turn).collect();
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
            summary: self.saved_summary(),
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
                 kept. Either way, a conversation goes only to the model you chat with.",
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
        // hands-free (C216): it listens, sends, reads the answer and listens again; only once voice is turned on
        if super::talk::CAN_HEAR && self.talk.settings.enabled {
            let can_talk = self.chat_running() && self.talk.hands_free.is_none();
            let hands_free = super::icons::menu_item(ui, super::icons::Icon::Wave, "Hands-free", can_talk, false)
                .on_hover_text(
                    "Talk to the model with nothing to press: it listens, sends what you said when you pause, reads the \
                     answer aloud and listens again (Ctrl+Shift+Space). Esc goes back to the chat.",
                );
            if hands_free.clicked() && can_talk {
                ui.close();
                self.start_hands_free(ui.ctx());
            }
        }
        // the summary that makes room, written now (C223): whether Auto-compact is on or not
        let can_summarize = self.chat_running() && !self.chat.replying() && self.can_summarize();
        let summarize = super::icons::menu_item(
            ui,
            super::icons::Icon::Fold,
            "Summarize earlier messages",
            can_summarize,
            false,
        )
        .on_hover_text(
            "Make room in the context now: the model summarizes the earlier messages, and reads the summary in their \
             place. You still see them all.",
        );
        if summarize.clicked() && can_summarize {
            ui.close();
            self.summarize_now(ui.ctx());
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
        // the whole of the chat, which hands-free covers (C216)
        let chat_rect = ui.max_rect();
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
        // Read aloud or Stop reading under an answer (C216): its turn, and which
        let mut read: Option<(usize, bool)> = None;
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
        // the summary's line and what was pressed on it, Continue, and the link of the note that the chat is nearly full
        let mut summary_action: Option<SummaryAction> = None;
        let mut summary_edit = self.chat.summary_edit.take();
        let summary_at = self.chat.summary.as_ref().map(|s| s.upto);
        let show_thoughts = self.stored.show_thinking();
        let mut go_on = false;
        let mut compact_on = false;
        if let Some(area) = turns_area {
            area.show(ui, |ui| {
                ui.add_space(4.0);
                let width = ui.available_width();
                for (i, turn) in self.chat.turns.iter().enumerate() {
                    // where the model's view begins: the summary stands for the turns above (C223)
                    if Some(i) == summary_at {
                        if let Some(summary) = &self.chat.summary {
                            if let Some(action) = show_summary_line(
                                ui,
                                summary,
                                self.chat.summary_open,
                                summary_edit.as_mut(),
                                width,
                            ) {
                                summary_action = Some(action);
                            }
                        }
                    }
                    let typing =
                        !turn.from_person && self.chat.replying() && i + 1 == self.chat.turns.len();
                    // a model that thinks first: the reply is on its way, and for how long it has been
                    let thinking_for = self
                        .chat
                        .thinking
                        .filter(|_| typing && turn.text.is_empty())
                        .map(|since| {
                            ui.ctx().request_repaint_after(Duration::from_secs(1));
                            format!("Thinking… {} s", since.elapsed().as_secs())
                        });
                    // what it thought, folded above the answer, when Show thinking is on (C223)
                    let fold = (show_thoughts && !turn.from_person && !turn.thought.is_empty())
                        .then(|| match (&thinking_for, turn.thought_secs) {
                            (Some(now), _) => now.clone(),
                            (None, Some(secs)) => format!("Thought for {secs} s"),
                            (None, None) => "Thought".to_string(),
                        });
                    let text = if typing && turn.text.is_empty() {
                        match (&thinking_for, &fold) {
                            // the fold says it
                            (Some(_), Some(_)) => String::new(),
                            (Some(now), None) => now.clone(),
                            (None, _) => "…".to_string(),
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
                    // the pictures that went with a message are shown small over its words (C226, B), each with its name
                    // when the pointer is on it; one with no preview is named under them
                    let shown: Vec<(&egui::TextureHandle, egui::Vec2, &str)> = turn
                        .pictures
                        .iter()
                        .filter_map(|pic| {
                            let texture = pic.preview.0.as_ref()?;
                            let [w, h] = texture.size();
                            let wide = (48.0 * w.max(1) as f32 / h.max(1) as f32).clamp(36.0, 120.0);
                            Some((texture, egui::vec2(wide, 48.0), pic.name.as_str()))
                        })
                        .collect();
                    let unshown: Vec<&str> = turn
                        .pictures
                        .iter()
                        .filter(|pic| pic.preview.0.is_none())
                        .map(|pic| pic.name.as_str())
                        .collect();
                    let with = (!unshown.is_empty()).then(|| format!("With: {}", unshown.join(", ")));
                    let row = shown.iter().map(|(_, size, _)| size.x).sum::<f32>()
                        + 6.0 * shown.len().saturating_sub(1) as f32;
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
                        let inner = inner.max(row.min(wrap)).ceil() + 1.0;
                        ui.horizontal(|ui| {
                            ui.add_space((width - inner - 20.0 - 10.0).max(0.0));
                            bubble.show(ui, |ui| {
                                ui.set_width(inner);
                                ui.vertical(|ui| {
                                    if !shown.is_empty() {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                            for (texture, size, name) in &shown {
                                                ui.add(
                                                    egui::Image::from_texture(egui::load::SizedTexture::new(
                                                        texture.id(),
                                                        *size,
                                                    ))
                                                    .corner_radius(4),
                                                )
                                                .on_hover_text(*name);
                                            }
                                        });
                                        ui.add_space(2.0);
                                    }
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
                        if !text.is_empty() || fold.is_some() || turn.calls.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                bubble.show(ui, |ui| {
                                    ui.set_max_width(width * 0.78);
                                    ui.vertical(|ui| {
                                        if let Some(head) = &fold {
                                            show_thought(ui, i, head, &turn.thought);
                                        }
                                        if !text.is_empty() {
                                            show_message(ui, &text, self.talk.reading_in(i));
                                        }
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
                        // wrapped, so that a narrow column (the list of kept chats beside it) does not widen the rest; its
                        // lines all start where the answer does
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
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
                            // an answer that was cut off goes on from where it stopped (C223)
                            if Some(i) == last_reply
                                && idle
                                && self.chat.cut_off
                                && (!self.chat.cut_by_context || self.can_summarize())
                                && theme::button_sized(
                                    ui,
                                    Role::Normal,
                                    true,
                                    "Continue",
                                    egui::vec2(0.0, 18.0),
                                )
                                .on_hover_text("Ask the model to go on with this answer from where it stopped.")
                                .clicked()
                            {
                                go_on = true;
                            }
                            if let Some(start) = self.show_read_aloud(ui, i) {
                                read = Some((i, start));
                            }
                            // a stopped answer says so (C224)
                            let said = match (&turn.stats, turn.stopped) {
                                (Some(stats), true) => Some(format!("Stopped · {}", stats.line())),
                                (Some(stats), false) => Some(stats.line()),
                                (None, true) => Some("Stopped".to_string()),
                                (None, false) => None,
                            };
                            if let Some(said) = said {
                                // cut with … where the row is narrow (Read aloud beside it, C216): a line that runs on
                                // widens the page under the panel and pushes the box's buttons out of it
                                ui.add(egui::Label::new(theme::muted(said)).truncate());
                            }
                            });
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
                // the summary that makes room, while it is written (C223)
                if let Some(summarizing) = &self.chat.summarizing {
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                    let messages = match summarizing.messages {
                        1 => "1 earlier message".to_string(),
                        n => format!("{n} earlier messages"),
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(theme::muted(format!(
                            "Making room: summarizing {messages}… {} s",
                            summarizing.started.elapsed().as_secs()
                        )));
                    });
                }
                if let Some(why) = &self.chat.error {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(8.0);
                        ui.colored_label(p.bad, why);
                    });
                }
                // what is said under the conversation informs: in grey, with an (i)
                if let Some(notice) = &self.chat.notice {
                    info_note(ui, notice);
                }
                // nearly full, with Auto-compact off (C223)
                if let Some((used, context)) = self.chat.room_note.filter(|_| !self.stored.auto_compact()) {
                    info_note(
                        ui,
                        &format!(
                            "This chat takes {} of the {} context. When it is full, the model cannot answer.",
                            super::compact::tokens_words(used),
                            context_words(i64::try_from(context).unwrap_or(i64::MAX))
                        ),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(30.0);
                        let link = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new("Turn on Auto-compact")
                                        .small()
                                        .color(p.accent_text),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text(
                                "Summarize the earlier messages when the conversation nears the end of the context.",
                            );
                        if link.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        compact_on = link.clicked();
                        ui.label(theme::muted("or start a new chat.").small());
                    });
                }
            });
        }
        match summary_action {
            Some(SummaryAction::Toggle) => self.chat.summary_open = !self.chat.summary_open,
            Some(SummaryAction::Copy) => {
                if let Some(summary) = &self.chat.summary {
                    ui.ctx().copy_text(summary.text.clone());
                }
            }
            Some(SummaryAction::Edit) => {
                summary_edit = self.chat.summary.as_ref().map(|s| s.text.clone());
            }
            Some(SummaryAction::Save) => {
                if let (Some(summary), Some(text)) =
                    (self.chat.summary.as_mut(), summary_edit.take())
                {
                    if !text.trim().is_empty() {
                        summary.text = text.trim().to_string();
                        summary.after = None;
                    }
                }
                self.keep_conversation();
            }
            Some(SummaryAction::Cancel) => summary_edit = None,
            None => {}
        }
        self.chat.summary_edit = summary_edit;
        if go_on {
            self.continue_reply(ui.ctx());
        }
        if compact_on {
            self.set_capability(super::compact::Capability::AutoCompact, true);
        }
        if let Some((turn, call, action)) = card {
            match action {
                super::tools::CardAction::Allow => self.allow_call(turn, call, false),
                super::tools::CardAction::Always => self.allow_call(turn, call, true),
                super::tools::CardAction::AllowAll => self.allow_all_tools(turn),
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
        match read {
            Some((turn, true)) => self.read_turn(turn, ui.ctx()),
            Some((_, false)) => self.stop_reading(),
            None => {}
        }
        if edit {
            self.edit_last_message();
        }
        let box_top = ui.cursor().top();
        // a run on a computer QuickAI set up may not say its model (llama.cpp serves the one it was started with)
        let ready = running && (model.is_some() || remote_run.is_some());
        // what is pasted and dropped, before what goes with the message is drawn (a note of it shows at once) and before
        // the box sees the keys
        self.take_paste_and_drop(ui, ready);
        // what goes with the next message (C226, B): a preview of each picture with the way to take it off, a tile that
        // waits while what was given is read, and what could not go
        self.take_given(ui.ctx());
        let reading = !self.chat.reading.is_empty();
        if !self.chat.pictures.is_empty() || reading {
            let mut off: Option<usize> = None;
            ui.horizontal_wrapped(|ui| {
                ui.add_space(20.0);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for (i, picture) in self.chat.pictures.iter().enumerate() {
                    if picture_tile(ui, picture, i) {
                        off = Some(i);
                    }
                }
                if reading {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(60.0, 60.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 7.0, p.card);
                    egui::Spinner::new().paint_at(ui, rect.shrink(20.0));
                }
                if !self.chat.pictures.is_empty() && !self.model_sees_pictures() {
                    ui.add_space(2.0);
                    given_note(
                        ui,
                        "This model may not see pictures.",
                        "One that does says vision, or came with a projector.",
                    );
                }
            });
            if let Some(i) = off {
                self.chat.pictures.remove(i);
            }
        }
        if !self.chat.given_notes.is_empty() {
            let mut clear = false;
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                let notes = &self.chat.given_notes;
                let mut said = notes[..notes.len().min(2)].join(" ");
                if notes.len() > 2 {
                    said.push_str(&format!(" And {} more could not go.", notes.len() - 2));
                }
                given_note(ui, &said, "Nothing is sent until you press Send.");
                clear = ui
                    .small_button("×")
                    .on_hover_text("Close this note.")
                    .clicked();
            });
            if clear {
                self.chat.given_notes.clear();
            }
        }
        let mut attach = false;
        let recording = self.talk.recording_for_box();
        let frame = egui::Frame::new()
            .fill(p.well)
            .stroke(egui::Stroke::new(
                if recording { 1.5 } else { 1.0 },
                if recording { p.bad.gamma_multiply(0.8) } else { p.outline },
            ))
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
                    // the clip and the tools side by side, with no room between them (no space wasted)
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let clip = icons::icon_button(ui, Icon::Clip, egui::vec2(24.0, 28.0), ready, false)
                            .on_hover_text(
                                "Add pictures (PNG, JPEG, WebP, GIF) for a model that sees them, or text files to \
                                 put in the message. You can also paste them (Ctrl+V) or drop them on the chat.",
                            );
                        super::help::mark_spot(ui, super::help::Spot::Clip, clip.rect);
                        attach = clip.clicked();
                        // the tools the model can use here: the MCP servers that are on (C204, C210)
                        self.show_tools_button(ui);
                    });
                    // while the microphone records, the box is the strip that shows it (C216, R1)
                    if recording {
                        self.show_recording_strip(ui);
                        return;
                    }
                    // Enter sends and Shift+Enter is a new line: the key is taken before the box sees it, so that it
                    // does not put a line break in the message wherever the cursor is
                    let input_id = egui::Id::new("chat_input");
                    let focused = ui.memory(|m| m.has_focus(input_id));
                    let enter = focused
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    // Up and Down bring back what was sent, as a terminal's history does (`recall`): taken before the box
                    // sees them only when they do, so that in what is being typed they move the cursor
                    if focused {
                        let cursor = egui::TextEdit::load_state(ui.ctx(), input_id)
                            .and_then(|s| s.cursor.char_range())
                            .map_or(self.chat.input.chars().count(), |r| r.primary.index.into());
                        for (key, up) in [(egui::Key::ArrowUp, true), (egui::Key::ArrowDown, false)] {
                            if !ui.input(|i| i.key_pressed(key) && i.modifiers.is_none()) {
                                continue;
                            }
                            let Some((at, text)) =
                                recall(&self.chat.sent, self.chat.recalled, &self.chat.input, cursor, up)
                            else {
                                continue;
                            };
                            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
                            self.chat.recalled = at;
                            self.chat.input = text;
                            // the cursor at its end, where Down goes on from
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), input_id) {
                                let end = egui::text::CCursor::new(self.chat.input.chars().count());
                                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                                state.store(ui.ctx(), input_id);
                            }
                        }
                    }
                    let talk_width = self.talk_buttons_width();
                    let hint = match &model {
                        _ if ready && self.talk.said_nothing_heard() => {
                            "Nothing was heard. Try again, closer to the microphone.".to_string()
                        }
                        Some(model) if ready => format!("Message {model}"),
                        _ => "Start a model to chat with it".to_string(),
                    };
                    let field = ui.add_enabled(
                        ready,
                        egui::TextEdit::multiline(&mut self.chat.input)
                            .id(input_id)
                            .frame(egui::Frame::NONE)
                            .desired_rows(1)
                            .desired_width(ui.available_width() - 40.0 - talk_width)
                            .hint_text(hint),
                    );
                    // what was said was put in the box: it has the keys, with the cursor at its end
                    if std::mem::take(&mut self.talk.focus_box) {
                        field.request_focus();
                        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), input_id) {
                            let end = egui::text::CCursor::new(self.chat.input.chars().count());
                            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            state.store(ui.ctx(), input_id);
                        }
                    }
                    // the speaker and the microphone, before the arrow (C216, M1)
                    self.show_talk_buttons(ui, ready);
                    // from the message sent to the answer's end, the arrow is the stop (C224): while the MCP servers start,
                    // while the model thinks and writes, and while its tools run before the next reply (a model that keeps
                    // asking for tools, with no limit to the calls in a row, is stopped here too)
                    if self.chat_busy() {
                        if stop_button(ui).on_hover_text("Stop the answer (Esc).").clicked() {
                            self.chat.stop();
                            self.poll_chat();
                        }
                    } else {
                        // what was given and is still being read goes with the message: it waits for it
                        let adding = !self.chat.reading.is_empty();
                        let can_send = ready && !self.chat.input.trim().is_empty() && !adding;
                        let clicked =
                            icons::icon_button(ui, Icon::Send, egui::vec2(32.0, 28.0), can_send, can_send)
                                .on_hover_text(if adding {
                                    "Adding what you gave the message…"
                                } else {
                                    "Send (Enter). Shift+Enter starts a new line."
                                })
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
        // files dragged over the window: the chat darkens under a frame that says what a drop does (C226, D1)
        if ui.input(|i| !i.raw.hovered_files.is_empty()) {
            paint_drop_cover(ui.ctx(), chat_rect, ready);
        }
        // hands-free covers the chat (C216, H1)
        self.show_hands_free(ui.ctx(), chat_rect);
        if attach {
            if let Some(paths) = rfd::FileDialog::new()
                .add_filter(
                    "Pictures and text",
                    &[
                        "png", "jpg", "jpeg", "webp", "gif", "txt", "md", "csv", "json", "log",
                    ],
                )
                .add_filter("Any file", &["*"])
                .pick_files()
            {
                self.give(given::Source::Files(paths), ui.ctx());
            }
        }
    }

    /// The options of a reply: the instructions, the temperature and the length, each the model's own until set.
    /// The window of the chat's options (C208): the instructions the model reads before
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

    /// Up and Down in the box go through what was sent as a terminal's history does: Up from an empty box to the newest
    /// and on to older ones, Down back and to an empty box; in what is typed, or past the first line of a message
    /// brought back, they are the box's own.
    #[test]
    fn up_and_down_bring_back_what_was_sent() {
        let sent: Vec<String> = ["first", "second\nwith a second line", "third"]
            .map(String::from)
            .to_vec();
        let up = |recalled, input: &str, cursor| recall(&sent, recalled, input, cursor, true);
        let down = |recalled, input: &str, cursor| recall(&sent, recalled, input, cursor, false);
        let third = (Some(0), "third".to_string());
        let second = (Some(1), sent[1].clone());
        let first = (Some(2), "first".to_string());

        // from an empty box: the newest, then older ones; none older than the first
        assert_eq!(up(None, "", 0), Some(third.clone()));
        assert_eq!(up(Some(0), "third", 5), Some(second.clone()));
        // in a message of two lines Up moves the cursor until it is on the first line
        assert_eq!(up(Some(1), &sent[1], sent[1].chars().count()), None);
        assert_eq!(up(Some(1), &sent[1], 3), Some(first.clone()));
        assert_eq!(up(Some(2), "first", 5), None);

        // Down: the newer ones (from the last line), then the empty box again
        assert_eq!(down(Some(2), "first", 5), Some(second.clone()));
        assert_eq!(down(Some(1), &sent[1], 3), None, "not on the last line");
        assert_eq!(
            down(Some(1), &sent[1], sent[1].chars().count()),
            Some(third.clone())
        );
        assert_eq!(down(Some(0), "third", 5), Some((None, String::new())));
        assert_eq!(down(None, "", 0), None);

        // what is being typed, or a message brought back and changed, keeps the keys for the cursor
        assert_eq!(up(None, "a draft", 7), None);
        assert_eq!(up(Some(0), "third, changed", 14), None);
        assert_eq!(down(Some(0), "third, changed", 14), None);
        // nothing sent yet: nothing to bring back
        assert_eq!(recall(&[], None, "", 0, true), None);
    }
    use crate::testkit::fake_ollama::FakeOllama;

    /// The pieces of a sentence cut at a dash are lit in the answer, which has the dash and not their comma.
    #[test]
    fn the_pieces_of_a_sentence_read_at_a_dash_are_lit() {
        let plain = "I'm active - no glitches.";
        assert_eq!(lit_range(plain, "I'm active,"), Some((0, 10)));
        assert_eq!(lit_range(plain, "no glitches."), Some((13, 24)));
    }

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
                ChatEvent::Thinking(_)
                | ChatEvent::Calls(_)
                | ChatEvent::Notice(_)
                | ChatEvent::Finished(_)
                | ChatEvent::Usage { .. } => {}
                ChatEvent::Done => break,
                ChatEvent::Failed(why) | ChatEvent::TooLong { why, .. } => {
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
    fn a_model_that_thought_and_stopped_is_told_why_by_the_reason_the_engine_gave() {
        // the reply limit the person set: Options raises it
        let said = stopped_thinking_words(65, Some("length"), Some(512), Some(4096));
        assert!(
            said.starts_with("It thought for 65 s and reached the reply limit (512 tokens)"),
            "{said}"
        );
        assert!(said.contains("Allow longer answers in Options"), "{said}");
        // no limit (the person's No limit): the context filled, which Options cannot help with
        let said = stopped_thinking_words(65, Some("length"), None, Some(4096));
        assert!(said.contains("ran out of context (4K tokens)"), "{said}");
        assert!(
            said.contains("longer context in How it runs") && said.contains("new chat"),
            "{said}"
        );
        assert!(!said.contains("Options"), "{said}");
        assert!(
            stopped_thinking_words(5, Some("length"), None, Some(5000)).contains("(5000 tokens)")
        );
        assert!(stopped_thinking_words(5, Some("length"), None, None)
            .contains("ran out of context before"));
        // it ended by itself, or the engine did not say why: no setting is blamed
        for finish in [Some("stop"), None] {
            let said = stopped_thinking_words(9, finish, None, Some(4096));
            assert!(
                said.contains("stopped before it answered")
                    && !said.contains("Options")
                    && !said.contains("context"),
                "{said}"
            );
        }
        // an answer that was cut off says which of the two cut it, and how it goes on (C223)
        let limit = cut_off_words(Some(256), Some(4096), false);
        assert!(
            limit.contains("reply limit (256 tokens)") && limit.contains("Continue"),
            "{limit}"
        );
        let full = cut_off_words(None, Some(8192), false);
        assert!(full.contains("context (8K tokens) was full"), "{full}");
        assert!(
            full.contains("Turn on Auto-compact") && !full.contains("Continue"),
            "{full}"
        );
        let room = cut_off_words(None, Some(8192), true);
        assert!(
            room.contains("Continue summarizes earlier messages"),
            "{room}"
        );
    }

    #[test]
    fn the_reason_the_engine_ends_a_reply_with_is_heard() {
        use std::io::{Read, Write};
        // an engine that thinks, then ends the reply for length before any word of it
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut buf = [0u8; 4096];
            while !String::from_utf8_lossy(&asked).contains("\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                asked.extend_from_slice(&buf[..n]);
            }
            let events = [
                r#"{"choices":[{"delta":{"reasoning_content":"Let me think"},"finish_reason":null}]}"#,
                r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#,
                // what the request took of the context comes after the end, as llama-server and Ollama send it
                r#"{"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":4,"total_tokens":13}}"#,
            ];
            let mut body = String::new();
            for event in events {
                body.push_str(&format!("data: {event}\n\n"));
            }
            body.push_str("data: [DONE]\n\n");
            let _ = stream.write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}").as_bytes(),
            );
            // the rest of the request is read before the connection closes: closed with some of it unread, Windows resets
            // the connection, and the client can lose the answer it has not read yet (os error 10053, seen on CI)
            let _ = stream.shutdown(std::net::Shutdown::Write);
            while matches!(stream.read(&mut buf), Ok(n) if n > 0) {}
        });
        let (rx, _cancel) = spawn_reply_with_tools(
            port,
            "m".into(),
            Options::default(),
            vec![Turn::said(true, "Hello")],
            None,
            || {},
        );
        let mut heard = Vec::new();
        loop {
            match rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("the reply ends")
            {
                ChatEvent::Done => break,
                ChatEvent::Failed(why) => panic!("{why}"),
                ChatEvent::Thinking(thought) => heard.push(format!("thinking {thought}")),
                ChatEvent::Usage { prompt, completion } => {
                    heard.push(format!("usage {prompt} {completion}"))
                }
                ChatEvent::Finished(why) => heard.push(format!("finished {why}")),
                ChatEvent::Text(text) => heard.push(format!("text {text}")),
                _ => {}
            }
        }
        assert_eq!(
            heard,
            ["thinking Let me think", "finished length", "usage 9 4"]
        );
    }

    #[test]
    fn what_the_person_set_is_in_the_request_and_nothing_else() {
        let hello = [Turn::said(true, "Hello")];
        let options = Options {
            instructions: Some("  Answer in French.  ".into()),
            temperature: Some(0.333),
            max_tokens: Some(256),
            ..Options::default()
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
            assert_eq!(
                base64_decoded(coded).as_deref(),
                Some(plain.as_bytes()),
                "{coded}"
            );
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(
            base64_decoded("//79").as_deref(),
            Some(&[0xff, 0xfe, 0xfd][..])
        );
        assert_eq!(base64_decoded("a*b="), None);
    }

    #[test]
    fn a_picture_goes_with_the_words_of_its_message() {
        let picture = Picture {
            name: "dot.png".into(),
            mime: "image/png".into(),
            base64: base64(&[0x89, b'P', b'N', b'G']),
            preview: Preview::default(),
        };
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

    /// C225: an error that can pass (the engine failed on its side, was busy, or could not be reached) is asked again
    /// twice, after a wait, and then said; any other refusal is said at once, since the same request would be refused
    /// the same way.
    #[test]
    fn an_error_that_can_pass_is_asked_again_twice_and_any_other_is_said_at_once() {
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 1 << 30);
        let hello = || vec![Turn::said(true, "Hello")];
        let route = "/v1/chat/completions";
        let asked = || fake.requests_to(route).len();
        fake.behave(|b| {
            b.fail_next.insert(route.into(), (503, 2));
        });
        let (text, failed) = reply_of(fake.port(), "qwen3:4b", Options::default(), hello());
        assert_eq!(failed, None);
        assert!(text.starts_with("Hello!"), "{text}");
        assert_eq!(asked(), 3);
        fake.behave(|b| {
            b.fail_next.insert(route.into(), (500, 5));
        });
        let (_, failed) = reply_of(fake.port(), "qwen3:4b", Options::default(), hello());
        assert_eq!(failed.as_deref(), Some("injected failure"));
        assert_eq!(asked(), 6, "asked three times in all");
        fake.behave(|b| {
            b.fail_next.insert(route.into(), (400, 5));
        });
        let (_, failed) = reply_of(fake.port(), "qwen3:4b", Options::default(), hello());
        assert_eq!(failed.as_deref(), Some("injected failure"));
        assert_eq!(asked(), 7, "a refusal is said at once");
    }

    /// C225: arguments that are not a JSON object (a call the context cut off, or one the model wrote wrong) go back to
    /// the engine as `{}`, which it reads; whole ones go back as they were.
    #[test]
    fn arguments_that_are_not_a_json_object_go_back_as_an_empty_object() {
        let call = |id: &str, arguments: &str| ToolCall {
            id: id.to_string(),
            name: "jsonquery_workspace_save".to_string(),
            server: "jsonquery".to_string(),
            tool: "workspace_save".to_string(),
            arguments: arguments.to_string(),
            state: CallState::Failed(CALL_CUT_OFF.to_string()),
            open: false,
            ticket: None,
        };
        let mut reply = Turn::said(false, "");
        reply.calls = vec![
            call(
                "c1",
                "{\"path\": \"q\", \"content\": \"[.departments[] | {\\n  name: .name,",
            ),
            call("c2", "[1, 2]"),
            call("c3", "{\"path\":\"q\"}"),
        ];
        let body = request_body_with(
            "m",
            &Options::default(),
            &[Turn::said(true, "save it"), reply],
            None,
        );
        let asked = &body["messages"][1]["tool_calls"];
        assert_eq!(asked[0]["function"]["arguments"], "{}");
        assert_eq!(asked[1]["function"]["arguments"], "{}");
        assert_eq!(asked[2]["function"]["arguments"], "{\"path\":\"q\"}");
        assert!(body["messages"][2]["content"]
            .as_str()
            .unwrap()
            .contains("cut off"));
        assert_eq!(
            call_arguments("{\"a\": 1}"),
            Some(serde_json::json!({"a": 1}))
        );
        assert_eq!(call_arguments("\"text\""), None);
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

    /// C223: every request asks the engine to count what it takes (`stream_options`); the summary is read after the
    /// person's instructions in the system's word; whether to think is said in each engine's own words, and only once
    /// the person chose.
    #[test]
    fn the_request_carries_the_summary_and_whether_to_think_in_the_engines_words() {
        let hello = [Turn::said(true, "Hello")];
        let plain = request_body("m", &Options::default(), &hello);
        assert_eq!(
            plain["stream_options"],
            serde_json::json!({"include_usage": true})
        );
        assert!(
            plain.get("chat_template_kwargs").is_none() && plain.get("reasoning_effort").is_none()
        );
        let summed = Options {
            instructions: Some("Answer briefly.".into()),
            summary: Some("- A garden plan.".into()),
            ..Options::default()
        };
        let system = request_body("m", &summed, &hello)["messages"][0].clone();
        assert_eq!(system["role"], "system");
        let said = system["content"].as_str().unwrap();
        assert!(said.starts_with("Answer briefly.\n\n"), "{said}");
        assert!(
            said.ends_with(&format!("{SUMMARY_LEAD}\n\n- A garden plan.")),
            "{said}"
        );
        let alone = Options {
            summary: Some("- A garden plan.".into()),
            ..Options::default()
        };
        let said = request_body("m", &alone, &hello)["messages"][0]["content"].clone();
        assert_eq!(said, format!("{SUMMARY_LEAD}\n\n- A garden plan."));
        let think = |dialect, thinking| {
            request_body(
                "m",
                &Options {
                    dialect,
                    thinking,
                    ..Options::default()
                },
                &hello,
            )
        };
        assert_eq!(
            think(Dialect::LlamaCpp, Some(false))["chat_template_kwargs"],
            serde_json::json!({"enable_thinking": false})
        );
        assert_eq!(
            think(Dialect::LlamaCpp, Some(true))["chat_template_kwargs"],
            serde_json::json!({"enable_thinking": true})
        );
        assert_eq!(
            think(Dialect::Ollama, Some(false))["reasoning_effort"],
            "none"
        );
        assert!(think(Dialect::Ollama, Some(true))
            .get("reasoning_effort")
            .is_none());
        assert!(think(Dialect::Other, Some(false))
            .get("chat_template_kwargs")
            .is_none());
        assert!(think(Dialect::LlamaCpp, None)
            .get("chat_template_kwargs")
            .is_none());
    }

    /// C223: a refusal that says the request does not fit in the context is told apart from others, with the context's
    /// length when the engine gives it.
    #[test]
    fn a_request_too_long_for_the_context_is_told_apart() {
        let llama = serde_json::json!({"error": {"code": 400, "message": "the request exceeds the available context size, try increasing it", "type": "exceed_context_size_error", "n_prompt_tokens": 9000, "n_ctx": 8192}});
        assert_eq!(too_long(&llama), Some(Some(8192)));
        let openai = serde_json::json!({"error": {"message": "This model's maximum context length is 4096 tokens.", "code": "context_length_exceeded"}});
        assert_eq!(too_long(&openai), Some(None));
        let other = serde_json::json!({"error": {"message": "model 'x' not found"}});
        assert_eq!(too_long(&other), None);
        assert_eq!(too_long(&serde_json::json!({"choices": []})), None);
    }

    /// C223: what Continue brings is joined to the answer as it reads: a new paragraph where the model begins anew, a
    /// space within a sentence, nothing where either side has the space or the mark.
    #[test]
    fn what_continue_brings_is_joined_as_it_reads() {
        assert_eq!(joint("about the", "The garden"), "\n\n");
        assert_eq!(joint("about the", "- a list"), "\n\n");
        assert_eq!(joint("about the", "school garden"), " ");
        assert_eq!(joint("about the", ", and"), "");
        assert_eq!(joint("about the ", "school"), "");
        assert_eq!(joint("about the", " school"), "");
        assert_eq!(joint("", "The"), "");
    }

    /// C224: an answer stopped while it is written stays, said to be stopped, with how it came and Continue; one that had
    /// said nothing yet is not kept; and the request is told to stop.
    #[test]
    fn a_stopped_answer_stays_said_to_be_stopped_and_can_go_on() {
        let mut chat = Chat {
            turns: vec![
                Turn::said(true, "Plan a garden"),
                Turn::said(false, "Choose the spot. Six hours of"),
            ],
            ..Chat::default()
        };
        let (_tx, rx) = crossbeam_channel::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        chat.reply = Some(rx);
        chat.cancel = Some(cancel.clone());
        chat.started = Some(Instant::now());
        chat.first = Some(Instant::now());
        chat.pieces = 7;
        chat.stop();
        assert!(cancel.load(Ordering::SeqCst), "the request is told to stop");
        assert!(!chat.replying());
        let last = chat.turns.last().unwrap();
        assert!(last.stopped && last.text.ends_with("Six hours of"));
        assert_eq!(last.stats.map(|s| s.tokens), Some(7));
        assert!(
            chat.cut_off && !chat.cut_by_context,
            "Continue goes on with it"
        );
        assert!(chat
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("stopped")));
        // nothing said yet: no turn is kept, and nothing is said to be stopped
        let mut chat = Chat {
            turns: vec![Turn::said(true, "Plan a garden"), Turn::said(false, "")],
            ..Chat::default()
        };
        let (_tx, rx) = crossbeam_channel::unbounded();
        chat.reply = Some(rx);
        chat.stop();
        assert_eq!(chat.turns.len(), 1);
        assert!(!chat.cut_off);
    }

    /// C224: a stop is heard while the engine says nothing yet (it reads a long conversation, or thinks without saying
    /// so): the request ends at once, and the engine sees its connection closed, which makes llama.cpp stop working on it.
    #[test]
    fn a_stop_ends_the_request_while_the_engine_says_nothing() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (closed_tx, closed_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
            let started = Instant::now();
            let closed = loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break true,
                    Ok(_) if started.elapsed() > Duration::from_secs(10) => break false,
                    Ok(_) => {}
                }
            };
            let _ = closed_tx.send(closed);
        });
        let (rx, cancel) = spawn_reply(
            port,
            "m".into(),
            Options::default(),
            vec![Turn::said(true, "Hello")],
            || {},
        );
        std::thread::sleep(Duration::from_millis(300));
        let stopped = Instant::now();
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)), Ok(ChatEvent::Done));
        assert!(
            stopped.elapsed() < Duration::from_secs(2),
            "{:?}",
            stopped.elapsed()
        );
        assert_eq!(
            closed_rx.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the engine saw the request closed"
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
