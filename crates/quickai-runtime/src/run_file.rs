//! The run that goes on a computer (WP4.33.2, docs/remote-runs.md section 3.1): one at a time for a person, found by
//! the lock it holds on a file of its data folder, said in a file beside it (what runs, since when, where it listens),
//! and stopped by a file another program writes there. It is the same on Linux, macOS and Windows: `quickai-run up`
//! holds it, `quickai-run status` and `down` read and write it, and the window asks for them over SSH.
//!
//! The folder is `run` in the data folder (`~/.quickai/run`):
//!
//! - `lock`, locked by the run for as long as it lives: the system lets go of it when the run ends, however it ends,
//!   so a run is never taken for one that goes on when it has not;
//! - `run.json`, what runs (`RunFile`), written when the run starts and as it goes, removed when it ends;
//! - `stop`, written to ask the run to stop (`down`): it looks for it each second;
//! - `last.json`, how the last run ended (`Ended`), for a run that ended with nobody watching;
//! - `events.jsonl`, what a run started in the background said, one event a line.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::ConfigDirs;
use crate::provision::{EndpointInfo, Event};

/// What runs on this computer, as `run.json` says it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunFile {
    pub pid: u32,
    /// The configuration, as it was named to `up` (an id or a path).
    pub configuration: String,
    /// The version of `quickai-run` that runs it.
    pub version: String,
    /// When it started, in seconds since 1970.
    pub started: u64,
    /// It was started in the background (`up --detach`), and its events are in `events.jsonl`.
    pub detached: bool,
    /// `starting`, `preparing` (installing and fetching), `ready`, `stopping`.
    pub state: String,
    /// Where each thing it started listens, once it is ready.
    #[serde(default)]
    pub endpoints: Vec<EndpointInfo>,
    #[serde(default)]
    pub model: Option<String>,
    /// The steps of its plan, as far as they have gone: what the window shows while it is being prepared.
    #[serde(default)]
    pub steps: Vec<RunStep>,
    /// The computer it runs on, in a few words (`Linux, NVIDIA GeForce RTX 4090`).
    #[serde(default)]
    pub computer: Option<String>,
}

/// A step of a run's plan (install a build, fetch a model, ...), and how far it has gone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunStep {
    /// `install`, `fetch`, `start`... (the plan's kinds).
    #[serde(default)]
    pub kind: String,
    pub text: String,
    /// `pending`, `doing` or `done`.
    pub state: String,
    /// Bytes done, and of how many, while it fetches or installs.
    #[serde(default)]
    pub done: u64,
    #[serde(default)]
    pub total: Option<u64>,
    /// When it began and ended, in seconds since 1970.
    #[serde(default)]
    pub began: Option<u64>,
    #[serde(default)]
    pub ended: Option<u64>,
}

impl RunFile {
    /// The configuration as a person names it: the file's name without its folder and `.toml` when it was given as
    /// a file (a run the window started is given the file it copied there), else the id as it was given.
    pub fn name(&self) -> String {
        configuration_name(&self.configuration)
    }
}

/// `/home/me/.quickai/remote/vision-chat.toml` is `vision-chat`; an id is itself.
pub fn configuration_name(given: &str) -> String {
    let path = Path::new(given);
    if path.extension().is_some_and(|e| e == "toml") {
        if let Some(stem) = path.file_stem() {
            return stem.to_string_lossy().into_owned();
        }
    }
    given.to_string()
}

/// How the last run ended, as `last.json` says it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ended {
    pub configuration: String,
    pub started: u64,
    pub ended: u64,
    /// The outcome's word (`stopped`, `failed`, ...), as the `finished` event says it.
    pub outcome: String,
    pub message: Option<String>,
    /// The exit code of `quickai-run`.
    pub code: i32,
}

/// What `quickai-run status --json` prints: what runs, or how the last run ended, and the last events of a run in the
/// background.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub running: Option<RunFile>,
    pub last: Option<Ended>,
    #[serde(default)]
    pub events: Vec<serde_json::Value>,
}

