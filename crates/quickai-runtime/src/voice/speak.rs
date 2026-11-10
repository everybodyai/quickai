//! Reading aloud, a sentence at a time: on a thread of its own, which says when it begins each sentence (so the window
//! can light it) and when it has read them all. More sentences can be given while it reads (an answer that is still
//! being written), and a new reading or Stop ends the one going on at once.
//!
//! QuickAI reads with a voice the person chose to have, never with a robotic one of the system's, and ships none
//! (`voices`): a pack downloaded or a folder of their own, read by the program quickai-voice (`engine`); a speech
//! server, whose sound quickai-voice plays; or a program of their own, a shell line that reads its input aloud (also
//! `QUICKAI_SPEECH_COMMAND`, which tests use). Without one there is nothing to read with, and Read aloud is not offered.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use super::settings::{Reader, VoiceSettings};
use super::voices::{self, ModelSpec};

/// What the voice did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spoken {
    /// It began sentence `index` of `utterance`.
    Began {
        utterance: u64,
        index: usize,
    },
    /// It read all of `utterance`, or was stopped.
    Ended {
        utterance: u64,
    },
    Failed {
        utterance: u64,
        why: String,
    },
}

/// What reads aloud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Backend {
    /// quickai-voice, reading with the voice it loads: speaker `speaker` of `spec`.
    Voice {
        program: PathBuf,
        spec: ModelSpec,
        speaker: usize,
    },
    /// A speech server (OpenAI's `/v1/audio/speech`), whose sound quickai-voice plays.
    Server {
        program: PathBuf,
        url: String,
        model: String,
        key: String,
        voice: String,
    },
    /// A shell line that reads its input aloud, given the voice and the speed as `READALOUD_VOICE` and
    /// `READALOUD_SPEED`.
    Command(String),
    /// quickai-voice with no voice loaded, to play sound it is given (a recording played back, `Speaker::play`).
    Player { program: PathBuf },
}

pub(crate) enum Order {
    Say {
        utterance: u64,
        sentences: Vec<String>,
        more: bool,
        voice: Option<String>,
        speed: f64,
    },
    More {
        utterance: u64,
        sentences: Vec<String>,
    },
    NoMore {
        utterance: u64,
    },
    /// Plays a WAV file's bytes as `utterance` (one sentence, index 0).
    Play {
        utterance: u64,
        sound: Vec<u8>,
    },
    Stop,
}

/// What a reading thread is told: the speaker's orders, and for quickai-voice what it and a speech server said.
pub(crate) enum Input {
    Order(Order),
    /// A line quickai-voice wrote.
    Event(serde_json::Value),
    /// The sound a speech server made for sentence `id` of round `round`.
    Fetched {
        round: u64,
        id: u64,
        sound: Result<Vec<u8>, String>,
    },
    /// quickai-voice ended.
    Gone,
    /// The speaker is gone.
    Quit,
}

/// The voice's thread; it ends when this is dropped.
pub struct Speaker {
    orders: Sender<Input>,
    spoken: Receiver<Spoken>,
    ready: Result<(), String>,
}

impl Speaker {
    /// Reads with `backend` (or says why it cannot); `wake` is called when it began a sentence or ended, for the
    /// window to look.
    pub fn new(backend: Result<Backend, String>, wake: impl Fn() + Send + 'static) -> Speaker {
        let (orders, inbox) = std::sync::mpsc::channel();
        let (out, spoken) = std::sync::mpsc::channel();
        let ready = backend.as_ref().map(|_| ()).map_err(Clone::clone);
        let feed = orders.clone();
        let wake: Box<dyn Fn() + Send> = Box::new(wake);
        std::thread::Builder::new()
            .name("read aloud".into())
            .spawn(move || match backend {
                Ok(
                    b @ (Backend::Voice { .. } | Backend::Server { .. } | Backend::Player { .. }),
                ) => super::engine::run(b, inbox, feed, out, wake),
                Ok(b) => Worker::new(Some(b), inbox, out, wake).run(),
                Err(_) => Worker::new(None, inbox, out, wake).run(),
            })
            .expect("a thread for reading aloud");
        Speaker {
            orders,
            spoken,
            ready,
        }
    }

