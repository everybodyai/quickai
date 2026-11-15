//! The room a conversation takes in the context (C223), for the chat here and on another computer alike:
//!
//! - what a request takes, reckoned from what the engine counted of the last one (`usage`), for the context chip of How
//!   it runs and for Auto-compact;
//! - Auto-compact (off until the person turns it on): once a request would leave the reply too little room, the model
//!   writes a summary of the earlier turns, which it then reads in their place; the last exchanges stay whole. The same
//!   summary is written when the person asks (Summarize earlier messages, in the chat's ⋯), and when the engine refuses
//!   a request as too long;
//! - Continue, for an answer that was cut off;
//! - what the run the chat talks to says of itself (its engine, how long its context is, whether its model thinks),
//!   asked once of each run, through the port the chat uses: this computer's, or a tunnel's to another computer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, TryRecvError};
use eframe::egui;
use serde_json::{json, Value};

use super::chat::{self, ChatEvent, Dialect, Options, Turn};
use super::App;

/// Auto-compact makes room once a request would take more than this share of the context: the rest is the reply's
/// (and the thinking of a model that thinks).
pub(super) const ROOM_TAKEN: f64 = 0.75;
/// From this share of the context on, a conversation is said to be nearly full while Auto-compact is off.
pub(super) const NEARLY_FULL: f64 = 0.9;
/// Continue is not asked for when a request would take this share of the context, with Auto-compact off: it would only
/// be refused.
const NO_ROOM: f64 = 0.97;
/// What a picture is reckoned to take before the engine counted one.
const PICTURE_TOKENS: u64 = 768;
/// What each message takes besides its words (the chat template's marks), in characters.
const MESSAGE_CHARS: usize = 16;
/// The longest a message and what a tool gave are put in the request for a summary: the beginning and the end of a
/// longer one, with how much was left out between them.
const LONGEST_MESSAGE: usize = 6000;
const LONGEST_TOOL_RESULT: usize = 2000;

/// What the model is asked, unseen, when the person presses Continue.
pub(super) const CONTINUE_ASK: &str = "Continue your last answer from exactly where it stopped. Do not repeat \
                                      what you already wrote, and do not say that you are continuing.";

/// What the model is asked, unseen, for a summary: after the turns it is to stand for, as a message of the person's.
/// The conversation stays a conversation (models know "summarize our conversation" well), and llama.cpp goes on from
/// what it read of it already.
pub(super) const SUMMARY_ASK: &str = "Before we go on: write a summary of our conversation so far (with the \
     summary of its earlier part, if you were given one), which you will read instead of the messages above. Keep \
     what I asked for, what was decided, and every number, price, date and name that may be needed later, exactly \
     as given. Write short points in the language of our conversation, with no introduction. Do not repeat the \
     messages, and add nothing that we did not say.";

/// What the last request took of the context, as the engine counted it: the conversation as it was sent with the words
/// of the reply after it, and the tokens a character of it came to, from which the next request is reckoned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Used {
    pub tokens: u64,
    pub per_char: f64,
}

impl Used {
    /// From the engine's count of what was sent (`prompt`), the pieces of the reply (about a token each) and the
    /// characters that were sent.
    pub fn counted(prompt: u64, reply_pieces: usize, chars: usize) -> Used {
        let per_char = if chars > 0 {
            (prompt as f64 / chars as f64).clamp(0.05, 4.0)
        } else {
            0.3
        };
        Used {
            tokens: prompt + reply_pieces as u64,
            per_char,
        }
    }
}

/// What the model reads in place of the first turns of a conversation once they were summarized to make room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub text: String,
    /// The turns it stands for: the first `upto`.
    pub upto: usize,
    /// About how many tokens those turns took, and the summary takes, when that was reckoned.
    pub before: Option<u64>,
    pub after: Option<u64>,
}

/// A summary being written: the request on its thread, what came of it so far, and what waits for it.
pub(super) struct Summarizing {
    rx: Receiver<ChatEvent>,
    cancel: Arc<AtomicBool>,
    pub started: Instant,
    /// The turns it is to stand for: the first `upto`, of which `messages` are new to a summary.
    pub upto: usize,
    pub messages: usize,
    text: String,
    before: u64,
    after: Option<u64>,
    /// The reply asked for waits for it.
    then_reply: bool,
}