/// The `run` folder of the data folder `dirs` names (`~/.quickai/run` when it names none).
pub fn run_dir(dirs: &ConfigDirs) -> PathBuf {
    let data = dirs.data.clone().unwrap_or_else(|| {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().join(".quickai"))
            .unwrap_or_else(|| std::env::temp_dir().join(".quickai"))
    });
    data.join("run")
}

/// Seconds since 1970, now.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The run this program holds: while it is kept, the lock is held and `run.json` says what runs. Dropped, the file
/// and a stop that was asked for are removed, and the lock is let go of. A run in a terminal leaves nothing behind
/// (the folders it made are removed when nothing else is in them, as a run that failed at once wrote nothing before);
/// a run in the background leaves how it ended (`last.json`) for `status` to say.
pub struct Held {
    dir: PathBuf,
    file: RunFile,
    /// The folders `hold` made, the outermost first.
    made: Vec<PathBuf>,
    _lock: File,
}

/// Why a run could not be held.
#[derive(Debug)]
pub enum NotHeld {
    /// Another run goes on, and this is what it says of itself (nothing, in the moment before it writes it).
    Busy(Option<Box<RunFile>>),
    Io(std::io::Error),
}

/// Holds the run of `dir` for a run of `configuration`, unless another goes on.
pub fn hold(dir: &Path, configuration: &str, detached: bool) -> Result<Held, NotHeld> {
    let mut made: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|d| !d.exists())
        .map(Path::to_path_buf)
        .collect();
    made.reverse();
    std::fs::create_dir_all(dir).map_err(NotHeld::Io)?;
    let lock = open_lock(dir).map_err(NotHeld::Io)?;
    // `running` takes the lock for a moment to see whether anyone holds it: a lock that stays taken for a second is
    // another run's
    let mut tries = 0;
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if tries < 5 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(TryLockError::WouldBlock) => {
                remove_made(&made);
                return Err(NotHeld::Busy(read_run_file(dir).map(Box::new)));
            }
            Err(TryLockError::Error(e)) => return Err(NotHeld::Io(e)),
        }
    }
    // a stop asked of a run that is gone is not this one's
    let _ = std::fs::remove_file(dir.join("stop"));
    let held = Held {
        dir: dir.to_path_buf(),
        file: RunFile {
            pid: std::process::id(),
            configuration: configuration.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            started: now(),
            detached,
            state: "starting".into(),
            endpoints: Vec::new(),
            model: None,
            steps: Vec::new(),
            computer: None,
        },
        made,
        _lock: lock,
    };
    held.write().map_err(NotHeld::Io)?;
    Ok(held)
}

impl Held {
    /// What `run.json` says.
    pub fn file(&self) -> &RunFile {
        &self.file
    }

    /// What `event` of the run says of it: its state, the steps of its plan and how far each has gone, and where it
    /// listens once it is ready.
    pub fn note(&mut self, event: &Event) {
        match event {
            Event::Computer { system, gpus, .. } => {
                let mut words = vec![system.clone()];
                words.extend(gpus.iter().cloned());
                self.file.computer = Some(words.join(", "));
            }
            Event::Plan { steps } => {
                self.file.state = "preparing".into();
                self.file.steps = steps
                    .iter()
                    .map(|s| RunStep {
                        kind: s.kind.clone(),
                        text: s.text.clone(),
                        state: "pending".into(),
                        done: 0,
                        total: s.bytes,
                        began: None,
                        ended: None,
                    })
                    .collect();
            }
            Event::Step { index, text, .. } => {
                self.file.state = "preparing".into();
                // a step the plan did not list (a model pulled by name) is added where it comes
                while self.file.steps.len() < *index {
                    self.file.steps.push(RunStep {
                        kind: String::new(),
                        text: text.clone(),
                        state: "pending".into(),
                        done: 0,
                        total: None,
                        began: None,
                        ended: None,
                    });
                }
                if let Some(step) = step(&mut self.file, *index) {
                    step.state = "doing".into();
                    step.began = Some(now());
                }
            }
            Event::Progress {
                index, done, total, ..
            } => {
                if let Some(step) = step(&mut self.file, *index) {
                    step.done = *done;
                    step.total = total.or(step.total);
                }
            }
            Event::StepDone { index } => {
                if let Some(step) = step(&mut self.file, *index) {
                    step.state = "done".into();
                    step.ended = Some(now());
                }
            }
            Event::Ready { endpoints, model } => {
                self.file.state = "ready".into();
                self.file.endpoints = endpoints.clone();
                self.file.model = model.clone();
                // what the plan said is done when the programs run (their start) is done now
                for step in self.file.steps.iter_mut().filter(|s| s.state != "done") {
                    step.state = "done".into();
                    step.ended = Some(now());
                }
            }
            Event::Stopping => self.file.state = "stopping".into(),
            _ => return,
        }
        let _ = self.write();
    }

