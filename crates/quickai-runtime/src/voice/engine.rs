//! Reading aloud with quickai-voice, kept running: it loads the voice once, makes each sentence while the one before it
//! plays, and says when each begins and ends. A speech server's sound is played by it too: a thread of its own asks the
//! server for each sentence, in order, and hands what comes back to quickai-voice.
//!
//! quickai-voice's protocol (its README): requests are lines on its input, `load\t<json>`, `say\t<id>\t<speaker>\t
//! <speed>\t<text>`, `play\t<id>\t<bytes>` and that many bytes of a WAV file, `stop`, `quit`; it answers with JSON
//! lines: `{"ready":true,…,"sound":true}`, `{"loaded":true,…}`, `{"id":7,"began":true}`, `{"id":7,"ended":true}`,
//! `{"id":7,"error":"…"}`, `{"stopped":true}`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde_json::Value;

use super::speak::{Backend, Input, Order, Spoken};

/// How long quickai-voice may take to start, and to load a voice (Kokoro takes a second or two).
const STARTING: Duration = Duration::from_secs(20);
const LOADING: Duration = Duration::from_secs(120);

/// quickai-voice running, with a voice loaded.
struct Engine {
    child: Child,
    input: ChildStdin,
}

impl Engine {
    /// Starts it and loads the voice of `backend` (a speech server needs none): what it says from then on comes as
    /// `Input::Event` on `feed`, and `Input::Gone` when it ends.
    fn start(backend: &Backend, feed: Sender<Input>) -> Result<Engine, String> {
        let program = match backend {
            Backend::Voice { program, .. }
            | Backend::Server { program, .. }
            | Backend::Player { program } => program,
            Backend::Command(_) => return Err("not quickai-voice".into()),
        };
        let mut command = Command::new(program);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hide_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("quickai-voice could not be started: {e}"))?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err("quickai-voice could not be started".into());
        };
        // its answers about itself (ready, loaded) come here; the rest go to the reading thread
        let (control_tx, control) = std::sync::mpsc::channel::<Value>();
        std::thread::Builder::new()
            .name("quickai-voice says".into())
            .spawn(move || {
                for line in BufReader::new(output).lines() {
                    let Ok(line) = line else { break };
                    let Ok(said) = serde_json::from_str::<Value>(line.trim()) else {
                        continue;
                    };
                    let about_itself = said.get("ready").is_some() || said.get("loaded").is_some();
                    let delivered = if about_itself {
                        control_tx.send(said).is_ok()
                    } else {
                        feed.send(Input::Event(said)).is_ok()
                    };
                    if !delivered {
                        break;
                    }
                }
                let _ = feed.send(Input::Gone);
            })
            .map_err(|e| e.to_string())?;
        let mut engine = Engine { child, input };
        let ready = control
            .recv_timeout(STARTING)
            .map_err(|_| "quickai-voice did not start".to_string());
        let ready = match ready {
            Ok(r) => r,
            Err(e) => {
                engine.end();
                return Err(e);
            }
        };
        if ready.get("ready").and_then(Value::as_bool) != Some(true) {
            engine.end();
            return Err(said_error(&ready, "quickai-voice did not start"));
        }
        if ready.get("sound").and_then(Value::as_bool) == Some(false) {
            engine.end();
            return Err("there is no sound device to read aloud to".into());
        }
        if let Backend::Voice { spec, .. } = backend {
            engine.line(&format!("load\t{}", spec.to_json()))?;
            let loaded = control
                .recv_timeout(LOADING)
                .map_err(|_| "the voice did not load".to_string());
            match loaded {
                Ok(l) if l.get("loaded").and_then(Value::as_bool) == Some(true) => {}
                Ok(l) => {
                    engine.end();
                    return Err(said_error(&l, "the voice could not be loaded"));
                }
                Err(e) => {
                    engine.end();
                    return Err(e);
                }
            }
        }
        Ok(engine)
    }

    fn line(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.input, "{line}")
            .and_then(|()| self.input.flush())
            .map_err(|_| "quickai-voice stopped".to_string())
    }

    fn play(&mut self, id: u64, sound: &[u8]) -> Result<(), String> {
        writeln!(self.input, "play\t{id}\t{}", sound.len())
            .and_then(|()| self.input.write_all(sound))
            .and_then(|()| self.input.flush())
            .map_err(|_| "quickai-voice stopped".to_string())
    }

    fn end(&mut self) {
        let _ = writeln!(self.input, "quit");
        let _ = self.input.flush();
        std::thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.end();
    }
}