    /// Whether it can read aloud here, or why not.
    pub fn available(&self) -> Result<(), String> {
        self.ready.clone()
    }

    /// Reads `sentences` as `utterance`, ending what it was reading; `more` says more will come (`more`, `no_more`).
    pub fn say(
        &self,
        utterance: u64,
        sentences: Vec<String>,
        more: bool,
        voice: Option<String>,
        speed: f64,
    ) {
        let _ = self.orders.send(Input::Order(Order::Say {
            utterance,
            sentences,
            more,
            voice,
            speed,
        }));
    }

    pub fn more(&self, utterance: u64, sentences: Vec<String>) {
        if !sentences.is_empty() {
            let _ = self.orders.send(Input::Order(Order::More {
                utterance,
                sentences,
            }));
        }
    }

    pub fn no_more(&self, utterance: u64) {
        let _ = self.orders.send(Input::Order(Order::NoMore { utterance }));
    }

    pub fn stop(&self) {
        let _ = self.orders.send(Input::Order(Order::Stop));
    }

    /// Plays `sound`, the bytes of a WAV file, as `utterance`, ending what it was reading (quickai-voice only).
    pub fn play(&self, utterance: u64, sound: Vec<u8>) {
        let _ = self
            .orders
            .send(Input::Order(Order::Play { utterance, sound }));
    }

    /// What it did since the last time.
    pub fn take(&self) -> Vec<Spoken> {
        self.spoken.try_iter().collect()
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        let _ = self.orders.send(Input::Order(Order::Stop));
        let _ = self.orders.send(Input::Quit);
    }
}

fn no_program() -> String {
    "QuickAI has no voice to read with yet".into()
}

/// Kokoro's voice read when none was chosen: af_heart.
const FIRST_SPEAKER: usize = 3;

/// What reads aloud with the person's choices, or why nothing can: `QUICKAI_SPEECH_COMMAND` when it is set (tests),
/// else the reader of the settings.
pub fn backend_for(settings: &VoiceSettings) -> Result<Backend, String> {
    if let Ok(line) = std::env::var("QUICKAI_SPEECH_COMMAND") {
        if !line.trim().is_empty() {
            return Ok(Backend::Command(line));
        }
    }
    let program = || {
        voices::program().ok_or_else(|| "the voice program is not on this computer yet".to_string())
    };
    match &settings.reader {
        Reader::None => Err(no_program()),
        Reader::Pack(id) => {
            let pack = voices::voice_pack(id).ok_or("that voice is no longer offered")?;
            if !pack.installed() {
                return Err(format!("{} is not on this computer yet", pack.name));
            }
            let speaker = settings
                .voice
                .as_deref()
                .and_then(|v| pack.speaker(v))
                .unwrap_or(FIRST_SPEAKER.min(pack.speakers.len().saturating_sub(1)));
            Ok(Backend::Voice {
                program: program()?,
                spec: pack.spec(),
                speaker,
            })
        }
        Reader::Folder(path) => {
            let spec = voices::describe_folder(Path::new(path))?;
            let speaker = settings
                .voice
                .as_deref()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            Ok(Backend::Voice {
                program: program()?,
                spec,
                speaker,
            })
        }
        Reader::Server { url, model, key } => {
            if url.trim().is_empty() {
                return Err("the speech server has no address yet".into());
            }
            Ok(Backend::Server {
                program: program()?,
                url: url.trim().to_string(),
                model: model.clone(),
                key: key.clone(),
                voice: settings.voice.clone().unwrap_or_default(),
            })
        }
        Reader::Program(line) => Ok(Backend::Command(line.clone())),
    }
}

impl Backend {
    fn name(&self) -> &str {
        match self {
            Backend::Voice { .. } | Backend::Server { .. } | Backend::Player { .. } => {
                "quickai-voice"
            }
            Backend::Command(_) => "the speech program",
        }
    }

