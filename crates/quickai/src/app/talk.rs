//! Talking to QuickAI (C216): the microphone in the chat's box, what
//! was said written down by whisper.cpp and put in the box (or sent), answers read aloud by a voice the person installed,
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
    language_name, speech_model, suggestion, SpeechEngine, SpeechModel, SpeechSource,
    VoiceSettings, LANGUAGES, SPEECH_MODELS,
};
use crate::voice::settings::{Reader, MAX_PAUSE_MS, MIN_PAUSE_MS};
use crate::voice::speak::{self, Speaker, Spoken};
use crate::voice::spoken::{self, SentenceFeed};
use crate::voice::voices::{self, VoicePack, VOICE_PACKS, VOICE_PROGRAM};
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
    /// Settings › Voice's try: the recording is kept, to be played back and written down when asked.
    Test,
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
    /// The speech model being downloaded, then unpacked (an archive, and quickai-voice when it came too), and what it
    /// is for.
    downloading: Option<&'static SpeechModel>,
    speech_unpacking: Option<(Unpacked, Option<Purpose>)>,
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
    /// A voice (and quickai-voice with it) being downloaded, then unpacked; what came of it, said under the choice.
    voice_download: Option<(Job<String>, VoiceFetch)>,
    voice_unpacking: Option<Receiver<Result<(), String>>>,
    voice_note: Option<String>,
    /// The terms of the chosen voice's licence are shown (its warning mark was clicked).
    terms_open: bool,
    /// Settings › Voice's try of speech to text.
    trial: Trial,
    /// What is typed in the search box at the top of the lists of languages and of voices.
    language_search: String,
    voice_search: String,
    /// There is something to read aloud with (a voice the person installed); without it Read aloud is not offered and
    /// hands-free does not read.
    pub(super) can_read: bool,
}

/// Settings › Voice's try of speech to text: a recording, played back and written down when asked.
#[derive(Default)]
pub(super) struct Trial {
    samples: Vec<f32>,
    /// What the speech model wrote down of it, which model, and how long it took; or why it could not.
    result: Option<Result<(String, &'static str, Duration), String>>,
    /// It is being written down, since then, by that model.
    writing: Option<(Instant, &'static str)>,
    note: Option<String>,
    /// The recording being played back, on the system's own speaker (no voice is needed).
    #[cfg(feature = "voice")]
    playback: Option<capture::Playback>,
}

impl Trial {
    /// Its recording is being played back.
    fn playing(&self) -> bool {
        #[cfg(feature = "voice")]
        {
            self.playback.is_some()
        }
        #[cfg(not(feature = "voice"))]
        {
            false
        }
    }
}

/// What a voice's download brings: a pack, quickai-voice, or both.
pub(super) struct VoiceFetch {
    pack: Option<&'static VoicePack>,
    program: bool,
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
            can_read: speak::backend_for(&settings).is_ok(),
            settings,
            ..Talk::default()
        }
    }

