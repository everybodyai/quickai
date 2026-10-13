//! Looking at the computer for what a configuration needs from it, and bringing it with the computer's own
//! package manager (docs/headless.md, section 12).
//!
//! The documents say what to look at (`[[prerequisite]]` of an engine) and which packages bring it; this is
//! the part that does it: is a program on the `PATH`, is there a header file in the folder of Python's
//! headers, is the driver of the card new enough, which package manager has the computer and may this program
//! use it, and a script run in an environment. Every function takes the `Machine` it looks at, so that a
//! test makes its own computer out of a folder and a few stand-in programs.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use quickai_core::doc::engine::PackageManager;
use quickai_core::prerequisite::version_at_least;

use crate::runtime::process::{
    run_captured, run_captured_until, without_package_loader_env, ChildEnv, RunFailure,
};

/// The computer that is looked at: where programs are, where the kernel and the driver keep their files,
/// what the environment says, and who is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    /// The folders of the `PATH`, absolute ones only: a program found by a relative entry is whatever the
    /// folder the program was started in has.
    pub path: Vec<PathBuf>,
    /// The root of the files the kernel and the driver keep (`/`; a test has a tree of its own).
    pub root: PathBuf,
    pub vars: BTreeMap<String, String>,
    /// The user the program runs as: 0 is the administrator.
    pub uid: u32,
}

impl Machine {
    /// This computer.
    pub fn this() -> Machine {
        let path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|dir| dir.is_absolute())
            .collect();
        Machine {
            path,
            root: PathBuf::from("/"),
            // `vars()` panics at a name or a value that is not text; such a variable is not one a document asks about
            vars: std::env::vars_os()
                .filter_map(|(name, value)| {
                    Some((name.into_string().ok()?, value.into_string().ok()?))
                })
                .collect(),
            uid: effective_user(),
        }
    }

    /// The `PATH` of the programs this program starts: the folders that were looked in.
    pub(crate) fn path_variable(&self) -> Option<std::ffi::OsString> {
        std::env::join_paths(&self.path).ok()
    }
}

#[cfg(unix)]
fn effective_user() -> u32 {
    // SAFETY: geteuid takes nothing and cannot fail.
    unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn effective_user() -> u32 {
    1
}

/// What looking at the computer found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// It is there, and this says what was found (`gcc at /usr/bin/gcc`).
    Met(String),
    /// It is not, and this says what was looked for.
    Missing(String),
    /// It could not be looked at (the Python it depends on is not known yet), and this says why.
    Unchecked(String),
}

