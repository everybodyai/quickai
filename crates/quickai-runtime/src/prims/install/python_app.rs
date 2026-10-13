//! `install.python_app@1`: installs an application written in Python from its source (ComfyUI), the
//! way a person would by hand and without any of the steps being left to them (docs/comfyui.md).
//!
//! ```text
//! python       a Python of the versions the document lists, found before anything is downloaded
//! source       the release's source archive, downloaded, checked against its pin, unpacked as app/;
//!              or, for an application installed from the package index, its wheel, downloaded and
//!              checked the same way and kept as wheel/<file> until the install is whole
//! nodes        the packs of extra nodes the document lists, each checked against its own pin
//! venv         <python> -m venv venv/
//! pip          each step of the document that applies to this machine, with --only-binary so that
//!              no package gets to run a build script of its own on the person's machine; the step
//!              of a wheel installs the file that was checked, and what it needs from the index
//! launcher     a script beside app/ and venv/ that runs the application with that environment: the
//!              build's executable, what the engine document starts and what its locate finds
//! check        the launcher's smoke test passes
//! rename       <label>.partial -> <label>, so a build is there whole or not at all
//! ```
//!
//! Each application has its own environment, inside its own build: nothing is installed into a
//! Python the person has, two applications never share one (ComfyUI's is 17 GB of PyTorch, Open
//! WebUI needs another Python), and the environment of the person's shell (`PYTHONPATH`, their user
//! site) is not what the application runs with.
//!
//! Everything happens in `<label>.partial/`, as for an archive (decision C26), and the pip steps put
//! their temporary files there too: the temporary directory of a machine can be memory (this one's
//! `/tmp` is), and a wheel of PyTorch is six gigabytes.
//!
//! An install takes twenty minutes, and the window it was started from can stop answering, be closed or
//! be killed in that time, so it can be gone on with (`journal.rs`): the directory keeps a journal of the
//! steps that are done, the next install of the same plan skips them, and one that is cancelled by the
//! person (`InstallSpec::cancel`) is the only one that throws its work away: the process group pip leads
//! is killed and nothing is left. One that fails after the environment was made keeps it.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use super::extract::{extract, ArchiveKind};
use super::journal::{self, Journal, StepRecord};
use super::{
    check_room_for, download, expected_digest, find_binary, rename_into_place, run_smoke,
    sweep_leftovers, InstallRequest, InstallSpec, Leftovers, Source, PART, PARTIAL,
};
use crate::hardware::{format_bytes, format_duration};
use crate::jobs::JobEvent;
use crate::runtime::process::{run_captured, without_package_loader_env, Daemon, RunFailure};

/// What an install stopped by the person says.
pub const CANCELLED: &str = "the install was cancelled";

/// What a `python_app` install does, worked out for this machine from the engine's document.
#[derive(Clone, Debug, Default)]
pub struct PythonApp {
    /// What the launcher runs.
    pub runs: Runs,
    /// The versions of Python to look for, best first (`3.13`).
    pub versions: Vec<String>,
    /// The Python the person named, which is used whatever its version (but not an older one than
    /// the document lists).
    pub python: Option<PathBuf>,
    pub pip: Vec<PipPlan>,
    pub nodes: Vec<NodePlan>,
    /// The free room an install needs, in bytes.
    pub space: Option<u64>,
}

/// What the launcher of a build runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Runs {
    /// A script of the unpacked source, relative to it (`main.py`).
    Script(String),
    /// The program a wheel puts in the environment (`open-webui`): the build is a wheel, installed.
    Program(String),
}

impl Default for Runs {
    fn default() -> Runs {
        Runs::Script(String::new())
    }
}

impl Runs {
    fn is_program(&self) -> bool {
        matches!(self, Runs::Program(_))
    }
}

/// One `pip install`.
#[derive(Clone, Debug)]
pub struct PipPlan {
    pub id: String,
    pub label: String,
    pub packages: Vec<String>,
    pub requirements: Option<String>,
    /// Installs the wheel the build is, the file that was downloaded and checked, with what it needs.
    pub wheel: bool,
    pub index: Option<String>,
    pub weight: u32,
}

/// One pack of nodes.
#[derive(Clone, Debug)]
pub struct NodePlan {
    pub id: String,
    pub label: String,
    pub url: String,
    pub sha256: String,
    pub into: String,
    pub requirements: Option<String>,
}

// ---- finding a Python ---------------------------------------------------------------------------------

/// A Python that was found and asked what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interpreter {
    pub program: PathBuf,
    /// Arguments that go before `-m venv` (the Windows launcher: `-3.13`).
    pub args: Vec<String>,
    pub version: (u32, u32),
}

impl Interpreter {
    pub fn describe(&self) -> String {
        format!(
            "Python {}.{} ({})",
            self.version.0,
            self.version.1,
            self.program.display()
        )
    }
}

/// Where Pythons are looked for: the places this machine keeps them.
#[derive(Clone, Debug, Default)]
pub struct Where {
    pub path_var: Option<OsString>,
    pub home: Option<PathBuf>,
}

impl Where {
    pub fn this_machine() -> Where {
        Where {
            path_var: std::env::var_os("PATH"),
            home: directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()),
        }
    }
}

/// `3.13` as numbers.
fn parse_version(text: &str) -> Option<(u32, u32)> {
    let (major, minor) = text.trim().split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The places a Python of this version may be, best first: the directories of the `PATH`, the
/// places the usual installers put one (the system's, Homebrew's, pyenv's, uv's), and the Windows
/// launcher. Only the names the machine has are returned.
fn candidates(version: &str, at: &Where) -> Vec<(PathBuf, Vec<String>)> {
    let mut found: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut add = |path: PathBuf, args: Vec<String>| {
        if !found.iter().any(|(p, a)| *p == path && *a == args) {
            found.push((path, args));
        }
    };
    let exe = |name: String| {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name
        }
    };
    let named = exe(format!("python{version}"));
    if let Some(path_var) = &at.path_var {
        for dir in std::env::split_paths(path_var) {
            add(dir.join(&named), Vec::new());
        }
    }
    if cfg!(unix) {
        for dir in [
            "/usr/bin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/opt/local/bin",
        ] {
            add(Path::new(dir).join(&named), Vec::new());
        }
    }
    if let Some(home) = &at.home {
        // pyenv: <home>/.pyenv/versions/3.13.7/bin/python
        for versions in [
            home.join(".pyenv").join("versions"),
            home.join(".pyenv").join("pyenv-win").join("versions"),
        ] {
            for dir in sub_dirs(&versions, |name| {
                name == version || name.starts_with(&format!("{version}."))
            }) {
                if cfg!(windows) {
                    add(dir.join("python.exe"), Vec::new());
                } else {
                    add(dir.join("bin").join("python"), Vec::new());
                }
            }
        }
        // uv: <home>/.local/share/uv/python/cpython-3.13.5-linux-x86_64-gnu/bin/python3
        for dir in sub_dirs(&home.join(".local/share/uv/python"), |name| {
            name.starts_with(&format!("cpython-{version}."))
        }) {
            add(dir.join("bin").join("python3"), Vec::new());
        }
    }
    if cfg!(windows) {
        add(PathBuf::from("py"), vec![format!("-{version}")]);
    }
    found
        .into_iter()
        .filter(|(program, _)| program.as_os_str() == "py" || program.is_file())
        .collect()
}

/// The directories below `dir` whose name `keep` accepts, newest names last in the list so that the
/// newest patch release of a version is the last one tried (names compare as text: 3.13.10 comes
/// before 3.13.9, which is the wrong way, so they are put in the order of their numbers).
fn sub_dirs(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<(Vec<u64>, PathBuf)> = read
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            keep(&name).then(|| {
                let numbers = name
                    .split(|c: char| !c.is_ascii_digit())
                    .filter_map(|part| part.parse().ok())
                    .collect();
                (numbers, entry.path())
            })
        })
        .collect();
    dirs.sort();
    dirs.reverse();
    dirs.into_iter().map(|(_, path)| path).collect()
}