impl Summarizing {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// What the run the chat talks to says of itself, and of which run (its port here, and the model).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RunFacts {
    pub port: u16,
    pub model: String,
    pub dialect: Dialect,
    /// The room of one request, in tokens.
    pub context: Option<u64>,
    /// Its model thinks before it answers, and can be told not to.
    pub thinks: bool,
    pub thinking_switch: bool,
    /// It was asked a second time (Ollama says the context of a model only once it is loaded).
    pub asked_again: bool,
}

/// The asking, on a thread of its own, and of which run.
pub(super) struct Probing {
    rx: Receiver<RunFacts>,
    port: u16,
    model: String,
}

/// The characters of what a request says: the system's word, each message, and each tool call with what came of it (a
/// picture is reckoned apart).
pub(super) fn request_chars(options: &Options, turns: &[Turn]) -> usize {
    let system = chat::system_text(options.instructions.as_deref(), options.summary.as_deref())
        .map_or(0, |t| t.chars().count() + MESSAGE_CHARS);
    system + turns.iter().map(turn_chars).sum::<usize>()
}

fn turn_chars(t: &Turn) -> usize {
    MESSAGE_CHARS
        + t.text.chars().count()
        + t.calls
            .iter()
            .map(|c| {
                2 * MESSAGE_CHARS
                    + c.name.chars().count()
                    + c.arguments.chars().count()
                    + c.answer().chars().count()
            })
            .sum::<usize>()
}

fn pictures(turns: &[Turn]) -> usize {
    turns.iter().map(|t| t.pictures.len()).sum()
}

/// What `chars` characters and `pictures` pictures are reckoned to take of the context, at `per_char` tokens a
/// character.
pub(super) fn reckon(chars: usize, pictures: usize, per_char: f64) -> u64 {
    (chars as f64 * per_char).ceil() as u64 + pictures as u64 * PICTURE_TOKENS
}

