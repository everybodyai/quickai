//! Talking to QuickAI (C216): the microphone in the chat's box, what
//! was said written down by whisper.cpp and put in the box (or sent), answers read aloud by the computer's own voice,
//! and hands-free: listen, send, read the answer, listen again. What hears and speaks is `crate::voice`; this is how
//! the window uses and shows it.
//!
//! - The microphone (beside the send arrow) records until ■, or while Ctrl+Space is held; what was said goes in the
//!   box to be checked, or is sent when "Send when I stop talking" is on (then a pause ends it).
//! - Read aloud on an answer reads it, lighting the sentence being read; the speaker in the box reads every answer as
//!   it comes.
//! - Hands-free (the chat's ⋯, the microphone's right click, Ctrl+Shift+Space) covers the chat with what it is doing.
//! - The first time, the speech model is downloaded (82 MB), unless the package came with it; a microphone the system
//!   keeps from apps is said, with the way to its switch.

// without speech to text (feature `voice` off) the parts that only hearing uses are left unused, not taken out one by one
#![cfg_attr(not(feature = "voice"), allow(dead_code, unused_imports))]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::jobs::Job;
use crate::paths;
use crate::voice::settings::{
    language_name, speech_model, SpeechModel, VoiceSettings, LANGUAGES, SPEECH_MODELS,
};
use crate::voice::speak::{Speaker, Spoken, VoiceInfo};
use crate::voice::spoken::{self, SentenceFeed};
#[cfg(feature = "voice")]
use crate::voice::{
    capture::{self, MicError, Microphone},
    transcribe::{Transcriber, Written},
    vad::{Heard, Vad},
};

/// What the words heard are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    /// The microphone was pressed: they go in the box.
    Box,
    /// Ctrl+Space was held: they go in the box when it is let go.
    Hold,
    HandsFree,
}

/// What hands-free is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Listening,
    /// What was said is being written down.
    Writing,
    /// The model is answering.
    Waiting,
    /// The answer is being read.
    Answering,
}

pub(super) struct HandsFree {
    pub(super) phase: Phase,
    /// The person's message the answer is to: the turns after it are its answer.
    after: Option<usize>,
    /// The answer that was read (or shown, muted), so that it is not read twice.
    read_turn: Option<usize>,
    /// What was heard last.
    heard: String,
    /// Something to say under the sign (nothing was heard; the answer failed).
    note: Option<String>,
}

/// A reading aloud going on.
pub(super) struct Reading {
    utterance: u64,
    /// The turn of the chat it reads.
    pub(super) turn: usize,
    sentences: Vec<String>,
    index: Option<usize>,
    /// An answer still being written: its sentences as they come, and how much of its text was taken.
    feed: Option<(SentenceFeed, usize)>,
    hands_free: bool,
}

impl Reading {
    /// The sentence being read.
    pub(super) fn sentence(&self) -> Option<&str> {
        self.index
            .and_then(|i| self.sentences.get(i))
            .map(String::as_str)
    }
}

/// A card over the microphone.
pub(super) enum Card {
    /// The speech model is needed and not here.
    Download {
        model: &'static SpeechModel,
        purpose: Option<Purpose>,
    },
    Downloading {
        model: &'static SpeechModel,
        purpose: Option<Purpose>,
    },
    /// The microphone does not work: why, and where its switch is.
    Blocked {
        title: String,
        words: String,
        settings: Option<&'static str>,
    },
    /// Something went wrong: a title and why.
    Said { title: String, words: String },
}

#[cfg(feature = "voice")]
struct Listening {
    rec: capture::Recording,
    purpose: Purpose,
    started: Instant,
    samples: Vec<f32>,
    vad: Vad,
    /// Someone began to talk.
    voiced: bool,
    /// How loud the last moments were, for the bars.
    levels: VecDeque<f32>,
}

/// Talking to QuickAI: what the window keeps of it.
#[derive(Default)]
pub(super) struct Talk {
    pub(super) settings: VoiceSettings,
    #[cfg(feature = "voice")]
    listening: Option<Listening>,
    #[cfg(feature = "voice")]
    transcriber: Option<Transcriber>,
    /// A recording being written down, and what for.
    writing: Option<(u64, Purpose)>,
    next: u64,
    speaker: Option<Speaker>,
    pub(super) reading: Option<Reading>,
    pub(super) hands_free: Option<HandsFree>,
    pub(super) card: Option<Card>,
    download: Option<Job<String>>,
    /// The answer that was read aloud as it came (the speaker of the box), so that it is not read again.
    auto_turn: Option<usize>,
    /// Where the microphone is, for the cards over it.
    mic_rect: Option<egui::Rect>,
    /// The box gets the keys again (what was said was put in it).
    pub(super) focus_box: bool,
    /// Nothing was heard: said in the box's hint for a moment.
    nothing_heard: Option<Instant>,
    #[cfg(feature = "voice")]
    mics: Option<Vec<Microphone>>,
    voices: Option<Result<Vec<VoiceInfo>, String>>,
    voices_asked: Option<Receiver<Result<Vec<VoiceInfo>, String>>>,
}

/// How long a recording may be, and how much is kept before someone talks in hands-free (so that the first syllable is
/// not lost).
const LONGEST_RECORDING: Duration = Duration::from_secs(120);
const LONGEST_HANDS_FREE: Duration = Duration::from_secs(30);
#[cfg(feature = "voice")]
const PRE_ROLL: usize = crate::voice::RATE as usize / 2;

impl Talk {
    pub(super) fn new(settings: VoiceSettings) -> Talk {
        Talk {
            settings,
            ..Talk::default()
        }
    }

    /// The microphone is recording for the box (not hands-free, which shows itself).
    pub(super) fn recording_for_box(&self) -> bool {
        #[cfg(feature = "voice")]
        {
            self.listening
                .as_ref()
                .is_some_and(|l| l.purpose != Purpose::HandsFree)
        }
        #[cfg(not(feature = "voice"))]
        {
            false
        }
    }

    /// What was said for the box is being written down.
    pub(super) fn writing_for_box(&self) -> bool {
        self.writing.is_some_and(|(_, p)| p != Purpose::HandsFree)
    }

    /// The sentence being read of `turn`, to light it.
    pub(super) fn reading_in(&self, turn: usize) -> Option<&str> {
        self.reading
            .as_ref()
            .filter(|r| r.turn == turn)
            .and_then(Reading::sentence)
    }

    /// "Nothing was heard" is shown in the box's hint for a few seconds.
    pub(super) fn said_nothing_heard(&self) -> bool {
        self.nothing_heard
            .is_some_and(|at| at.elapsed() < Duration::from_secs(4))
    }
}

/// The microphone works in this build.
pub(super) const CAN_HEAR: bool = cfg!(feature = "voice");