/// Asks a Python what version it is. `Err` says why it is not usable.
fn ask_version(program: &Path, args: &[String]) -> Result<(u32, u32), String> {
    let mut command = Command::new(program);
    command.args(args).args([
        "-c",
        "import sys, venv; print(sys.version_info[0], sys.version_info[1])",
    ]);
    without_package_loader_env(&mut command);
    let ran =
        run_captured(&mut command, Duration::from_secs(20)).map_err(|failure| match failure {
            RunFailure::Start(e) => format!("could not be run: {e}"),
            RunFailure::TimedOut => "did not answer".to_string(),
            RunFailure::Wait(e) => format!("could not be waited for: {e}"),
            RunFailure::Stopped => "was stopped".to_string(),
        })?;
    if !ran.status.success() {
        let said = ran.stderr.trim().lines().last().unwrap_or("").to_string();
        return Err(format!("ended with {} ({said})", ran.status));
    }
    let mut numbers = ran.stdout.split_whitespace().map(|n| n.parse::<u32>());
    match (numbers.next(), numbers.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => Ok((major, minor)),
        _ => Err(format!(
            "said {:?}, which is not a version",
            ran.stdout.trim()
        )),
    }
}

/// The Python that makes the environment: the one the person named, else the first of `versions`
/// that the machine has. Nothing is downloaded or installed to find it.
pub fn find_python(
    versions: &[String],
    named: Option<&Path>,
    at: &Where,
) -> Result<Interpreter, String> {
    let oldest = versions
        .iter()
        .filter_map(|v| parse_version(v))
        .min()
        .unwrap_or((3, 10));
    if let Some(path) = named {
        let version = ask_version(path, &[])
            .map_err(|why| format!("the Python you chose ({}) {why}", path.display()))?;
        if version < oldest {
            return Err(format!(
                "the Python you chose ({}) is {}.{}, and this needs {}.{} or newer",
                path.display(),
                version.0,
                version.1,
                oldest.0,
                oldest.1
            ));
        }
        return Ok(Interpreter {
            program: path.to_path_buf(),
            args: Vec::new(),
            version,
        });
    }
    for wanted in versions {
        let Some(wanted_version) = parse_version(wanted) else {
            continue;
        };
        for (program, args) in candidates(wanted, at) {
            if ask_version(&program, &args) == Ok(wanted_version) {
                return Ok(Interpreter {
                    program,
                    args,
                    version: wanted_version,
                });
            }
        }
    }
    Err(format!(
        "no Python {} was found on this machine. Install one (on Debian and Ubuntu: sudo apt install python3.12-venv; on Fedora: sudo dnf install python3.12; on a Mac: brew install python@3.12) or choose the Python you have in the Python box of Settings",
        list_in_words(versions)
    ))
}

/// `3.13, 3.12 or 3.11`.
fn list_in_words(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

// ---- running pip ----------------------------------------------------------------------------------------

/// The Python of a virtual environment.
fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// Reads a pipe line by line into the channel until it ends.
fn pump<R: Read + Send + 'static>(pipe: Option<R>, tx: Sender<String>) {
    let Some(pipe) = pipe else { return };
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
}

/// `(6224.9 MB)` at the end of pip's `Downloading <address> (6224.9 MB)`: how many bytes it fetches.
fn announced_size(line: &str) -> Option<u64> {
    let inside = line.trim_end().strip_suffix(')')?.rsplit_once('(')?.1;
    let (number, unit) = inside.split_once(' ')?;
    let number: f64 = number.parse().ok()?;
    let scale = match unit {
        "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        _ => return None,
    };
    Some((number * scale) as u64)
}

/// The size of the largest file below `dir`: a wheel pip is fetching is the largest file in its
/// temporary directory, whatever else it keeps there.
fn largest_file(dir: &Path) -> u64 {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    read.flatten()
        .map(|entry| {
            let path = entry.path();
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => largest_file(&path),
                Ok(meta) => meta.len(),
                Err(_) => 0,
            }
        })
        .max()
        .unwrap_or(0)
}

/// What the person's lines of pip's output are, kept to show why it failed.
const TAIL: usize = 14;

/// How long pip may say nothing, with no file growing, before the line under the bar says so.
const QUIET_AFTER: Duration = Duration::from_secs(45);

/// What pip is doing, as far as its words and the files it writes tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// Looking up the packages and what they need.
    Looking,
    /// Fetching a wheel.
    Fetching,
    /// Putting what was fetched into the environment: for PyTorch minutes in which pip says nothing.
    Unpacking,
}

/// What the line under the bar says while pip works (`pip_detail`).
pub(super) struct PipView<'a> {
    /// `Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2)`
    pub step: &'a str,
    pub phase: Phase,
    /// The wheel that is being fetched.
    pub file: Option<&'a str>,
    pub fetched: u64,
    pub total: Option<u64>,
    /// Bytes a second, once there is enough to tell.
    pub speed: Option<f64>,
    /// How long it has been in this phase.
    pub in_phase: Duration,
    /// How long pip has said nothing and no file has grown.
    pub quiet: Duration,
}

/// The line under the bar while pip works: the step, what pip is doing and, so that a long wait can be
/// told from a stuck one, how far a download is, how fast, and how long nothing has happened.
pub(super) fn pip_detail(view: &PipView) -> String {
    let mut text = match view.phase {
        Phase::Looking => format!("{} · looking up the packages", view.step),
        Phase::Fetching => {
            let file = view.file.unwrap_or("a package");
            let mut text = match view.total {
                Some(total) => format!(
                    "{} · downloading {file}: {} of {}",
                    view.step,
                    format_bytes(view.fetched.min(total)),
                    format_bytes(total)
                ),
                None => format!(
                    "{} · downloading {file}: {}",
                    view.step,
                    format_bytes(view.fetched)
                ),
            };
            if let Some(speed) = view.speed.filter(|speed| *speed >= 1.0) {
                text.push_str(&format!(", {}/s", format_bytes(speed as u64)));
                if let Some(total) = view.total.filter(|total| *total > view.fetched) {
                    let left = Duration::from_secs(((total - view.fetched) as f64 / speed) as u64);
                    text.push_str(&format!(", about {} left", format_duration(left)));
                }
            }
            text
        }
        Phase::Unpacking => format!(
            "{} · putting what it fetched into the environment, which takes minutes · {}",
            view.step,
            format_duration(view.in_phase)
        ),
    };
    if view.quiet >= QUIET_AFTER {
        text.push_str(&format!(
            " · nothing new for {}, pip is still running",
            format_duration(view.quiet)
        ));
    }
    text
}

/// The wheel a `Downloading <address> (<size>)` line of pip is about and how big it says it is. The
/// small files that tell what a wheel needs (`....whl.metadata`) are not wheels.
fn wheel_of(line: &str) -> Option<(String, Option<u64>)> {
    let address = line.trim_start().strip_prefix("Downloading ")?;
    let name = address.split_whitespace().next()?.rsplit('/').next()?;
    let name = name.split(['#', '?']).next()?;
    if name.ends_with(".metadata") || name.is_empty() {
        return None;
    }
    Some((
        name.replace("%2B", "+").replace("%2b", "+"),
        announced_size(line),
    ))
}

/// How fast a file grows, smoothed over the last few seconds.
struct Speed {
    at: Instant,
    bytes: u64,
    per_second: Option<f64>,
}

impl Speed {
    fn new() -> Speed {
        Speed {
            at: Instant::now(),
            bytes: 0,
            per_second: None,
        }
    }

    /// A new file begins.
    fn reset(&mut self) {
        *self = Speed::new();
    }

    fn sample(&mut self, bytes: u64) {
        let seconds = self.at.elapsed().as_secs_f64();
        if seconds < 0.4 {
            return;
        }
        if bytes >= self.bytes {
            let now = (bytes - self.bytes) as f64 / seconds;
            self.per_second = Some(match self.per_second {
                Some(before) => 0.6 * before + 0.4 * now,
                None => now,
            });
        }
        self.at = Instant::now();
        self.bytes = bytes;
    }
}

/// What the lines of pip so far say it is doing.
struct Watch {
    phase: Phase,
    phase_began: Instant,
    file: Option<String>,
    announced: Option<u64>,
    /// The size of the file it fetches when last looked at.
    seen: u64,
    /// When pip last said something or a file last grew.
    last_news: Instant,
    tail: VecDeque<String>,
}

impl Watch {
    fn new() -> Watch {
        let now = Instant::now();
        Watch {
            phase: Phase::Looking,
            phase_began: now,
            file: None,
            announced: None,
            seen: 0,
            last_news: now,
            tail: VecDeque::new(),
        }
    }