    /// Says the run is in `state`.
    pub fn set_state(&mut self, state: &str) {
        if self.file.state != state {
            self.file.state = state.to_string();
            let _ = self.write();
        }
    }

    /// Says the run is ready, and where it listens.
    pub fn set_ready(&mut self, endpoints: &[EndpointInfo], model: Option<&str>) {
        self.file.state = "ready".into();
        self.file.endpoints = endpoints.to_vec();
        self.file.model = model.map(str::to_string);
        let _ = self.write();
    }

    /// Writes how the run ended, for `status` to say once it is gone: of a run in the background, which nobody watched.
    pub fn record_end(&self, outcome: &str, message: Option<String>, code: i32) {
        if !self.file.detached {
            return;
        }
        let ended = Ended {
            configuration: self.file.configuration.clone(),
            started: self.file.started,
            ended: now(),
            outcome: outcome.to_string(),
            message,
            code,
        };
        if let Ok(text) = serde_json::to_string_pretty(&ended) {
            let _ = write_whole(&self.dir.join("last.json"), &text);
        }
    }

    fn write(&self) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(&self.file).map_err(std::io::Error::other)?;
        write_whole(&self.dir.join("run.json"), &text)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dir.join("run.json"));
        let _ = std::fs::remove_file(self.dir.join("stop"));
        if !self.file.detached {
            let _ = std::fs::remove_file(self.dir.join("lock"));
            remove_made(&self.made);
        }
    }
}

/// Removes the folders of `made` that nothing else is in, the innermost first.
fn remove_made(made: &[PathBuf]) {
    for folder in made.iter().rev() {
        if std::fs::remove_dir(folder).is_err() {
            break;
        }
    }
}

/// What runs in `dir`, if a run goes on: its lock is held by someone.
pub fn running(dir: &Path) -> Option<RunFile> {
    let lock = open_lock(dir).ok()?;
    match lock.try_lock() {
        // nobody holds it: nothing runs, and a file left by a run that was killed says nothing true
        Ok(()) => {
            let _ = std::fs::remove_file(dir.join("run.json"));
            None
        }
        Err(TryLockError::WouldBlock) => Some(read_run_file(dir).unwrap_or_else(|| RunFile {
            pid: 0,
            configuration: String::new(),
            version: String::new(),
            started: 0,
            detached: false,
            state: "starting".into(),
            endpoints: Vec::new(),
            model: None,
            steps: Vec::new(),
            computer: None,
        })),
        Err(TryLockError::Error(_)) => None,
    }
}

/// Asks the run of `dir` to stop: it looks for the file each second.
pub fn ask_stop(dir: &Path) -> std::io::Result<()> {
    std::fs::write(dir.join("stop"), b"stop\n")
}

/// A stop was asked of the run of `dir` (`down`): the file is taken away, so that it is answered once.
pub fn take_stop(dir: &Path) -> bool {
    std::fs::remove_file(dir.join("stop")).is_ok()
}

/// What `run.json` says, without asking whether its run goes on (`running` asks).
pub fn read(dir: &Path) -> Option<RunFile> {
    read_run_file(dir)
}

