//! The computer's own voice, a sentence at a time: on a thread of its own, which says when it begins each sentence (so
//! the window can light it) and when it has read them all. More sentences can be given while it reads (an answer that
//! is still being written), and a new reading or Stop ends the one going on at once.
//!
//! - macOS: `say`; Linux: `espeak-ng`, `espeak` or speech-dispatcher's `spd-say`, a run of the program a sentence,
//!   with the text on its input (never on a command line);
//! - Windows: System.Speech through one PowerShell kept running, a line a sentence (PowerShell takes a while to start);
//! - `QUICKAI_SPEECH_COMMAND`: a shell line that reads its input aloud (tests write it to a file).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
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

/// A speech program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Backend {
    Say,
    Windows,
    EspeakNg,
    Espeak,
    SpdSay,
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
    if cfg!(target_os = "linux") {
        "this computer has no speech program: install espeak-ng (sudo apt install espeak-ng, or sudo dnf install \
         espeak-ng) or speech-dispatcher, then try again"
            .into()
    } else {
        "this computer's speech program could not be found".into()
    }
}

/// The program this system reads with: `QUICKAI_SPEECH_COMMAND` when it is set, else the system's own.
pub fn detect() -> Option<Backend> {
    if let Ok(line) = std::env::var("QUICKAI_SPEECH_COMMAND") {
        return (!line.trim().is_empty()).then_some(Backend::Command(line));
    }
    if cfg!(target_os = "macos") {
        return Some(Backend::Say);
    }
    if cfg!(windows) {
        return Some(Backend::Windows);
    }
    [Backend::EspeakNg, Backend::Espeak, Backend::SpdSay]
        .into_iter()
        .find(|b| on_path(b.name()))
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(program).is_file()))
}

impl Backend {
    fn name(&self) -> &str {
        match self {
            Backend::Say => "say",
            Backend::Windows => "windows",
            Backend::EspeakNg => "espeak-ng",
            Backend::Espeak => "espeak",
            Backend::SpdSay => "spd-say",
            Backend::Command(_) => "command",
        }
    }

    /// The command that reads one sentence on its input aloud (not Windows, which keeps one running).
    pub fn command(&self, voice: Option<&str>, speed: f64) -> Command {
        match self {
            Backend::Say => {
                let mut c = Command::new("say");
                c.args(["-r", &format!("{}", (180.0 * speed).round())]);
                if let Some(v) = voice {
                    c.args(["-v", v]);
                }
                c
            }
            Backend::EspeakNg | Backend::Espeak => {
                let mut c = Command::new(self.name());
                c.args(["-s", &format!("{}", (175.0 * speed).round())]);
                if let Some(v) = voice {
                    c.args(["-v", v]);
                }
                c.arg("--stdin");
                c
            }
            Backend::SpdSay => {
                let mut c = Command::new("spd-say");
                let rate = ((speed - 1.0) * 100.0).round().clamp(-100.0, 100.0);
                c.args(["-e", "-w", "-r", &format!("{rate}")]);
                if let Some(v) = voice {
                    c.args(["-y", v]);
                }
                c
            }
            Backend::Windows => {
                let mut c = Command::new("powershell.exe");
                c.args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    &windows_script(voice, speed),
                ]);
                hide_window(&mut c);
                c
            }
            Backend::Command(line) => {
                let mut c = shell(line);
                c.env("READALOUD_VOICE", voice.unwrap_or(""));
                c.env("READALOUD_SPEED", format!("{speed}"));
                c
            }
        }
    }

    fn voices(&self) -> Result<Vec<VoiceInfo>, String> {
        let mut c = match self {
            Backend::Say => {
                let mut c = Command::new("say");
                c.args(["-v", "?"]);
                c
            }
            Backend::EspeakNg | Backend::Espeak => {
                let mut c = Command::new(self.name());
                c.arg("--voices");
                c
            }
            Backend::SpdSay => {
                let mut c = Command::new("spd-say");
                c.arg("-L");
                c
            }
            Backend::Windows => {
                let mut c = Command::new("powershell.exe");
                c.args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Add-Type -AssemblyName System.Speech; \
                     (New-Object System.Speech.Synthesis.SpeechSynthesizer).GetInstalledVoices() | \
                     Where-Object { $_.Enabled } | ForEach-Object { $_.VoiceInfo.Name + \"`t\" + $_.VoiceInfo.Culture.Name }",
                ]);
                hide_window(&mut c);
                c
            }
            Backend::Command(_) => return Ok(Vec::new()),
        };
        let out = c
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{} could not be asked: {e}", self.name()))?;
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(match self {
            Backend::Say => parse_say(&text),
            Backend::EspeakNg | Backend::Espeak => parse_espeak(&text),
            Backend::SpdSay => parse_spd(&text),
            _ => text
                .lines()
                .filter_map(|l| {
                    let (name, lang) = l.trim().split_once('\t')?;
                    Some(VoiceInfo {
                        id: name.into(),
                        language: lang.into(),
                        about: String::new(),
                    })
                })
                .collect(),
        })
    }
}