impl State {
    pub fn is_met(&self) -> bool {
        matches!(self, State::Met(_))
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The program of this name that the `PATH` finds.
pub fn which(machine: &Machine, name: &str) -> Option<PathBuf> {
    machine
        .path
        .iter()
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

// ---- a program ---------------------------------------------------------------------------------------

/// Is one of `any_of` on the computer? When `variable` names an environment variable that is set and not
/// empty, that decides, as it does for the tool that reads it: the program it names (the first word of it)
/// must be there. The second answer is whether the variable decided, because then the packages of a
/// document cannot put it right.
pub fn check_program(
    machine: &Machine,
    any_of: &[String],
    variable: Option<&str>,
) -> (State, bool) {
    if let Some(name) = variable {
        let said = machine.vars.get(name).map(|v| v.trim()).unwrap_or_default();
        if let Some(first) = said.split_whitespace().next() {
            let found = if first.contains('/') {
                let path = PathBuf::from(first);
                is_executable(&path).then_some(path)
            } else {
                which(machine, first)
            };
            return match found {
                Some(path) => (
                    State::Met(format!("{name} is {said} ({})", path.display())),
                    true,
                ),
                None => (
                    State::Missing(format!(
                        "the variable {name} says {said}, which is not there"
                    )),
                    true,
                ),
            };
        }
    }
    for program in any_of {
        if let Some(path) = which(machine, program) {
            return (
                State::Met(format!("{program} at {}", path.display())),
                false,
            );
        }
    }
    (
        State::Missing(format!("none of {} is on the PATH", any_of.join(", "))),
        false,
    )
}

// ---- the driver of an NVIDIA card --------------------------------------------------------------------

/// The version the driver of the NVIDIA card says it is: what `nvidia-smi` prints, else the driver's own file.
/// `None` when neither answers (there is no driver, or it is not loaded). The tool comes first as the card is
/// found by a tool first (`gpu::detect`): whoever puts a stand-in of their own on the `PATH` decides what the
/// program is told.
pub fn nvidia_driver_version(machine: &Machine) -> Option<String> {
    if let Some(smi) = which(machine, "nvidia-smi") {
        let mut command = Command::new(smi);
        command.args(["--query-gpu=driver_version", "--format=csv,noheader"]);
        if let Some(path) = machine.path_variable() {
            command.env("PATH", path);
        }
        if let Ok(ran) = run_captured(&mut command, Duration::from_secs(20)) {
            if ran.status.success() {
                if let Some(version) = ran
                    .stdout
                    .lines()
                    .map(str::trim)
                    .find(|line| is_driver_version(line))
                {
                    return Some(version.to_string());
                }
            }
        }
    }
    let text = std::fs::read_to_string(machine.root.join("proc/driver/nvidia/version")).ok()?;
    version_in_proc_file(&text)
}

/// The version in the first line of `/proc/driver/nvidia/version`: `NVRM version: NVIDIA UNIX x86_64 Kernel
/// Module  595.58.03  Thu ...`.
fn version_in_proc_file(text: &str) -> Option<String> {
    text.lines()
        .next()?
        .split_whitespace()
        .find(|word| is_driver_version(word))
        .map(str::to_string)
}

fn is_driver_version(word: &str) -> bool {
    word.contains('.')
        && word
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// Is the driver `at_least` (`580`) or newer?
pub fn check_nvidia_driver(machine: &Machine, at_least: &str) -> State {
    match nvidia_driver_version(machine) {
        Some(version) if version_at_least(&version, at_least) => {
            State::Met(format!("driver {version}"))
        }
        Some(version) => State::Missing(format!("the driver is {version}, older than {at_least}")),
        None => State::Missing(
            "no NVIDIA driver answered: nvidia-smi is not there or did not say, and the driver's own file is not there"
                .to_string(),
        ),
    }
}

// ---- Python's headers ----------------------------------------------------------------------------------

/// What a Python says about itself that the headers depend on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonFacts {
    /// The interpreter that was asked, and the words that go before its arguments (the Windows launcher's `-3.12`):
    /// what later questions of the same Python (a module it can import) are put to.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// `(3, 12)`.
    pub version: (u32, u32),
    /// The folder the headers of this Python are looked for in: the one a build of an extension is given
    /// (Debian's `posix_local` scheme names a folder of `/usr/local` that is not where its package puts them,
    /// so it is read as `posix_prefix`, as the tools that build extensions do).
    pub include_dir: PathBuf,
    /// The installation the Python (or the environment made from it) belongs to: `/usr` for the system's.
    pub base_prefix: PathBuf,
}

impl PythonFacts {
    /// The Python the computer's package manager brings, and so the one its `-dev` package is for.
    pub fn is_the_systems(&self) -> bool {
        self.base_prefix == Path::new("/usr")
    }
}

const PYTHON_FACTS: &str = "\
import sys, sysconfig
scheme = sysconfig.get_default_scheme() if hasattr(sysconfig, 'get_default_scheme') else sysconfig._get_default_scheme()
if scheme == 'posix_local':
    scheme = 'posix_prefix'
print(sys.version_info[0], sys.version_info[1])
print(sysconfig.get_paths(scheme=scheme)['include'])
print(sys.base_prefix)
";

/// Asks a Python for `PythonFacts`. `args` go before the script (the Windows launcher's `-3.12`).
pub fn python_facts(program: &Path, args: &[String]) -> Result<PythonFacts, String> {
    let mut command = Command::new(program);
    command.args(args).args(["-I", "-c", PYTHON_FACTS]);
    without_package_loader_env(&mut command);
    let ran =
        run_captured(&mut command, Duration::from_secs(30)).map_err(|failure| match failure {
            RunFailure::Start(e) => format!("{} could not be run: {e}", program.display()),
            RunFailure::TimedOut => format!("{} did not answer", program.display()),
            RunFailure::Wait(e) => format!("{} could not be waited for: {e}", program.display()),
            RunFailure::Stopped => "it was stopped".to_string(),
        })?;
    if !ran.status.success() {
        let said = ran.stderr.trim().lines().last().unwrap_or("").to_string();
        return Err(format!(
            "{} ended with {} ({said})",
            program.display(),
            ran.status
        ));
    }
    let mut lines = ran.stdout.lines();
    let version = lines.next().and_then(|line| {
        let mut numbers = line.split_whitespace().map(|n| n.parse::<u32>());
        match (numbers.next(), numbers.next()) {
            (Some(Ok(major)), Some(Ok(minor))) => Some((major, minor)),
            _ => None,
        }
    });
    let (include, base) = (lines.next(), lines.next());
    match (version, include, base) {
        (Some(version), Some(include), Some(base)) if !include.is_empty() && !base.is_empty() => {
            Ok(PythonFacts {
                program: program.to_path_buf(),
                args: args.to_vec(),
                version,
                include_dir: PathBuf::from(include),
                base_prefix: PathBuf::from(base),
            })
        }
        _ => Err(format!(
            "{} said {:?}, which is not what was asked",
            program.display(),
            ran.stdout.trim()
        )),
    }
}

/// Can this Python import `module`? When it cannot, the answer is its own last line (`ModuleNotFoundError: No
/// module named 'ensurepip'`, which is what Debian's Python says without `python3-venv`).
pub fn check_python_module(facts: &PythonFacts, module: &str) -> State {
    let mut command = Command::new(&facts.program);
    command
        .args(&facts.args)
        .args(["-I", "-c", &format!("import {module}")]);
    without_package_loader_env(&mut command);
    let version = format!("Python {}.{}", facts.version.0, facts.version.1);
    match run_captured(&mut command, Duration::from_secs(30)) {
        Ok(ran) if ran.status.success() => State::Met(format!("{version} imports {module}")),
        Ok(ran) => {
            let said = ran.stderr.trim().lines().last().unwrap_or("").trim();
            State::Missing(if said.is_empty() {
                format!("{version} cannot import {module}")
            } else {
                format!("{version} cannot import {module} ({said})")
            })
        }
        Err(RunFailure::TimedOut) => State::Unchecked(format!("{version} did not answer")),
        Err(RunFailure::Start(e)) => State::Unchecked(format!("{version} could not be run: {e}")),
        Err(_) => State::Unchecked(format!("{version} could not be asked")),
    }
}

/// Is `file` in the folder of this Python's headers?
pub fn check_python_header(facts: &PythonFacts, file: &str) -> State {
    let path = facts.include_dir.join(file);
    if path.is_file() {
        State::Met(path.display().to_string())
    } else {
        State::Missing(format!(
            "{file} is not in {}, where Python {}.{} keeps its headers",
            facts.include_dir.display(),
            facts.version.0,
            facts.version.1
        ))
    }
}

// ---- the package manager -------------------------------------------------------------------------------

/// The package manager the computer has: the first of the five that is on the `PATH`.
pub fn package_manager(machine: &Machine) -> Option<PackageManager> {
    PackageManager::ALL
        .into_iter()
        .find(|manager| which(machine, manager.program()).is_some())
}

/// Whether this program may install packages, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// It runs as the administrator.
    Root,
    /// It does not, and `sudo` takes it without asking for a password.
    Sudo,
    /// It does not, and nothing here can: a person has to.
    Unavailable,
}

/// Who may install: the administrator, or a user `sudo` lets do it without a password (nothing here can
/// answer a question).
pub fn elevation(machine: &Machine) -> Elevation {
    if machine.uid == 0 {
        return Elevation::Root;
    }
    let Some(sudo) = which(machine, "sudo") else {
        return Elevation::Unavailable;
    };
    let mut command = Command::new(sudo);
    command.args(["-n", "true"]);
    match run_captured(&mut command, Duration::from_secs(10)) {
        Ok(ran) if ran.status.success() => Elevation::Sudo,
        _ => Elevation::Unavailable,
    }
}

/// The commands that bring `packages`, as a person would type them and in the order they are run, joined with
/// `&&`; `sudo` before each when the person is not the administrator.
pub fn command_line(manager: PackageManager, packages: &[String], root: bool) -> String {
    manager
        .install_commands(packages)
        .iter()
        .map(|words| {
            if root {
                words.join(" ")
            } else {
                format!("sudo {}", words.join(" "))
            }
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

/// Runs the commands that bring `packages`, one after the other, passing on every line they print. Nothing
/// asks a question (the administrator's `apt-get` is told so, and sees no terminal). Where the manager can wait
/// for a lock that another process holds it is told to, for `PackageManager::LOCK_WAIT_SECONDS` (what is said
/// is still the command a person would type). Each command leads a process group of its own, so that a Ctrl-C
/// at the terminal does not stop a package manager half way through a package; `stop` is looked at between the
/// commands, so one given while the manager waits for a lock is answered when the wait is over.
pub fn install_packages(
    machine: &Machine,
    manager: PackageManager,
    packages: &[String],
    elevation: Elevation,
    say: &mut dyn FnMut(String),
    stop: &AtomicBool,
) -> Result<(), String> {
    if elevation == Elevation::Unavailable {
        return Err(
            "this program is not the administrator and sudo does not take it without a password"
                .into(),
        );
    }
    let commands = manager.install_commands(packages);
    let to_run = manager.install_commands_to_run(packages);
    let last = commands.len() - 1;
    for (index, (words, to_run)) in commands.into_iter().zip(to_run).enumerate() {
        if stop.load(Ordering::SeqCst) {
            return Err("it was stopped".into());
        }
        let shown = if elevation == Elevation::Root {
            words.join(" ")
        } else {
            format!("sudo {}", words.join(" "))
        };
        say(format!("running: {shown}"));
        let quiet_apt = manager == PackageManager::Apt;
        let mut run: Vec<String> = Vec::new();
        if elevation == Elevation::Sudo {
            run.extend(["sudo".to_string(), "-n".to_string()]);
            if quiet_apt {
                run.extend([
                    "env".to_string(),
                    "DEBIAN_FRONTEND=noninteractive".to_string(),
                ]);
            }
        }
        run.extend(to_run);
        let program =
            which(machine, &run[0]).ok_or_else(|| format!("{} is not on the PATH", run[0]))?;
        let mut command = Command::new(program);
        command.args(&run[1..]);
        if let Some(path) = machine.path_variable() {
            command.env("PATH", path);
        }
        if elevation == Elevation::Root && quiet_apt {
            command.env("DEBIAN_FRONTEND", "noninteractive");
        }
        match stream(&mut command, say) {
            Ok(()) => {}
            // a list of packages that cannot be refreshed (a repository of the person's that is down) does not stop the
            // install of a package the list already has: if it has not, the install says so
            Err(why) if index < last => say(format!(
                "could not refresh the list of packages (`{shown}` {}); going on with the one there is",
                why.lines().next().unwrap_or_default().trim_end_matches(';')
            )),
            Err(why) => return Err(format!("`{shown}` {why}")),
        }
    }
    Ok(())
}

/// Runs a command in a process group of its own and passes on what it prints, line by line; it is waited
/// for however long it takes. `Err` says how it ended, with the last lines it wrote.
fn stream(command: &mut Command, say: &mut dyn FnMut(String)) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = crate::runtime::process::spawn_retrying(command)
        .map_err(|e| format!("could not be started: {e}"))?;
    let (tx, rx) = crossbeam_channel::unbounded::<String>();
    fn pump<R: std::io::Read + Send + 'static>(
        pipe: Option<R>,
        tx: crossbeam_channel::Sender<String>,
    ) {
        if let Some(pipe) = pipe {
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
    }
    pump(child.stdout.take(), tx.clone());
    pump(child.stderr.take(), tx);
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut tell = |line: String, tail: &mut std::collections::VecDeque<String>| {
        if tail.len() == 12 {
            tail.pop_front();
        }
        tail.push_back(line.clone());
        say(line);
    };
    // the channel closes when both pipes have been read to their end; a program the command left running (a
    // service a package started) can hold them open for ever, so the end is the command's own
    let status = loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => tell(line, &mut tail),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|e| format!("could not be waited for: {e}"))?
                {
                    break status;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                break child
                    .wait()
                    .map_err(|e| format!("could not be waited for: {e}"))?;
            }
        }
    };
    // what it wrote just before it ended is still on its way: a moment for it, and not more
    let until = Instant::now() + Duration::from_millis(300);
    while let Ok(line) = rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
        tell(line, &mut tail);
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "ended with {status}; the last it wrote was:\n  {}",
            tail.iter().cloned().collect::<Vec<_>>().join("\n  ")
        ))
    }
}

