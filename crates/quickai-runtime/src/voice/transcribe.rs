//! What was said, written down by quickai-whisper (everybodyai/quickai-whisper: whisper.cpp in a program of its own,
//! which the packages carry next to the app, as they carry llama-server). It is started with the speech model the first
//! time something is to be written down and kept running, so the model is loaded once; a recording goes to it on its
//! input and its words come back as a line of JSON (the protocol is its README's). A thread of its own talks to it, so
//! the window never waits.
//!
//! The program runs on the processor alone. `quickai-whisper-avx2` is taken where the processor has AVX2, FMA, F16C and
//! BMI2 (two to five times as fast), else `quickai-whisper`; `QUICKAI_WHISPER_BIN` names one (tests, development).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::RATE;

/// What to write down.
struct Job {
    id: u64,
    model: PathBuf,
    samples: Vec<f32>,
    language: Option<String>,
}

/// What came of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    /// The words (empty when nothing was said), and the language heard.
    Words {
        id: u64,
        text: String,
        language: Option<String>,
    },
    Failed {
        id: u64,
        why: String,
    },
}

/// The thread that writes down; it ends, and the program with it, when this is dropped.
pub struct Transcriber {
    jobs: Sender<Job>,
    written: Receiver<Written>,
}

/// How long the program may take to load a model, and to write down a recording (ten minutes of speech on a slow
/// processor).
const LOADING: Duration = Duration::from_secs(120);
const WRITING: Duration = Duration::from_secs(600);

impl Transcriber {
    /// `wake` is called when something is written, for the window to look.
    pub fn new(wake: impl Fn() + Send + 'static) -> Transcriber {
        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let (out, written) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("speech to text".into())
            .spawn(move || {
                let mut running: Option<Running> = None;
                while let Ok(job) = inbox.recv() {
                    let id = job.id;
                    let done = write_down(&mut running, job)
                        .unwrap_or_else(|why| Written::Failed { id, why });
                    if out.send(done).is_err() {
                        break;
                    }
                    wake();
                }
                if let Some(mut r) = running {
                    r.end();
                }
            })
            .expect("a thread for speech to text");
        Transcriber { jobs, written }
    }

    /// Asks for `samples` (16 kHz mono) to be written down with `model`; the answer comes with `id`.
    pub fn write(&self, id: u64, model: &Path, samples: Vec<f32>, language: Option<String>) {
        let _ = self.jobs.send(Job {
            id,
            model: model.to_path_buf(),
            samples,
            language,
        });
    }

    /// What was written since the last time.
    pub fn take(&self) -> Vec<Written> {
        self.written.try_iter().collect()
    }
}