fn said_error(said: &Value, otherwise: &str) -> String {
    said.get("error")
        .and_then(Value::as_str)
        .map_or_else(|| otherwise.to_string(), str::to_string)
}

#[cfg(windows)]
fn hide_window(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_: &mut Command) {}

/// A sentence for a speech server to say.
struct Ask {
    round: u64,
    id: u64,
    text: String,
    speed: f64,
}

/// The thread that asks a speech server for each sentence, in order, and gives the sound back on `feed`.
fn ask_server(
    url: &str,
    model: String,
    key: String,
    voice: String,
    asks: Receiver<Ask>,
    feed: Sender<Input>,
) {
    let url = speech_url(url);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    for ask in asks {
        let body = serde_json::json!({
            "model": model,
            "input": ask.text,
            "voice": voice,
            "response_format": "wav",
            "speed": ask.speed,
        });
        let mut request = agent
            .post(&url)
            .header("content-type", "application/json")
            .header("user-agent", concat!("quickai/", env!("CARGO_PKG_VERSION")));
        if !key.trim().is_empty() {
            request = request.header("authorization", format!("Bearer {}", key.trim()));
        }
        let sound = request
            .send(body.to_string())
            .map_err(|e| format!("the speech server could not be reached: {e}"))
            .and_then(|response| {
                let status = response.status().as_u16();
                let mut body = response.into_body();
                if (200..300).contains(&status) {
                    body.with_config()
                        .limit(64 * 1024 * 1024)
                        .read_to_vec()
                        .map_err(|e| e.to_string())
                } else {
                    let said = body.read_to_string().unwrap_or_default();
                    let said: String = said.trim().chars().take(200).collect();
                    Err(format!("the speech server answered {status}: {said}"))
                }
            });
        if feed
            .send(Input::Fetched {
                round: ask.round,
                id: ask.id,
                sound,
            })
            .is_err()
        {
            break;
        }
    }
}

/// The address to post a sentence to: `…/v1/audio/speech`, from a server's address with or without it.
fn speech_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.ends_with("/audio/speech") {
        url.to_string()
    } else if url.ends_with("/v1") {
        format!("{url}/audio/speech")
    } else {
        format!("{url}/v1/audio/speech")
    }
}