// ---- a script in an environment ------------------------------------------------------------------------

/// How a script ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptEnd {
    /// Status 0, and what it printed.
    Passed(Vec<String>),
    /// Anything else: how, and the last lines it wrote (its output and its errors).
    Failed { how: String, lines: Vec<String> },
    /// The person asked to stop.
    Stopped,
}

/// A line as long as a command line of a compiler is cut, so that the lines before it are read.
fn shortened(line: &str) -> String {
    const LIMIT: usize = 300;
    if line.chars().count() <= LIMIT {
        line.to_string()
    } else {
        let cut: String = line.chars().take(LIMIT).collect();
        format!("{cut}...")
    }
}

/// Runs `script` with `python`, with the environment `env` made of what this program has, from `cwd`, for
/// at most `timeout`. It passes when it ends with status 0.
pub fn run_script(
    python: &Path,
    script: &Path,
    env: &ChildEnv,
    cwd: Option<&Path>,
    timeout: Duration,
    stop: &AtomicBool,
) -> ScriptEnd {
    let mut command = Command::new(python);
    command.arg(script);
    without_package_loader_env(&mut command);
    for name in &env.remove {
        command.env_remove(name);
    }
    for (name, value) in &env.set {
        command.env(name, value);
    }
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let started = Instant::now();
    match run_captured_until(&mut command, timeout, stop) {
        Ok(ran) => {
            let mut lines: Vec<String> = ran.stdout.lines().map(str::to_string).collect();
            lines.extend(ran.stderr.lines().map(str::to_string));
            if ran.status.success() {
                ScriptEnd::Passed(lines)
            } else {
                let keep = lines.len().saturating_sub(15);
                ScriptEnd::Failed {
                    how: format!("ended with {}", ran.status),
                    lines: lines.split_off(keep).iter().map(|l| shortened(l)).collect(),
                }
            }
        }
        Err(RunFailure::TimedOut) => ScriptEnd::Failed {
            how: format!(
                "had not ended after {} seconds",
                started.elapsed().as_secs().max(timeout.as_secs())
            ),
            lines: Vec::new(),
        },
        Err(RunFailure::Start(e)) => ScriptEnd::Failed {
            how: format!("could not be started: {e}"),
            lines: Vec::new(),
        },
        Err(RunFailure::Wait(e)) => ScriptEnd::Failed {
            how: format!("could not be waited for: {e}"),
            lines: Vec::new(),
        },
        Err(RunFailure::Stopped) => ScriptEnd::Stopped,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quickai-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A program in `dir`: a shell script with this body.
    fn program(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A computer that has only what a test puts in `bin` and `root`.
    fn machine(base: &Path, uid: u32) -> Machine {
        let bin = base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        // the shell and the few programs a stand-in uses
        for tool in [
            "sh", "cat", "env", "sleep", "echo", "printf", "true", "false", "tee",
        ] {
            for dir in ["/usr/bin", "/bin"] {
                let from = Path::new(dir).join(tool);
                if from.exists() {
                    let _ = std::os::unix::fs::symlink(&from, bin.join(tool));
                    break;
                }
            }
        }
        Machine {
            path: vec![bin],
            root: base.join("root"),
            vars: BTreeMap::new(),
            uid,
        }
    }

    fn bin(machine: &Machine) -> PathBuf {
        machine.path[0].clone()
    }

    /// Y71: a program is found by the `PATH`, and only a file that can be run, in a folder given in full, counts.
    #[test]
    fn p3_y71_a_program_is_found_on_the_path_and_only_a_file_that_can_be_run_counts() {
        let base = scratch("y71");
        let mut here = machine(&base, 1000);
        program(&bin(&here), "gcc", "exit 0");
        let plain = bin(&here).join("clang");
        std::fs::write(&plain, "not a program").unwrap();
        assert_eq!(which(&here, "gcc"), Some(bin(&here).join("gcc")));
        assert_eq!(
            which(&here, "clang"),
            None,
            "a file nobody may run is not a program"
        );
        assert_eq!(which(&here, "tcc"), None);
        // a folder is not a program either
        std::fs::create_dir_all(bin(&here).join("cc")).unwrap();
        assert_eq!(which(&here, "cc"), None);
        // a relative folder of the PATH is not looked in (this computer's own filter)
        here.path.push(PathBuf::from("."));
        let real = Machine::this();
        assert!(real.path.iter().all(|dir| dir.is_absolute()));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y72: the compiler is any of the programs a document names; a variable that is set decides instead, as
    /// it does for the tool that reads it, and then the packages cannot put it right.
    #[test]
    fn p3_y72_the_compiler_is_one_of_the_programs_or_the_one_the_variable_names() {
        let base = scratch("y72");
        let mut here = machine(&base, 1000);
        let names = vec!["gcc".to_string(), "clang".to_string()];
        let (state, by_variable) = check_program(&here, &names, Some("CC"));
        assert_eq!(
            state,
            State::Missing("none of gcc, clang is on the PATH".to_string())
        );
        assert!(!by_variable);
        program(&bin(&here), "clang", "exit 0");
        let (state, _) = check_program(&here, &names, Some("CC"));
        assert_eq!(
            state,
            State::Met(format!("clang at {}", bin(&here).join("clang").display()))
        );
        // the variable decides: a compiler it names that is there
        program(&bin(&here), "cc-17", "exit 0");
        here.vars.insert("CC".into(), "cc-17 -m64".into());
        let (state, by_variable) = check_program(&here, &[], Some("CC"));
        assert!(state.is_met() && by_variable, "{state:?}");
        // and one it names that is not, although another would do
        here.vars.insert("CC".into(), "no-such-cc".into());
        let (state, by_variable) = check_program(&here, &names, Some("CC"));
        assert_eq!(
            state,
            State::Missing("the variable CC says no-such-cc, which is not there".into())
        );
        assert!(by_variable);
        // an empty variable says nothing
        here.vars.insert("CC".into(), "  ".into());
        assert!(check_program(&here, &names, Some("CC")).0.is_met());
        // a path in the variable is a file
        let own = program(&base, "my-cc", "exit 0");
        here.vars.insert("CC".into(), own.display().to_string());
        assert!(check_program(&here, &names, Some("CC")).0.is_met());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y73: the version of the driver is read from `nvidia-smi`, else from the driver's own file, and is held to
    /// the version a document wants number by number.
    #[test]
    fn p3_y73_the_drivers_version_is_read_from_nvidia_smi_or_from_its_file_and_compared_by_numbers()
    {
        let base = scratch("y73");
        let here = machine(&base, 1000);
        // nothing answers
        assert_eq!(nvidia_driver_version(&here), None);
        let State::Missing(said) = check_nvidia_driver(&here, "580") else {
            panic!("no driver is not a driver");
        };
        assert!(said.contains("no NVIDIA driver answered"), "{said}");
        // the driver's own file answers, read as the real one is written
        let proc_dir = here.root.join("proc/driver/nvidia");
        std::fs::create_dir_all(&proc_dir).unwrap();
        std::fs::write(
            proc_dir.join("version"),
            "NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  595.58.03  Release Build  (dvs-builder@U22-I3-A07-1-3)  Wed Mar  4 19:55:53 UTC 2026\nGCC version:  gcc version 13.3.0 (Ubuntu 13.3.0-6ubuntu2~24.04)\n",
        )
        .unwrap();
        assert_eq!(nvidia_driver_version(&here).as_deref(), Some("595.58.03"));
        assert_eq!(
            check_nvidia_driver(&here, "580"),
            State::Met("driver 595.58.03".into())
        );
        // nvidia-smi comes first (a stand-in that prints what the real one does with this query)
        program(&bin(&here), "nvidia-smi", "echo 575.64.03");
        assert_eq!(nvidia_driver_version(&here).as_deref(), Some("575.64.03"));
        assert_eq!(
            check_nvidia_driver(&here, "580"),
            State::Missing("the driver is 575.64.03, older than 580".into())
        );
        // a tool that fails, or says what is not a version, leaves the file to answer
        program(
            &bin(&here),
            "nvidia-smi",
            "echo 'NVIDIA-SMI has failed' >&2; exit 9",
        );
        assert_eq!(nvidia_driver_version(&here).as_deref(), Some("595.58.03"));
        program(&bin(&here), "nvidia-smi", "echo 'No devices were found'");
        assert_eq!(nvidia_driver_version(&here).as_deref(), Some("595.58.03"));
        // and a file that says nothing of a version leaves nothing
        std::fs::write(proc_dir.join("version"), "NVRM version: unknown\n").unwrap();
        assert_eq!(nvidia_driver_version(&here), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y74: a Python is asked for its version, the folder of its headers and its installation; the header file
    /// is there or it is not, and only the system's Python is the one a package brings headers for.
    #[test]
    fn p3_y74_pythons_headers_are_looked_for_in_the_folder_it_names() {
        let base = scratch("y74");
        let here = machine(&base, 1000);
        let include = base.join("include/python3.12");
        let python = program(
            &bin(&here),
            "python3.12",
            &format!("printf '3 12\\n{}\\n/usr\\n'", include.display()),
        );
        let facts = python_facts(&python, &[]).unwrap();
        assert_eq!(facts.version, (3, 12));
        assert_eq!(facts.include_dir, include);
        assert!(facts.is_the_systems());
        let State::Missing(said) = check_python_header(&facts, "Python.h") else {
            panic!("no header yet");
        };
        assert!(said.contains("Python.h is not in"), "{said}");
        assert!(said.contains("Python 3.12"), "{said}");
        std::fs::create_dir_all(&include).unwrap();
        std::fs::write(include.join("Python.h"), "/* the header */").unwrap();
        assert_eq!(
            check_python_header(&facts, "Python.h"),
            State::Met(include.join("Python.h").display().to_string())
        );
        // a Python of a version manager is not the system's
        let own = program(
            &bin(&here),
            "python-own",
            &format!(
                "printf '3 13\\n{}\\n/home/me/.pyenv/versions/3.13.1\\n'",
                include.display()
            ),
        );
        assert!(!python_facts(&own, &[]).unwrap().is_the_systems());
        // a Python that fails, and one that says something else
        let broken = program(
            &bin(&here),
            "python-broken",
            "echo 'no module named sysconfig' >&2; exit 1",
        );
        let why = python_facts(&broken, &[]).unwrap_err();
        assert!(why.contains("no module named sysconfig"), "{why}");
        let odd = program(&bin(&here), "python-odd", "echo hello");
        assert!(python_facts(&odd, &[])
            .unwrap_err()
            .contains("not what was asked"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y78b: a module is imported by the Python or its own words say why not (`ModuleNotFoundError: No module named
    /// 'ensurepip'` is what Debian's Python says without `python3.12-venv`), and a Python that cannot be run is not a
    /// Python that lacks the module.
    #[test]
    fn p3_y78b_a_module_is_imported_by_the_python_or_its_own_words_say_why_not() {
        let base = scratch("y78b");
        let here = machine(&base, 1000);
        let marker = base.join("no-ensurepip");
        let python = program(
            &bin(&here),
            "python3.12",
            &format!(
                "case \"$3\" in\n*sysconfig*) printf '3 12\\n/x/include\\n/usr\\n'; exit 0;;\n*ensurepip*) if [ -f '{}' ]; then echo \"Traceback (most recent call last):\" >&2; echo \"ModuleNotFoundError: No module named 'ensurepip'\" >&2; exit 1; fi; exit 0;;\nesac\nexit 9",
                marker.display()
            ),
        );
        let facts = python_facts(&python, &[]).unwrap();
        assert_eq!(
            facts.program, python,
            "the Python that was asked is the one asked again"
        );
        assert_eq!(
            check_python_module(&facts, "ensurepip"),
            State::Met("Python 3.12 imports ensurepip".into())
        );
        std::fs::write(&marker, "").unwrap();
        assert_eq!(
            check_python_module(&facts, "ensurepip"),
            State::Missing(
                "Python 3.12 cannot import ensurepip (ModuleNotFoundError: No module named 'ensurepip')"
                    .into()
            )
        );
        // a Python that is not there cannot say it lacks a module
        let gone = PythonFacts {
            program: base.join("no-python"),
            ..facts
        };
        let State::Unchecked(said) = check_python_module(&gone, "ensurepip") else {
            panic!("a Python that cannot be run is not a Python without the module");
        };
        assert!(said.contains("could not be run"), "{said}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y75: the package manager is the first of the five that the computer has, and the person may install if
    /// they are the administrator or `sudo` takes them without a password.
    #[test]
    fn p3_y75_the_package_manager_is_the_one_the_computer_has_and_the_install_is_for_who_may() {
        let base = scratch("y75");
        let mut here = machine(&base, 1000);
        assert_eq!(package_manager(&here), None);
        program(&bin(&here), "apk", "exit 0");
        assert_eq!(package_manager(&here), Some(PackageManager::Apk));
        program(&bin(&here), "dnf", "exit 0");
        assert_eq!(
            package_manager(&here),
            Some(PackageManager::Dnf),
            "the order of the list"
        );
        program(&bin(&here), "apt-get", "exit 0");
        assert_eq!(package_manager(&here), Some(PackageManager::Apt));
        // who may
        assert_eq!(elevation(&here), Elevation::Unavailable, "no sudo");
        program(&bin(&here), "sudo", "exit 1");
        assert_eq!(
            elevation(&here),
            Elevation::Unavailable,
            "sudo asks for a password"
        );
        program(&bin(&here), "sudo", "[ \"$1\" = \"-n\" ] || exit 2\nexit 0");
        assert_eq!(elevation(&here), Elevation::Sudo);
        here.uid = 0;
        assert_eq!(elevation(&here), Elevation::Root);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y76: the command that is printed is the one a person would type: the list of packages is refreshed first
    /// where the manager has one, and `sudo` stands before each when the person is not the administrator. What is
    /// run adds only what a program needs (`sudo -n`, the frontend, apt's wait for a lock: Y77, Y78c).
    #[test]
    fn p3_y76_the_command_that_is_printed_is_the_one_a_person_would_type() {
        let packages = vec!["build-essential".to_string(), "python3.12-dev".to_string()];
        assert_eq!(
            command_line(PackageManager::Apt, &packages, true),
            "apt-get update && apt-get install -y build-essential python3.12-dev"
        );
        assert_eq!(
            command_line(PackageManager::Apt, &packages, false),
            "sudo apt-get update && sudo apt-get install -y build-essential python3.12-dev"
        );
        let one = vec!["gcc".to_string()];
        assert_eq!(
            command_line(PackageManager::Dnf, &one, true),
            "dnf install -y gcc"
        );
        assert_eq!(
            command_line(PackageManager::Zypper, &one, true),
            "zypper --non-interactive install gcc"
        );
        assert_eq!(
            command_line(PackageManager::Pacman, &one, false),
            "sudo pacman -S --needed --noconfirm gcc"
        );
        assert_eq!(command_line(PackageManager::Apk, &one, true), "apk add gcc");
    }

    /// Y77: the install runs the commands in order, as the administrator or through `sudo -n`, tells every line
    /// they print, asks nothing, and says how it failed with the last lines when one does.
    #[test]
    fn p3_y77_the_install_runs_the_commands_in_order_and_says_how_it_failed() {
        let base = scratch("y77");
        let here = machine(&base, 0);
        let log = base.join("log");
        program(
            &bin(&here),
            "apt-get",
            &format!(
                "echo \"$* [frontend=$DEBIAN_FRONTEND]\" >> {}\necho \"out: $1\"\necho \"err: $1\" >&2",
                log.display()
            ),
        );
        let packages = vec!["build-essential".to_string(), "python3.12-dev".to_string()];
        let mut said = Vec::new();
        install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Root,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "update [frontend=noninteractive]\ninstall -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev [frontend=noninteractive]\n"
        );
        assert!(
            said.contains(&"running: apt-get update".to_string()),
            "{said:?}"
        );
        assert!(
            said.contains(
                &"running: apt-get install -y build-essential python3.12-dev".to_string()
            ),
            "{said:?}"
        );
        assert!(
            said.contains(&"out: install".to_string())
                && said.contains(&"err: install".to_string()),
            "{said:?}"
        );
        // through sudo: it is told not to ask (-n), and the frontend goes through env
        let _ = std::fs::remove_file(&log);
        let user = machine(&base, 1000);
        program(
            &bin(&user),
            "apt-get",
            &format!(
                "echo \"$* [frontend=$DEBIAN_FRONTEND]\" >> {}",
                log.display()
            ),
        );
        program(
            &bin(&user),
            "sudo",
            &format!("echo \"sudo $*\" >> {}\nshift\nexec \"$@\"", log.display()),
        );
        let mut said = Vec::new();
        install_packages(
            &user,
            PackageManager::Apt,
            &packages,
            Elevation::Sudo,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap();
        let logged = std::fs::read_to_string(&log).unwrap();
        assert!(
            logged.contains("sudo -n env DEBIAN_FRONTEND=noninteractive apt-get update"),
            "{logged}"
        );
        assert!(
            logged.contains("install -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev [frontend=noninteractive]"),
            "{logged}"
        );
        assert!(
            said.contains(&"running: sudo apt-get update".to_string()),
            "{said:?}"
        );
        // a failure says which command, how it ended and what it wrote last
        program(
            &bin(&here),
            "apt-get",
            "echo 'E: Unable to locate package python3.12-dev' >&2; exit 100",
        );
        let mut said = Vec::new();
        let why = install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Root,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        // the refresh failing did not stop it; the install failing did, and is the one told
        assert!(
            said.iter().any(|l| l.starts_with(
                "could not refresh the list of packages (`apt-get update` ended with exit status: 100"
            ) && l.ends_with("going on with the one there is")),
            "{said:?}"
        );
        assert!(
            why.starts_with("`apt-get install -y build-essential python3.12-dev` "),
            "{why}"
        );
        assert!(
            why.contains("exit status: 100") || why.contains("exit code: 100"),
            "{why}"
        );
        assert!(
            why.contains("Unable to locate package python3.12-dev"),
            "{why}"
        );
        // a refresh that fails and an install that works is an install that works
        program(
            &bin(&here),
            "apt-get",
            &format!(
                "echo \"$*\" >> {}\n[ \"$1\" = update ] && {{ echo 'E: Failed to fetch https://repo.example/ Temporary failure' >&2; exit 100; }}\nexit 0",
                log.display()
            ),
        );
        let _ = std::fs::remove_file(&log);
        let mut said = Vec::new();
        install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Root,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "update\ninstall -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev\n"
        );
        assert!(said
            .iter()
            .any(|l| l.contains("could not refresh the list of packages")));
        // nobody who may not
        let why = install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Unavailable,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(why.contains("sudo"), "{why}");
        // a stop between the commands is a stop
        let why = install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Root,
            &mut |_| {},
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert!(why.contains("stopped"), "{why}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y77b: a program the package manager left running (a service a package started keeps the pipes it was given) does
    /// not hold the install: it ends when the command does, and what the command wrote before it ended is passed on.
    #[test]
    fn p3_y77b_a_program_left_running_does_not_hold_the_install() {
        let base = scratch("y77b");
        let here = machine(&base, 0);
        program(
            &bin(&here),
            "apt-get",
            "echo \"before\"\nsleep 5 &\necho \"last words\"\nexit 0",
        );
        let packages = vec!["gcc".to_string()];
        let mut said = Vec::new();
        let started = Instant::now();
        install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Root,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "waited for what was left running: {:?}",
            started.elapsed()
        );
        assert!(said.contains(&"before".to_string()), "{said:?}");
        assert!(said.contains(&"last words".to_string()), "{said:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y78c: apt is told to wait for a lock another process holds (`-o DPkg::Lock::Timeout=120`, on the install and
    /// not on the refresh of the lists, which takes another lock the option does not cover), through `sudo` as well,
    /// and what is printed for a person to type does not say so. Without it a computer that is busy with upgrades of
    /// its own (a rented one in its first hours) ends the install at once; with it the install goes on when the lock
    /// is let go, and when the wait is over first it fails with the manager's words and the plain command.
    #[test]
    fn p3_y78c_apt_waits_for_a_lock_another_process_holds_and_the_printed_command_does_not_say_so()
    {
        let base = scratch("y78c");
        let here = machine(&base, 0);
        let log = base.join("log");
        let held = base.join("held");
        let released = base.join("released");
        let gives_up = base.join("gives-up");
        // apt-get as apt 2.8 was measured with a process holding /var/lib/dpkg/lock-frontend: the install ends at once with
        // code 100 and two lines of `E:` without the option; with it, a line of `Waiting for cache lock` (once a second) and
        // then it goes on when the lock is let go (here: the other process leaves the file `released`) or, when the wait is
        // over first, ends with `E: Unable to acquire the dpkg frontend lock`. Only what the shell has in it is used: the
        // computer of these tests has no other program.
        let body = format!(
            r#"echo "$*" >> {log}
if [ "$1" = install ] && [ -e {held} ] && [ ! -e {released} ]; then
  case " $* " in
    *" DPkg::Lock::Timeout=120 "*)
      echo 'Waiting for cache lock: Could not get lock /var/lib/dpkg/lock-frontend. It is held by process 2535 (unattended-upgr)...'
      if [ -e {gives_up} ]; then
        echo 'E: Unable to acquire the dpkg frontend lock (/var/lib/dpkg/lock-frontend), is another process using it?' >&2
        exit 100
      fi
      : > {released}
      ;;
    *)
      echo 'E: Could not get lock /var/lib/dpkg/lock-frontend. It is held by process 2535 (unattended-upgr)' >&2
      echo 'E: Unable to acquire the dpkg frontend lock (/var/lib/dpkg/lock-frontend), is another process using it?' >&2
      exit 100
      ;;
  esac
fi
echo "installed: $*""#,
            log = log.display(),
            held = held.display(),
            released = released.display(),
            gives_up = gives_up.display()
        );
        program(&bin(&here), "apt-get", &body);
        let packages = vec!["build-essential".to_string(), "python3.12-dev".to_string()];
        let joined = |commands: Vec<Vec<String>>| -> Vec<String> {
            commands.iter().map(|words| words.join(" ")).collect()
        };

        // what a person types has no option; what a program runs has it, on the install only; the other managers are
        // run as they are printed
        assert_eq!(
            joined(PackageManager::Apt.install_commands(&packages)),
            [
                "apt-get update",
                "apt-get install -y build-essential python3.12-dev"
            ]
        );
        assert_eq!(
            joined(PackageManager::Apt.install_commands_to_run(&packages)),
            [
                "apt-get update",
                "apt-get install -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev"
            ]
        );
        for other in [
            PackageManager::Dnf,
            PackageManager::Zypper,
            PackageManager::Pacman,
            PackageManager::Apk,
        ] {
            assert_eq!(
                other.install_commands_to_run(&packages),
                other.install_commands(&packages),
                "{other:?} is run as it is printed"
            );
        }
        assert_eq!(
            command_line(PackageManager::Apt, &packages, true),
            "apt-get update && apt-get install -y build-essential python3.12-dev"
        );

        // the stand-in is not one that always works: without the option it ends at once, as apt does
        std::fs::write(&held, "").unwrap();
        let without = Command::new(bin(&here).join("apt-get"))
            .args(["install", "-y", "gcc"])
            .output()
            .unwrap();
        assert_eq!(without.status.code(), Some(100));
        assert!(String::from_utf8_lossy(&without.stderr).contains("Could not get lock"));
        let _ = std::fs::remove_file(&log);

        // as the administrator: the lock is held, the install waits and goes on when it is let go
        let mut said = Vec::new();
        install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Root,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "update\ninstall -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev\n",
            "the refresh is not given the option"
        );
        assert!(released.exists(), "the lock was let go while it waited");
        assert!(
            said.iter().any(|l| l.starts_with(
                "Waiting for cache lock: Could not get lock /var/lib/dpkg/lock-frontend."
            )),
            "the manager's own words are passed on: {said:?}"
        );
        assert!(
            said.contains(
                &"running: apt-get install -y build-essential python3.12-dev".to_string()
            ),
            "what is said is the plain command: {said:?}"
        );
        assert!(
            said.iter().any(|l| l.starts_with("installed: install -y")),
            "{said:?}"
        );

        // through sudo: the option goes through env with the frontend (the lock is held again)
        let _ = std::fs::remove_file(&log);
        std::fs::remove_file(&released).unwrap();
        let user = machine(&base, 1000);
        program(
            &bin(&user),
            "sudo",
            &format!("echo \"sudo $*\" >> {}\nshift\nexec \"$@\"", log.display()),
        );
        let mut said = Vec::new();
        install_packages(
            &user,
            PackageManager::Apt,
            &packages,
            Elevation::Sudo,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap();
        let logged = std::fs::read_to_string(&log).unwrap();
        assert!(
            logged.contains("sudo -n env DEBIAN_FRONTEND=noninteractive apt-get install -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev"),
            "{logged}"
        );
        assert!(
            said.contains(
                &"running: sudo apt-get install -y build-essential python3.12-dev".to_string()
            ),
            "{said:?}"
        );

        // the wait is over before the lock is let go: it fails with the plain command, the manager's words and what it
        // waited for in the last lines it wrote
        assert!(released.exists(), "the sudo run let the lock go too");
        std::fs::remove_file(&released).unwrap();
        std::fs::write(&gives_up, "").unwrap();
        let mut said = Vec::new();
        let why = install_packages(
            &here,
            PackageManager::Apt,
            &packages,
            Elevation::Root,
            &mut |line| said.push(line),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(
            why.starts_with("`apt-get install -y build-essential python3.12-dev` "),
            "{why}"
        );
        assert!(
            why.contains("Waiting for cache lock")
                && why.contains("Unable to acquire the dpkg frontend lock"),
            "{why}"
        );
        assert!(!released.exists(), "the other process still holds it");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Y78: a script is run by the Python it is given, in the environment it is given, and passes on status 0;
    /// when it does not, how it ended and the last lines of what it wrote are the answer; and it is stopped when
    /// its time is up or the person says stop.
    #[test]
    fn p3_y78_a_script_passes_on_status_0_and_fails_with_the_last_lines_it_wrote() {
        let base = scratch("y78");
        let here = machine(&base, 1000);
        // a stand-in for a Python: runs the script as a shell script
        let python = bin(&here).join("sh");
        let script = base.join("selftest.py");
        std::fs::write(&script, "echo \"home is $HOME\"\necho \"fine\"\n").unwrap();
        let env = ChildEnv {
            remove: vec!["QUICKAI_TEST_GONE".into()],
            set: vec![("HOME".into(), "/engine/home".into())],
        };
        let end = run_script(
            &python,
            &script,
            &env,
            Some(&base),
            Duration::from_secs(20),
            &AtomicBool::new(false),
        );
        assert_eq!(
            end,
            ScriptEnd::Passed(vec!["home is /engine/home".into(), "fine".into()])
        );
        // a failure keeps the last fifteen lines, the compiler's among them
        std::fs::write(
            &script,
            "i=1; while [ $i -le 30 ]; do echo \"line $i\"; i=$((i+1)); done\necho 'fatal error: Python.h: No such file or directory' >&2\nexit 1\n",
        )
        .unwrap();
        let ScriptEnd::Failed { how, lines } = run_script(
            &python,
            &script,
            &env,
            None,
            Duration::from_secs(20),
            &AtomicBool::new(false),
        ) else {
            panic!("status 1 is a failure");
        };
        assert!(
            how.contains("exit status: 1") || how.contains("exit code: 1"),
            "{how}"
        );
        assert_eq!(lines.len(), 15, "{lines:?}");
        assert_eq!(
            lines.last().unwrap(),
            "fatal error: Python.h: No such file or directory"
        );
        // a line as long as the command line of a compiler is cut
        std::fs::write(
            &script,
            "i=0; long=''; while [ $i -lt 400 ]; do long=\"${long}ab\"; i=$((i+1)); done\necho \"$long\" >&2\nexit 1\n",
        )
        .unwrap();
        let ScriptEnd::Failed { lines, .. } = run_script(
            &python,
            &script,
            &env,
            None,
            Duration::from_secs(20),
            &AtomicBool::new(false),
        ) else {
            panic!("status 1 is a failure");
        };
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].len() == 303 && lines[0].ends_with("..."),
            "{}",
            lines[0].len()
        );
        // too slow
        std::fs::write(&script, "sleep 30\n").unwrap();
        let started = Instant::now();
        let ScriptEnd::Failed { how, .. } = run_script(
            &python,
            &script,
            &env,
            None,
            Duration::from_millis(300),
            &AtomicBool::new(false),
        ) else {
            panic!("a script that does not end in time fails");
        };
        assert!(how.contains("had not ended"), "{how}");
        assert!(started.elapsed() < Duration::from_secs(10));
        // stopped by the person
        let stop = AtomicBool::new(true);
        assert_eq!(
            run_script(&python, &script, &env, None, Duration::from_secs(20), &stop),
            ScriptEnd::Stopped
        );
        // a Python that is not there
        let ScriptEnd::Failed { how, .. } = run_script(
            &base.join("no-python"),
            &script,
            &env,
            None,
            Duration::from_secs(5),
            &AtomicBool::new(false),
        ) else {
            panic!("no Python, no test");
        };
        assert!(how.contains("could not be started"), "{how}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