    /// Hands-free does not read its answers: muted, or there is nothing to read with.
    fn quiet(&self) -> bool {
        self.settings.hands_free_muted || !self.can_read
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

/// What came of unpacking a download, on its thread.
type Unpacked = Receiver<Result<(), String>>;

/// `model` can write down here: its files are on this computer, and, for Omnilingual, the voice program that runs it
/// (downloaded with it). quickai-whisper, which the packages carry, is not asked for: its absence is said on its own.
fn speech_ready(model: &SpeechModel) -> bool {
    model
        .found_in(&paths::bundled_speech_dir(), &paths::user_speech_dir())
        .is_some()
        && (model.engine == SpeechEngine::Whisper || voices::program().is_some())
}

impl App {
    fn speech_model_now(&self) -> &'static SpeechModel {
        speech_model(&self.talk.settings.model).unwrap_or(&SPEECH_MODELS[0])
    }

    /// The chosen model's file, when it can write down here (`speech_ready`).
    fn speech_model_path(&self) -> Option<PathBuf> {
        let model = self.speech_model_now();
        if !speech_ready(model) {
            return None;
        }
        model.found_in(&paths::bundled_speech_dir(), &paths::user_speech_dir())
    }

    fn speaker(&mut self, ctx: &egui::Context) -> &Speaker {
        let ctx = ctx.clone();
        let settings = self.talk.settings.clone();
        self.talk.speaker.get_or_insert_with(|| {
            Speaker::new(speak::backend_for(&settings), move || ctx.request_repaint())
        })
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
        // what reads may have changed: the next reading starts it again, with the choice as it is now
        self.stop_reading();
        self.talk.speaker = None;
        self.talk.can_read = speak::backend_for(&self.talk.settings).is_ok();
    }

    /// Downloads what reading with `pack` needs that is not here (quickai-voice, and the pack), or quickai-voice alone
    /// for a voice of the person's own; then unpacks it.
    fn download_voice(&mut self, pack: Option<&'static VoicePack>, ctx: &egui::Context) {
        if self.talk.voice_download.is_some() || self.talk.voice_unpacking.is_some() {
            return;
        }
        use quickai_core::lookup::present::{Download, Piece};
        let piece = |label: &str, url: String, archive: &str, bytes: u64, sha256: &str| Piece {
            kind: None,
            label: label.into(),
            url,
            store_as: archive.into(),
            name: archive.into(),
            size: Some(bytes),
            sha256: Some(sha256.into()),
            place: None,
        };
        let mut files = Vec::new();
        let program = voices::program().is_none();
        if program {
            let Some(asset) = VOICE_PROGRAM.here() else {
                self.talk.voice_note = Some(
                    "The voice program cannot be downloaded yet: its first release is still being made.".into(),
                );
                return;
            };
            files.push(piece(
                "voice program",
                VOICE_PROGRAM.url(&asset),
                asset.archive,
                asset.bytes,
                asset.sha256,
            ));
        }
        let pack = pack.filter(|p| !p.installed());
        if let Some(pack) = pack {
            // an archive, or files kept in a folder of the pack's name
            let place = match pack.source {
                voices::PackSource::HuggingFace { .. } => Some(pack.folder.to_string()),
                voices::PackSource::Archive { .. } => None,
            };
            for (url, name, bytes, sha256) in pack.downloads() {
                files.push(Piece {
                    place: place.clone(),
                    ..piece("voice", url, name, bytes, sha256)
                });
            }
            // a voice of Piper's form reads with espeak-ng's data, downloaded once
            if pack.piper && !voices::espeak_dir().join("phontab").is_file() {
                let (archive, bytes, sha256) = voices::ESPEAK_DATA;
                files.push(piece(
                    "espeak-ng data",
                    format!(
                        "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/{archive}"
                    ),
                    archive,
                    bytes,
                    sha256,
                ));
            }
        }
        if files.is_empty() {
            return;
        }
        let download = Download {
            files,
            bundle: None,
            source: "github".into(),
            mode: None,
        };
        let with = crate::downloader::FetchWith {
            strict_size: true,
            ..Default::default()
        };
        let job = crate::downloader::spawn_files_with(
            download,
            voices::voice_dir().join("downloads"),
            with,
            super::calm_wake(ctx.clone()),
        );
        self.talk.voice_note = None;
        self.talk.voice_download = Some((job, VoiceFetch { pack, program }));
    }

    /// The voice's download ended: what came is unpacked, on a thread of its own (a pack is a few hundred MB).
    fn poll_voice_download(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.talk.voice_unpacking {
            ctx.request_repaint_after(Duration::from_millis(250));
            if let Ok(done) = rx.try_recv() {
                self.talk.voice_unpacking = None;
                match done {
                    Ok(()) => {
                        self.talk.voice_note = None;
                        self.push_log("[quickai] The voice is on this computer.".to_string());
                    }
                    Err(why) => self.talk.voice_note = Some(why),
                }
                self.voice_changed();
            }
            return;
        }
        let Some((job, _)) = &mut self.talk.voice_download else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(250));
        let Some(done) = job.poll() else {
            return;
        };
        let Some((_, fetch)) = self.talk.voice_download.take() else {
            return;
        };
        match done {
            Ok(_) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let wake = ctx.clone();
                std::thread::spawn(move || {
                    let downloads = voices::voice_dir().join("downloads");
                    let mut done = Ok(());
                    if fetch.program {
                        if let Some(asset) = VOICE_PROGRAM.here() {
                            done = voices::install_program(&downloads.join(asset.archive));
                        }
                    }
                    if let (Ok(()), Some(pack)) = (&done, fetch.pack) {
                        done = voices::install_pack(pack, &downloads);
                    }
                    let _ = tx.send(done);
                    wake.request_repaint();
                });
                self.talk.voice_unpacking = Some(rx);
            }
            Err(why) => {
                self.talk.voice_note = Some(format!("The voice could not be downloaded: {why}"))
            }
        }
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
            // the program that writes down what was said with whisper (quickai-whisper), which the packages carry next to
            // the app; Omnilingual's (quickai-voice) is downloaded with its model. Settings › Voice › Try it records and
            // plays back without either: only writing it down needs them, and says so
            if purpose != Purpose::Test
                && self.speech_model_now().engine == SpeechEngine::Whisper
                && crate::voice::transcribe::program().is_none()
            {
                self.talk.card = Some(Card::Said {
                    title: "Speech to text is not here".into(),
                    words: crate::voice::transcribe::missing(),
                });
                if purpose == Purpose::HandsFree {
                    self.talk.hands_free = None;
                }
                return;
            }
            if purpose != Purpose::Test && self.speech_model_path().is_none() {
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
                        // the pause that ends what was said is the person's (Settings › Voice)
                        vad: Vad::new(self.talk.settings.pause_ms),
                        voiced: false,
                        levels: VecDeque::new(),
                    });
                    ctx.request_repaint();
                }
                Err(e) if purpose == Purpose::Test => {
                    self.talk.trial.note = Some(card_words(&blocked_card(&e)));
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
            if l.purpose == Purpose::Test {
                if keep {
                    self.talk.trial.samples = samples;
                    self.talk.trial.result = None;
                    self.talk.trial.note = None;
                }
                return;
            }
            if !keep {
                return;
            }
            let Some(model) = self.speech_model_path() else {
                return;
            };
            let engine = self.speech_model_now().engine;
            let language = self.talk.settings.language.clone();
            self.talk.next += 1;
            let id = self.talk.next;
            let wake = ctx.clone();
            let transcriber = self
                .talk
                .transcriber
                .get_or_insert_with(|| Transcriber::new(move || wake.request_repaint()));
            transcriber.write(id, engine, &model, samples, language);
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
        if let (Some(e), Purpose::Test) = (&error, purpose) {
            self.talk.listening = None;
            self.talk.trial.note = Some(card_words(&blocked_card(e)));
        } else if let Some(e) = error {
            self.talk.listening = None;
            self.talk.card = Some(blocked_card(&e));
            self.talk.hands_free = None;
        } else if silent && purpose == Purpose::Test {
            self.talk.listening = None;
            self.talk.trial.note = Some(card_words(&blocked_card(&MicError::Blocked(
                "only silence came from it".into(),
            ))));
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
            if purpose == Purpose::Test {
                let (took, model) = self
                    .talk
                    .trial
                    .writing
                    .take()
                    .map_or((Duration::ZERO, ""), |(since, model)| {
                        (since.elapsed(), model)
                    });
                self.talk.trial.result = Some(result.map(|text| (text, model, took)));
                continue;
            }
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
        // the voice is given each sentence as it is said (short forms in words, acronyms spelled, symbols left out); the
        // reading keeps them as written, for the chat to light
        let symbols = self.talk.settings.read_symbols;
        let voiced = sentences
            .iter()
            .map(|s| spoken::voiced(s, symbols))
            .collect();
        self.speaker(ctx).say(utterance, voiced, more, voice, speed);
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
        if !self.talk.settings.enabled || !self.talk.can_read {
            return;
        }
        let last = self.chat.turns.len().checked_sub(1);
        let last_reply = last.filter(|&i| !self.chat.turns[i].from_person);
        let replying = self.chat.replying();
        // the answer to follow, and whether it is hands-free's
        let wanted = match &self.talk.hands_free {
            Some(hf)
                if !self.talk.quiet() && matches!(hf.phase, Phase::Waiting | Phase::Answering) =>
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
        let symbols = self.talk.settings.read_symbols;
        if let Some(s) = &self.talk.speaker {
            s.more(
                utterance,
                fresh.iter().map(|f| spoken::voiced(f, symbols)).collect(),
            );
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
        let heard_all = self.talk.quiet()
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
        self.poll_voice_download(ctx);
        self.poll_trial(ctx);
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
        if self.talk.download.is_some() || self.talk.speech_unpacking.is_some() {
            return;
        }
        use quickai_core::lookup::present::{Download, Piece};
        // its files into its folder, or its archive into the speech models folder, to be unpacked
        let place = match model.source {
            SpeechSource::HuggingFace { .. } if !model.folder.is_empty() => {
                Some(model.folder.to_string())
            }
            _ => None,
        };
        let mut files: Vec<Piece> = model
            .downloads()
            .into_iter()
            .map(|(url, name, bytes, sha256)| Piece {
                kind: None,
                label: "speech model".into(),
                url,
                store_as: name.into(),
                name: name.into(),
                size: Some(bytes),
                sha256: Some(sha256.into()),
                place: place.clone(),
            })
            .collect();
        // Omnilingual is run by quickai-voice: it comes too when it is not here, and is unpacked once it has
        if model.engine == SpeechEngine::Omnilingual && voices::program().is_none() {
            let Some(asset) = VOICE_PROGRAM.here() else {
                self.talk.card = Some(Card::Said {
                    title: "The speech model cannot run here".into(),
                    words: "QuickAI's voice program, which runs it, is not made for this system."
                        .into(),
                });
                return;
            };
            files.push(Piece {
                kind: None,
                label: "voice program".into(),
                url: VOICE_PROGRAM.url(&asset),
                store_as: asset.archive.into(),
                name: asset.archive.into(),
                size: Some(asset.bytes),
                sha256: Some(asset.sha256.into()),
                place: None,
            });
        }
        let source = match model.source {
            SpeechSource::HuggingFace { repo, .. } => format!("huggingface:{repo}"),
            SpeechSource::Archive { .. } => "github".into(),
        };
        let download = Download {
            files,
            bundle: None,
            source,
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
        self.talk.downloading = Some(model);
        self.talk.card = Some(Card::Downloading { model, purpose });
    }

    fn poll_speech_download(&mut self, ctx: &egui::Context) {
        // unpacked: it is ready, or why not
        if let Some((rx, purpose)) = &self.talk.speech_unpacking {
            ctx.request_repaint_after(Duration::from_millis(250));
            let purpose = *purpose;
            if let Ok(done) = rx.try_recv() {
                self.talk.speech_unpacking = None;
                self.talk.downloading = None;
                self.speech_model_came(done, purpose, ctx);
            }
            return;
        }
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
        if let Err(why) = done {
            self.talk.downloading = None;
            self.speech_model_came(Err(why), purpose, ctx);
            return;
        }
        // what came is unpacked on a thread of its own (an archive is a few hundred MB): the model's archive, and the
        // voice program when it came with the model
        let model = self.talk.downloading;
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let root = paths::user_speech_dir();
            let mut done = Ok(());
            if let Some(archive) = VOICE_PROGRAM
                .here()
                .map(|asset| root.join(asset.archive))
                .filter(|archive| archive.is_file())
            {
                done = voices::install_program(&archive);
            }
            if let (Ok(()), Some(model)) = (&done, model) {
                if let SpeechSource::Archive { name, .. } = model.source {
                    done = model.install_archive(&root.join(name), &root);
                }
            }
            let _ = tx.send(done);
            wake.request_repaint();
        });
        self.talk.speech_unpacking = Some((rx, purpose));
    }

    /// The speech model's download (and unpacking) ended: it listens, if that was what it was for, or says why not.
    fn speech_model_came(
        &mut self,
        done: Result<(), String>,
        purpose: Option<Purpose>,
        ctx: &egui::Context,
    ) {
        match done {
            Ok(()) => {
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
        if !self.talk.settings.enabled || !self.talk.can_read {
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
                "Read this answer aloud."
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
        let muted = self.talk.quiet();
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
        let unpacking = self.talk.speech_unpacking.is_some();
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
                            Card::Downloading { .. } if unpacking => {
                                head(ui, Icon::Download, p.body, "Unpacking the speech model");
                                ui.add(egui::ProgressBar::new(1.0).desired_height(6.0).animate(true));
                                ui.label(egui::RichText::new("Downloaded; this takes a moment.").small().color(p.faint));
                            }
                            Card::Downloading { model, .. } => {
                                head(ui, Icon::Download, p.body, "Downloading the speech model");
                                let (done, total) = progress.unwrap_or((0, Some(model.bytes())));
                                let total = total.unwrap_or(model.bytes()).max(1);
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
                self.talk.downloading = None;
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

    /// The try's recording written down by the speech model chosen.
    fn write_down_trial(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "voice")]
        {
            if self.talk.trial.samples.is_empty() || self.talk.writing.is_some() {
                return;
            }
            let Some(path) = self.speech_model_path() else {
                self.talk.trial.note =
                    Some("The speech model is not on this computer yet: download it above.".into());
                return;
            };
            let model = self.speech_model_now().name;
            let engine = self.speech_model_now().engine;
            self.talk.next += 1;
            let id = self.talk.next;
            let wake = ctx.clone();
            let transcriber = self
                .talk
                .transcriber
                .get_or_insert_with(|| Transcriber::new(move || wake.request_repaint()));
            transcriber.write(
                id,
                engine,
                &path,
                self.talk.trial.samples.clone(),
                self.talk.settings.language.clone(),
            );
            self.talk.writing = Some((id, Purpose::Test));
            self.talk.trial.writing = Some((Instant::now(), model));
            self.talk.trial.result = None;
            self.talk.trial.note = None;
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = ctx;
        }
    }

    /// The try's recording played back on the system's own speaker: it needs no voice.
    fn play_trial(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "voice")]
        {
            if self.talk.trial.samples.is_empty() {
                return;
            }
            self.talk.trial.note = None;
            self.talk.trial.playback = Some(capture::play(self.talk.trial.samples.clone()));
            ctx.request_repaint();
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = ctx;
        }
    }

    /// The try's playing back ended, or could not be: it says why.
    fn poll_trial(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "voice")]
        {
            let Some(playback) = &self.talk.trial.playback else {
                return;
            };
            match playback.finished() {
                None => ctx.request_repaint_after(Duration::from_millis(100)),
                Some(done) => {
                    self.talk.trial.playback = None;
                    if let Err(why) = done {
                        self.talk.trial.note = Some(format!("It could not be played back: {why}"));
                    }
                }
            }
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = ctx;
        }
    }

    /// The try of speech to text on the Voice page: record, play it back, write it down.
    fn show_trial(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = theme::palette();
        #[cfg(feature = "voice")]
        let recording = self
            .talk
            .listening
            .as_ref()
            .filter(|l| l.purpose == Purpose::Test)
            .map(|l| (l.started.elapsed(), l.vad.level));
        #[cfg(not(feature = "voice"))]
        let recording: Option<(Duration, f32)> = None;
        let busy_elsewhere = {
            #[cfg(feature = "voice")]
            {
                self.talk
                    .listening
                    .as_ref()
                    .is_some_and(|l| l.purpose != Purpose::Test)
            }
            #[cfg(not(feature = "voice"))]
            {
                false
            }
        };
        let has = !self.talk.trial.samples.is_empty();
        let seconds = self.talk.trial.samples.len() as f32 / crate::voice::RATE as f32;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Try it").color(p.text)).on_hover_text(
                "Record a few words, play them back, and see what the speech model above writes down of them.",
            );
            ui.add_space(8.0);
            let size = egui::vec2(30.0, 28.0);
            if let Some((elapsed, _)) = recording {
                if icons::icon_button(ui, Icon::Stop, size, true, true)
                    .on_hover_text("Stop recording")
                    .clicked()
                {
                    self.stop_listening(true, ctx);
                }
                ui.label(theme::muted(format!("{:.1} s", elapsed.as_secs_f32())).small());
            } else if icons::icon_button(ui, Icon::Mic, size, !busy_elsewhere, false)
                .on_hover_text("Record what you say, until you stop it")
                .clicked()
            {
                self.talk.trial.note = None;
                self.start_listening(Purpose::Test, ctx);
            }
            if self.talk.trial.playing() {
                if icons::icon_button(ui, Icon::Stop, size, true, true)
                    .on_hover_text("Stop playing")
                    .clicked()
                {
                    #[cfg(feature = "voice")]
                    if let Some(playback) = &self.talk.trial.playback {
                        playback.stop();
                    }
                }
            } else if icons::icon_button(ui, Icon::Play, size, has && recording.is_none(), false)
                .on_hover_text("Play the recording")
                .clicked()
            {
                self.play_trial(ctx);
            }
            let can_write = has && recording.is_none() && self.talk.trial.writing.is_none();
            if icons::icon_button(ui, Icon::Pencil, size, can_write, false)
                .on_hover_text("Write it down with the speech model above")
                .clicked()
            {
                self.write_down_trial(ctx);
            }
            if has && recording.is_none() {
                ui.label(theme::muted(format!("{seconds:.1} s")).small());
            }
        });
        if let Some((_, level)) = recording {
            ui.add(egui::ProgressBar::new(level.clamp(0.0, 1.0)).desired_height(4.0));
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if let Some((since, model)) = self.talk.trial.writing {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0).color(p.muted));
                ui.label(
                    theme::muted(format!("{model} · {:.1} s", since.elapsed().as_secs_f32()))
                        .small(),
                );
            });
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        match &self.talk.trial.result {
            Some(Ok((text, model, took))) => {
                ui.add_space(4.0);
                if text.trim().is_empty() {
                    ui.label(theme::muted("Nothing heard.").small());
                } else {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("“{}”", text.trim())).color(p.text),
                        )
                        .wrap(),
                    );
                }
                ui.label(theme::muted(format!("{model} · {:.1} s", took.as_secs_f32())).small());
            }
            Some(Err(why)) => {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("It could not be written down: {why}"))
                        .small()
                        .color(p.warn),
                );
            }
            None => {}
        }
        if let Some(note) = &self.talk.trial.note {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(note).small().color(p.warn));
        }
    }

    /// A pack chosen: its download, or once here, which of its voices reads.
    fn show_pack(
        &mut self,
        ui: &mut egui::Ui,
        pack: &'static VoicePack,
        ctx: &egui::Context,
    ) -> bool {
        use super::settings_window::setting_row;
        let p = theme::palette();
        let mut changed = false;
        let here = pack.installed() && voices::program().is_some();
        if !here {
            let downloading = self
                .talk
                .voice_download
                .as_ref()
                .map(|(job, _)| (job.downloaded, job.total));
            if let Some((done, total)) = downloading {
                let total = total.unwrap_or(pack.bytes()).max(1);
                ui.label(
                    theme::muted(format!(
                        "Downloading {}: {} of {}",
                        pack.name,
                        megabytes(done),
                        megabytes(total)
                    ))
                    .small(),
                );
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0));
            } else if self.talk.voice_unpacking.is_some() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(p.muted));
                    ui.label(theme::muted(format!("Unpacking {}…", pack.name)).small());
                });
            } else {
                let what = if pack.installed() {
                    "The voice program is not on this computer yet.".to_string()
                } else {
                    format!(
                        "{} is not on this computer yet ({}, {} licence).",
                        pack.name,
                        megabytes(pack.bytes()),
                        pack.license
                    )
                };
                ui.label(theme::muted(what).small());
                if theme::button_sized(ui, Role::Primary, true, "Download", egui::vec2(96.0, 26.0))
                    .clicked()
                {
                    self.download_voice(Some(pack), ctx);
                }
            }
            return changed;
        }
        let voice = self
            .talk
            .settings
            .voice
            .clone()
            .filter(|v| pack.speaker(v).is_some());
        let shown = voices::speaker_label(
            voice
                .as_deref()
                .unwrap_or(pack.speakers[3.min(pack.speakers.len() - 1)]),
        );
        setting_row(ui, "Voice", Some("The voices of this pack."), None, |ui| {
            egui::ComboBox::from_id_salt("voice_voice")
                .selected_text(shown)
                .width(220.0)
                .height(320.0)
                .truncate()
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| {
                    let query = search_box(ui, &mut self.talk.voice_search, "Search voices");
                    for name in pack
                        .speakers
                        .iter()
                        .filter(|n| matches(&query, &[n, &voices::speaker_label(n)]))
                    {
                        if ui
                            .selectable_label(
                                voice.as_deref() == Some(*name),
                                voices::speaker_label(name),
                            )
                            .clicked()
                        {
                            self.talk.settings.voice = Some(name.to_string());
                            changed = true;
                            ui.close();
                        }
                    }
                });
        });
        changed
    }

    /// A voice of the person's own, or a speech server, is played by quickai-voice: its download when it is not here.
    fn show_program_needed(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        if voices::program().is_some() {
            return false;
        }
        let p = theme::palette();
        ui.add_space(4.0);
        if let Some((done, total)) = self
            .talk
            .voice_download
            .as_ref()
            .map(|(job, _)| (job.downloaded, job.total))
        {
            let total = total.unwrap_or(1).max(1);
            ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0));
        } else if self.talk.voice_unpacking.is_some() {
            ui.add(egui::Spinner::new().size(14.0).color(p.muted));
        } else {
            ui.horizontal(|ui| {
                ui.label(
                    theme::muted(
                        "It plays through QuickAI's voice program, which is not here yet.",
                    )
                    .small(),
                );
                if theme::button_sized(ui, Role::Normal, true, "Download it", egui::vec2(0.0, 24.0))
                    .clicked()
                {
                    self.download_voice(None, ctx);
                }
            });
        }
        false
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
            Some("Experimental. Talk to QuickAI and hear its answers. What you say is written down on this computer, and the answers are read aloud on it."),
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
            let language_about = if self.speech_model_now().engine == SpeechEngine::Omnilingual {
                "What you speak in, for the Whisper models: Omnilingual tells it from what is said."
            } else {
                "What you speak in. Told from what is said, it can mistake a short sentence."
            };
            setting_row(ui, "Language", Some(language_about), None, |ui| {
                egui::ComboBox::from_id_salt("voice_language")
                    .selected_text(shown)
                    .width(220.0)
                    .height(320.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show_ui(ui, |ui| {
                        let query =
                            search_box(ui, &mut self.talk.language_search, "Search languages");
                        if query.is_empty()
                            && ui
                                .selectable_label(language.is_none(), "Tell it from what is said")
                                .clicked()
                        {
                            self.talk.settings.language = None;
                            changed = true;
                            ui.close();
                        }
                        for (code, name) in LANGUAGES
                            .iter()
                            .filter(|(code, name)| matches(&query, &[code, name]))
                        {
                            if ui
                                .selectable_label(language.as_deref() == Some(*code), *name)
                                .clicked()
                            {
                                self.talk.settings.language = Some(code.to_string());
                                changed = true;
                                ui.close();
                            }
                        }
                    });
            });
            // a model more accurate for the language chosen than the one in use (SUGGESTED: Omnilingual for Urdu and
            // most languages of India, Omnilingual large for Arabic and Persian, Whisper small for Hindi)
            let better = self
                .talk
                .settings
                .language
                .as_deref()
                .and_then(|code| Some((code, suggestion(code, self.speech_model_now())?)));
            if let Some((code, better)) = better {
                ui.add_space(2.0);
                ui.label(
                    theme::muted(format!(
                        "For {}, {} ({}) is the most accurate speech model QuickAI offers.",
                        language_name(code),
                        better.name,
                        better.size_words()
                    ))
                    .small(),
                );
                ui.add_space(4.0);
                if theme::button_sized(
                    ui,
                    Role::Normal,
                    true,
                    format!("Use {}", better.name),
                    egui::vec2(0.0, 24.0),
                )
                .clicked()
                {
                    self.talk.settings.model = better.id.to_string();
                    changed = true;
                }
                ui.add_space(4.0);
            }
            row_line(ui);
            let chosen = self.speech_model_now();
            let here = |m: &SpeechModel| speech_ready(m);
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
                    let unpacking = self.talk.speech_unpacking.is_some();
                    if theme::button_sized(
                        ui,
                        Role::Normal,
                        !downloading && !unpacking,
                        if unpacking {
                            "Unpacking…"
                        } else if downloading {
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
            row_line(ui);
            let mut pause = self.talk.settings.pause_ms as f32 / 1000.0;
            setting_row(
                ui,
                "Pause",
                Some("How long you pause before what you said is sent: with Send when I stop talking, and in hands-free."),
                None,
                |ui| {
                    let slider = ui.add(
                        egui::Slider::new(
                            &mut pause,
                            MIN_PAUSE_MS as f32 / 1000.0..=MAX_PAUSE_MS as f32 / 1000.0,
                        )
                        .step_by(0.1)
                        .custom_formatter(|v, _| format!("{v:.1} s")),
                    );
                    if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                        changed = true;
                    }
                },
            );
            self.talk.settings.pause_ms = (pause * 1000.0).round() as u32;
            row_line(ui);
            self.show_trial(ui, &ctx);
        });
        ui.add_space(14.0);
        ui.label(theme::muted("ANSWERS").small());
        ui.add_space(4.0);
        group(ui, |ui| {
            // what reads aloud: nothing until the person chooses, for QuickAI ships no voice
            let reader = self.talk.settings.reader.clone();
            let mut chosen: Option<Reader> = None;
            setting_row(
                ui,
                "Read with",
                Some("No voice comes with QuickAI: download one, or use one of your own."),
                None,
                |ui| {
                    // a voice whose licence limits commercial use: a warning mark beside it, which shows the terms
                    let terms = match &reader {
                        Reader::Pack(id) => voices::voice_pack(id).and_then(|p| p.terms()),
                        _ => None,
                    };
                    if let Some(terms) = terms {
                        if warning_mark(ui, self.talk.terms_open)
                            .on_hover_text(format!(
                                "{}: click for the terms of its license",
                                terms.title
                            ))
                            .clicked()
                        {
                            self.talk.terms_open = !self.talk.terms_open;
                        }
                    }
                    egui::ComboBox::from_id_salt("voice_reader")
                        .selected_text(reader_label(&reader))
                        .width(220.0)
                        .truncate()
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(reader == Reader::None, "Nothing").clicked() {
                                chosen = Some(Reader::None);
                            }
                            for pack in &VOICE_PACKS {
                                let on = reader == Reader::Pack(pack.id.into());
                                let hover = match pack.terms() {
                                    Some(t) => format!("{}. {} licence: {}.", pack.about, pack.license, t.title.to_lowercase()),
                                    None => format!("{}. {} licence.", pack.about, pack.license),
                                };
                                let clicked = ui
                                    .horizontal(|ui| {
                                        let label = ui
                                            .selectable_label(on, format!("{} · {}", pack.name, megabytes(pack.bytes())))
                                            .on_hover_text(&hover);
                                        if pack.terms().is_some() {
                                            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                                            icons::paint(ui.painter(), r, Icon::Warn, theme::palette().warn);
                                        }
                                        label.clicked()
                                    })
                                    .inner;
                                if clicked {
                                    chosen = Some(Reader::Pack(pack.id.into()));
                                }
                            }
                            if ui
                                .selectable_label(matches!(reader, Reader::Folder(_)), "A voice from a folder…")
                                .on_hover_text("A voice of sherpa-onnx's you have: Kokoro, Kitten, Piper or another VITS voice, Matcha.")
                                .clicked()
                            {
                                chosen = Some(Reader::Folder(String::new()));
                            }
                            if ui
                                .selectable_label(matches!(reader, Reader::Server { .. }), "A speech server…")
                                .on_hover_text("A server that speaks OpenAI's /v1/audio/speech, for voices QuickAI cannot run itself.")
                                .clicked()
                            {
                                chosen = Some(Reader::Server { url: String::new(), model: String::new(), key: String::new() });
                            }
                            if ui
                                .selectable_label(matches!(reader, Reader::Program(_)), "A program of my own…")
                                .on_hover_text("A command that reads its input aloud.")
                                .clicked()
                            {
                                chosen = Some(Reader::Program(String::new()));
                            }
                        });
                },
            );
            if let Some(next) = chosen {
                // a different kind of reader keeps nothing of the last one's voice
                if std::mem::discriminant(&next) != std::mem::discriminant(&reader) {
                    self.talk.settings.voice = None;
                }
                if next != reader {
                    self.talk.settings.reader = next;
                    self.talk.terms_open = false;
                    changed = true;
                }
            }
            let reader = self.talk.settings.reader.clone();
            // the terms of the chosen voice's licence, under its warning mark
            if let Reader::Pack(id) = &reader {
                if let Some(terms) = voices::voice_pack(id).and_then(|p| p.terms()) {
                    if self.talk.terms_open && show_terms(ui, &terms) {
                        self.talk.terms_open = false;
                    }
                }
            }
            match &reader {
                Reader::None => {}
                Reader::Pack(id) => {
                    if let Some(pack) = voices::voice_pack(id) {
                        row_line(ui);
                        changed |= self.show_pack(ui, pack, &ctx);
                    }
                }
                Reader::Folder(path) => {
                    row_line(ui);
                    let mut text = path.clone();
                    setting_row(
                        ui,
                        "Folder",
                        Some("The folder of the voice, with its .onnx model and tokens.txt."),
                        None,
                        |ui| {
                            if ui.button("Choose…").clicked() {
                                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                    text = dir.to_string_lossy().into_owned();
                                }
                            }
                            ui.add(
                                egui::TextEdit::singleline(&mut text)
                                    .desired_width(220.0)
                                    .hint_text("/path/to/the/voice"),
                            );
                        },
                    );
                    if text != *path {
                        self.talk.settings.reader = Reader::Folder(text.clone());
                        changed = true;
                    }
                    if !text.trim().is_empty() {
                        match voices::describe_folder(std::path::Path::new(text.trim())) {
                            Ok(spec) => {
                                ui.label(
                                    theme::muted(format!("A {} voice.", kind_words(spec.kind)))
                                        .small(),
                                );
                            }
                            Err(why) => {
                                ui.label(egui::RichText::new(why).small().color(p.warn));
                            }
                        }
                    }
                    row_line(ui);
                    let mut number: u32 = self
                        .talk
                        .settings
                        .voice
                        .as_deref()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    setting_row(
                        ui,
                        "Speaker",
                        Some("The number of the voice in it, for a voice that has several."),
                        None,
                        |ui| {
                            if ui
                                .add(egui::DragValue::new(&mut number).range(0..=999))
                                .changed()
                            {
                                self.talk.settings.voice = Some(number.to_string());
                                changed = true;
                            }
                        },
                    );
                    changed |= self.show_program_needed(ui, &ctx);
                }
                Reader::Server { url, model, key } => {
                    let (mut url_text, mut model_text, mut key_text) =
                        (url.clone(), model.clone(), key.clone());
                    let mut voice_text = self.talk.settings.voice.clone().unwrap_or_default();
                    for (label, desc, text, hint, secret) in [
                        ("Address", "Where the server is; QuickAI asks …/v1/audio/speech there.", &mut url_text, "http://127.0.0.1:8880", false),
                        ("Model", "The model to ask it for, if it has several.", &mut model_text, "kokoro", false),
                        ("Voice", "The voice to ask it for.", &mut voice_text, "af_heart", false),
                        ("Key", "Its API key, if it asks for one. It is kept in QuickAI's settings on this computer.", &mut key_text, "", true),
                    ] {
                        row_line(ui);
                        setting_row(ui, label, Some(desc), None, |ui| {
                            ui.add(egui::TextEdit::singleline(text).desired_width(220.0).hint_text(hint).password(secret));
                        });
                    }
                    if url_text != *url || model_text != *model || key_text != *key {
                        self.talk.settings.reader = Reader::Server {
                            url: url_text,
                            model: model_text,
                            key: key_text,
                        };
                        changed = true;
                    }
                    if voice_text != self.talk.settings.voice.clone().unwrap_or_default() {
                        self.talk.settings.voice =
                            (!voice_text.trim().is_empty()).then(|| voice_text.trim().to_string());
                        changed = true;
                    }
                    ui.label(
                        theme::muted(
                            "Each sentence of the answers is sent to this address to be read.",
                        )
                        .small(),
                    );
                    changed |= self.show_program_needed(ui, &ctx);
                }
                Reader::Program(command) => {
                    row_line(ui);
                    let mut text = command.clone();
                    setting_row(
                        ui,
                        "Command",
                        Some("Reads its input aloud; it is given READALOUD_VOICE and READALOUD_SPEED."),
                        None,
                        |ui| {
                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0).hint_text("piper … | aplay"));
                        },
                    );
                    if text != *command {
                        self.talk.settings.reader = Reader::Program(text);
                        changed = true;
                    }
                }
            }
            if let Some(note) = &self.talk.voice_note {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(note).small().color(p.warn));
            }
            if reader == Reader::None {
                return;
            }
            row_line(ui);
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
            let mut symbols = self.talk.settings.read_symbols;
            if switch_row(
                ui,
                "Read symbols aloud",
                Some("Off, symbols such as # / → and emoji are left out of what is read, rather than named."),
                None,
                &mut symbols,
                true,
            ) {
                self.talk.settings.read_symbols = symbols;
                changed = true;
            }
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
            let why_not = speak::backend_for(&self.talk.settings).err();
            let pack = match &self.talk.settings.reader {
                Reader::Pack(id) => voices::voice_pack(id),
                _ => None,
            };
            let sample = voices::sample(pack, self.talk.settings.voice.as_deref());
            ui.horizontal(|ui| {
                if theme::button_sized(
                    ui,
                    Role::Normal,
                    why_not.is_none(),
                    "Hear it",
                    egui::vec2(0.0, 26.0),
                )
                .clicked()
                {
                    let (voice, speed) =
                        (self.talk.settings.voice.clone(), self.talk.settings.speed);
                    self.talk.next += 1;
                    let id = self.talk.next;
                    self.speaker(&ctx)
                        .say(id, vec![sample.into()], false, voice, speed);
                }
                ui.label(theme::muted(format!("“{sample}”")).small());
            });
            if let Some(why) = why_not {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("It cannot read aloud yet: {why}."))
                        .small()
                        .color(p.warn),
                );
            }
        });
        ui.add_space(10.0);
        ui.label(
            theme::muted(
                "QuickAI is designed to keep what you say on this computer: it writes your speech down and reads the \
                 answers aloud here, unless you chose a speech server elsewhere. Shortcuts: hold Ctrl+Space to talk, \
                 Ctrl+Shift+Space for hands-free, Esc to stop.",
            )
            .small(),
        );
        if changed {
            self.voice_changed();
        }
    }
}