/// The tokens a character of `turns` is guessed to come to before the engine counted any: about one for three letters
/// of the Latin alphabet, more for the letters of other scripts.
pub(super) fn guess_per_char(turns: &[Turn]) -> f64 {
    let (mut ascii, mut other) = (0usize, 0usize);
    for c in turns.iter().flat_map(|t| t.text.chars()) {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    match ascii + other {
        0 => 0.3,
        all => (ascii as f64 * 0.3 + other as f64 * 0.8) / all as f64,
    }
}

/// Where a summary of the turns from `from` on should end: before the person's message that begins the last two
/// exchanges, or only the last when the two would take more than half the context (`kept`, of a turn on), so that the
/// model reads them whole; never inside an exchange (a reply and the tools it used stay with the message they answer);
/// and no further than one request for a summary can take (`asked`, of the turns up to an end), but at least one
/// exchange. `None` when there is nothing to summarize but what is kept whole.
pub(super) fn summary_end(
    turns: &[Turn],
    from: usize,
    context: u64,
    kept: impl Fn(usize) -> u64,
    asked: impl Fn(usize) -> u64,
) -> Option<usize> {
    let starts: Vec<usize> = (from..turns.len())
        .filter(|&i| turns[i].from_person)
        .collect();
    let n = starts.len();
    if n < 2 {
        return None;
    }
    let mut end = if n >= 3 && kept(starts[n - 2]) <= context / 2 {
        starts[n - 2]
    } else {
        starts[n - 1]
    };
    while asked(end) as f64 > context as f64 * 0.6 {
        match starts.iter().rev().find(|&&s| s < end && s > starts[0]) {
            Some(&earlier) => end = earlier,
            None => break,
        }
    }
    (end > from).then_some(end)
}

/// A text no longer than `most` characters: a longer one is its beginning and its end, with how much was left out.
fn shortened(text: &str, most: usize) -> String {
    let n = text.chars().count();
    if n <= most {
        return text.to_string();
    }
    let head: String = text.chars().take(most * 3 / 4).collect();
    let tail: String = text.chars().skip(n - most / 4).collect();
    let left_out = n - most * 3 / 4 - most / 4;
    format!("{head}\n[… {left_out} characters left out …]\n{tail}")
}

/// The turns as the request for a summary gives them: as the chat sent them, but with long messages and what tools gave
/// shortened, and pictures named instead of sent (a summary needs neither whole, and the request has to fit in the
/// context however long they were). The arguments of a call stay whole: the engine reads them as JSON.
fn for_summary(turns: &[Turn]) -> Vec<Turn> {
    turns
        .iter()
        .map(|t| {
            let mut t = t.clone();
            t.text = shortened(&t.text, LONGEST_MESSAGE);
            if !t.pictures.is_empty() {
                let names: Vec<&str> = t.pictures.iter().map(|p| p.name.as_str()).collect();
                t.text
                    .push_str(&format!("\n(Pictures sent with it: {})", names.join(", ")));
                t.pictures.clear();
            }
            for c in &mut t.calls {
                if let chat::CallState::Done(said) | chat::CallState::Failed(said) = &mut c.state {
                    *said = shortened(said, LONGEST_TOOL_RESULT);
                }
            }
            t
        })
        .collect()
}

/// How long a summary may be: a sixth of the context, between 256 and 2048 tokens.
fn summary_length(context: Option<u64>) -> u32 {
    context.map_or(1024, |c| (c / 6).clamp(256, 2048) as u32)
}

/// The options of the request for a summary: the chat's own (its instructions, and the summary there was, so that what
/// the engine read of them is read again from where it was), steadier, without thinking where the model can be told
/// so, and then no longer than a share of the context (a model that thinks all the same is given the room to).
fn summary_options(
    options: &Options,
    (thinks, switch): (bool, bool),
    context: Option<u64>,
) -> Options {
    // llama.cpp's templates that do not read `enable_thinking` pass it over: it is said to every model there
    let told = switch || options.dialect == Dialect::LlamaCpp;
    Options {
        temperature: Some(0.2),
        max_tokens: (switch || !thinks).then(|| summary_length(context)),
        thinking: told.then_some(false),
        ..options.clone()
    }
}

/// The request for a summary of `turns`: the conversation as the chat had it, then the ask.
pub(super) fn summary_request(
    model: &str,
    options: &Options,
    thinking: (bool, bool),
    turns: &[Turn],
    context: Option<u64>,
) -> Value {
    let mut asked = for_summary(turns);
    asked.push(Turn::said(true, SUMMARY_ASK));
    chat::request_body_with(
        model,
        &summary_options(options, thinking, context),
        &asked,
        None,
    )
}

/// What the chat says when a conversation does not fit in the context: with Auto-compact off, what makes room; on, that
/// the last messages alone are too long.
pub(super) fn too_long_words(context: Option<u64>, compacts: bool) -> String {
    let size = context
        .and_then(|c| i64::try_from(c).ok())
        .map(|c| format!(" ({} tokens)", chat::context_words(c)))
        .unwrap_or_default();
    if compacts {
        format!(
            "The conversation does not fit in the context{size}, even with the earlier messages summarized: the last \
             messages alone are too long. Choose a longer context in How it runs, or start a new chat."
        )
    } else {
        format!(
            "The conversation no longer fits in the context{size}. Turn on Auto-compact in How it runs to make room, \
             choose a longer context, or start a new chat."
        )
    }
}

/// A number of tokens in a few characters: `800`, `2.8K`, `12K`.
pub(super) fn tokens_words(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1000..=9_949 => format!("{:.1}K", n as f64 / 1000.0),
        _ => format!("{}K", (n as f64 / 1000.0).round() as u64),
    }
}