    /// One line of pip's: passed on, remembered to show why it failed, and read for what it is doing.
    fn line(
        &mut self,
        line: String,
        speed: &mut Speed,
        say: &dyn Fn(String),
        fraction: &dyn Fn(f32),
    ) {
        let line = line.trim_end().to_string();
        if line.trim().is_empty() {
            return;
        }
        let now = Instant::now();
        self.last_news = now;
        // pip indents what it fetches under the package that asked for it
        let words = line.trim_start();
        if let Some((file, size)) = wheel_of(words) {
            self.phase = Phase::Fetching;
            self.phase_began = now;
            self.file = Some(file);
            self.announced = size;
            self.seen = 0;
            speed.reset();
            fraction(0.02);
        } else if words.starts_with("Installing collected packages") {
            self.phase = Phase::Unpacking;
            self.phase_began = now;
            self.announced = None;
            fraction(0.92);
        } else if words.starts_with("Successfully installed") {
            fraction(1.0);
        }
        if self.tail.len() == TAIL {
            self.tail.pop_front();
        }
        self.tail.push_back(line.clone());
        say(line);
    }
}

/// One `pip install`, as `run_pip` is asked for it.
struct PipRun<'a> {
    spec: &'a InstallSpec,
    python: &'a Path,
    args: &'a [String],
    temporary: &'a Path,
    /// `Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2)`
    step: &'a str,
}

/// Runs `python -m pip install <args>` of the environment in its own process group, passing what it
/// says on to `say`, how far a download it announced has got to `fraction` (0 to 1) and what it is doing
/// to `detail`. Stops it, and everything it started, when `spec.cancel` is set.
fn run_pip(
    run: &PipRun,
    say: &dyn Fn(String),
    fraction: &dyn Fn(f32),
    detail: &dyn Fn(String),
) -> Result<(), String> {
    let mut command = Command::new(run.python);
    command
        .args([
            "-m",
            "pip",
            "install",
            "--no-input",
            "--disable-pip-version-check",
            "--only-binary=:all:",
            "--progress-bar",
            "off",
        ])
        .args(run.args)
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONNOUSERSITE", "1")
        .env("PIP_USER", "0")
        .env("TMPDIR", run.temporary)
        .env("TEMP", run.temporary)
        .env("TMP", run.temporary);
    // pip keeps a copy of every wheel it fetches in its cache: another six gigabytes of PyTorch next
    // to the environment it was installed into, for a person who did not ask for a cache
    if std::env::var_os("PIP_CACHE_DIR").is_none() {
        command.arg("--no-cache-dir");
    }
    without_package_loader_env(&mut command);
    let mut daemon = Daemon::start(command, true).map_err(|e| {
        format!(
            "could not run pip with {}: {e}",
            run.python.file_name().unwrap_or_default().to_string_lossy()
        )
    })?;
    let (out, err) = daemon.take_output();
    let (tx, rx): (Sender<String>, Receiver<String>) = crossbeam_channel::unbounded();
    pump(out, tx.clone());
    pump(err, tx);

    let mut watch = Watch::new();
    let mut speed = Speed::new();
    let mut polled = Instant::now();
    let status = loop {
        if run.spec.cancel.load(Ordering::SeqCst) {
            daemon.kill();
            let _ = daemon.wait();
            return Err(CANCELLED.to_string());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => watch.line(line, &mut speed, say, fraction),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                break daemon.wait().map_err(|e| format!("pip: {e}"))?;
            }
        }
        if polled.elapsed() >= Duration::from_millis(500) {
            polled = Instant::now();
            // a wheel that is being fetched is the largest file of the temporary directory
            if watch.phase == Phase::Fetching {
                let seen = largest_file(run.temporary);
                if seen > watch.seen {
                    watch.last_news = polled;
                }
                watch.seen = seen;
                speed.sample(seen);
                if let Some(total) = watch.announced {
                    fraction(0.02 + 0.88 * (seen.min(total) as f32 / total.max(1) as f32));
                }
            }
            detail(pip_detail(&PipView {
                step: run.step,
                phase: watch.phase,
                file: watch.file.as_deref(),
                fetched: watch.seen,
                total: watch.announced,
                speed: speed.per_second,
                in_phase: watch.phase_began.elapsed(),
                quiet: watch.last_news.elapsed(),
            }));
        }
        if let Ok(Some(status)) = daemon.try_wait() {
            // what it said last
            std::thread::sleep(Duration::from_millis(50));
            while let Ok(line) = rx.try_recv() {
                watch.line(line, &mut speed, say, fraction);
            }
            break status;
        }
    };
    daemon.sweep();
    if status.success() {
        return Ok(());
    }
    let said = watch.tail.iter().cloned().collect::<Vec<_>>().join("\n");
    Err(format!("pip ended with {status}:\n{said}"))
}

// ---- the install --------------------------------------------------------------------------------------------

/// How far along an install is, as the bar shows it: each step is a share of it.
struct Bar<'a> {
    send: &'a dyn Fn(JobEvent<PathBuf>),
    total: f32,
    done: f32,
}

impl Bar<'_> {
    /// Inside the step that has this much weight, `within` (0 to 1) of it is done.
    fn show(&self, weight: f32, within: f32) {
        let value = (self.done + weight * within.clamp(0.0, 1.0)) / self.total;
        (self.send)(JobEvent::Fraction(value.clamp(0.0, 0.99)));
    }

    fn finished(&mut self, weight: f32) {
        self.done += weight;
        self.show(0.0, 0.0);
    }
}

/// A step of the install ends early when the person has cancelled it.
fn check_cancelled(spec: &InstallSpec) -> Result<(), String> {
    if spec.cancel.load(Ordering::SeqCst) {
        Err(CANCELLED.to_string())
    } else {
        Ok(())
    }
}

/// Takes the single directory an archive of source has at its top (`ComfyUI-0.39.0/`) out of
/// `unpacked` and puts it at `place`.
fn move_top_directory(unpacked: &Path, place: &Path) -> Result<(), String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(unpacked)
        .map_err(|e| format!("could not read {}: {e}", unpacked.display()))?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    let [top] = entries.as_slice() else {
        return Err(
            "the archive does not have the shape of a source archive: it should hold one folder"
                .to_string(),
        );
    };
    if !top.is_dir() {
        return Err("the archive does not have the shape of a source archive".to_string());
    }
    if let Some(parent) = place.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::rename(top, place)
        .map_err(|e| format!("could not move the source to {}: {e}", place.display()))?;
    let _ = std::fs::remove_dir_all(unpacked);
    Ok(())
}

/// Downloads `source` into `part`, and checks it against `expected`.
fn fetch_checked(
    spec: &InstallSpec,
    source: &Source,
    expected: Option<&str>,
    what: &str,
    part: &Path,
    send: &dyn Fn(JobEvent<PathBuf>),
) -> Result<(), String> {
    send(JobEvent::Status(format!("Downloading {what}")));
    let fetched = download(&source.url, part, 0, source.size, &spec.cancel, &|event| {
        // the bar of an install that has steps is the steps': a download is told in the log
        if !matches!(event, JobEvent::Progress { .. }) {
            send(event);
        }
    })?;
    match expected {
        Some(want) if want != fetched.sha256 => {
            return Err(format!(
                "the download of {what} does not match its checksum: its sha256 is {}… but {}… was expected (it is damaged, or was changed on the way)",
                &fetched.sha256[..12],
                &want[..12]
            ));
        }
        Some(want) => send(JobEvent::Status(format!(
            "Checked {what} against its sha256 ({}…)",
            &want[..12]
        ))),
        None if spec.verify == quickai_core::doc::engine::Verify::Digest => {
            send(JobEvent::Status(format!(
                "{what} is not verified: no checksum is known for it (it is not the release this was tested with), so it is installed as it came"
            )))
        }
        None => {}
    }
    check_cancelled(spec)
}

/// The file name of the wheel a URL is the address of: the last part of its path, which pip needs to be
/// the file's own (it reads the tags from it).
fn wheel_file_name(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.trim_end_matches('/').rsplit('/').next()?;
    (name.ends_with(".whl")
        && name.len() > ".whl".len()
        && !name.contains(['\\', ':'])
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '+')))
    .then(|| name.to_string())
}