/// System.Speech kept running: a line on its input is a sentence, read; `done` on its output says it was.
fn windows_script(voice: Option<&str>, speed: f64) -> String {
    let rate = if speed >= 1.0 {
        (speed - 1.0) * 10.0
    } else {
        (speed - 1.0) * 20.0
    };
    let rate = rate.round().clamp(-10.0, 10.0);
    let select = voice
        .map(|v| {
            format!(
                "try {{ $s.SelectVoice('{}') }} catch {{}}; ",
                v.replace('\'', "''")
            )
        })
        .unwrap_or_default();
    format!(
        "[Console]::InputEncoding=[Text.Encoding]::UTF8; [Console]::OutputEncoding=[Text.Encoding]::UTF8; \
         Add-Type -AssemblyName System.Speech; $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.Rate={rate}; {select}\
         while (($l=[Console]::In.ReadLine()) -ne $null) {{ if ($l.Length -gt 0) {{ $s.Speak($l) }}; \
         [Console]::Out.WriteLine('done'); [Console]::Out.Flush() }}"
    )
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

/// `say -v ?`: `Alex                en_US    # Most people recognize me by my voice.`
fn parse_say(out: &str) -> Vec<VoiceInfo> {
    out.lines()
        .filter_map(|l| {
            let (left, about) = l.split_once('#').unwrap_or((l, ""));
            let mut words: Vec<&str> = left.split_whitespace().collect();
            let lang = words.pop()?;
            (!words.is_empty()).then(|| VoiceInfo {
                id: words.join(" "),
                language: lang.into(),
                about: about.trim().into(),
            })
        })
        .collect()
}

/// `espeak-ng --voices`: `Pty Language Age/Gender VoiceName File Other Languages`, a voice a line.
fn parse_espeak(out: &str) -> Vec<VoiceInfo> {
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            let (lang, name) = (cols.get(1)?, cols.get(3)?);
            Some(VoiceInfo {
                id: lang.to_string(),
                language: lang.to_string(),
                about: name.replace('_', " "),
            })
        })
        .collect()
}

/// `spd-say -L`: `NAME LANGUAGE VARIANT`, a voice a line.
fn parse_spd(out: &str) -> Vec<VoiceInfo> {
    out.lines()
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            let (name, lang) = (cols.first()?, cols.get(1)?);
            (*name != "NAME").then(|| VoiceInfo {
                id: name.to_string(),
                language: lang.to_string(),
                about: String::new(),
            })
        })
        .collect()
}