/// Asks the run on `port` what it is: llama.cpp's server says its context and its chat template (`/props`), Ollama the
/// context of the model it loaded (`/api/ps`) and whether it thinks (`/api/show`); another server says nothing of these.
pub(super) fn probe(port: u16, model: &str) -> RunFacts {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let base = format!("http://127.0.0.1:{port}");
    let read = |response: Result<ureq::http::Response<ureq::Body>, ureq::Error>| -> Option<Value> {
        let response = response.ok()?;
        if !response.status().is_success() {
            return None;
        }
        serde_json::from_reader(response.into_body().into_reader()).ok()
    };
    let mut facts = RunFacts {
        port,
        model: model.to_string(),
        ..RunFacts::default()
    };
    if let Some(props) = read(agent.get(format!("{base}/props")).call())
        .filter(|p| p.get("default_generation_settings").is_some())
    {
        facts.dialect = Dialect::LlamaCpp;
        facts.context = props
            .pointer("/default_generation_settings/n_ctx")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0);
        let template = props
            .get("chat_template")
            .and_then(Value::as_str)
            .unwrap_or_default();
        (facts.thinks, facts.thinking_switch) = quickai_core::gguf::template_thinking(template);
        return facts;
    }
    if let Some(ps) =
        read(agent.get(format!("{base}/api/ps")).call()).filter(|p| p.get("models").is_some())
    {
        facts.dialect = Dialect::Ollama;
        let loaded = ps["models"].as_array().cloned().unwrap_or_default();
        let named = |m: &&Value| {
            ["name", "model"]
                .iter()
                .any(|k| m.get(*k).and_then(Value::as_str) == Some(model))
        };
        let entry = loaded.iter().find(named).or(match loaded.as_slice() {
            [only] => Some(only),
            _ => None,
        });
        facts.context = entry
            .and_then(|m| m.get("context_length"))
            .and_then(Value::as_u64)
            .filter(|n| *n > 0);
        let shown = read(
            agent
                .post(format!("{base}/api/show"))
                .header("Content-Type", "application/json")
                .send(json!({ "model": model }).to_string()),
        );
        facts.thinks = shown
            .as_ref()
            .and_then(|s| s.get("capabilities"))
            .and_then(Value::as_array)
            .is_some_and(|c| c.iter().any(|v| v == "thinking"));
        facts.thinking_switch = facts.thinks;
    }
    facts
}

impl App {
    /// What the run the chat talks to said of itself, when it was asked of this run.
    pub(super) fn run_facts_now(&self) -> Option<&RunFacts> {
        let (port, model) = self.chat_target()?;
        self.chat
            .run_facts
            .as_ref()
            .filter(|f| f.port == port && f.model == model)
    }

    /// The room of one request of the run the chat talks to: as its engine said it, else as it was started with.
    pub(super) fn chat_context(&self) -> Option<u64> {
        self.run_facts_now()
            .and_then(|f| f.context)
            .or_else(|| {
                self.values
                    .int("ctx_size")
                    .and_then(|n| u64::try_from(n).ok())
            })
            .filter(|n| *n > 0)
    }

    /// How the engine of the run is told what the API has no word for: as it answered, else by the engine chosen.
    pub(super) fn chat_dialect(&self) -> Dialect {
        if let Some(f) = self.run_facts_now().filter(|f| f.dialect != Dialect::Other) {
            return f.dialect;
        }
        match self.engine.as_str() {
            "llamacpp" => Dialect::LlamaCpp,
            "ollama" => Dialect::Ollama,
            _ => Dialect::Other,
        }
    }

    /// Whether the model the chat talks to thinks before its answers, and whether it can be told not to: as its run
    /// said, else as the selected model's file (or the engine's list of models) says.
    pub(super) fn chat_thinking(&self) -> (bool, bool) {
        if let Some(f) = self.run_facts_now() {
            return (f.thinks, f.thinking_switch);
        }
        if self.setup_in_use().is_some() {
            return (false, false);
        }
        let facts = self
            .model_facts()
            .filter(|_| self.remote_in_use().is_none())
            .map(|f| (f.thinks, f.thinking_switch))
            .unwrap_or_default();
        let listed = self
            .models
            .get(self.selected)
            .is_some_and(|m| m.note.split(", ").any(|w| w == "thinking"));
        (facts.0 || listed, facts.1 || listed)
    }

    /// Whether the model is told to think, as the Thinking switch shows it: as the person chose, else as it starts.
    pub(super) fn thinking_on(&self) -> bool {
        self.stored
            .thinking()
            .unwrap_or_else(|| !self.starts_without_thinking())
    }

    /// What the next request is asked with: what the person set, the summary, and whether the model is to think
    /// (only once the person chose, and only for a model that can be told).
    pub(super) fn reply_options(&self) -> Options {
        let (_, switch) = self.chat_thinking();
        Options {
            thinking: self.stored.thinking().filter(|_| switch),
            dialect: self.chat_dialect(),
            ..self.chat.options()
        }
    }

    /// What the request for the next reply is reckoned to take of the context: at what the engine counted a character
    /// of the last one to come to, or as guessed before it counted any.
    pub(super) fn reckon_next(&self) -> u64 {
        let options = self.reply_options();
        let turns = &self.chat.turns[self.chat.summarized_upto()..];
        let mut chars = request_chars(&options, turns);
        if self.chat.continuing {
            chars += MESSAGE_CHARS + CONTINUE_ASK.len();
        }
        let per_char = self
            .chat
            .used
            .map_or_else(|| guess_per_char(turns), |u| u.per_char);
        reckon(chars, pictures(turns), per_char)
    }