/// Downloads the wheel of the build and checks it, and puts it in `wheel/` of the staging directory.
#[allow(clippy::too_many_arguments)]
fn fetch_wheel(
    spec: &InstallSpec,
    source: &Source,
    expected: Option<&str>,
    what: &str,
    root: &Path,
    staging: &Path,
    leftovers: &mut Leftovers,
    send: &dyn Fn(JobEvent<PathBuf>),
) -> Result<PathBuf, String> {
    let name = wheel_file_name(&source.url).ok_or_else(|| {
        format!(
            "{} is not the address of a wheel: its name should end in .whl",
            source.url
        )
    })?;
    let part = root.join(format!("{what}.whl{PART}"));
    leftovers.paths.push(part.clone());
    fetch_checked(spec, source, expected, what, &part, send)?;
    let dir = staging.join("wheel");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let place = dir.join(&name);
    std::fs::rename(&part, &place)
        .or_else(|_| std::fs::copy(&part, &place).map(|_| ()))
        .map_err(|e| format!("could not keep the wheel in {}: {e}", place.display()))?;
    let _ = std::fs::remove_file(&part);
    Ok(place)
}

/// Downloads `source`, checks it against `expected`, unpacks it and puts its top folder at `place`.
#[allow(clippy::too_many_arguments)]
fn fetch_and_unpack(
    spec: &InstallSpec,
    source: &Source,
    expected: Option<&str>,
    what: &str,
    root: &Path,
    staging: &Path,
    leftovers: &mut Leftovers,
    place: &Path,
    send: &dyn Fn(JobEvent<PathBuf>),
) -> Result<(), String> {
    let kind = ArchiveKind::from_url(&source.url).ok_or_else(|| {
        format!(
            "don't know how to extract {} ({})",
            source.url,
            ArchiveKind::EXPECTED
        )
    })?;
    let part = root.join(format!("{what}.{}{PART}", kind.extension()));
    leftovers.paths.push(part.clone());
    fetch_checked(spec, source, expected, what, &part, send)?;
    let unpacked = staging.join(format!("{what}.unpacked"));
    // what an install that was stopped in the middle of this left
    let _ = std::fs::remove_dir_all(&unpacked);
    std::fs::create_dir_all(&unpacked)
        .map_err(|e| format!("could not create {}: {e}", unpacked.display()))?;
    send(JobEvent::Status(format!("Unpacking {what}")));
    extract(kind, &part, &unpacked, &[])?;
    let _ = std::fs::remove_file(&part);
    move_top_directory(&unpacked, place)
}

/// What a console script of an installed wheel runs: `open_webui:app` is the name `app` in the module
/// `open_webui`, called with no arguments and its result the exit status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EntryPoint {
    /// The module to import (`open_webui`).
    pub module: String,
    /// What to call in it, possibly with dots (`app`, `cli.main`).
    pub attr: String,
}

/// What the environment's Python is asked to find the entry point of a console script: the first argument is
/// the script's name, and it prints what the package's records say it is (`module:attr`), or nothing.
const ENTRY_POINT_QUERY: &str = "import sys; from importlib.metadata import entry_points as e; print(next((x.value for x in e(group='console_scripts') if x.name == sys.argv[1]), ''))";

/// `module:attr` (with, perhaps, ` [extras]` after it) as an entry point; `None` for anything that is not a name
/// of a module and a name in it. Only such names go into a launcher, which is written from what a package says.
pub(super) fn parse_entry_point(text: &str) -> Option<EntryPoint> {
    let text = text.split('[').next().unwrap_or_default().trim();
    let (module, attr) = text.split_once(':')?;
    let name = |part: &str| {
        !part.is_empty()
            && part.split('.').all(|piece| {
                let mut chars = piece.chars();
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    };
    (name(module) && name(attr)).then(|| EntryPoint {
        module: module.to_string(),
        attr: attr.to_string(),
    })
}

/// Asks the environment's own Python what the console script `program` runs.
///
/// A launcher that ran the script itself would not survive the install: pip writes into a script the path of the
/// Python it was installed with (`#!/…/<label>.partial/venv/bin/python`), and a build is moved to its name when it is
/// whole, so the script would look for a Python that is no longer there (found by running the real thing, T90). The
/// Python of the environment, which finds its environment from where it is, is moved without harm: that is what the
/// launcher runs, and it is told which function to call.
fn resolve_entry_point(python: &Path, program: &str) -> Result<EntryPoint, String> {
    let mut command = Command::new(python);
    command
        .args(["-c", ENTRY_POINT_QUERY, program])
        .env("PYTHONNOUSERSITE", "1");
    without_package_loader_env(&mut command);
    let ran =
        run_captured(&mut command, Duration::from_secs(60)).map_err(|failure| match failure {
            RunFailure::Start(e) => format!("could not run the environment's Python: {e}"),
            RunFailure::TimedOut => {
                "the environment's Python did not answer in a minute".to_string()
            }
            RunFailure::Wait(e) => format!("could not wait for the environment's Python: {e}"),
            RunFailure::Stopped => CANCELLED.to_string(),
        })?;
    if !ran.status.success() {
        return Err(format!(
            "the environment's Python could not look for `{program}` ({}): {}",
            ran.status,
            ran.stderr.trim().chars().take(300).collect::<String>()
        ));
    }
    parse_entry_point(ran.stdout.trim()).ok_or_else(|| {
        format!(
            "the package that was installed has no program called `{program}` (its records say {:?})",
            ran.stdout.trim()
        )
    })
}

/// The file name and the text of the launcher, for Windows (`name.cmd`, a batch file) or any other system (`name`, a
/// shell script). A pure function, so that both are tested on every platform.
fn launcher_text(
    name: &str,
    runs: &Runs,
    entry: Option<&EntryPoint>,
    windows: bool,
) -> (String, String) {
    // the program of a wheel is run by the environment's Python, told what to call (see `resolve_entry_point`)
    let program = |program: &str| -> String {
        let entry =
            entry.expect("the entry point of a program is found before its launcher is written");
        let (first, _) = entry.attr.split_once('.').unwrap_or((&entry.attr, ""));
        format!(
            "import sys; from {} import {first}; sys.argv[0] = '{program}'; sys.exit({}())",
            entry.module, entry.attr
        )
    };
    if windows {
        let run = match runs {
            Runs::Script(entry) => format!(
                "\"%~dp0venv\\Scripts\\python.exe\" \"%~dp0app\\{}\" %*",
                entry.replace('/', "\\")
            ),
            Runs::Program(name) => format!(
                "\"%~dp0venv\\Scripts\\python.exe\" -c \"{}\" %*",
                program(name)
            ),
        };
        (
            format!("{name}.cmd"),
            format!(
                "@echo off\r\nset PYTHONPATH=\r\nset PYTHONHOME=\r\nset PYTHONSTARTUP=\r\nset PYTHONUSERBASE=\r\nset VIRTUAL_ENV=\r\nset PYTHONNOUSERSITE=1\r\n{run}\r\n"
            ),
        )
    } else {
        let run = match runs {
            Runs::Script(entry) => {
                format!("exec \"$here/venv/bin/python\" \"$here/app/{entry}\" \"$@\"")
            }
            Runs::Program(name) => {
                format!(
                    "exec \"$here/venv/bin/python\" -c \"{}\" \"$@\"",
                    program(name)
                )
            }
        };
        (
            name.to_string(),
            format!(
                "#!/bin/sh\n# Written by QuickAI: runs the application with the Python environment installed beside it.\nhere=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd -P)\n# the person's own Python variables are not the application's\nunset PYTHONPATH PYTHONHOME PYTHONSTARTUP PYTHONUSERBASE VIRTUAL_ENV\nPYTHONNOUSERSITE=1\nexport PYTHONNOUSERSITE\n{run}\n"
            ),
        )
    }
}

/// The launcher: a script that runs the application with the environment beside it.
///
/// The Python variables of the person's shell are put away first: a `PYTHONPATH` that is theirs would put
/// their packages in front of the environment's, and their user site is a place the environment is not
/// to read.
fn write_launcher(
    staging: &Path,
    name: &str,
    runs: &Runs,
    entry: Option<&EntryPoint>,
) -> Result<PathBuf, String> {
    let (file, text) = launcher_text(name, runs, entry, cfg!(windows));
    let path = staging.join(file);
    std::fs::write(&path, text).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("could not make {} executable: {e}", path.display()))?;
    }
    Ok(path)
}

// ---- the steps, and the record of which are done --------------------------------------------------------------------