/// PowerShell kept running for Windows' speech, with the voice and the speed it was started with.
struct Kept {
    child: Child,
    input: ChildStdin,
    done: Receiver<()>,
    voice: Option<String>,
    speed: f64,
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
    kept: Option<Kept>,
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
            kept: None,
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
        if let Some(mut k) = self.kept.take() {
            let _ = k.child.kill();
            let _ = k.child.wait();
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
        if backend == Backend::Windows {
            return self.read_kept(sentence);
        }
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
                    // speech-dispatcher goes on with what it was given after its client ends: it is told to stop
                    if backend == Backend::SpdSay {
                        let _ = Command::new("spd-say")
                            .arg("-S")
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
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

    /// Windows: the sentence as a line to the PowerShell kept running (started, or started again for another voice
    /// or speed); stopping ends that PowerShell, and the next sentence starts another.
    fn read_kept(&mut self, sentence: &str) -> Outcome {
        let fits = self
            .kept
            .as_ref()
            .is_some_and(|k| k.voice == self.voice && (k.speed - self.speed).abs() < 1e-6);
        if !fits {
            if let Some(mut k) = self.kept.take() {
                let _ = k.child.kill();
                let _ = k.child.wait();
            }
            let mut command = Backend::Windows.command(self.voice.as_deref(), self.speed);
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = match command.spawn() {
                Ok(c) => c,
                Err(e) => {
                    return Outcome::Failed(format!("Windows' speech could not be started: {e}"))
                }
            };
            let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
                let _ = child.kill();
                return Outcome::Failed("Windows' speech could not be started".into());
            };
            let (tx, done) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(output).lines() {
                    match line {
                        Ok(l) if l.trim() == "done" => {
                            if tx.send(()).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            });
            self.kept = Some(Kept {
                child,
                input,
                done,
                voice: self.voice.clone(),
                speed: self.speed,
            });
        }
        let Some(kept) = self.kept.as_mut() else {
            return Outcome::Failed("Windows' speech is not running".into());
        };
        let line = sentence.replace(['\r', '\n'], " ");
        if writeln!(kept.input, "{line}")
            .and_then(|()| kept.input.flush())
            .is_err()
        {
            self.kept = None;
            return Outcome::Failed("Windows' speech stopped".into());
        }
        loop {
            match kept.done.recv_timeout(Duration::from_millis(20)) {
                Ok(()) => return Outcome::Whole,
                Err(RecvTimeoutError::Disconnected) => {
                    self.kept = None;
                    return Outcome::Failed("Windows' speech stopped".into());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            match self.inbox.try_recv() {
                Ok(order @ (Order::Say { .. } | Order::Stop)) => {
                    if let Some(mut k) = self.kept.take() {
                        let _ = k.child.kill();
                        let _ = k.child.wait();
                    }
                    return Outcome::Cut(order);
                }
                Ok(other) => {
                    self.obey(other);
                    // obey borrows self: the kept PowerShell is looked up again
                    return self.wait_kept();
                }
                Err(_) => {}
            }
        }
    }

    /// The rest of `read_kept`'s wait, after an order that did not end the reading.
    fn wait_kept(&mut self) -> Outcome {
        loop {
            let Some(kept) = self.kept.as_mut() else {
                return Outcome::Failed("Windows' speech stopped".into());
            };
            match kept.done.recv_timeout(Duration::from_millis(20)) {
                Ok(()) => return Outcome::Whole,
                Err(RecvTimeoutError::Disconnected) => {
                    self.kept = None;
                    return Outcome::Failed("Windows' speech stopped".into());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            match self.inbox.try_recv() {
                Ok(order @ (Order::Say { .. } | Order::Stop)) => {
                    if let Some(mut k) = self.kept.take() {
                        let _ = k.child.kill();
                        let _ = k.child.wait();
                    }
                    return Outcome::Cut(order);
                }
                Ok(other) => self.obey(other),
                Err(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(c: Command) -> String {
        std::iter::once(c.get_program().to_string_lossy().to_string())
            .chain(c.get_args().map(|a| a.to_string_lossy().to_string()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn each_program_is_given_the_voice_and_the_speed_its_own_way() {
        assert_eq!(
            line(Backend::Say.command(Some("Samantha"), 1.0)),
            "say -r 180 -v Samantha"
        );
        assert_eq!(
            line(Backend::EspeakNg.command(Some("en-gb"), 2.0)),
            "espeak-ng -s 350 -v en-gb --stdin"
        );
        assert_eq!(
            line(Backend::SpdSay.command(None, 0.5)),
            "spd-say -e -w -r -50"
        );
        let windows = windows_script(Some("it's"), 2.0);
        assert!(windows.contains("$s.Rate=10;"), "{windows}");
        assert!(windows.contains("SelectVoice('it''s')"), "{windows}");
    }

    #[test]
    fn the_voice_lists_are_read() {
        assert_eq!(
            parse_say("Alex                en_US    # Most people recognize me.\n")[0].id,
            "Alex"
        );
        let espeak = "Pty Language Age/Gender VoiceName File Other\n 2  en-us  --/M  English_(America)  gmw/en-US  (en 3)\n";
        assert_eq!(parse_espeak(espeak)[0].about, "English (America)");
        assert_eq!(
            parse_spd("NAME LANGUAGE VARIANT\nafrikaans af none\n")[0].id,
            "afrikaans"
        );
    }

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