/// A card's words on one line, for where a card is not shown (Settings › Voice's try).
#[cfg(feature = "voice")]
fn card_words(card: &Card) -> String {
    match card {
        Card::Blocked { title, words, .. } | Card::Said { title, words } => {
            format!("{title}: {words}")
        }
        _ => String::new(),
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

/// What reads aloud, in a few words, for the list's head.
fn reader_label(reader: &Reader) -> String {
    match reader {
        Reader::None => "Nothing".into(),
        Reader::Pack(id) => {
            voices::voice_pack(id).map_or_else(|| id.clone(), |p| p.name.to_string())
        }
        Reader::Folder(_) => "A voice from a folder".into(),
        Reader::Server { .. } => "A speech server".into(),
        Reader::Program(_) => "A program of my own".into(),
    }
}

/// The warning mark of a voice whose licence limits commercial use: a button, pressed while its terms are shown.
fn warning_mark(ui: &mut egui::Ui, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let p = theme::palette();
        let fill = if open || response.is_pointer_button_down_on() {
            p.raised_active
        } else if response.hovered() {
            p.raised_hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 5.0, fill);
        let square = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
        icons::paint(ui.painter(), square, Icon::Warn, p.warn);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The terms of a voice's licence, in a box: what it allows, a link to the licence itself, and that QuickAI does not
/// include the voice. True when the box was closed.
fn show_terms(ui: &mut egui::Ui, terms: &voices::Terms) -> bool {
    let p = theme::palette();
    let mut close = false;
    egui::Frame::new()
        .fill(p.raised)
        .stroke(egui::Stroke::new(1.0, p.warn))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                icons::paint(ui.painter(), r, Icon::Warn, p.warn);
                ui.label(egui::RichText::new(terms.title).family(theme::semibold(ui)).color(p.text));
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new(terms.summary).small().color(p.body));
            ui.add_space(4.0);
            ui.label(
                theme::muted(
                    "QuickAI does not include this voice: it downloads it at your request, and you are responsible \
                     for using it within its license.",
                )
                .small(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.hyperlink_to("Read the full license", terms.url);
                ui.add_space(12.0);
                if theme::button_sized(ui, Role::Quiet, true, "Close", egui::vec2(0.0, 24.0)).clicked() {
                    close = true;
                }
            });
        });
    close
}

fn megabytes(bytes: u64) -> String {
    format!("{} MB", (bytes + 500_000) / 1_000_000)
}

fn kind_words(kind: voices::ModelKind) -> &'static str {
    match kind {
        voices::ModelKind::Kokoro => "Kokoro",
        voices::ModelKind::Kitten => "Kitten",
        voices::ModelKind::Vits => "VITS (Piper)",
        voices::ModelKind::Matcha => "Matcha",
    }
}

/// The search box at the top of a long list: what is typed, made small, to match the list's words with.
fn search_box(ui: &mut egui::Ui, text: &mut String, hint: &str) -> String {
    let field = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    // the list opened: what is typed goes in the box, with nothing to click first
    if !field.has_focus() && ui.memory(|m| m.focused().is_none()) {
        field.request_focus();
    }
    ui.add_space(4.0);
    text.trim().to_lowercase()
}

/// Whether any of `words` has what was searched for in it (everything, when nothing was).
fn matches(query: &str, words: &[&str]) -> bool {
    query.is_empty() || words.iter().any(|w| w.to_lowercase().contains(query))
}