impl App {
    fn speech_model_now(&self) -> &'static SpeechModel {
        speech_model(&self.talk.settings.model).unwrap_or(&SPEECH_MODELS[0])
    }

    fn speech_model_path(&self) -> Option<PathBuf> {
        self.speech_model_now()
            .found_in(&paths::bundled_speech_dir(), &paths::user_speech_dir())
    }

    fn speaker(&mut self, ctx: &egui::Context) -> &Speaker {
        let ctx = ctx.clone();
        self.talk
            .speaker
            .get_or_insert_with(|| Speaker::new(move || ctx.request_repaint()))
    }

    /// Voice turned off in the Settings: whatever of it was going stops.
    fn voice_turned_off(&mut self, ctx: &egui::Context) {
        if self.talk.hands_free.is_some() {
            self.end_hands_free(ctx);
        }
        self.stop_listening(false, ctx);
        self.stop_reading();
    }

    /// The person changed something of the voice: it is kept.
    fn voice_changed(&mut self) {
        self.save_settings();
    }

    /// Whether a message can be sent now: a model runs and the box takes it.
    fn chat_ready(&self) -> bool {
        self.chat_running() && (self.chat_model().is_some() || self.setup_in_use().is_some())
    }

    // ---- the microphone ----------------------------------------------------------------------------------------

    /// The microphone pressed: it listens for `purpose`, or says why it cannot.
    pub(super) fn start_listening(&mut self, purpose: Purpose, ctx: &egui::Context) {
        #[cfg(feature = "voice")]
        {
            // voice off: the microphone is never opened
            if !self.talk.settings.enabled || self.talk.listening.is_some() {
                return;
            }
            // what it says itself is not heard
            if let Some(r) = self.talk.reading.take() {
                if let Some(s) = &self.talk.speaker {
                    s.stop();
                }
                if r.hands_free {
                    if let Some(hf) = &mut self.talk.hands_free {
                        hf.read_turn = Some(r.turn);
                    }
                }
            }
            // the program that writes down what was said (quickai-whisper), which the packages carry next to the app
            if crate::voice::transcribe::program().is_none() {
                self.talk.card = Some(Card::Said {
                    title: "Speech to text is not here".into(),
                    words: crate::voice::transcribe::missing(),
                });
                if purpose == Purpose::HandsFree {
                    self.talk.hands_free = None;
                }
                return;
            }
            if self.speech_model_path().is_none() {
                if !matches!(self.talk.card, Some(Card::Downloading { .. })) {
                    self.talk.card = Some(Card::Download {
                        model: self.speech_model_now(),
                        purpose: Some(purpose),
                    });
                }
                if purpose == Purpose::HandsFree {
                    self.talk.hands_free = None;
                }
                return;
            }
            match capture::start(self.talk.settings.microphone.as_deref()) {
                Ok(rec) => {
                    self.talk.card = None;
                    self.talk.listening = Some(Listening {
                        rec,
                        purpose,
                        started: Instant::now(),
                        samples: Vec::new(),
                        vad: Vad::new(if purpose == Purpose::HandsFree {
                            1000
                        } else {
                            1400
                        }),
                        voiced: false,
                        levels: VecDeque::new(),
                    });
                    ctx.request_repaint();
                }
                Err(e) => {
                    self.talk.card = Some(blocked_card(&e));
                    if purpose == Purpose::HandsFree {
                        self.talk.hands_free = None;
                    }
                }
            }
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = (purpose, ctx);
        }
    }

    /// Ends the recording: what was said is written down (`keep`), or thrown away.
    pub(super) fn stop_listening(&mut self, keep: bool, ctx: &egui::Context) {
        #[cfg(feature = "voice")]
        {
            let Some(l) = self.talk.listening.take() else {
                return;
            };
            let mut samples = l.samples;
            samples.extend(l.rec.take());
            drop(l.rec);
            if !keep {
                return;
            }
            let Some(model) = self.speech_model_path() else {
                return;
            };
            self.talk.next += 1;
            let id = self.talk.next;
            let wake = ctx.clone();
            let transcriber = self
                .talk
                .transcriber
                .get_or_insert_with(|| Transcriber::new(move || wake.request_repaint()));
            transcriber.write(id, &model, samples, self.talk.settings.language.clone());
            self.talk.writing = Some((id, l.purpose));
            if let Some(hf) = &mut self.talk.hands_free {
                hf.phase = Phase::Writing;
            }
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = (keep, ctx);
        }
    }

    #[cfg(feature = "voice")]
    fn poll_listening(&mut self, ctx: &egui::Context) {
        let send_when_stopped = self.talk.settings.send_when_stopped;
        let Some(l) = &mut self.talk.listening else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(40));
        let fresh = l.rec.take();
        let heard = l.vad.push(&fresh);
        l.levels.push_back(l.vad.level);
        while l.levels.len() > 120 {
            l.levels.pop_front();
        }
        l.samples.extend(fresh);
        if heard == Heard::Began {
            l.voiced = true;
        }
        let purpose = l.purpose;
        // before anyone talks, hands-free keeps only the last half second
        if purpose == Purpose::HandsFree && !l.voiced && l.samples.len() > PRE_ROLL * 2 {
            let drop = l.samples.len() - PRE_ROLL;
            l.samples.drain(..drop);
        }
        let elapsed = l.started.elapsed();
        let error = l.rec.error();
        // macOS gives silence, not an error, to an app it keeps the microphone from
        let silent = cfg!(target_os = "macos")
            && elapsed > Duration::from_millis(2500)
            && !l.rec.heard_sound();
        let ends = (heard == Heard::Ended
            && (purpose == Purpose::HandsFree || (purpose == Purpose::Box && send_when_stopped)))
            || (purpose == Purpose::HandsFree && l.voiced && elapsed > LONGEST_HANDS_FREE)
            || elapsed > LONGEST_RECORDING;
        if let Some(e) = error {
            self.talk.listening = None;
            self.talk.card = Some(blocked_card(&e));
            self.talk.hands_free = None;
        } else if silent {
            self.talk.listening = None;
            self.talk.card = Some(blocked_card(&MicError::Blocked(
                "only silence came from it".into(),
            )));
            self.talk.hands_free = None;
        } else if ends {
            self.stop_listening(true, ctx);
        }
    }

    #[cfg(feature = "voice")]
    fn poll_written(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.talk.transcriber else {
            return;
        };
        for written in t.take() {
            let (id, result) = match written {
                Written::Words { id, text, .. } => (id, Ok(text)),
                Written::Failed { id, why } => (id, Err(why)),
            };
            let Some((wanted, purpose)) = self.talk.writing else {
                continue;
            };
            if wanted != id {
                continue;
            }
            self.talk.writing = None;
            match (purpose, result) {
                (Purpose::HandsFree, Ok(text)) => self.hands_free_heard(text, ctx),
                (Purpose::HandsFree, Err(why)) => {
                    if let Some(hf) = &mut self.talk.hands_free {
                        hf.note = Some(format!("It could not be written down: {why}"));
                    }
                    self.listen_again(ctx);
                }
                (_, Ok(text)) if text.is_empty() => self.talk.nothing_heard = Some(Instant::now()),
                (_, Ok(text)) => {
                    let input = &mut self.chat.input;
                    if !input.trim().is_empty() && !input.ends_with([' ', '\n']) {
                        input.push(' ');
                    }
                    input.push_str(&text);
                    self.talk.focus_box = true;
                    if self.talk.settings.send_when_stopped
                        && self.chat_ready()
                        && !self.chat.replying()
                    {
                        self.send_chat(ctx);
                    }
                }
                (_, Err(why)) => {
                    self.talk.card = Some(Card::Said {
                        title: "What you said could not be written down".into(),
                        words: why,
                    })
                }
            }
        }
    }

    // ---- reading aloud ------------------------------------------------------------------------------------------

    /// Reads the answer of `turn` aloud, from the start.
    pub(super) fn read_turn(&mut self, turn: usize, ctx: &egui::Context) {
        if !self.talk.settings.enabled {
            return;
        }
        let Some(text) = self.chat.turns.get(turn).map(|t| t.text.clone()) else {
            return;
        };
        let sentences = spoken::sentences(&text);
        if sentences.is_empty() {
            return;
        }
        self.begin_reading(turn, sentences, None, false, ctx);
    }

    /// Starts reading `turn`: `sentences` known now, and a feed when more come.
    fn begin_reading(
        &mut self,
        turn: usize,
        sentences: Vec<String>,
        feed: Option<(SentenceFeed, usize)>,
        hands_free: bool,
        ctx: &egui::Context,
    ) {
        let (voice, speed) = (self.talk.settings.voice.clone(), self.talk.settings.speed);
        if let Err(why) = self.speaker(ctx).available() {
            self.talk.card = Some(Card::Said {
                title: "QuickAI cannot read aloud here".into(),
                words: why,
            });
            return;
        }
        self.talk.next += 1;
        let utterance = self.talk.next;
        let more = feed.is_some();
        self.speaker(ctx)
            .say(utterance, sentences.clone(), more, voice, speed);
        self.talk.reading = Some(Reading {
            utterance,
            turn,
            sentences,
            index: None,
            feed,
            hands_free,
        });
    }

    pub(super) fn stop_reading(&mut self) {
        if let Some(r) = self.talk.reading.take() {
            if let Some(s) = &self.talk.speaker {
                s.stop();
            }
            if r.hands_free {
                if let Some(hf) = &mut self.talk.hands_free {
                    hf.read_turn = Some(r.turn);
                }
            }
        }
    }

    /// Reads every answer as it comes (the speaker in the box), and hands-free's answer: a reading follows the answer
    /// that is being written, a sentence as soon as it is whole.
    fn follow_answer(&mut self, ctx: &egui::Context) {
        if !self.talk.settings.enabled {
            return;
        }
        let last = self.chat.turns.len().checked_sub(1);
        let last_reply = last.filter(|&i| !self.chat.turns[i].from_person);
        let replying = self.chat.replying();
        // the answer to follow, and whether it is hands-free's
        let wanted = match &self.talk.hands_free {
            Some(hf)
                if !self.talk.settings.hands_free_muted
                    && matches!(hf.phase, Phase::Waiting | Phase::Answering) =>
            {
                last_reply
                    .filter(|&i| hf.after.is_some_and(|a| i > a) && hf.read_turn != Some(i))
                    .map(|i| (i, true))
            }
            Some(_) => None,
            None if self.talk.settings.read_answers && replying => last_reply
                .filter(|&i| self.talk.auto_turn != Some(i))
                .map(|i| (i, false)),
            None => None,
        };
        if let Some((turn, hands_free)) = wanted {
            let following = self.talk.reading.as_ref().is_some_and(|r| r.turn == turn);
            if !following {
                if !hands_free {
                    self.talk.auto_turn = Some(turn);
                }
                self.begin_reading(
                    turn,
                    Vec::new(),
                    Some((SentenceFeed::default(), 0)),
                    hands_free,
                    ctx,
                );
            }
        }
        // the reading that follows an answer takes what came of it since
        let Some(r) = &mut self.talk.reading else {
            return;
        };
        let Some((feed, taken)) = &mut r.feed else {
            return;
        };
        let Some(turn) = self.chat.turns.get(r.turn) else {
            self.stop_reading();
            return;
        };
        let mut fresh = Vec::new();
        if turn.text.len() > *taken && turn.text.is_char_boundary(*taken) {
            fresh = feed.push(&turn.text[*taken..]);
            *taken = turn.text.len();
        }
        let done = !(replying && last == Some(r.turn));
        if done {
            fresh.extend(feed.finish());
            r.feed = None;
        }
        let (utterance, ends) = (r.utterance, done);
        r.sentences.extend(fresh.iter().cloned());
        if let Some(s) = &self.talk.speaker {
            s.more(utterance, fresh);
            if ends {
                s.no_more(utterance);
            }
        }
    }

    fn poll_spoken(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.talk.speaker else {
            return;
        };
        for event in s.take() {
            match event {
                Spoken::Began { utterance, index } => {
                    if let Some(r) = self
                        .talk
                        .reading
                        .as_mut()
                        .filter(|r| r.utterance == utterance)
                    {
                        r.index = Some(index);
                    }
                }
                Spoken::Ended { utterance } => {
                    if self
                        .talk
                        .reading
                        .as_ref()
                        .is_some_and(|r| r.utterance == utterance)
                    {
                        self.stop_reading();
                    }
                }
                Spoken::Failed { utterance, why } => {
                    if self
                        .talk
                        .reading
                        .as_ref()
                        .is_some_and(|r| r.utterance == utterance)
                    {
                        let hands_free = self.talk.reading.as_ref().is_some_and(|r| r.hands_free);
                        self.stop_reading();
                        match &mut self.talk.hands_free {
                            Some(hf) if hands_free => {
                                hf.note = Some(format!("It could not read aloud: {why}"))
                            }
                            _ => {
                                self.talk.card = Some(Card::Said {
                                    title: "It could not read aloud".into(),
                                    words: why,
                                })
                            }
                        }
                    }
                }
            }
        }
        if self.talk.reading.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    // ---- hands-free ---------------------------------------------------------------------------------------------

    pub(super) fn start_hands_free(&mut self, ctx: &egui::Context) {
        if !CAN_HEAR
            || !self.talk.settings.enabled
            || self.talk.hands_free.is_some()
            || !self.chat_ready()
        {
            return;
        }
        if self.talk.recording_for_box() {
            self.stop_listening(false, ctx);
        }
        self.stop_reading();
        self.talk.hands_free = Some(HandsFree {
            phase: Phase::Listening,
            after: None,
            read_turn: None,
            heard: String::new(),
            note: None,
        });
        self.start_listening(Purpose::HandsFree, ctx);
    }

    pub(super) fn end_hands_free(&mut self, ctx: &egui::Context) {
        if self.talk.hands_free.take().is_none() {
            return;
        }
        #[cfg(feature = "voice")]
        if self
            .talk
            .listening
            .as_ref()
            .is_some_and(|l| l.purpose == Purpose::HandsFree)
        {
            self.stop_listening(false, ctx);
        }
        if self
            .talk
            .writing
            .is_some_and(|(_, p)| p == Purpose::HandsFree)
        {
            self.talk.writing = None;
        }
        if self.talk.reading.as_ref().is_some_and(|r| r.hands_free) {
            self.stop_reading();
        }
        let _ = ctx;
    }

    /// Interrupt: the answer stops (being written and read), and it listens.
    pub(super) fn interrupt_hands_free(&mut self, ctx: &egui::Context) {
        if self.talk.hands_free.is_none() {
            return;
        }
        if self.chat.replying() {
            self.chat.stop();
            self.poll_chat();
        }
        self.stop_reading();
        if let Some(hf) = &mut self.talk.hands_free {
            hf.read_turn = self.chat.turns.iter().rposition(|t| !t.from_person);
        }
        self.listen_again(ctx);
    }

    pub(super) fn toggle_hands_free_mute(&mut self) {
        self.talk.settings.hands_free_muted = !self.talk.settings.hands_free_muted;
        if self.talk.settings.hands_free_muted
            && self.talk.reading.as_ref().is_some_and(|r| r.hands_free)
        {
            self.stop_reading();
        }
        self.voice_changed();
    }

    fn listen_again(&mut self, ctx: &egui::Context) {
        if let Some(hf) = &mut self.talk.hands_free {
            hf.phase = Phase::Listening;
            self.start_listening(Purpose::HandsFree, ctx);
        }
    }

    /// What hands-free heard: sent, or listened for again when it was nothing.
    fn hands_free_heard(&mut self, text: String, ctx: &egui::Context) {
        if self.talk.hands_free.is_none() {
            return;
        }
        if text.is_empty() {
            if let Some(hf) = &mut self.talk.hands_free {
                hf.note = Some("Nothing was heard. Say it again.".into());
            }
            self.listen_again(ctx);
            return;
        }
        if self.chat.replying() {
            self.chat.stop();
            self.poll_chat();
        }
        let before = self.chat.turns.len();
        self.chat.input = text.clone();
        self.send_chat(ctx);
        let sent = self.chat.turns.len() > before;
        if let Some(hf) = &mut self.talk.hands_free {
            hf.heard = text;
            if sent {
                hf.note = None;
                hf.after = self.chat.turns.iter().rposition(|t| t.from_person);
                hf.phase = Phase::Waiting;
            } else {
                hf.note = self
                    .chat
                    .error
                    .clone()
                    .or_else(|| Some("It could not be sent.".into()));
            }
        }
        if !sent {
            self.listen_again(ctx);
        }
    }

    /// Hands-free goes on: from the answer to listening again, when the answer is written and read.
    fn step_hands_free(&mut self, ctx: &egui::Context) {
        let Some(hf) = &self.talk.hands_free else {
            return;
        };
        if !self.chat_ready() {
            self.end_hands_free(ctx);
            self.talk.card = Some(Card::Said {
                title: "Hands-free ended".into(),
                words: "The model stopped. Start it again to talk to it.".into(),
            });
            return;
        }
        if !matches!(hf.phase, Phase::Waiting | Phase::Answering) {
            return;
        }
        let busy = self.chat.replying() || self.chat.waiting_for_tools || self.chat.tools_waiting;
        let reading = self.talk.reading.as_ref().is_some_and(|r| r.hands_free);
        let last_reply = self.chat.turns.iter().rposition(|t| !t.from_person);
        let answered = last_reply.filter(|&i| hf.after.is_some_and(|a| i > a));
        let heard_all = self.talk.settings.hands_free_muted
            || answered.is_none()
            || hf.read_turn == answered
            || answered.is_some_and(|i| self.chat.turns[i].text.trim().is_empty());
        let error = self.chat.error.clone();
        // (the answer read counts as read when its reading ends: `stop_reading`)
        if let Some(hf) = &mut self.talk.hands_free {
            hf.phase = if reading {
                Phase::Answering
            } else {
                Phase::Waiting
            };
        }
        if !busy && !reading && (heard_all || error.is_some()) {
            if let Some(hf) = &mut self.talk.hands_free {
                if let Some(why) = error {
                    hf.note = Some(why);
                }
            }
            self.listen_again(ctx);
        } else if !busy && !reading && !heard_all {
            // the answer came whole before its reading began (a short one): it is read now
            ctx.request_repaint();
        }
    }

    // ---- each frame ---------------------------------------------------------------------------------------------

    pub(super) fn poll_talk(&mut self, ctx: &egui::Context) {
        self.talk_keys(ctx);
        self.poll_speech_download(ctx);
        #[cfg(feature = "voice")]
        {
            self.poll_listening(ctx);
            self.poll_written(ctx);
        }
        self.follow_answer(ctx);
        self.poll_spoken(ctx);
        self.step_hands_free(ctx);
        if let Some(rx) = &self.talk.voices_asked {
            if let Ok(voices) = rx.try_recv() {
                self.talk.voices = Some(voices);
                self.talk.voices_asked = None;
            }
        }
    }

    /// Ctrl+Space held records for the box; Ctrl+Shift+Space starts and ends hands-free; Escape stops reading and ends
    /// hands-free.
    fn talk_keys(&mut self, ctx: &egui::Context) {
        // voice off: the keys are left to the rest of the window
        if !self.talk.settings.enabled {
            return;
        }
        let mut hold: Option<bool> = None;
        let mut toggle = false;
        let mut escape = false;
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } = event
                {
                    match key {
                        egui::Key::Space
                            if *pressed && !*repeat && modifiers.command && modifiers.shift =>
                        {
                            toggle = true
                        }
                        egui::Key::Space if *pressed && !*repeat && modifiers.command => {
                            hold = Some(true)
                        }
                        egui::Key::Space if !*pressed => hold = Some(false),
                        egui::Key::Escape if *pressed => escape = true,
                        _ => {}
                    }
                }
            }
        });
        if toggle {
            if self.talk.hands_free.is_some() {
                self.end_hands_free(ctx);
            } else {
                self.start_hands_free(ctx);
            }
            return;
        }
        #[cfg(feature = "voice")]
        match hold {
            Some(true)
                if self.talk.listening.is_none()
                    && self.talk.hands_free.is_none()
                    && self.chat_ready() =>
            {
                self.start_listening(Purpose::Hold, ctx);
            }
            Some(false)
                if self
                    .talk
                    .listening
                    .as_ref()
                    .is_some_and(|l| l.purpose == Purpose::Hold) =>
            {
                self.stop_listening(true, ctx);
            }
            _ => {}
        }
        #[cfg(not(feature = "voice"))]
        let _ = hold;
        if escape {
            if self.talk.hands_free.is_some() {
                self.end_hands_free(ctx);
            } else if self.talk.reading.is_some() {
                self.stop_reading();
            } else if self.talk.card.is_some() {
                self.talk.card = None;
            }
        }
    }

    // ---- the speech model ---------------------------------------------------------------------------------------

    pub(super) fn download_speech_model(
        &mut self,
        model: &'static SpeechModel,
        purpose: Option<Purpose>,
        ctx: &egui::Context,
    ) {
        if self.talk.download.is_some() {
            return;
        }
        use quickai_core::lookup::present::{Download, Piece};
        let download = Download {
            files: vec![Piece {
                kind: None,
                label: "speech model".into(),
                url: model.url(),
                store_as: model.file.into(),
                name: model.file.into(),
                size: Some(model.bytes),
                sha256: Some(model.sha256.into()),
                place: None,
            }],
            bundle: None,
            source: "huggingface:ggerganov/whisper.cpp".into(),
            mode: None,
        };
        let with = crate::downloader::FetchWith {
            strict_size: true,
            ..Default::default()
        };
        self.talk.download = Some(crate::downloader::spawn_files_with(
            download,
            paths::user_speech_dir(),
            with,
            super::calm_wake(ctx.clone()),
        ));
        self.talk.card = Some(Card::Downloading { model, purpose });
    }

    fn poll_speech_download(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.talk.download else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(250));
        let Some(done) = job.poll() else {
            return;
        };
        self.talk.download = None;
        let purpose = match &self.talk.card {
            Some(Card::Downloading { purpose, .. }) => *purpose,
            _ => None,
        };
        match done {
            Ok(_) => {
                self.talk.card = None;
                self.push_log("[quickai] The speech model is on this computer.".to_string());
                if let Some(purpose) = purpose {
                    if purpose == Purpose::HandsFree {
                        self.start_hands_free(ctx);
                    } else if self.chat_ready() {
                        self.start_listening(purpose, ctx);
                    }
                }
            }
            Err(why) => {
                self.talk.card = Some(Card::Said {
                    title: "The speech model could not be downloaded".into(),
                    words: why,
                })
            }
        }
    }

    // ---- what is drawn ------------------------------------------------------------------------------------------

    /// The room the microphone takes in the box.
    pub(super) fn talk_buttons_width(&self) -> f32 {
        if CAN_HEAR && self.talk.settings.enabled {
            32.0
        } else {
            0.0
        }
    }

    /// The microphone, before the send arrow (and no speaker button:
    /// reading every answer aloud is a setting of Settings › Voice, off unless turned on).
    pub(super) fn show_talk_buttons(&mut self, ui: &mut egui::Ui, ready: bool) {
        // no microphone in the box until voice is turned on in Settings › Voice
        if !CAN_HEAR || !self.talk.settings.enabled {
            return;
        }
        let p = theme::palette();
        if self.talk.writing_for_box() {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(30.0, 28.0), egui::Sense::hover());
            egui::Spinner::new().size(14.0).color(p.muted).paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0)),
            );
            response.on_hover_text("Writing down what you said…");
            return;
        }
        let mic = icons::icon_button(ui, Icon::Mic, egui::vec2(30.0, 28.0), ready, false).on_hover_text(
            "Talk instead of typing: what you say goes in the box (hold Ctrl+Space to talk while it is held). Right-click \
             for hands-free.",
        );
        self.talk.mic_rect = Some(mic.rect);
        super::help::mark_spot(ui, super::help::Spot::Mic, mic.rect);
        if mic.clicked() {
            self.start_listening(Purpose::Box, ui.ctx());
        }
        mic.context_menu(|ui| {
            ui.set_max_width(170.0);
            if icons::menu_item(ui, Icon::Wave, "Hands-free", ready, false).clicked() {
                self.start_hands_free(ui.ctx());
                ui.close();
            }
        });
    }

    /// The box while the microphone records (R1): how long, how loud, × to throw it away and ■ to stop.
    pub(super) fn show_recording_strip(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "voice")]
        {
            let p = theme::palette();
            let Some(l) = &self.talk.listening else {
                return;
            };
            let elapsed = l.started.elapsed().as_secs();
            let levels: Vec<f32> = l.levels.iter().copied().collect();
            let hold = l.purpose == Purpose::Hold;
            let (dot, _) = ui.allocate_exact_size(egui::vec2(18.0, 28.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(dot.center(), 7.0, p.bad.gamma_multiply(0.25));
            ui.painter().circle_filled(dot.center(), 4.5, p.bad);
            ui.label(
                egui::RichText::new(format!("{}:{:02}", elapsed / 60, elapsed % 60))
                    .monospace()
                    .color(p.body),
            );
            // the bars: the newest at the right
            let room = (ui.available_width() - 150.0).max(40.0);
            let (bars, _) = ui.allocate_exact_size(egui::vec2(room, 24.0), egui::Sense::hover());
            let count = (room / 5.0) as usize;
            let colour = p.bad.lerp_to_gamma(p.body, 0.25);
            for k in 0..count {
                let level = levels
                    .len()
                    .checked_sub(count - k)
                    .and_then(|i| levels.get(i))
                    .copied()
                    .unwrap_or(0.0);
                let h = 3.0 + level * 20.0;
                let x = bars.left() + k as f32 * 5.0 + 1.5;
                ui.painter().line_segment(
                    [
                        egui::pos2(x, bars.center().y - h / 2.0),
                        egui::pos2(x, bars.center().y + h / 2.0),
                    ],
                    egui::Stroke::new(3.0, colour),
                );
            }
            ui.label(
                theme::muted(if hold {
                    "Listening (let go to stop)"
                } else {
                    "Listening"
                })
                .small(),
            );
            if icons::icon_button(ui, Icon::Close, egui::vec2(28.0, 28.0), true, false)
                .on_hover_text("Throw it away.")
                .clicked()
            {
                self.stop_listening(false, ui.ctx());
                return;
            }
            let (rect, stop) = ui.allocate_exact_size(egui::vec2(30.0, 28.0), egui::Sense::click());
            let fill = if stop.hovered() {
                p.bad.gamma_multiply(0.85)
            } else {
                p.bad
            };
            ui.painter().rect_filled(rect.shrink(1.0), 7.0, fill);
            icons::paint(
                ui.painter(),
                rect.shrink(8.0),
                Icon::Stop,
                egui::Color32::WHITE,
            );
            let words = if self.talk.settings.send_when_stopped {
                "Stop: what you said is sent."
            } else {
                "Stop: what you said goes in the box, to check before it is sent."
            };
            if stop.on_hover_text(words).clicked() {
                self.stop_listening(true, ui.ctx());
            }
        }
        #[cfg(not(feature = "voice"))]
        let _ = ui;
    }

    /// Read aloud (or Stop reading) under an answer.
    pub(super) fn show_read_aloud(&self, ui: &mut egui::Ui, turn: usize) -> Option<bool> {
        if !self.talk.settings.enabled {
            return None;
        }
        let reading = self
            .talk
            .reading
            .as_ref()
            .is_some_and(|r| r.turn == turn && !r.hands_free);
        let words = if reading {
            "Stop reading"
        } else {
            "Read aloud"
        };
        let pressed = theme::button_sized(ui, Role::Quiet, true, words, egui::vec2(0.0, 18.0))
            .on_hover_text(if reading {
                "Stop reading this answer (Esc)."
            } else {
                "Read this answer aloud in the computer's own voice."
            })
            .clicked();
        pressed.then_some(!reading)
    }

    /// Hands-free over the chat (H1): one large sign of what it is doing, what was heard or is being read, and Mute,
    /// Interrupt and End.
    pub(super) fn show_hands_free(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let Some(hf) = &self.talk.hands_free else {
            return;
        };
        let p = theme::palette();
        let phase = hf.phase;
        let heard = hf.heard.clone();
        let note = hf.note.clone();
        let muted = self.talk.settings.hands_free_muted;
        #[cfg(feature = "voice")]
        let level = self.talk.listening.as_ref().map_or(0.0, |l| l.vad.level);
        #[cfg(not(feature = "voice"))]
        let level = 0.0_f32;
        let reading = self
            .talk
            .reading
            .as_ref()
            .filter(|r| r.hands_free)
            .and_then(|r| r.sentence())
            .map(str::to_string);
        let mut action: Option<&str> = None;
        egui::Area::new(egui::Id::new("hands_free"))
            .fixed_pos(rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let (area, _) = ui.allocate_exact_size(rect.size(), egui::Sense::click());
                let painter = ui.painter();
                painter.rect_filled(area, 0.0, p.page.gamma_multiply(0.97));
                let side = (area.height() * 0.22).clamp(40.0, 70.0);
                let centre = egui::pos2(area.center().x, area.top() + side + 40.0);
                let (colour, icon, title) = match phase {
                    Phase::Listening => (p.bad, Icon::Mic, "Listening…"),
                    Phase::Writing => (p.muted, Icon::Mic, "Writing it down…"),
                    Phase::Waiting => (p.muted, Icon::Chat, "Thinking…"),
                    Phase::Answering => (p.accent_text, Icon::Wave, "Answering"),
                };
                let grow = if phase == Phase::Listening {
                    level * side * 0.35
                } else {
                    0.0
                };
                painter.circle_filled(centre, side + 22.0 + grow, colour.gamma_multiply(0.06));
                painter.circle_filled(
                    centre,
                    side + 10.0 + grow * 0.6,
                    colour.gamma_multiply(0.12),
                );
                painter.circle_filled(centre, side, p.card.lerp_to_gamma(colour, 0.3));
                if matches!(phase, Phase::Writing | Phase::Waiting) {
                    egui::Spinner::new()
                        .size(side * 0.6)
                        .color(p.text)
                        .paint_at(
                            ui,
                            egui::Rect::from_center_size(
                                centre,
                                egui::vec2(side * 0.6, side * 0.6),
                            ),
                        );
                } else {
                    icons::paint(
                        painter,
                        egui::Rect::from_center_size(centre, egui::vec2(side * 0.7, side * 0.7)),
                        icon,
                        p.text,
                    );
                }
                let mut y = centre.y + side + 34.0;
                let title_font = egui::FontId::new(19.0, theme::semibold(ui));
                let g = painter.layout_no_wrap(title.to_string(), title_font, p.text);
                painter.galley(
                    egui::pos2(centre.x - g.size().x / 2.0, y),
                    g.clone(),
                    p.text,
                );
                y += g.size().y + 8.0;
                let sub = match phase {
                    Phase::Listening if muted => {
                        "Ask anything. The answers are shown, not read, and it listens again."
                            .to_string()
                    }
                    Phase::Listening => {
                        "Ask anything. It answers aloud, then listens again.".to_string()
                    }
                    Phase::Writing => "A moment.".to_string(),
                    Phase::Waiting => format!("“{}”", heard),
                    Phase::Answering => reading.map(|s| format!("“{s}”")).unwrap_or_default(),
                };
                let width = (area.width() - 60.0).max(120.0);
                let body = egui::TextStyle::Body.resolve(ui.style());
                let g = painter.layout(sub, body.clone(), p.muted, width);
                painter.galley(
                    egui::pos2(centre.x - g.size().x / 2.0, y),
                    g.clone(),
                    p.muted,
                );
                y += g.size().y + 10.0;
                if let Some(note) = note {
                    let g = painter.layout(note, body, p.warn, width);
                    painter.galley(egui::pos2(centre.x - g.size().x / 2.0, y), g, p.warn);
                }
                // the buttons at the foot
                let foot = egui::Rect::from_center_size(
                    egui::pos2(area.center().x, area.bottom() - 34.0),
                    egui::vec2(360.0_f32.min(area.width() - 20.0), 32.0),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(foot)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        // centred: the room the buttons take is measured as they are laid out
                        let interrupt = matches!(phase, Phase::Waiting | Phase::Answering);
                        let n = if interrupt { 3.0 } else { 2.0 };
                        ui.add_space(((foot.width() - n * 110.0) / 2.0).max(0.0));
                        let mute = if muted {
                            "Read answers"
                        } else {
                            "Mute answers"
                        };
                        if theme::button_sized(
                            ui,
                            Role::Normal,
                            true,
                            mute,
                            egui::vec2(110.0, 30.0),
                        )
                        .on_hover_text(if muted {
                            "Read the answers aloud again."
                        } else {
                            "Show the answers without reading them."
                        })
                        .clicked()
                        {
                            action = Some("mute");
                        }
                        if interrupt
                            && theme::button_sized(
                                ui,
                                Role::Normal,
                                true,
                                "Interrupt",
                                egui::vec2(100.0, 30.0),
                            )
                            .on_hover_text("Stop the answer and listen.")
                            .clicked()
                        {
                            action = Some("interrupt");
                        }
                        if theme::button_sized(
                            ui,
                            Role::Danger,
                            true,
                            "End",
                            egui::vec2(90.0, 30.0),
                        )
                        .on_hover_text("End hands-free (Esc, or Ctrl+Shift+Space).")
                        .clicked()
                        {
                            action = Some("end");
                        }
                    },
                );
            });
        match action {
            Some("mute") => self.toggle_hands_free_mute(),
            Some("interrupt") => self.interrupt_hands_free(ctx),
            Some("end") => self.end_hands_free(ctx),
            _ => {}
        }
        if phase == Phase::Listening {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    /// The card over the microphone: the speech model to download, a microphone that does not work, what went wrong.
    pub(super) fn show_talk_card(&mut self, ctx: &egui::Context) {
        let Some(card) = &self.talk.card else {
            return;
        };
        let p = theme::palette();
        let screen = ctx.content_rect();
        let anchor = self.talk.mic_rect.map_or(
            egui::pos2(screen.right() - 24.0, screen.bottom() - 80.0),
            |r| egui::pos2(r.right() + 40.0, r.top() - 10.0),
        );
        let mut action: Option<&str> = None;
        let progress = self.talk.download.as_ref().map(|j| (j.downloaded, j.total));
        egui::Area::new(egui::Id::new("talk_card"))
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(anchor)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                theme::card_frame()
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(14))
                    .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(90) })
                    .show(ui, |ui| {
                        ui.set_width(330.0);
                        let head = |ui: &mut egui::Ui, icon: Icon, colour: egui::Color32, title: &str| {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                icons::paint(ui.painter(), r, icon, colour);
                                ui.label(egui::RichText::new(title).family(theme::semibold(ui)).color(p.text));
                            });
                            ui.add_space(4.0);
                        };
                        match card {
                            Card::Download { model, .. } => {
                                head(ui, Icon::Mic, p.body, "Talk to QuickAI");
                                ui.label(theme::muted("To understand what you say, QuickAI needs a speech model. It stays on this computer, like your models.").small());
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(format!("{} · {} · many languages", model.name, model.size_words())).small().color(p.faint));
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    if theme::button_sized(ui, Role::Primary, true, "Download", egui::vec2(96.0, 28.0)).clicked() {
                                        action = Some("download");
                                    }
                                    if theme::button_sized(ui, Role::Quiet, true, "Not now", egui::vec2(0.0, 28.0)).clicked() {
                                        action = Some("close");
                                    }
                                });
                            }
                            Card::Downloading { model, .. } => {
                                head(ui, Icon::Download, p.body, "Downloading the speech model");
                                let (done, total) = progress.unwrap_or((0, Some(model.bytes)));
                                let total = total.unwrap_or(model.bytes).max(1);
                                ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0));
                                ui.label(egui::RichText::new(format!(
                                    "{:.0} of {:.0} MB",
                                    done as f64 / 1e6,
                                    total as f64 / 1e6
                                )).small().color(p.faint));
                                ui.add_space(6.0);
                                if theme::button_sized(ui, Role::Quiet, true, "Cancel", egui::vec2(0.0, 28.0)).clicked() {
                                    action = Some("cancel");
                                }
                            }
                            Card::Blocked { title, words, settings } => {
                                head(ui, Icon::Warn, p.warn, title);
                                ui.label(egui::RichText::new(words.as_str()).small().color(p.muted));
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    if settings.is_some()
                                        && theme::button_sized(ui, Role::Normal, true, "Open those settings", egui::vec2(0.0, 28.0)).clicked()
                                    {
                                        action = Some("settings");
                                    }
                                    if theme::button_sized(ui, Role::Quiet, true, "Close", egui::vec2(0.0, 28.0)).clicked() {
                                        action = Some("close");
                                    }
                                });
                            }
                            Card::Said { title, words } => {
                                head(ui, Icon::Warn, p.warn, title);
                                ui.label(egui::RichText::new(words.as_str()).small().color(p.muted));
                                ui.add_space(6.0);
                                if theme::button_sized(ui, Role::Quiet, true, "Close", egui::vec2(0.0, 28.0)).clicked() {
                                    action = Some("close");
                                }
                            }
                        }
                    });
            });
        match action {
            Some("download") => {
                if let Some(Card::Download { model, purpose }) = self.talk.card.take() {
                    self.download_speech_model(model, purpose, ctx);
                }
            }
            Some("cancel") => {
                if let Some(job) = self.talk.download.take() {
                    job.cancel();
                }
                self.talk.card = None;
            }
            Some("settings") => {
                if let Some(Card::Blocked {
                    settings: Some(url),
                    ..
                }) = &self.talk.card
                {
                    let _ = webbrowser::open(url);
                }
            }
            Some("close") => self.talk.card = None,
            _ => {}
        }
    }

    // ---- Settings › Voice ----------------------------------------------------------------------------------------

    /// The Voice page of the Settings.
    pub(super) fn show_voice_page(&mut self, ui: &mut egui::Ui) {
        use super::settings_window::{group, page_head, row_line, setting_row, switch_row};
        let p = theme::palette();
        let ctx = ui.ctx().clone();
        page_head(
            ui,
            "Voice",
            Some("Experimental. Talk to QuickAI and hear its answers. What you say is written down on this computer, and the answers are read by its own voice."),
        );
        let mut changed = false;
        // the switch for all of it: off, QuickAI shows no microphone, no Read aloud and no hands-free, and never opens
        // the microphone (nor lists them, nor starts the speech program)
        group(ui, |ui| {
            let mut on = self.talk.settings.enabled;
            if switch_row(
                ui,
                "Use voice (experimental)",
                Some(
                    "The microphone in the chat's box, Read aloud under answers, and hands-free. Off, QuickAI shows none of \
                     them and never opens the microphone.",
                ),
                None,
                &mut on,
                true,
            ) {
                self.talk.settings.enabled = on;
                if !on {
                    self.voice_turned_off(&ctx);
                }
                changed = true;
            }
        });
        if !self.talk.settings.enabled {
            if changed {
                self.voice_changed();
            }
            return;
        }
        ui.add_space(14.0);
        ui.label(theme::muted("TALKING").small());
        ui.add_space(4.0);
        group(ui, |ui| {
            if !CAN_HEAR {
                ui.label(theme::muted("This build of QuickAI has no microphone (it was built without speech to text)."));
                return;
            }
            #[cfg(feature = "voice")]
            {
                if self.talk.mics.is_none() {
                    self.talk.mics = Some(capture::microphones());
                }
                let mics = self.talk.mics.clone().unwrap_or_default();
                let current = self.talk.settings.microphone.clone();
                let shown = match &current {
                    None => "The system's".to_string(),
                    Some(id) => mics
                        .iter()
                        .find(|m| &m.id == id)
                        .map_or(id.clone(), |m| m.name.clone()),
                };
                setting_row(ui, "Microphone", None, None, |ui| {
                    egui::ComboBox::from_id_salt("voice_mic")
                        .selected_text(shown)
                        .width(220.0)
                        .truncate()
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(current.is_none(), "The system's")
                                .clicked()
                            {
                                self.talk.settings.microphone = None;
                                changed = true;
                            }
                            for m in &mics {
                                if ui
                                    .selectable_label(
                                        current.as_deref() == Some(m.id.as_str()),
                                        &m.name,
                                    )
                                    .on_hover_text(&m.id)
                                    .clicked()
                                {
                                    self.talk.settings.microphone = Some(m.id.clone());
                                    changed = true;
                                }
                            }
                        });
                });
            }
            row_line(ui);
            let language = self.talk.settings.language.clone();
            let shown = language
                .as_deref()
                .map_or("Tell it from what is said", language_name)
                .to_string();
            setting_row(
                ui,
                "Language",
                Some("What you speak in. Told from what is said, it can mistake a short sentence."),
                None,
                |ui| {
                    egui::ComboBox::from_id_salt("voice_language")
                        .selected_text(shown)
                        .width(220.0)
                        .height(320.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(language.is_none(), "Tell it from what is said")
                                .clicked()
                            {
                                self.talk.settings.language = None;
                                changed = true;
                            }
                            for (code, name) in LANGUAGES {
                                if ui
                                    .selectable_label(language.as_deref() == Some(code), name)
                                    .clicked()
                                {
                                    self.talk.settings.language = Some(code.to_string());
                                    changed = true;
                                }
                            }
                        });
                },
            );
            row_line(ui);
            let chosen = self.speech_model_now();
            let here = |m: &SpeechModel| {
                m.found_in(&paths::bundled_speech_dir(), &paths::user_speech_dir())
                    .is_some()
            };
            setting_row(ui, "Speech model", Some(chosen.about), None, |ui| {
                egui::ComboBox::from_id_salt("voice_model")
                    .selected_text(format!("{} · {}", chosen.name, chosen.size_words()))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for m in &SPEECH_MODELS {
                            let note = if here(m) {
                                "on this computer".to_string()
                            } else {
                                format!("{} to download", m.size_words())
                            };
                            if ui
                                .selectable_label(m.id == chosen.id, format!("{} · {note}", m.name))
                                .on_hover_text(m.about)
                                .clicked()
                            {
                                self.talk.settings.model = m.id.to_string();
                                changed = true;
                            }
                        }
                    });
            });
            if !here(chosen) {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::muted(format!("{} is not on this computer yet.", chosen.name))
                            .small(),
                    );
                    let downloading = self.talk.download.is_some();
                    if theme::button_sized(
                        ui,
                        Role::Normal,
                        !downloading,
                        if downloading {
                            "Downloading…"
                        } else {
                            "Download"
                        },
                        egui::vec2(0.0, 24.0),
                    )
                    .clicked()
                    {
                        self.download_speech_model(chosen, None, &ctx);
                    }
                });
            }
            row_line(ui);
            let mut send = self.talk.settings.send_when_stopped;
            if switch_row(
                ui,
                "Send when I stop talking",
                Some("A pause ends what you say, and it is sent; off, it goes in the box for you to check."),
                None,
                &mut send,
                true,
            ) {
                self.talk.settings.send_when_stopped = send;
                changed = true;
            }
        });
        ui.add_space(14.0);
        ui.label(theme::muted("ANSWERS").small());
        ui.add_space(4.0);
        let available = self.speaker(&ctx).available();
        if self.talk.voices.is_none() && self.talk.voices_asked.is_none() {
            self.talk.voices_asked = Some(self.speaker(&ctx).voices());
        }
        group(ui, |ui| {
            let mut read = self.talk.settings.read_answers;
            if switch_row(
                ui,
                "Read answers aloud",
                Some("Every answer, as it comes. Read aloud under one reads only that."),
                None,
                &mut read,
                true,
            ) {
                self.talk.settings.read_answers = read;
                self.talk.auto_turn = None;
                changed = true;
            }
            row_line(ui);
            let voices = match &self.talk.voices {
                Some(Ok(v)) => v.clone(),
                _ => Vec::new(),
            };
            let voice = self.talk.settings.voice.clone();
            let shown = voice.clone().unwrap_or_else(|| "The usual one".into());
            setting_row(
                ui,
                "Voice",
                Some("The voices this computer has."),
                None,
                |ui| {
                    egui::ComboBox::from_id_salt("voice_voice")
                        .selected_text(shown)
                        .width(220.0)
                        .height(320.0)
                        .truncate()
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(voice.is_none(), "The usual one")
                                .clicked()
                            {
                                self.talk.settings.voice = None;
                                changed = true;
                            }
                            for v in &voices {
                                let words = if v.about.is_empty() {
                                    format!("{} ({})", v.id, v.language)
                                } else {
                                    format!("{} · {}", v.about, v.language)
                                };
                                if ui
                                    .selectable_label(
                                        voice.as_deref() == Some(v.id.as_str()),
                                        words,
                                    )
                                    .clicked()
                                {
                                    self.talk.settings.voice = Some(v.id.clone());
                                    changed = true;
                                }
                            }
                        });
                },
            );
            row_line(ui);
            let mut speed = self.talk.settings.speed as f32;
            setting_row(ui, "Speed", None, None, |ui| {
                let slider = ui.add(
                    egui::Slider::new(&mut speed, 0.5..=2.0)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("×{v:.2}")),
                );
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    changed = true;
                }
            });
            self.talk.settings.speed = f64::from(speed);
            row_line(ui);
            ui.horizontal(|ui| {
                if theme::button_sized(
                    ui,
                    Role::Normal,
                    available.is_ok(),
                    "Hear it",
                    egui::vec2(0.0, 26.0),
                )
                .clicked()
                {
                    let (voice, speed) =
                        (self.talk.settings.voice.clone(), self.talk.settings.speed);
                    self.talk.next += 1;
                    let id = self.talk.next;
                    self.speaker(&ctx).say(
                        id,
                        vec!["Hello. This is how QuickAI reads its answers.".into()],
                        false,
                        voice,
                        speed,
                    );
                }
                ui.label(theme::muted("“Hello. This is how QuickAI reads its answers.”").small());
            });
            if let Err(why) = &available {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("It cannot read aloud here: {why}."))
                        .small()
                        .color(p.warn),
                );
            }
        });
        ui.add_space(10.0);
        ui.label(
            theme::muted(
                "Nothing you say leaves this computer: QuickAI writes it down itself, and the answers are read by its own \
                 voice. Shortcuts: hold Ctrl+Space to talk, Ctrl+Shift+Space for hands-free, Esc to stop.",
            )
            .small(),
        );
        if changed {
            self.voice_changed();
        }
    }
}