    /// The command that reads one sentence on its input aloud (a program of the person's own).
    pub fn command(&self, voice: Option<&str>, speed: f64) -> Command {
        match self {
            Backend::Command(line) => {
                let mut c = shell(line);
                c.env("READALOUD_VOICE", voice.unwrap_or(""));
                c.env("READALOUD_SPEED", format!("{speed}"));
                c
            }
            // quickai-voice is kept running (`engine`), never run a sentence at a time
            Backend::Voice { program, .. }
            | Backend::Server { program, .. }
            | Backend::Player { program } => Command::new(program),
        }
    }
}

#[cfg(windows)]
fn hide_window(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_: &mut Command) {}

fn shell(line: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", line]);
        hide_window(&mut c);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", line]);
        c
    }
}

/// How a sentence went.
enum Outcome {
    Whole,
    /// An order came that ends the reading (a new one, or Stop): it is handled next.
    Cut(Order),
    Failed(String),
}

struct Worker {
    backend: Option<Backend>,
    inbox: Receiver<Input>,
    out: Sender<Spoken>,
    wake: Box<dyn Fn() + Send>,
    utterance: Option<u64>,
    queue: VecDeque<(usize, String)>,
    next: usize,
    open: bool,
    voice: Option<String>,
    speed: f64,
}

impl Worker {
    fn new(
        backend: Option<Backend>,
        inbox: Receiver<Input>,
        out: Sender<Spoken>,
        wake: Box<dyn Fn() + Send>,
    ) -> Worker {
        Worker {
            backend,
            inbox,
            out,
            wake,
            utterance: None,
            queue: VecDeque::new(),
            next: 0,
            open: false,
            voice: None,
            speed: 1.0,
        }
    }

    fn tell(&self, spoken: Spoken) {
        let _ = self.out.send(spoken);
        (self.wake)();
    }

    fn run(mut self) {
        loop {
            // all read, and nothing more to come: it ended
            if self.queue.is_empty() && !self.open {
                if let Some(u) = self.utterance.take() {
                    self.tell(Spoken::Ended { utterance: u });
                }
            }
            let input = if self.queue.is_empty() {
                match self.inbox.recv_timeout(Duration::from_millis(500)) {
                    Ok(i) => Some(i),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                self.inbox.try_recv().ok()
            };
            let order = match input {
                Some(Input::Order(o)) => Some(o),
                Some(Input::Quit) => break,
                _ => None,
            };
            if let Some(order) = order {
                self.obey(order);
                continue;
            }
            let Some((index, sentence)) = self.queue.pop_front() else {
                continue;
            };
            let Some(utterance) = self.utterance else {
                continue;
            };
            self.tell(Spoken::Began { utterance, index });
            match self.read(&sentence) {
                Outcome::Whole => {}
                Outcome::Cut(order) => self.obey(order),
                Outcome::Failed(why) => {
                    self.queue.clear();
                    self.open = false;
                    self.utterance = None;
                    self.tell(Spoken::Failed { utterance, why });
                }
            }
        }
    }

    fn obey(&mut self, order: Order) {
        match order {
            Order::Say {
                utterance,
                sentences,
                more,
                voice,
                speed,
            } => {
                if let Some(u) = self.utterance.take() {
                    if u != utterance {
                        self.tell(Spoken::Ended { utterance: u });
                    }
                }
                self.utterance = Some(utterance);
                self.queue = sentences.into_iter().enumerate().collect();
                self.next = self.queue.len();
                self.open = more;
                self.voice = voice;
                self.speed = speed;
            }
            Order::More {
                utterance,
                sentences,
            } => {
                if self.utterance == Some(utterance) {
                    for s in sentences {
                        self.queue.push_back((self.next, s));
                        self.next += 1;
                    }
                }
            }
            Order::NoMore { utterance } => {
                if self.utterance == Some(utterance) {
                    self.open = false;
                }
            }
            Order::Play { utterance, .. } => {
                self.tell(Spoken::Failed {
                    utterance,
                    why: "only QuickAI's voice program plays a recording".into(),
                });
            }
            Order::Stop => {
                self.queue.clear();
                self.open = false;
                if let Some(u) = self.utterance.take() {
                    self.tell(Spoken::Ended { utterance: u });
                }
            }
        }
    }

    /// Reads one sentence, listening for orders that end it.
    fn read(&mut self, sentence: &str) -> Outcome {
        let Some(backend) = self.backend.clone() else {
            return Outcome::Failed(no_program());
        };
        let mut command = backend.command(self.voice.as_deref(), self.speed);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Outcome::Failed(format!("{} could not be started: {e}", backend.name()))
            }
        };
        if let Some(mut input) = child.stdin.take() {
            let text = sentence.to_string();
            std::thread::spawn(move || {
                let _ = input.write_all(text.as_bytes());
            });
        }
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Outcome::Whole,
                Ok(Some(status)) => {
                    let mut said = String::new();
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_string(&mut said);
                    }
                    let said = said.trim().lines().last().unwrap_or("").to_string();
                    return Outcome::Failed(if said.is_empty() {
                        format!("{} ended with {status}", backend.name())
                    } else {
                        format!("{}: {said}", backend.name())
                    });
                }
                Ok(None) => {}
                Err(e) => return Outcome::Failed(e.to_string()),
            }
            match self.inbox.recv_timeout(Duration::from_millis(20)) {
                Ok(Input::Order(order @ (Order::Say { .. } | Order::Stop))) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Cut(order);
                }
                Ok(Input::Order(other)) => self.obey(other),
                Ok(Input::Quit) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Cut(Order::Stop);
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Whole;
                }
            }
        }
    }
}