/// The program QuickAI runs, where it is: `QUICKAI_WHISPER_BIN`, else the faster build the processor can run, in the
/// first of `paths::speech_program_dirs` that has one.
pub fn program() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("QUICKAI_WHISPER_BIN") {
        return Some(PathBuf::from(named)).filter(|p| p.is_file());
    }
    let mut names = Vec::new();
    if fast_processor() {
        names.push("quickai-whisper-avx2");
    }
    names.push("quickai-whisper");
    for dir in crate::paths::speech_program_dirs() {
        for name in &names {
            let path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// The processor runs the avx2 build: AVX2, FMA, F16C and BMI2.
fn fast_processor() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("fma")
            && std::arch::is_x86_feature_detected!("f16c")
            && std::arch::is_x86_feature_detected!("bmi2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// The program running with a model: its input, the lines it says, and the last of what it said on its error output.
struct Running {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
    errors: Arc<Mutex<VecDeque<String>>>,
    program: PathBuf,
    model: PathBuf,
}

impl Running {
    fn start(program: &Path, model: &Path) -> Result<Running, String> {
        let mut command = Command::new(program);
        command
            .arg("--model")
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("{} could not be started: {e}", program.display()))?;
        let (Some(input), Some(output), Some(error)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            return Err("the speech program could not be talked to".into());
        };
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let errors = Arc::new(Mutex::new(VecDeque::new()));
        let kept = Arc::clone(&errors);
        std::thread::spawn(move || {
            let mut text = String::new();
            let mut error = error;
            let mut buf = [0u8; 1024];
            while let Ok(n) = error.read(&mut buf) {
                if n == 0 {
                    break;
                }
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                while let Some(end) = text.find('\n') {
                    let line: String = text.drain(..=end).collect();
                    if let Ok(mut lines) = kept.lock() {
                        lines.push_back(line.trim_end().to_string());
                        while lines.len() > 20 {
                            lines.pop_front();
                        }
                    }
                }
            }
        });
        let mut running = Running {
            child,
            input,
            lines,
            errors,
            program: program.to_path_buf(),
            model: model.to_path_buf(),
        };
        let hello = running.line(LOADING)?;
        let said: serde_json::Value = serde_json::from_str(&hello)
            .map_err(|_| format!("the speech program said something else: {hello}"))?;
        if said.get("ready").and_then(serde_json::Value::as_bool) != Some(true) {
            let why = said
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("it did not start");
            running.end();
            return Err(why.to_string());
        }
        Ok(running)
    }

    /// The next line it says, within `wait`; why not, when it does not.
    fn line(&mut self, wait: Duration) -> Result<String, String> {
        match self.lines.recv_timeout(wait) {
            Ok(line) => Ok(line),
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                Err(format!(
                    "the speech program said nothing in {} s",
                    wait.as_secs()
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                let status = self
                    .child
                    .wait()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|e| e.to_string());
                let said = self.errors.lock().map(|l| {
                    l.iter()
                        .rev()
                        .take(3)
                        .rev()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" ")
                });
                Err(match said.unwrap_or_default() {
                    s if s.is_empty() => format!("the speech program stopped ({status})"),
                    s => format!("the speech program stopped ({status}): {s}"),
                })
            }
        }
    }

    fn end(&mut self) {
        let _ = self.input.write_all(b"quit\n");
        let _ = self.input.flush();
        // a moment to end by itself, then it is stopped
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_down(running: &mut Option<Running>, job: Job) -> Result<Written, String> {
    // too short to be words (a click of the button): nothing was said, and nothing need be started
    if job.samples.len() < RATE as usize / 4 {
        return Ok(Written::Words {
            id: job.id,
            text: String::new(),
            language: None,
        });
    }
    let program = program().ok_or_else(missing)?;
    // started, or started again for another model, another program, or after it stopped
    let current = running.as_mut().is_some_and(|r| {
        r.model == job.model && r.program == program && matches!(r.child.try_wait(), Ok(None))
    });
    if !current {
        if let Some(mut old) = running.take() {
            old.end();
        }
        *running = Some(Running::start(&program, &job.model)?);
    }
    let Some(r) = running.as_mut() else {
        return Err("the speech program is not running".into());
    };
    let language = job
        .language
        .as_deref()
        .filter(|l| !l.is_empty())
        .unwrap_or("auto");
    let mut request =
        format!("transcribe {} {} {language}\n", job.id, job.samples.len()).into_bytes();
    request.reserve(job.samples.len() * 4);
    for s in &job.samples {
        request.extend_from_slice(&s.to_le_bytes());
    }
    if let Err(e) = r.input.write_all(&request).and_then(|()| r.input.flush()) {
        let why = r
            .line(Duration::from_millis(200))
            .err()
            .unwrap_or_else(|| e.to_string());
        *running = None;
        return Err(why);
    }
    let answer = match r.line(WRITING) {
        Ok(line) => line,
        Err(why) => {
            *running = None;
            return Err(why);
        }
    };
    let said: serde_json::Value = serde_json::from_str(&answer)
        .map_err(|_| format!("the speech program said something else: {answer}"))?;
    if let Some(why) = said.get("error").and_then(serde_json::Value::as_str) {
        return Err(why.to_string());
    }
    let text = said
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let language = said
        .get("language")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Ok(Written::Words {
        id: job.id,
        text: clean(text),
        language,
    })
}

/// Why speech to text cannot work here: the program is not next to the app.
pub fn missing() -> String {
    format!(
        "QuickAI's speech program (quickai-whisper{}) is not next to it. The packages of QuickAI carry it; a build of \
         your own finds it in ~/.quickai/speech/bin, or where QUICKAI_WHISPER_BIN says (the program's releases are at \
         github.com/everybodyai/quickai-whisper)",
        std::env::consts::EXE_SUFFIX
    )
}

/// What whisper wrote, without what it writes for sounds that are not words (`[BLANK_AUDIO]`, `(music)`) and the words
/// it is known to make up from silence.
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => out.push(c),
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let bare = out
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    const MADE_UP: [&str; 7] = [
        "thank you",
        "thanks for watching",
        "thank you for watching",
        "you",
        "bye",
        "subtitles by the amara.org community",
        "please subscribe",
    ];
    if bare.is_empty() || MADE_UP.contains(&bare.as_str()) {
        return String::new();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_and_made_up_words_are_not_what_was_said() {
        assert_eq!(clean(" [BLANK_AUDIO]"), "");
        assert_eq!(
            clean(" (music playing) Hello there. [laughs]"),
            "Hello there."
        );
        assert_eq!(clean(" Thank you."), "");
        assert_eq!(
            clean(" Thank you for the recipe."),
            "Thank you for the recipe."
        );
        assert_eq!(clean("  What can I   cook?  "), "What can I cook?");
    }

    /// A program that is not there is said, and nothing is started for a click of the button.
    #[test]
    fn a_missing_program_is_said_and_a_click_needs_none() {
        let (tx, rx) = std::sync::mpsc::channel();
        let t = Transcriber::new(move || {
            let _ = tx.send(());
        });
        t.write(1, Path::new("/nowhere/model.bin"), vec![0.0; 100], None);
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            t.take(),
            [Written::Words {
                id: 1,
                text: String::new(),
                language: None
            }]
        );
        let mut gone: Option<Running> = None;
        let job = Job {
            id: 2,
            model: PathBuf::from("/nowhere/model.bin"),
            samples: vec![0.0; 16_000],
            language: None,
        };
        // QUICKAI_WHISPER_BIN naming a file that is not there is no program
        if std::env::var_os("QUICKAI_WHISPER_BIN").is_none() && program().is_none() {
            assert!(write_down(&mut gone, job)
                .unwrap_err()
                .contains("quickai-whisper"));
        }
    }

    /// With the program, the model and the recording of whisper.cpp's own sample (the program where `program` finds it,
    /// `QUICKAI_TEST_WHISPER_MODEL`, `QUICKAI_TEST_WHISPER_WAV`), what it says is written down, and the program stays
    /// running for the next one. Left out when they are not there.
    #[test]
    fn a_recording_is_written_down_by_the_program() {
        let (Some(model), Some(wav)) = (
            std::env::var_os("QUICKAI_TEST_WHISPER_MODEL"),
            std::env::var_os("QUICKAI_TEST_WHISPER_WAV"),
        ) else {
            return;
        };
        if program().is_none() {
            return;
        }
        let bytes = std::fs::read(wav).unwrap();
        let samples: Vec<f32> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
            .collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let t = Transcriber::new(move || {
            let _ = tx.send(());
        });
        t.write(1, Path::new(&model), samples.clone(), None);
        t.write(
            2,
            Path::new(&model),
            samples[..16_000 * 3].to_vec(),
            Some("en".into()),
        );
        let mut all = Vec::new();
        while all.len() < 2 {
            rx.recv_timeout(Duration::from_secs(120)).unwrap();
            all.extend(t.take());
        }
        match &all[..] {
            [Written::Words {
                id: 1,
                text,
                language,
            }, Written::Words {
                id: 2, text: short, ..
            }] => {
                assert!(
                    text.contains("ask not what your country can do for you"),
                    "{text}"
                );
                assert_eq!(language.as_deref(), Some("en"));
                assert!(short.contains("fellow Americans"), "{short}");
            }
            other => panic!("{other:?}"),
        }
    }
}