/// The card of a microphone that does not work, in the system's words, with the way to its switch where there is one.
#[cfg(feature = "voice")]
fn blocked_card(e: &MicError) -> Card {
    let detail = e.detail().to_string();
    match e {
        MicError::Blocked(_) if cfg!(windows) => Card::Blocked {
            title: "QuickAI can't hear the microphone".into(),
            words: "Windows keeps it from apps. Turn on \"Let desktop apps access your microphone\" in Settings › Privacy & \
                    security › Microphone, then try again."
                .into(),
            settings: Some("ms-settings:privacy-microphone"),
        },
        MicError::Blocked(_) if cfg!(target_os = "macos") => Card::Blocked {
            title: "QuickAI can't hear the microphone".into(),
            words: "macOS keeps it from QuickAI. Turn QuickAI on in System Settings › Privacy & Security › Microphone, then \
                    start QuickAI again."
                .into(),
            settings: Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"),
        },
        MicError::Blocked(_) => Card::Blocked {
            title: "QuickAI can't hear the microphone".into(),
            words: format!("The system does not let QuickAI open it ({detail}). Check your sound settings, and that QuickAI may use the microphone."),
            settings: None,
        },
        MicError::NoMicrophone(_) => Card::Blocked {
            title: "No microphone found".into(),
            words: format!("{detail}. Plug one in, or choose another in Settings › Voice."),
            settings: None,
        },
        MicError::NoSoundSystem(_) => Card::Blocked {
            title: "No sound system".into(),
            words: format!("{detail}. On Linux, QuickAI hears through ALSA (the package libasound2), which PipeWire and PulseAudio serve."),
            settings: None,
        },
        MicError::Failed(_) => Card::Blocked {
            title: "The microphone did not start".into(),
            words: format!("{detail}. Another program may be using it, or choose another in Settings › Voice."),
            settings: None,
        },
    }
}