/// A WAV file of `samples` (-1 to 1, mono, at `rate`) in 16-bit PCM: a recording, for quickai-voice to play.
pub fn wav16(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A speaker whose program writes each sentence to a file.
    #[cfg(unix)]
    #[test]
    fn sentences_are_read_in_order_and_said_as_they_begin_and_a_new_reading_ends_the_last() {
        let file = std::env::temp_dir().join(format!("quickai-speak-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let (wake_tx, wake) = std::sync::mpsc::channel();
        let (orders, inbox) = std::sync::mpsc::channel();
        let (out, spoken) = std::sync::mpsc::channel();
        let backend = Backend::Command(format!(
            "cat >> {}; echo >> {}; sleep 0.1",
            file.display(),
            file.display()
        ));
        std::thread::spawn(move || {
            Worker::new(
                Some(backend),
                inbox,
                out,
                Box::new(move || {
                    let _ = wake_tx.send(());
                }),
            )
            .run()
        });
        let speaker = Speaker {
            orders,
            spoken,
            ready: Ok(()),
        };
        speaker.say(1, vec!["One.".into(), "Two.".into()], true, None, 1.0);
        speaker.more(1, vec!["Three.".into()]);
        speaker.no_more(1);
        let mut heard = Vec::new();
        while !heard.contains(&Spoken::Ended { utterance: 1 }) {
            wake.recv_timeout(Duration::from_secs(10))
                .expect("it reads");
            heard.extend(speaker.take());
        }
        assert_eq!(
            heard,
            [
                Spoken::Began {
                    utterance: 1,
                    index: 0
                },
                Spoken::Began {
                    utterance: 1,
                    index: 1
                },
                Spoken::Began {
                    utterance: 1,
                    index: 2
                },
                Spoken::Ended { utterance: 1 },
            ]
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "One.\nTwo.\nThree.\n"
        );
        // a long one, cut by another
        speaker.say(2, vec!["Long.".into(); 50], false, None, 1.0);
        std::thread::sleep(Duration::from_millis(250));
        speaker.say(3, vec!["Short.".into()], false, None, 1.0);
        let mut heard = Vec::new();
        while !heard.contains(&Spoken::Ended { utterance: 3 }) {
            wake.recv_timeout(Duration::from_secs(10))
                .expect("it reads");
            heard.extend(speaker.take());
        }
        assert!(heard.contains(&Spoken::Ended { utterance: 2 }), "{heard:?}");
        assert!(
            heard
                .iter()
                .filter(|s| matches!(s, Spoken::Began { utterance: 2, .. }))
                .count()
                < 50
        );
        let _ = std::fs::remove_file(&file);
    }
}