    /// Whether the next request would leave the reply too little room in the context.
    pub(super) fn needs_room(&self) -> bool {
        self.chat_context()
            .is_some_and(|c| self.reckon_next() as f64 > c as f64 * ROOM_TAKEN)
    }

    /// Whether there are turns to summarize beyond those kept whole and those summarized already.
    pub(super) fn can_summarize(&self) -> bool {
        let from = self.chat.summarized_upto();
        self.chat.turns[from..]
            .iter()
            .filter(|t| t.from_person)
            .count()
            >= 2
    }

    /// Starts writing the summary of the earlier turns (and of the summary there was), the reply waiting for it when
    /// `then_reply`; `false` when there is nothing to summarize, or nothing runs.
    pub(super) fn start_summary(&mut self, ctx: &egui::Context, then_reply: bool) -> bool {
        let Some((port, model)) = self.chat_target() else {
            return false;
        };
        let from = self.chat.summarized_upto();
        let context = self.chat_context();
        let room = context.unwrap_or(u64::MAX / 4);
        let per_char = self
            .chat
            .used
            .map_or_else(|| guess_per_char(&self.chat.turns), |u| u.per_char);
        let options = self.reply_options();
        let thinking = self.chat_thinking();
        let asked_options = summary_options(&options, thinking, context);
        let previous = self
            .chat
            .summary
            .as_ref()
            .map_or(0, |s| s.text.chars().count());
        let turns = &self.chat.turns;
        let kept = |at: usize| {
            reckon(
                turns[at..].iter().map(turn_chars).sum(),
                pictures(&turns[at..]),
                per_char,
            )
        };
        let asked = |end: usize| {
            let chars = request_chars(&asked_options, &for_summary(&turns[from..end]))
                + MESSAGE_CHARS
                + SUMMARY_ASK.len();
            reckon(chars, 0, per_char)
        };
        let Some(end) = summary_end(turns, from, room, kept, asked) else {
            return false;
        };
        let before = reckon(
            previous + turns[from..end].iter().map(turn_chars).sum::<usize>(),
            pictures(&turns[from..end]),
            per_char,
        );
        let body = summary_request(&model, &options, thinking, &turns[from..end], context);
        let wake = ctx.clone();
        let (rx, cancel) = chat::spawn_request(
            move || wake.request_repaint(),
            move |stop, send| chat::stream_reply_retried(port, &body, stop, send),
        );
        self.chat.summarizing = Some(Summarizing {
            rx,
            cancel,
            started: Instant::now(),
            upto: end,
            messages: end - from,
            text: String::new(),
            before,
            after: None,
            then_reply,
        });
        self.chat.error = None;
        self.chat.notice = None;
        self.chat.room_note = None;
        true
    }

    /// Summarize earlier messages, in the chat's ⋯: the summary is written now, whether Auto-compact is on or not.
    pub(super) fn summarize_now(&mut self, ctx: &egui::Context) {
        if self.chat.replying() || !self.chat_running() || !self.can_summarize() {
            return;
        }
        if !self.start_summary(ctx, false) {
            self.chat.notice = Some("There is nothing earlier to summarize yet.".to_string());
        }
    }

    /// Continue, under an answer that was cut off: the model is asked to go on with it, after a summary that makes room
    /// when Auto-compact is on. With it off and no room left, what makes room is said instead.
    pub(super) fn continue_reply(&mut self, ctx: &egui::Context) {
        // an answer the full context cut off goes on only after earlier messages make room
        let room_to_make = !self.chat.cut_by_context || self.can_summarize();
        if self.chat.replying() || !self.chat.cut_off || !room_to_make || !self.chat_running() {
            return;
        }
        if !self.chat.turns.last().is_some_and(|t| !t.from_person) {
            return;
        }
        self.chat.continuing = true;
        let full = self
            .chat_context()
            .is_some_and(|c| self.reckon_next() as f64 >= c as f64 * NO_ROOM);
        if full && !self.stored.auto_compact() {
            self.chat.continuing = false;
            self.chat.notice = Some(
                "There is no room left in the context to go on. Turn on Auto-compact in How it runs, or choose \
                 Summarize earlier messages in ⋯, then Continue."
                    .to_string(),
            );
            return;
        }
        self.chat.retried = false;
        self.chat.notice = None;
        self.ask_for_reply(ctx);
    }