/// How the last run ended, if one did.
pub fn last(dir: &Path) -> Option<Ended> {
    let text = std::fs::read_to_string(dir.join("last.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Where a run in the background writes its events.
pub fn events_path(dir: &Path) -> PathBuf {
    dir.join("events.jsonl")
}

/// The last `count` events a run in the background wrote, read from the end of the file (it is as long as the run
/// was: a download says how far along it is every moment).
pub fn last_events(dir: &Path, count: usize) -> Vec<serde_json::Value> {
    const TAIL: u64 = 256 * 1024;
    let Ok(mut file) = File::open(events_path(dir)) else {
        return Vec::new();
    };
    let length = file.metadata().map_or(0, |m| m.len());
    let from = length.saturating_sub(TAIL);
    if file.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.lines().collect();
    // a tail that starts in the middle of a line has half of one first
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let mut events: Vec<serde_json::Value> = lines
        .iter()
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
        .take(count)
        .collect();
    events.reverse();
    events
}

/// What `status` says of `dir`.
pub fn status(dir: &Path, events: usize) -> Status {
    let running = running(dir);
    let detached = running.as_ref().is_some_and(|r| r.detached);
    Status {
        last: if running.is_none() { last(dir) } else { None },
        events: if detached || running.is_none() {
            last_events(dir, events)
        } else {
            Vec::new()
        },
        running,
    }
}

/// The step `index` of `file` (the provisioner counts them from 1).
fn step(file: &mut RunFile, index: usize) -> Option<&mut RunStep> {
    file.steps.get_mut(index.checked_sub(1)?)
}

fn open_lock(dir: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("lock"))
}

fn read_run_file(dir: &Path) -> Option<RunFile> {
    let text = std::fs::read_to_string(dir.join("run.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes `text` to `path` whole or not at all: a reader never meets half of it.
fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, format!("{text}\n"))?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::scratch;

    #[test]
    fn a_run_is_held_once_said_while_it_goes_and_gone_with_it() {
        let scratch = scratch("run-file");
        let dir = scratch.join("run");
        assert_eq!(
            running(&dir),
            None,
            "nothing runs in a folder that is not there"
        );
        let mut held = hold(&dir, "vision", true).expect("held");
        assert!(
            matches!(hold(&dir, "other", false), Err(NotHeld::Busy(Some(r))) if r.configuration == "vision"),
            "one run at a time"
        );
        held.set_ready(
            &[EndpointInfo {
                role: "api".into(),
                program: "llama-server".into(),
                url: "http://127.0.0.1:8080".into(),
            }],
            Some("bonsai"),
        );
        let now = running(&dir).expect("it runs");
        assert_eq!(now.state, "ready");
        assert_eq!(now.endpoints[0].url, "http://127.0.0.1:8080");
        assert!(!take_stop(&dir));
        ask_stop(&dir).unwrap();
        assert!(take_stop(&dir));
        assert!(!take_stop(&dir), "a stop is answered once");
        held.record_end("stopped", None, 0);
        drop(held);
        // a program another test starts at that moment has the lock's file for the moment between its fork and its exec
        crate::testkit::wait_until(
            std::time::Duration::from_secs(5),
            "the lock to be free",
            || running(&dir).is_none(),
        );
        assert!(!dir.join("run.json").exists());
        let said = status(&dir, 10);
        assert_eq!(said.running, None);
        assert_eq!(said.last.map(|l| l.outcome).as_deref(), Some("stopped"));
        hold(&dir, "again", false).expect("free again");
    }

    #[test]
    fn the_last_events_are_read_from_the_end_of_a_long_file() {
        let dir = scratch("run-events");
        let mut text = String::new();
        for i in 0..20_000 {
            text.push_str(&format!("{{\"event\":\"progress\",\"index\":{i}}}\n"));
        }
        std::fs::write(events_path(&dir), text).unwrap();
        let events = last_events(&dir, 3);
        let indexes: Vec<i64> = events
            .iter()
            .map(|e| e["index"].as_i64().unwrap())
            .collect();
        assert_eq!(indexes, [19_997, 19_998, 19_999]);
    }
}