/// The steps of the install in the order they are done: what the journal records, and what the line
/// under the bar counts.
fn plan_steps(app: &PythonApp, request: &InstallRequest, check: bool) -> Vec<StepRecord> {
    let step = |id: String, label: String| StepRecord { id, label };
    let mut steps = vec![step(
        "source".into(),
        if app.runs.is_program() {
            format!("the package of {}", request.label)
        } else {
            format!("the source of {}", request.label)
        },
    )];
    for pack in &app.nodes {
        steps.push(step(format!("node:{}", pack.id), pack.label.clone()));
    }
    steps.push(step("venv".into(), "the Python environment".into()));
    for pip in &app.pip {
        steps.push(step(format!("pip:{}", pip.id), pip.label.clone()));
    }
    for pack in app.nodes.iter().filter(|n| n.requirements.is_some()) {
        steps.push(step(
            format!("needs:{}", pack.id),
            format!("what {} needs", pack.label),
        ));
    }
    if check {
        steps.push(step("check".into(), "the check that it runs".into()));
    }
    steps
}

/// What the install is made for: the build, where its source is, which Python, and every package and pack.
/// An install that stopped is gone on with only if the one now asked for has the same.
fn plan_fingerprint(app: &PythonApp, request: &InstallRequest, python: &Interpreter) -> String {
    let mut parts = vec![
        request.label.clone(),
        request.base.url.clone(),
        format!("{:?}", app.runs),
        python.program.display().to_string(),
        format!("{}.{}", python.version.0, python.version.1),
    ];
    for step in &app.pip {
        parts.push(format!(
            "pip|{}|{}|{}|{}|{}",
            step.id,
            step.index.as_deref().unwrap_or(""),
            step.packages.join(" "),
            step.requirements.as_deref().unwrap_or(""),
            step.wheel
        ));
    }
    for pack in &app.nodes {
        parts.push(format!(
            "node|{}|{}|{}|{}|{}",
            pack.id,
            pack.url,
            pack.sha256,
            pack.into,
            pack.requirements.as_deref().unwrap_or("")
        ));
    }
    journal::fingerprint(&parts)
}

/// The directory of an install of `label` that stopped, under whatever name it has.
fn earlier_install(spec: &InstallSpec, label: &str) -> Option<PathBuf> {
    let staging = spec.root.join(format!("{label}{PARTIAL}"));
    if journal::is_unfinished(&staging) {
        return Some(staging);
    }
    journal::unfinished_named(&spec.root, label, &staging)
}

/// The staging directory and the right to work in it. It goes when this does, unless the install says to
/// keep it: a install that is cancelled, or fails before the environment is made, leaves nothing; the
/// lock is let go first, since a directory with a file open in it cannot be removed on Windows.
struct Staging {
    path: PathBuf,
    claim: Option<journal::Claim>,
    keep: bool,
}