    /// The engine said how long its context is (in a request it refused as too long): the run's facts say so from now.
    pub(super) fn note_context(&mut self, n: u64) {
        let Some((port, model)) = self.chat_target() else {
            return;
        };
        if let Some(f) = self
            .chat
            .run_facts
            .as_mut()
            .filter(|f| f.port == port && f.model == model)
        {
            f.context = Some(n);
        }
    }

    /// Each frame: what the run said of itself, the summary as it comes and what waits for it, and a request refused as
    /// too long, which is made room for and asked again once.
    pub(super) fn poll_capabilities(&mut self, ctx: &egui::Context) {
        self.probe_run(ctx);
        if std::mem::take(&mut self.chat.too_long) {
            self.chat.retried = true;
            if !self.start_summary(ctx, true) {
                self.chat.continuing = false;
                self.chat.error = Some(too_long_words(self.chat_context(), true));
            }
        }
        let Some(summarizing) = self.chat.summarizing.as_mut() else {
            return;
        };
        let mut outcome: Option<Result<(), String>> = None;
        while let Ok(event) = summarizing.rx.try_recv() {
            match event {
                ChatEvent::Text(piece) => summarizing.text.push_str(&piece),
                ChatEvent::Usage { completion, .. } => summarizing.after = Some(completion),
                ChatEvent::Done => outcome = Some(Ok(())),
                ChatEvent::Failed(why) | ChatEvent::TooLong { why, .. } => outcome = Some(Err(why)),
                _ => {}
            }
        }
        let Some(outcome) = outcome else {
            return;
        };
        let Some(done) = self.chat.summarizing.take() else {
            return;
        };
        let text = done.text.trim().to_string();
        match outcome {
            Ok(()) if !text.is_empty() => {
                self.chat.summary = Some(Summary {
                    text,
                    upto: done.upto,
                    before: Some(done.before),
                    after: done.after,
                });
                self.chat.summary_edit = None;
                // the context chip says at once what the conversation now takes
                let now = self.reckon_next();
                if let Some(used) = self.chat.used.as_mut() {
                    used.tokens = now;
                }
                self.keep_conversation();
                if done.then_reply {
                    self.ask_for_reply(ctx);
                }
            }
            Ok(()) => {
                self.chat.continuing = false;
                self.chat.error = Some(
                    "The model wrote no summary of the earlier messages. Try again, or start a new chat.".to_string(),
                );
            }
            Err(why) => {
                self.chat.continuing = false;
                self.chat.error = Some(format!(
                    "The earlier messages could not be summarized: {why}"
                ));
            }
        }
    }

    /// Asks the run the chat talks to what it is, once for each run: this computer's, or one on another computer through
    /// its tunnel. A run that stops is forgotten, so that one started again (with another context) is asked again.
    fn probe_run(&mut self, ctx: &egui::Context) {
        if let Some(probing) = &self.chat.probing {
            match probing.rx.try_recv() {
                Ok(facts) => {
                    self.chat.run_facts = Some(facts);
                    self.chat.probing = None;
                }
                Err(TryRecvError::Disconnected) => {
                    // nothing was said: the run is taken for one that says nothing, and not asked again
                    self.chat.run_facts = Some(RunFacts {
                        port: probing.port,
                        model: probing.model.clone(),
                        ..RunFacts::default()
                    });
                    self.chat.probing = None;
                }
                Err(TryRecvError::Empty) => return,
            }
        }
        if !self.chat_running() {
            self.chat.run_facts = None;
            return;
        }
        let Some((port, model)) = self.chat_target() else {
            return;
        };
        let known = self.run_facts_now().cloned();
        // Ollama says the context of a model once it is loaded: asked again after the first reply, if it had not
        let again = known.as_ref().is_some_and(|f| {
            f.dialect == Dialect::Ollama
                && f.context.is_none()
                && !f.asked_again
                && self.chat.used.is_some()
        });
        if known.is_some() && !again {
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        let asked = model.clone();
        std::thread::spawn(move || {
            let mut facts = probe(port, &asked);
            facts.asked_again = again;
            let _ = tx.send(facts);
            wake.request_repaint();
        });
        self.chat.probing = Some(Probing { rx, port, model });
    }

    /// Writes down a capability the person switched (How it runs, Capabilities).
    pub(super) fn set_capability(&mut self, which: Capability, on: bool) {
        let mut stored = self.settings_now();
        match which {
            Capability::AutoCompact => stored.set_auto_compact(on),
            Capability::Thinking => stored.set_thinking(Some(on)),
            Capability::ShowThinking => stored.set_show_thinking(on),
        }
        self.write_settings(&stored);
        self.stored = stored;
        if which == Capability::AutoCompact && on {
            self.chat.room_note = None;
        }
    }
}

/// The capabilities of the chat, each a switch in How it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Capability {
    AutoCompact,
    Thinking,
    ShowThinking,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(n: usize) -> Vec<Turn> {
        (0..n)
            .flat_map(|i| {
                [
                    Turn::said(true, format!("question {i}")),
                    Turn::said(false, format!("answer {i}")),
                ]
            })
            .collect()
    }

