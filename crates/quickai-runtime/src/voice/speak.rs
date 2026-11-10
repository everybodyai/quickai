//! Reading aloud, a sentence at a time: on a thread of its own, which says when it begins each sentence (so the window
//! can light it) and when it has read them all. More sentences can be given while it reads (an answer that is still
//! being written), and a new reading or Stop ends the one going on at once.
//!
//! QuickAI reads with a voice the person chose to have, never with a robotic one of the system's: today the program of
//! `QUICKAI_SPEECH_COMMAND`, a shell line that reads its input aloud (tests write it to a file). Without one there is
//! nothing to read with, and Read aloud is not offered.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

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

/// A voice the program can read in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceInfo {
    /// What to name it by.
    pub id: String,
    pub language: String,
    pub about: String,
}

/// What reads aloud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Backend {
    /// A shell line that reads its input aloud, given the voice and the speed as `READALOUD_VOICE` and
    /// `READALOUD_SPEED`.
    Command(String),
}

enum Order {
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
    Stop,
}

/// The voice's thread; it ends when this is dropped.
pub struct Speaker {
    orders: Sender<Order>,
    spoken: Receiver<Spoken>,
    backend: Option<Backend>,
}

impl Speaker {
    /// `wake` is called when it began a sentence or ended, for the window to look.
    pub fn new(wake: impl Fn() + Send + 'static) -> Speaker {
        let backend = detect();
        let (orders, inbox) = std::sync::mpsc::channel();
        let (out, spoken) = std::sync::mpsc::channel();
        let worker_backend = backend.clone();
        std::thread::Builder::new()
            .name("read aloud".into())
            .spawn(move || Worker::new(worker_backend, inbox, out, Box::new(wake)).run())
            .expect("a thread for reading aloud");
        Speaker {
            orders,
            spoken,
            backend,
        }
    }

    /// Whether it can read aloud here, or why not.
    pub fn available(&self) -> Result<(), String> {
        match self.backend {
            Some(_) => Ok(()),
            None => Err(no_program()),
        }
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
        let _ = self.orders.send(Order::Say {
            utterance,
            sentences,
            more,
            voice,
            speed,
        });
    }

    pub fn more(&self, utterance: u64, sentences: Vec<String>) {
        if !sentences.is_empty() {
            let _ = self.orders.send(Order::More {
                utterance,
                sentences,
            });
        }
    }

    pub fn no_more(&self, utterance: u64) {
        let _ = self.orders.send(Order::NoMore { utterance });
    }

    pub fn stop(&self) {
        let _ = self.orders.send(Order::Stop);
    }

    /// What it did since the last time.
    pub fn take(&self) -> Vec<Spoken> {
        self.spoken.try_iter().collect()
    }

    /// The voices of the program, asked on a thread of its own: the answer comes on the receiver.
    pub fn voices(&self) -> Receiver<Result<Vec<VoiceInfo>, String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let backend = self.backend.clone();
        std::thread::spawn(move || {
            let _ = tx.send(match backend {
                Some(b) => b.voices(),
                None => Err(no_program()),
            });
        });
        rx
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        let _ = self.orders.send(Order::Stop);
    }
}

fn no_program() -> String {
    "QuickAI has no voice to read with yet".into()
}

/// What this computer reads with: the program of `QUICKAI_SPEECH_COMMAND` when it is set, else nothing.
pub fn detect() -> Option<Backend> {
    let line = std::env::var("QUICKAI_SPEECH_COMMAND").ok()?;
    (!line.trim().is_empty()).then_some(Backend::Command(line))
}

impl Backend {
    fn name(&self) -> &str {
        match self {
            Backend::Command(_) => "the speech program",
        }
    }

    /// The command that reads one sentence on its input aloud.
    pub fn command(&self, voice: Option<&str>, speed: f64) -> Command {
        match self {
            Backend::Command(line) => {
                let mut c = shell(line);
                c.env("READALOUD_VOICE", voice.unwrap_or(""));
                c.env("READALOUD_SPEED", format!("{speed}"));
                c
            }
        }
    }

    fn voices(&self) -> Result<Vec<VoiceInfo>, String> {
        match self {
            Backend::Command(_) => Ok(Vec::new()),
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
    inbox: Receiver<Order>,
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
        inbox: Receiver<Order>,
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
            let order = if self.queue.is_empty() {
                match self.inbox.recv_timeout(Duration::from_millis(500)) {
                    Ok(o) => Some(o),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                self.inbox.try_recv().ok()
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
                Ok(order @ (Order::Say { .. } | Order::Stop)) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Cut(order);
                }
                Ok(other) => self.obey(other),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Whole;
                }
            }
        }
    }
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
            backend: None,
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