impl Staging {
    fn release(&mut self) {
        self.claim = None;
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        self.claim = None;
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// The journal of the install and where it is kept.
struct Steps {
    dir: PathBuf,
    journal: Journal,
}

impl Steps {
    /// Whether `id` is done and what it made is still there. A step whose work is gone is not done, and
    /// neither is any that came after it, since those were made on top of it.
    fn done(&mut self, id: &str, there: impl FnOnce() -> bool) -> bool {
        if !self.journal.is_done(id) {
            return false;
        }
        if there() {
            return true;
        }
        self.journal.undo_from(id);
        let _ = self.journal.write(&self.dir);
        false
    }

    fn finish(&mut self, id: &str) -> Result<(), String> {
        self.journal.mark(id);
        self.journal.write(&self.dir)
    }

    fn label(&self, id: &str) -> String {
        self.journal
            .steps
            .iter()
            .find(|s| s.id == id)
            .map_or_else(|| id.to_string(), |s| s.label.clone())
    }

    /// `Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2)`
    fn title(&self, id: &str) -> String {
        let number = self
            .journal
            .steps
            .iter()
            .position(|s| s.id == id)
            .map_or(0, |at| at + 1);
        format!(
            "Step {number} of {} · {}",
            self.journal.steps.len(),
            self.label(id)
        )
    }
}

/// What the steps need to know about the install they are part of.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    spec: &'a InstallSpec,
    app: &'a PythonApp,
    request: &'a InstallRequest,
    send: &'a dyn Fn(JobEvent<PathBuf>),
    staging: &'a Path,
    expected: Option<&'a str>,
    python: &'a Interpreter,
}

/// Every step from the source to the check, skipping those the journal says are done.
fn work(ctx: Ctx, steps: &mut Steps, leftovers: &mut Leftovers) -> Result<(), String> {
    let Ctx {
        spec,
        app,
        request,
        send,
        staging,
        expected,
        python,
    } = ctx;
    let say = |text: String| send(JobEvent::Status(text));
    let before = |steps: &Steps, id: &str| {
        say(format!(
            "Already done by the install that stopped: {}",
            steps.label(id)
        ))
    };
    let temporary = staging.join("tmp");
    let app_dir = staging.join("app");
    let venv = staging.join("venv");

    // how big a share of the bar each step is
    let weight_of = |plan: &PipPlan| plan.weight.max(1) as f32;
    let node_steps = app
        .nodes
        .iter()
        .filter(|n| n.requirements.is_some())
        .count() as f32;
    let (source_weight, node_weight, venv_weight, smoke_weight) = (3.0, 1.0, 2.0, 3.0);
    let mut bar = Bar {
        send,
        total: source_weight
            + node_weight * app.nodes.len() as f32
            + venv_weight
            + app.pip.iter().map(weight_of).sum::<f32>()
            + node_steps
            + smoke_weight,
        done: 0.0,
    };
    bar.show(0.0, 0.0);

    // the source: the unpacked archive of a release, or the wheel that is the build
    let wheel_there =
        wheel_file_name(&request.base.url).map(|name| staging.join("wheel").join(name));
    match &app.runs {
        Runs::Script(entry) => {
            if steps.done("source", || app_dir.join(entry).is_file()) {
                before(steps, "source");
            } else {
                check_cancelled(spec)?;
                send(JobEvent::Detail(steps.title("source")));
                let _ = std::fs::remove_dir_all(&app_dir);
                fetch_and_unpack(
                    spec,
                    &request.base,
                    expected,
                    &request.label,
                    &spec.root,
                    staging,
                    leftovers,
                    &app_dir,
                    send,
                )?;
                steps.finish("source")?;
            }
        }
        Runs::Program(_) => {
            if steps.done("source", || {
                wheel_there.as_ref().is_some_and(|w| w.is_file())
            }) {
                before(steps, "source");
            } else {
                check_cancelled(spec)?;
                send(JobEvent::Detail(steps.title("source")));
                fetch_wheel(
                    spec,
                    &request.base,
                    expected,
                    &request.label,
                    &spec.root,
                    staging,
                    leftovers,
                    send,
                )?;
                steps.finish("source")?;
            }
        }
    }
    bar.finished(source_weight);

    // the nodes
    for pack in &app.nodes {
        let id = format!("node:{}", pack.id);
        let place = app_dir.join(&pack.into);
        if steps.done(&id, || place.is_dir()) {
            before(steps, &id);
        } else {
            check_cancelled(spec)?;
            send(JobEvent::Detail(steps.title(&id)));
            let source = Source {
                url: pack.url.clone(),
                host_digest: None,
                pin: Some(pack.sha256.clone()),
                size: None,
            };
            let want = pack
                .sha256
                .trim()
                .trim_start_matches("sha256:")
                .to_ascii_lowercase();
            let _ = std::fs::remove_dir_all(&place);
            fetch_and_unpack(
                spec,
                &source,
                Some(&want),
                &pack.label,
                &spec.root,
                staging,
                leftovers,
                &place,
                send,
            )?;
            steps.finish(&id)?;
        }
        bar.finished(node_weight);
    }

    // the environment
    check_cancelled(spec)?;
    let inside = venv_python(&venv);
    if steps.done("venv", || inside.is_file()) {
        before(steps, "venv");
    } else {
        send(JobEvent::Status(
            "Making the Python environment".to_string(),
        ));
        send(JobEvent::Detail(steps.title("venv")));
        let _ = std::fs::remove_dir_all(&venv);
        let mut make = Command::new(&python.program);
        make.args(&python.args).args(["-m", "venv"]).arg(&venv);
        without_package_loader_env(&mut make);
        let made =
            run_captured(&mut make, Duration::from_secs(300)).map_err(|failure| match failure {
                RunFailure::Start(e) => format!("could not run {}: {e}", python.program.display()),
                RunFailure::TimedOut => {
                    "making the Python environment took more than five minutes".into()
                }
                RunFailure::Wait(e) => {
                    format!("could not wait for {}: {e}", python.program.display())
                }
                RunFailure::Stopped => CANCELLED.to_string(),
            })?;
        if !made.status.success() {
            let said = format!("{}\n{}", made.stdout.trim(), made.stderr.trim());
            return Err(format!(
                "{} could not make a Python environment ({}): {}. On Debian and Ubuntu the package python3-venv has to be installed",
                python.describe(),
                made.status,
                said.trim().chars().take(400).collect::<String>()
            ));
        }
        steps.finish("venv")?;
    }
    bar.finished(venv_weight);

    // the packages
    let in_source = |relative: &str| app_dir.join(relative).display().to_string();
    for step in &app.pip {
        check_cancelled(spec)?;
        let id = format!("pip:{}", step.id);
        let weight = weight_of(step);
        if steps.done(&id, || true) {
            before(steps, &id);
            bar.finished(weight);
            continue;
        }
        send(JobEvent::Status(format!("Installing {}", step.label)));
        let mut args: Vec<String> = Vec::new();
        if let Some(index) = &step.index {
            args.extend(["--index-url".to_string(), index.clone()]);
        }
        if let Some(file) = &step.requirements {
            args.extend(["-r".to_string(), in_source(file)]);
        }
        if step.wheel {
            // the file that was checked, by its path: pip takes what it needs from the index
            let Some(path) = wheel_there.as_ref().filter(|w| w.is_file()) else {
                return Err("the wheel of the build is not in the install's folder".to_string());
            };
            args.push(path.display().to_string());
        }
        args.extend(step.packages.iter().cloned());
        let title = steps.title(&id);
        send(JobEvent::Detail(title.clone()));
        let bar_ref = &bar;
        run_pip(
            &PipRun {
                spec,
                python: &inside,
                args: &args,
                temporary: &temporary,
                step: &title,
            },
            &|line| send(JobEvent::Status(line)),
            &|within| bar_ref.show(weight, within),
            &|text| send(JobEvent::Detail(text)),
        )?;
        steps.finish(&id)?;
        bar.finished(weight);
    }
    for pack in &app.nodes {
        let Some(file) = &pack.requirements else {
            continue;
        };
        check_cancelled(spec)?;
        let id = format!("needs:{}", pack.id);
        if steps.done(&id, || true) {
            before(steps, &id);
            bar.finished(1.0);
            continue;
        }
        send(JobEvent::Status(format!(
            "Installing what {} needs",
            pack.label
        )));
        let args = vec![
            "-r".to_string(),
            app_dir.join(&pack.into).join(file).display().to_string(),
        ];
        let title = steps.title(&id);
        send(JobEvent::Detail(title.clone()));
        let bar_ref = &bar;
        run_pip(
            &PipRun {
                spec,
                python: &inside,
                args: &args,
                temporary: &temporary,
                step: &title,
            },
            &|line| send(JobEvent::Status(line)),
            &|within| bar_ref.show(1.0, within),
            &|text| send(JobEvent::Detail(text)),
        )?;
        steps.finish(&id)?;
        bar.finished(1.0);
    }

    // the launcher, and whether it runs: every time, since it is quick and it is what counts
    check_cancelled(spec)?;
    let launcher_name = spec.binary.trim_end_matches(".cmd");
    let entry = match &app.runs {
        Runs::Program(program) => Some(resolve_entry_point(&inside, program)?),
        Runs::Script(_) => None,
    };
    let launcher = write_launcher(staging, launcher_name, &app.runs, entry.as_ref())?;
    if let Some(smoke) = &spec.smoke {
        send(JobEvent::Status("Checking that it runs".to_string()));
        send(JobEvent::Detail(format!(
            "{} (it loads every node; the first time can take a minute)",
            steps.title("check")
        )));
        run_smoke(&launcher, smoke, &spec.cancel)?;
    }
    bar.finished(smoke_weight);
    let _ = std::fs::remove_dir_all(&temporary);
    // the wheel is installed and the build runs: a copy of it beside the environment is only room. It
    // stays until here so that an install that stops after the packages (the check) goes on from there
    let _ = std::fs::remove_dir_all(staging.join("wheel"));
    Ok(())
}

/// The install of a `python_app`: what `install_blocking` does for one (see the module).
pub(super) fn install(
    spec: &InstallSpec,
    app: &PythonApp,
    request: &InstallRequest,
    send: &dyn Fn(JobEvent<PathBuf>),
) -> Result<PathBuf, String> {
    let final_dir = spec.install_dir(&request.label);
    if !journal::is_unfinished(&final_dir) {
        if let Some(existing) = find_binary(&final_dir, &spec.binary, spec.search_depth) {
            return Ok(existing);
        }
    }

    // everything that can be known without the network is settled first
    if app.runs.is_program() {
        if wheel_file_name(&request.base.url).is_none() {
            return Err(format!(
                "{} is not the address of a wheel: its name should end in .whl",
                request.base.url
            ));
        }
    } else if ArchiveKind::from_url(&request.base.url).is_none() {
        return Err(format!(
            "don't know how to extract {} ({})",
            request.base.url,
            ArchiveKind::EXPECTED
        ));
    }
    for url in app.nodes.iter().map(|n| &n.url) {
        if ArchiveKind::from_url(url).is_none() {
            return Err(format!(
                "don't know how to extract {url} ({})",
                ArchiveKind::EXPECTED
            ));
        }
    }
    let expected = expected_digest(spec, &request.base)?;
    // what an install that stopped wrote already counts against the room this one needs
    let earlier = earlier_install(spec, &request.label);
    let written = earlier.as_deref().map_or(0, super::dir_size);
    check_room_for(spec, app.space.map(|need| need.saturating_sub(written)))?;
    let python = find_python(&app.versions, app.python.as_deref(), &Where::this_machine())?;
    send(JobEvent::Status(format!("Using {}", python.describe())));
    check_cancelled(spec)?;

    std::fs::create_dir_all(&spec.root)
        .map_err(|e| format!("could not create {}: {e}", spec.root.display()))?;
    sweep_leftovers(&spec.root);
    let staging_path = spec.root.join(format!("{}{PARTIAL}", request.label));
    // an install that stopped and has another name (the directory was renamed by hand, or it was in
    // place when it stopped) is the staging directory again
    if let Some(other) = earlier.filter(|dir| *dir != staging_path) {
        if !staging_path.exists() {
            std::fs::rename(&other, &staging_path).map_err(|e| {
                format!(
                    "could not take up the install that stopped in {}: {e}",
                    other.display()
                )
            })?;
        }
    }

    let steps_plan = plan_steps(app, request, spec.smoke.is_some());
    let fingerprint = plan_fingerprint(app, request, &python);
    let busy = || {
        format!(
            "{} is being installed by another window of QuickAI: wait for it, or close that window",
            request.label
        )
    };
    let existed = staging_path.is_dir();
    if !existed {
        std::fs::create_dir(&staging_path)
            .map_err(|e| format!("could not create {}: {e}", staging_path.display()))?;
    }
    let claim = match journal::claim(&staging_path) {
        Ok(claim) => claim,
        Err(_) => {
            if !existed {
                let _ = std::fs::remove_dir_all(&staging_path);
            }
            return Err(busy());
        }
    };
    // what is there is kept unless the install says to throw it away: a record that cannot be written
    // (the disk is full) must not cost the work of the install that stopped
    let mut staging = Staging {
        path: staging_path.clone(),
        claim: Some(claim),
        keep: true,
    };
    let mut steps = Steps {
        dir: staging_path.clone(),
        journal: Journal::new(&request.label, fingerprint.clone(), steps_plan.clone()),
    };
    let recorded = super::SourceRecord {
        url: request.base.url.clone(),
        host_digest: request.base.host_digest.clone(),
        pin: request.base.pin.clone(),
        size: request.base.size,
    };
    steps.journal.source = Some(recorded.clone());
    if existed {
        match Journal::read(&staging_path) {
            Some(mut found) if found.fingerprint == fingerprint && found.label == request.label => {
                send(JobEvent::Status(format!(
                    "Going on with the install that stopped ({} of {} steps are done)",
                    found.done.len(),
                    steps_plan.len()
                )));
                found.steps = steps_plan;
                found.source = Some(recorded);
                steps.journal = found;
            }
            other => {
                if other.is_some() {
                    send(JobEvent::Status(
                        "The install that stopped was made for other things (another Python or other packages), so it is started again".to_string(),
                    ));
                }
                journal::clear_except_lock(&staging_path)?;
            }
        }
    }
    steps.journal.write(&staging_path)?;
    let temporary = staging_path.join("tmp");
    let _ = std::fs::remove_dir_all(&temporary);
    std::fs::create_dir(&temporary)
        .map_err(|e| format!("could not create {}: {e}", temporary.display()))?;

    let mut leftovers = Leftovers { paths: Vec::new() };
    let worked = work(
        Ctx {
            spec,
            app,
            request,
            send,
            staging: &staging_path,
            expected: expected.as_deref(),
            python: &python,
        },
        &mut steps,
        &mut leftovers,
    );
    // the work is whole: if the move into place fails, it is still there to be moved
    let outcome = match worked {
        Ok(()) => put_in_place(spec, request, &mut staging, &final_dir, send),
        Err(e) => Err(e),
    };
    if let Err(e) = &outcome {
        // what takes the time is the environment: an install that fails after it was made keeps it, and
        // the next one goes on from there. One the person cancelled, or that failed before, leaves nothing.
        if e == CANCELLED || !steps.journal.is_done("venv") {
            staging.keep = false;
        } else if staging_path.exists() {
            send(JobEvent::Status(format!(
                "The work so far is kept in {}: install again to go on from it",
                staging_path.display()
            )));
        }
    }
    outcome
}

/// Moves the staging directory to the build's name, and takes the record of the install out of it: a
/// directory without one is whole.
fn put_in_place(
    spec: &InstallSpec,
    request: &InstallRequest,
    staging: &mut Staging,
    final_dir: &Path,
    send: &dyn Fn(JobEvent<PathBuf>),
) -> Result<PathBuf, String> {
    staging.release();
    let _ = std::fs::remove_file(staging.path.join(journal::LOCK));
    if final_dir.exists() {
        std::fs::remove_dir_all(final_dir)
            .map_err(|e| format!("could not replace {}: {e}", final_dir.display()))?;
    }
    rename_into_place(&staging.path, final_dir)?;
    std::fs::remove_file(final_dir.join(journal::JOURNAL)).map_err(|e| {
        format!(
            "{} is in place, but the record of the install in it could not be removed ({e}): install again to finish",
            final_dir.display()
        )
    })?;
    send(JobEvent::Status(format!(
        "Installed {} ({} of files)",
        request.label,
        format_bytes(super::dir_size(final_dir))
    )));
    Ok(final_dir.join(&spec.binary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_pip_announces_is_read_in_bytes() {
        let line = "  Downloading https://download-r2.pytorch.org/whl/rocm7.2/torch-2.14.1%2Brocm7.2-cp313-cp313-manylinux_2_28_x86_64.whl (6224.9 MB)";
        assert_eq!(announced_size(line), Some(6_224_900_000));
        assert_eq!(announced_size("Downloading x.whl (12.5 kB)"), Some(12_500));
        assert_eq!(announced_size("Downloading x.whl"), None);
        assert_eq!(announced_size("Collecting torch (>=2.0)"), None);
    }

    #[test]
    fn the_launcher_runs_what_the_build_runs_with_the_persons_python_variables_put_away() {
        let app = EntryPoint {
            module: "open_webui".into(),
            attr: "app".into(),
        };
        // a script of the source, as ComfyUI's has always been, on a system with a shell
        let (file, text) = launcher_text("comfyui", &Runs::Script("main.py".into()), None, false);
        assert_eq!(file, "comfyui");
        assert!(text.starts_with("#!/bin/sh\n"), "{text}");
        assert!(
            text.contains(r#"exec "$here/venv/bin/python" "$here/app/main.py" "$@""#),
            "{text}"
        );
        // the program of a wheel is run by the environment's Python and told what to call: a script of the
        // environment holds the path of the folder it was made in, and the build is moved when it is whole
        let program = Runs::Program("open-webui".into());
        let (file, text) = launcher_text("open-webui", &program, Some(&app), false);
        assert_eq!(file, "open-webui");
        // the suites' stand-in for a finished build writes this very file (`resources/owui_world.py` of the test
        // repository holds its text to it, `resources/test_fake_pypi.py`): they change together or not at all
        assert_eq!(
            text,
            include_str!("../../../tests/fixtures/launchers/open-webui.sh"),
            "the launcher changed: tests/fixtures/launchers/open-webui.sh and the suites' stand-in are to change with it"
        );
        assert!(
            text.ends_with(
                "exec \"$here/venv/bin/python\" -c \"import sys; from open_webui import app; sys.argv[0] = 'open-webui'; sys.exit(app())\" \"$@\"\n"
            ),
            "{text}"
        );
        assert!(
            !text.contains("venv/bin/open-webui"),
            "the script of the environment is never run: {text}"
        );
        for line in [
            "unset PYTHONPATH PYTHONHOME PYTHONSTARTUP PYTHONUSERBASE VIRTUAL_ENV",
            "PYTHONNOUSERSITE=1",
            "export PYTHONNOUSERSITE",
        ] {
            assert!(text.contains(line), "{line} in {text}");
        }
        // a name with dots in it is called whole, and its first part is what is imported
        let dotted = EntryPoint {
            module: "pkg.cli".into(),
            attr: "tools.main".into(),
        };
        let (_, text) = launcher_text("tool", &Runs::Program("tool".into()), Some(&dotted), false);
        assert!(
            text.contains(
                "from pkg.cli import tools; sys.argv[0] = 'tool'; sys.exit(tools.main())"
            ),
            "{text}"
        );
        // and the same on Windows, where a launcher is a batch file
        let (file, text) =
            launcher_text("comfyui", &Runs::Script("nodes/main.py".into()), None, true);
        assert_eq!(file, "comfyui.cmd");
        assert!(
            text.contains("\"%~dp0venv\\Scripts\\python.exe\" \"%~dp0app\\nodes\\main.py\" %*"),
            "{text}"
        );
        let (file, text) = launcher_text("open-webui", &program, Some(&app), true);
        assert_eq!(file, "open-webui.cmd");
        assert!(
            text.contains(
                "\"%~dp0venv\\Scripts\\python.exe\" -c \"import sys; from open_webui import app; sys.argv[0] = 'open-webui'; sys.exit(app())\" %*\r\n"
            ) && text.contains("set PYTHONPATH=\r\n")
                && text.contains("set PYTHONUSERBASE=\r\n")
                && text.contains("set VIRTUAL_ENV=\r\n")
                && text.contains("set PYTHONNOUSERSITE=1\r\n"),
            "{text:?}"
        );
    }

    #[test]
    fn an_entry_point_is_a_module_and_a_name_in_it_and_nothing_else_is_written_into_a_launcher() {
        let ep = |text: &str| parse_entry_point(text).map(|e| (e.module, e.attr));
        assert_eq!(
            ep("open_webui:app"),
            Some(("open_webui".into(), "app".into()))
        );
        assert_eq!(
            ep("  a.b.c:cli.main  "),
            Some(("a.b.c".into(), "cli.main".into()))
        );
        assert_eq!(
            ep("pkg:main [extra]"),
            Some(("pkg".into(), "main".into())),
            "extras are the installer's and not the call's"
        );
        for bad in [
            "",
            "open_webui",
            ":app",
            "open_webui:",
            "open-webui:app",
            "a:b; import os; os.system('x')",
            "a b:c",
            "1a:b",
            "a:b:c",
            "a..b:c",
            "a:\"; evil; \"",
            "a:b'; evil; '",
        ] {
            assert_eq!(ep(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_wheel_is_named_by_the_last_part_of_its_address_and_only_a_wheel_has_such_a_name() {
        let name = |url: &str| wheel_file_name(url);
        let wheel = "open_webui-0.11.4-py3-none-any.whl";
        for url in [
            format!("https://files.pythonhosted.org/packages/ca/8d/051d/{wheel}"),
            format!("http://127.0.0.1:8000/{wheel}"),
            format!("https://host/{wheel}?download=1"),
            format!("https://host/{wheel}#sha256=abc"),
        ] {
            assert_eq!(name(&url).as_deref(), Some(wheel), "{url}");
        }
        assert_eq!(
            name("https://host/a/b/torch-2.14.1+cpu-cp312-cp312-manylinux_2_28_x86_64.whl")
                .as_deref(),
            Some("torch-2.14.1+cpu-cp312-cp312-manylinux_2_28_x86_64.whl"),
            "a version may have a plus sign"
        );
        for url in [
            "https://host/open_webui-0.11.4.tar.gz",
            "https://host/.whl",
            "https://host/whl",
            "https://host/a b.whl",
            "https://host/a%2fb.whl",
            "https://host/",
            "",
        ] {
            assert_eq!(name(url), None, "{url:?}");
        }
    }

    #[test]
    fn versions_are_read_as_two_numbers() {
        assert_eq!(parse_version("3.13"), Some((3, 13)));
        assert_eq!(parse_version(" 3.9 "), Some((3, 9)));
        assert_eq!(parse_version("3"), None);
        assert_eq!(parse_version("three.thirteen"), None);
    }

    fn view<'a>(phase: Phase) -> PipView<'a> {
        PipView {
            step: "Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2)",
            phase,
            file: None,
            fetched: 0,
            total: None,
            speed: None,
            in_phase: Duration::ZERO,
            quiet: Duration::ZERO,
        }
    }

    #[test]
    fn p3_f76_the_line_under_the_bar_says_where_a_download_is_how_fast_and_how_long_nothing_has_happened(
    ) {
        const GB: u64 = 1 << 30;
        const MB: f64 = 1048576.0;
        // looking up
        assert_eq!(
            pip_detail(&view(Phase::Looking)),
            "Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2) · looking up the packages"
        );
        // a download: the file, how much of it, how fast, and how long it will take at that speed
        let fetching = PipView {
            file: Some("torch-2.14.1+rocm7.2-cp313-cp313-manylinux_2_28_x86_64.whl"),
            fetched: 2 * GB,
            total: Some(6 * GB),
            speed: Some(64.0 * MB),
            ..view(Phase::Fetching)
        };
        assert_eq!(
            pip_detail(&fetching),
            "Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2) · downloading torch-2.14.1+rocm7.2-cp313-cp313-manylinux_2_28_x86_64.whl: 2.0 GB of 6.0 GB, 64.0 MB/s, about 1 min 04 s left"
        );
        // no speed yet, no total announced, more than the total: nothing is claimed that is not known
        let early = PipView {
            speed: None,
            ..fetching
        };
        assert!(!pip_detail(&early).contains("/s") && !pip_detail(&early).contains("left"));
        let unknown = PipView {
            total: None,
            speed: Some(64.0 * MB),
            ..view(Phase::Fetching)
        };
        let text = pip_detail(&unknown);
        assert!(
            text.contains("downloading a package: 0 B, 64.0 MB/s") && !text.contains("left"),
            "{text}"
        );
        let over = PipView {
            fetched: 7 * GB,
            speed: Some(64.0 * MB),
            ..fetching
        };
        assert!(
            pip_detail(&over).contains("6.0 GB of 6.0 GB") && !pip_detail(&over).contains("left"),
            "{}",
            pip_detail(&over)
        );
        // unpacking says how long it has been, since pip says nothing in it
        let unpacking = PipView {
            in_phase: Duration::from_secs(131),
            ..view(Phase::Unpacking)
        };
        assert_eq!(
            pip_detail(&unpacking),
            "Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2) · putting what it fetched into the environment, which takes minutes · 2 min 11 s"
        );
        // and a silence is said once it is long, so that a wait can be told from a stuck window
        let quiet = PipView {
            quiet: Duration::from_secs(75),
            ..unpacking
        };
        assert!(
            pip_detail(&quiet).ends_with(" · nothing new for 1 min 15 s, pip is still running"),
            "{}",
            pip_detail(&quiet)
        );
        assert!(!pip_detail(&PipView {
            quiet: Duration::from_secs(44),
            ..view(Phase::Looking)
        })
        .contains("nothing new"));

        assert_eq!(format_duration(Duration::from_secs(5)), "5 s");
        assert_eq!(format_duration(Duration::from_secs(60)), "1 min 00 s");
        assert_eq!(format_duration(Duration::from_secs(3725)), "1 h 02 min");
    }

    #[test]
    fn p3_f76b_pips_words_are_read_as_it_prints_them_indented_under_the_package_that_asked() {
        let seen = std::cell::RefCell::new(Vec::<f32>::new());
        let said = std::cell::RefCell::new(Vec::<String>::new());
        let mut watch = Watch::new();
        let mut speed = Speed::new();
        let mut line = |text: &str, watch: &mut Watch| {
            watch.line(
                text.to_string(),
                &mut speed,
                &|l| said.borrow_mut().push(l),
                &|f| seen.borrow_mut().push(f),
            )
        };
        line(
            "Looking in indexes: https://download.pytorch.org/whl/rocm7.2",
            &mut watch,
        );
        assert_eq!(watch.phase, Phase::Looking);
        line("Collecting torch", &mut watch);
        // the small file that says what a wheel needs is not the wheel
        line("  Downloading https://x/torch-2.14.1%2Brocm7.2-cp313-cp313-linux_x86_64.whl.metadata (8.0 kB)", &mut watch);
        assert_eq!((watch.phase, watch.announced), (Phase::Looking, None));
        line("  Downloading https://download.pytorch.org/whl/rocm7.2/torch-2.14.1%2Brocm7.2-cp313-cp313-manylinux_2_28_x86_64.whl (6224.9 MB)", &mut watch);
        assert_eq!(watch.phase, Phase::Fetching);
        assert_eq!(watch.announced, Some(6_224_900_000));
        assert_eq!(
            watch.file.as_deref(),
            Some("torch-2.14.1+rocm7.2-cp313-cp313-manylinux_2_28_x86_64.whl"),
            "the address is read as a file name"
        );
        line("Installing collected packages: torch", &mut watch);
        assert_eq!((watch.phase, watch.announced), (Phase::Unpacking, None));
        line("Successfully installed torch-2.14.1+rocm7.2", &mut watch);
        // the bar was told what it can be told, and the person is told every line
        assert_eq!(*seen.borrow(), [0.02, 0.92, 1.0]);
        assert_eq!(said.borrow().len(), 6);
        // the last lines are kept for a failure to be shown with
        assert_eq!(watch.tail.len(), 6);
        assert_eq!(
            wheel_of("  Downloading https://files/x-1.0-py3-none-any.whl#sha256=ab (12 kB)"),
            Some(("x-1.0-py3-none-any.whl".to_string(), Some(12_000)))
        );
        assert_eq!(wheel_of("Collecting torch"), None);
    }

    #[test]
    fn the_steps_are_listed_in_the_order_they_are_done_with_the_check_last() {
        let app = PythonApp {
            runs: Runs::Script("main.py".into()),
            versions: vec!["3.13".into()],
            python: None,
            pip: vec![PipPlan {
                id: "torch".into(),
                label: "PyTorch".into(),
                packages: vec!["torch".into()],
                requirements: None,
                wheel: false,
                index: None,
                weight: 1,
            }],
            nodes: vec![
                NodePlan {
                    id: "a".into(),
                    label: "Pack A".into(),
                    url: "https://x/a.tar.gz".into(),
                    sha256: "sha256:00".into(),
                    into: "custom_nodes/a".into(),
                    requirements: Some("requirements.txt".into()),
                },
                NodePlan {
                    id: "b".into(),
                    label: "Pack B".into(),
                    url: "https://x/b.tar.gz".into(),
                    sha256: "sha256:00".into(),
                    into: "custom_nodes/b".into(),
                    requirements: None,
                },
            ],
            space: None,
        };
        let request = InstallRequest::bare("https://x/v1.tar.gz", "v1");
        let ids = |check| -> Vec<String> {
            plan_steps(&app, &request, check)
                .into_iter()
                .map(|s| s.id)
                .collect()
        };
        assert_eq!(
            ids(true),
            [
                "source",
                "node:a",
                "node:b",
                "venv",
                "pip:torch",
                "needs:a",
                "check"
            ]
        );
        assert_eq!(ids(false).last().map(String::as_str), Some("needs:a"));
    }

    #[test]
    fn the_versions_are_listed_in_words() {
        let v = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(list_in_words(&v(&["3.13"])), "3.13");
        assert_eq!(list_in_words(&v(&["3.13", "3.12"])), "3.13 or 3.12");
        assert_eq!(
            list_in_words(&v(&["3.13", "3.12", "3.11"])),
            "3.13, 3.12 or 3.11"
        );
    }
}