    /// The summary ends before the last two exchanges, which the model reads whole; before the last alone when the two
    /// take more than half the context; and never inside an exchange. Nothing is summarized of fewer than two.
    #[test]
    fn a_summary_ends_where_the_last_exchanges_kept_whole_begin() {
        let turns = exchange(5);
        let small = |_| 10;
        assert_eq!(summary_end(&turns, 0, 1000, small, small), Some(6));
        // the two last exchanges would take more than half the context: only the last is kept
        let big_from = |at: usize| if at <= 6 { 600 } else { 300 };
        assert_eq!(summary_end(&turns, 0, 1000, big_from, small), Some(8));
        // from a summary there was (of the first two exchanges) on
        assert_eq!(summary_end(&turns, 4, 1000, small, small), Some(6));
        assert_eq!(summary_end(&turns, 6, 1000, small, small), Some(8));
        assert_eq!(summary_end(&turns, 8, 1000, small, small), None);
        assert_eq!(summary_end(&exchange(1), 0, 1000, small, small), None);
        // a reply that used tools stays with the message it answers: the end is at a message of the person's
        let mut tools = exchange(3);
        tools.insert(3, Turn::said(false, "asked for a tool"));
        let end = summary_end(&tools, 0, 1000, small, small).unwrap();
        assert!(tools[end].from_person, "{end}");
        // a summary request that would take too much of the context ends earlier, but takes one exchange at least
        let asked = |end: usize| end as u64 * 100;
        assert_eq!(summary_end(&turns, 0, 1000, small, asked), Some(6));
        let asked = |end: usize| end as u64 * 200;
        assert_eq!(summary_end(&turns, 0, 1000, small, asked), Some(2));
        let huge = |_| 10_000;
        assert_eq!(summary_end(&turns, 0, 1000, small, huge), Some(2));
    }