/// The reading thread for quickai-voice (`Backend::Voice` or `Backend::Server`).
pub(crate) fn run(
    backend: Backend,
    inbox: Receiver<Input>,
    feed: Sender<Input>,
    out: Sender<Spoken>,
    wake: Box<dyn Fn() + Send>,
) {
    let mut reading = Reading {
        backend,
        feed,
        out,
        wake,
        engine: None,
        asks: None,
        utterance: None,
        open: false,
        next_index: 0,
        next_id: 1,
        round: 0,
        speed: 1.0,
        waiting: HashMap::new(),
    };
    loop {
        match inbox.recv_timeout(Duration::from_secs(60)) {
            Ok(Input::Order(order)) => reading.obey(order),
            Ok(Input::Event(said)) => reading.heard(&said),
            Ok(Input::Fetched { round, id, sound }) => reading.fetched(round, id, sound),
            Ok(Input::Gone) => reading.gone(),
            Ok(Input::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    reading.engine = None;
}

struct Reading {
    backend: Backend,
    feed: Sender<Input>,
    out: Sender<Spoken>,
    wake: Box<dyn Fn() + Send>,
    engine: Option<Engine>,
    asks: Option<Sender<Ask>>,
    utterance: Option<u64>,
    /// More sentences may come for this utterance.
    open: bool,
    next_index: usize,
    next_id: u64,
    /// The reading's round: a server's sound for an earlier one is not played.
    round: u64,
    speed: f64,
    /// The sentences given and not yet heard to their end: quickai-voice's id, and the sentence's index.
    waiting: HashMap<u64, usize>,
}

impl Reading {
    fn tell(&self, spoken: Spoken) {
        let _ = self.out.send(spoken);
        (self.wake)();
    }

    fn obey(&mut self, order: Order) {
        match order {
            Order::Say {
                utterance,
                sentences,
                more,
                speed,
                ..
            } => {
                self.halt();
                if let Some(u) = self.utterance.take() {
                    if u != utterance {
                        self.tell(Spoken::Ended { utterance: u });
                    }
                }
                if let Err(why) = self.ready() {
                    self.tell(Spoken::Failed { utterance, why });
                    return;
                }
                self.utterance = Some(utterance);
                self.open = more;
                self.speed = speed;
                self.next_index = 0;
                for sentence in sentences {
                    self.give(sentence);
                }
                self.check_done();
            }
            Order::More {
                utterance,
                sentences,
            } => {
                if self.utterance == Some(utterance) {
                    for sentence in sentences {
                        self.give(sentence);
                    }
                }
            }
            Order::NoMore { utterance } => {
                if self.utterance == Some(utterance) {
                    self.open = false;
                    self.check_done();
                }
            }
            Order::Play { utterance, sound } => {
                self.halt();
                if let Some(u) = self.utterance.take() {
                    if u != utterance {
                        self.tell(Spoken::Ended { utterance: u });
                    }
                }
                if let Err(why) = self.ready() {
                    self.tell(Spoken::Failed { utterance, why });
                    return;
                }
                self.utterance = Some(utterance);
                self.open = false;
                let id = self.next_id;
                self.next_id += 1;
                self.waiting.insert(id, 0);
                let played = self
                    .engine
                    .as_mut()
                    .ok_or_else(|| "quickai-voice is not running".to_string())
                    .and_then(|e| e.play(id, &sound));
                if let Err(why) = played {
                    self.fail(why);
                }
            }
            Order::Stop => {
                self.halt();
                if let Some(u) = self.utterance.take() {
                    self.tell(Spoken::Ended { utterance: u });
                }
            }
        }
    }

    /// quickai-voice running with the voice loaded, and for a server the thread that asks it.
    fn ready(&mut self) -> Result<(), String> {
        if self.engine.is_none() {
            self.engine = Some(Engine::start(&self.backend, self.feed.clone())?);
        }
        if let (
            Backend::Server {
                url,
                model,
                key,
                voice,
                ..
            },
            None,
        ) = (&self.backend, &self.asks)
        {
            let (asks, to_ask) = std::sync::mpsc::channel();
            let (url, model, key, voice) = (url.clone(), model.clone(), key.clone(), voice.clone());
            let feed = self.feed.clone();
            std::thread::Builder::new()
                .name("speech server".into())
                .spawn(move || ask_server(&url, model, key, voice, to_ask, feed))
                .map_err(|e| e.to_string())?;
            self.asks = Some(asks);
        }
        Ok(())
    }

    /// Stops what is being read: what quickai-voice plays and has waiting, and the server's sound still to come.
    fn halt(&mut self) {
        self.round += 1;
        self.waiting.clear();
        if let Some(engine) = &mut self.engine {
            if engine.line("stop").is_err() {
                self.engine = None;
            }
        }
    }

    /// One sentence more of the reading.
    fn give(&mut self, sentence: String) {
        let (id, index) = (self.next_id, self.next_index);
        self.next_id += 1;
        self.next_index += 1;
        let text = sentence.replace(['\t', '\r', '\n'], " ");
        self.waiting.insert(id, index);
        let sent = match &self.backend {
            Backend::Voice { speaker, .. } => {
                let line = format!("say\t{id}\t{speaker}\t{}\t{text}", self.speed);
                self.engine
                    .as_mut()
                    .ok_or_else(|| "quickai-voice is not running".to_string())
                    .and_then(|e| e.line(&line))
            }
            Backend::Server { .. } => self
                .asks
                .as_ref()
                .ok_or_else(|| "the speech server is not being asked".to_string())
                .and_then(|asks| {
                    asks.send(Ask {
                        round: self.round,
                        id,
                        text,
                        speed: self.speed,
                    })
                    .map_err(|_| "the speech server is not being asked".to_string())
                }),
            Backend::Command(_) | Backend::Player { .. } => {
                Err("there is no voice to read with".into())
            }
        };
        if let Err(why) = sent {
            self.fail(why);
        }
    }

    fn heard(&mut self, said: &Value) {
        let Some(id) = said.get("id").and_then(Value::as_u64) else {
            return;
        };
        let (Some(utterance), Some(&index)) = (self.utterance, self.waiting.get(&id)) else {
            return;
        };
        if let Some(why) = said.get("error").and_then(Value::as_str) {
            self.fail(why.to_string());
        } else if said.get("began").and_then(Value::as_bool) == Some(true) {
            self.tell(Spoken::Began { utterance, index });
        } else if said.get("ended").and_then(Value::as_bool) == Some(true) {
            self.waiting.remove(&id);
            self.check_done();
        }
    }

    fn fetched(&mut self, round: u64, id: u64, sound: Result<Vec<u8>, String>) {
        if round != self.round || !self.waiting.contains_key(&id) {
            return;
        }
        match sound {
            Ok(sound) => {
                let played = self
                    .engine
                    .as_mut()
                    .ok_or_else(|| "quickai-voice is not running".to_string())
                    .and_then(|e| e.play(id, &sound));
                if let Err(why) = played {
                    self.fail(why);
                }
            }
            Err(why) => self.fail(why),
        }
    }

    fn gone(&mut self) {
        self.engine = None;
        if self.utterance.is_some() {
            self.fail("quickai-voice stopped".into());
        }
    }

    fn fail(&mut self, why: String) {
        self.halt();
        self.open = false;
        if let Some(utterance) = self.utterance.take() {
            self.tell(Spoken::Failed { utterance, why });
        }
    }

    /// All given was heard and no more is to come: the reading ended.
    fn check_done(&mut self) {
        if !self.open && self.waiting.is_empty() {
            if let Some(utterance) = self.utterance.take() {
                self.tell(Spoken::Ended { utterance });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::speak::Speaker;
    #[cfg(unix)]
    use crate::voice::voices::{ModelKind, ModelSpec};

    /// A stand-in for quickai-voice, in Python: its protocol, with each sentence "played" for 50 ms.
    #[cfg(unix)]
    fn stand_in(dir: &std::path::Path, loads: bool) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("quickai-voice");
        let script = format!(
            r#"#!/usr/bin/env python3
import sys, json, threading, time, queue
def say(o):
    sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()
say({{"ready": True, "version": "0.1.0", "sherpa_onnx": "stand-in", "sound": True}})
q = queue.Queue(); stops = [0]
def player():
    while True:
        gen, i = q.get()
        if gen != stops[0]: continue
        say({{"id": i, "began": True}}); time.sleep(0.05)
        if gen != stops[0]: continue
        say({{"id": i, "ended": True}})
threading.Thread(target=player, daemon=True).start()
while True:
    raw = sys.stdin.buffer.readline()
    if not raw:
        break
    line = raw.decode().rstrip("\n")
    if line.startswith("load	"):
        say({{"loaded": True, "speakers": 54, "rate": 24000}} if {loads} else {{"loaded": False, "error": "no such voice"}})
    elif line.startswith("say	"):
        _, i, speaker, speed, text = line.split("	", 4)
        q.put((stops[0], int(i)))
    elif line.startswith("play	"):
        _, i, size = line.split("	")
        sys.stdin.buffer.read(int(size))
        q.put((stops[0], int(i)))
    elif line == "stop":
        stops[0] += 1; say({{"stopped": True}})
    elif line == "quit":
        break
"#,
            loads = if loads { "True" } else { "False" }
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    fn spec() -> ModelSpec {
        ModelSpec {
            kind: ModelKind::Kokoro,
            files: [("model", "/nowhere/model.onnx".to_string())]
                .into_iter()
                .collect(),
        }
    }

    #[cfg(unix)]
    fn heard_until(
        speaker: &Speaker,
        wake: &Receiver<()>,
        done: impl Fn(&[Spoken]) -> bool,
    ) -> Vec<Spoken> {
        let mut heard = Vec::new();
        while !done(&heard) {
            wake.recv_timeout(Duration::from_secs(20))
                .expect("it says something");
            heard.extend(speaker.take());
        }
        heard
    }

    /// The sentences are given to quickai-voice at once and said as they begin, in order; a new reading stops the one
    /// before; a voice that does not load is said to have failed.
    #[cfg(unix)]
    #[test]
    fn quickai_voice_reads_the_sentences_in_order_and_a_new_reading_ends_the_last() {
        let dir = std::env::temp_dir().join(format!("quickai-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = stand_in(&dir, true);
        let (wake_tx, wake) = std::sync::mpsc::channel();
        let speaker = Speaker::new(
            Ok(Backend::Voice {
                program,
                spec: spec(),
                speaker: 3,
            }),
            move || {
                let _ = wake_tx.send(());
            },
        );
        assert_eq!(speaker.available(), Ok(()));
        speaker.say(1, vec!["One.".into(), "Two.".into()], true, None, 1.0);
        speaker.more(1, vec!["Three.".into()]);
        speaker.no_more(1);
        let heard = heard_until(&speaker, &wake, |h| {
            h.contains(&Spoken::Ended { utterance: 1 })
        });
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
        // a long reading, cut by another
        speaker.say(2, vec!["Long.".into(); 40], false, None, 1.0);
        std::thread::sleep(Duration::from_millis(200));
        speaker.say(3, vec!["Short.".into()], false, None, 1.0);
        let heard = heard_until(&speaker, &wake, |h| {
            h.contains(&Spoken::Ended { utterance: 3 })
        });
        assert!(heard.contains(&Spoken::Ended { utterance: 2 }), "{heard:?}");
        assert!(
            heard
                .iter()
                .filter(|s| matches!(s, Spoken::Began { utterance: 2, .. }))
                .count()
                < 40,
            "{heard:?}"
        );
        assert!(
            heard.contains(&Spoken::Began {
                utterance: 3,
                index: 0
            }),
            "{heard:?}"
        );
        // a recording played back
        speaker.play(9, crate::voice::speak::wav16(&[0.0; 1600], 16_000));
        let heard = heard_until(&speaker, &wake, |h| {
            h.contains(&Spoken::Ended { utterance: 9 })
        });
        assert_eq!(
            heard,
            [
                Spoken::Began {
                    utterance: 9,
                    index: 0
                },
                Spoken::Ended { utterance: 9 }
            ]
        );
        drop(speaker);

        // a voice that does not load
        let program = stand_in(&dir, false);
        let (wake_tx, wake) = std::sync::mpsc::channel();
        let speaker = Speaker::new(
            Ok(Backend::Voice {
                program,
                spec: spec(),
                speaker: 0,
            }),
            move || {
                let _ = wake_tx.send(());
            },
        );
        speaker.say(4, vec!["Hello.".into()], false, None, 1.0);
        let heard = heard_until(&speaker, &wake, |h| !h.is_empty());
        assert_eq!(
            heard,
            [Spoken::Failed {
                utterance: 4,
                why: "no such voice".into()
            }]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With the real quickai-voice and a voice (`QUICKAI_TEST_VOICE_BIN`, `QUICKAI_TEST_VOICE_DIR`, a folder of a
    /// Kokoro pack): two sentences are read in order, each heard for about its length. Left out when they are not
    /// there.
    #[test]
    fn the_real_program_reads_a_kokoro_voice() {
        let (Some(program), Some(dir)) = (
            std::env::var_os("QUICKAI_TEST_VOICE_BIN"),
            std::env::var_os("QUICKAI_TEST_VOICE_DIR"),
        ) else {
            return;
        };
        let spec = crate::voice::voices::describe_folder(std::path::Path::new(&dir)).unwrap();
        let (wake_tx, wake) = std::sync::mpsc::channel::<()>();
        let speaker = Speaker::new(
            Ok(Backend::Voice {
                program: program.into(),
                spec,
                speaker: 3,
            }),
            move || {
                let _ = wake_tx.send(());
            },
        );
        let asked = std::time::Instant::now();
        speaker.say(
            1,
            vec![
                "Hello from QuickAI.".into(),
                "This is Kokoro, reading on this computer.".into(),
            ],
            false,
            None,
            1.0,
        );
        let mut heard = Vec::new();
        let mut first = None;
        while !heard.contains(&Spoken::Ended { utterance: 1 }) {
            wake.recv_timeout(Duration::from_secs(60))
                .expect("it reads");
            for s in speaker.take() {
                if first.is_none() && matches!(s, Spoken::Began { .. }) {
                    first = Some(asked.elapsed());
                }
                heard.push(s);
            }
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
                Spoken::Ended { utterance: 1 },
            ]
        );
        let all = asked.elapsed();
        eprintln!(
            "first sentence began after {:?} (with the voice loading), all heard after {all:?}",
            first.unwrap()
        );
        assert!(
            all > Duration::from_secs(2),
            "two sentences take a while to be heard: {all:?}"
        );
    }

    #[test]
    fn a_servers_address_is_made_the_address_of_its_speech() {
        assert_eq!(
            speech_url("http://127.0.0.1:8880"),
            "http://127.0.0.1:8880/v1/audio/speech"
        );
        assert_eq!(
            speech_url("http://127.0.0.1:8880/v1/"),
            "http://127.0.0.1:8880/v1/audio/speech"
        );
        assert_eq!(
            speech_url("https://x.example/v1/audio/speech"),
            "https://x.example/v1/audio/speech"
        );
    }
}