    /// The request for a summary gives the conversation as the chat had it (its instructions and the summary there was,
    /// long texts and what tools gave shortened, pictures named), then asks for the summary as the person; without
    /// thinking where the engine can be told, and with a length only where thinking cannot take it.
    #[test]
    fn the_request_for_a_summary_is_the_conversation_and_the_ask() {
        let mut turns = exchange(2);
        turns[0].text = format!("{}{}", "a".repeat(5000), "b".repeat(3000));
        turns[0].pictures.push(chat::Picture {
            name: "plot.png".into(),
            mime: "image/png".into(),
            base64: "AAAA".into(),
        });
        turns[1].calls.push(chat::ToolCall {
            id: "c".into(),
            name: "files_read".into(),
            server: "files".into(),
            tool: "read".into(),
            arguments: "{\"path\":\"budget.xlsx\"}".into(),
            state: chat::CallState::Done("x".repeat(9000)),
            open: false,
            ticket: None,
        });
        let options = Options {
            instructions: Some("Answer briefly.".into()),
            summary: Some("- Earlier: a garden.".into()),
            dialect: Dialect::LlamaCpp,
            ..Options::default()
        };
        let body = summary_request("qwen", &options, (true, true), &turns, Some(65_536));
        let messages = body["messages"].as_array().unwrap();
        // the instructions and the summary there was, as every request has them
        let system = messages[0]["content"].as_str().unwrap();
        assert!(
            system.starts_with("Answer briefly.") && system.ends_with("- Earlier: a garden."),
            "{system}"
        );
        let first = messages[1]["content"].as_array().map_or_else(
            || messages[1]["content"].as_str().unwrap().to_string(),
            |parts| parts[0]["text"].as_str().unwrap().to_string(),
        );
        assert!(
            first.contains("characters left out") && first.contains("bbbb"),
            "long texts are shortened"
        );
        assert!(
            first.ends_with("(Pictures sent with it: plot.png)"),
            "pictures are named: {first}"
        );
        assert!(!body.to_string().contains("AAAA"), "and not sent");
        let call = &messages[2]["tool_calls"][0]["function"]["arguments"];
        assert_eq!(
            call, "{\"path\":\"budget.xlsx\"}",
            "the arguments stay whole"
        );
        let result = messages[3]["content"].as_str().unwrap();
        assert!(
            result.contains("characters left out") && result.chars().count() < 2100,
            "{}",
            result.len()
        );
        let last = messages.last().unwrap();
        assert_eq!(
            (last["role"].as_str(), last["content"].as_str()),
            (Some("user"), Some(SUMMARY_ASK))
        );
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking": false})
        );
        assert_eq!(body["max_tokens"], json!(2048));
        assert_eq!(body["temperature"], json!(0.2));
        assert!(body.get("tools").is_none());
        // Ollama is told only of a model that thinks; a model that cannot be told is given the room to think
        let ollama = Options {
            dialect: Dialect::Ollama,
            ..Options::default()
        };
        let thinks = summary_request("q", &ollama, (true, true), &turns, Some(8192));
        assert_eq!(thinks["reasoning_effort"], json!("none"));
        assert_eq!(thinks["max_tokens"], json!(1365));
        let plain = summary_request("q", &ollama, (false, false), &turns, None);
        assert!(plain.get("reasoning_effort").is_none());
        assert_eq!(plain["max_tokens"], json!(1024));
        let other = Options {
            dialect: Dialect::Other,
            ..Options::default()
        };
        let always = summary_request("r1", &other, (true, false), &turns, Some(4096));
        assert!(always.get("max_tokens").is_none() && always.get("chat_template_kwargs").is_none());
    }

    /// What a request takes is reckoned at what the engine counted a character to come to; before it counted any,
    /// other scripts than the Latin one are reckoned at more a character.
    #[test]
    fn what_a_request_takes_is_reckoned_from_what_the_engine_counted() {
        let used = Used::counted(3000, 120, 10_000);
        assert_eq!(used.tokens, 3120);
        assert!((used.per_char - 0.3).abs() < 1e-9);
        assert_eq!(reckon(1000, 0, used.per_char), 300);
        assert_eq!(reckon(1000, 2, 0.3), 300 + 2 * PICTURE_TOKENS);
        let latin = [Turn::said(true, "a garden plan")];
        let malayalam = [Turn::said(true, "മലയാളം")];
        assert!(guess_per_char(&malayalam) > 2.0 * guess_per_char(&latin));
        let options = Options {
            summary: Some("- a summary".into()),
            ..Options::default()
        };
        assert!(request_chars(&options, &latin) > request_chars(&Options::default(), &latin));
        assert_eq!(tokens_words(800), "800");
        assert_eq!(tokens_words(2840), "2.8K");
        assert_eq!(tokens_words(12_345), "12K");
    }

    /// The run is asked what it is: llama.cpp by its `/props` (the room of a request and the chat template), Ollama by
    /// `/api/ps` (the context of the model loaded) and `/api/show` (whether it thinks); another server says nothing.
    #[test]
    fn a_run_says_its_engine_its_context_and_whether_its_model_thinks() {
        use crate::testkit::fake_ollama::FakeOllama;
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 2_500_000_000);
        let facts = probe(fake.port(), "qwen3:4b");
        assert_eq!(facts.dialect, Dialect::Ollama);
        assert_eq!(facts.port, fake.port());
        assert_eq!(
            facts.context, None,
            "Ollama says it once the model is loaded"
        );
        // a request loads it
        let _ = ureq::post(format!(
            "http://127.0.0.1:{}/v1/chat/completions",
            fake.port()
        ))
        .send(
            json!({"model": "qwen3:4b", "messages": [{"role": "user", "content": "Hi"}]})
                .to_string(),
        );
        assert_eq!(probe(fake.port(), "qwen3:4b").context, Some(4096));
        // a port nothing listens on says nothing
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert_eq!(probe(port, "m").dialect, Dialect::Other);
    }
}
