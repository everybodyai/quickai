//! The `spawn@1` primitive's process side: starting a program the way a run plan says, keeping it
//! and everything it starts from outliving QuickAI, stopping it, and streaming its output into the
//! log (docs/architecture.md, section 11.2). The thread-plus-channel idiom is the one
//! `downloader.rs` and the proxy use: workers send `Event`s, the UI drains them once per frame.
//!
//! What "hygiene" means here, by platform:
//!
//! * **Unix.** A program whose document says `group = true` leads a process group of its own, so
//!   one signal reaches it and the runners it started (Ollama's `llama-server`). On Linux every
//!   program, with a group or without, is told to follow QuickAI if QuickAI dies
//!   (`PR_SET_PDEATHSIG`, with SIGTERM: only a graceful stop of `ollama serve` takes its runner
//!   down, A3, and `llama-server` leaves on it too). The flag fires when the *thread* that
//!   started the program exits, so the supervisor thread that starts it lives as long as the
//!   program does.
//! * **Windows.** Every program is put in a job object that kills what is in it when QuickAI's
//!   handle to it closes, and is started with no console window. There is no graceful signal a
//!   GUI program can send, so a graceful stop is a kill of the job.
//! * **macOS** has process groups but no parent-death signal: a program stays running if QuickAI
//!   is killed outright, and is stopped by Stop and by the exit path.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use quickai_core::loaderenv;
use quickai_core::plan::{apply_env, SpawnPlan};

use crate::events::Event;

/// Tells the interface something happened so it repaints promptly.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Sends `event` to the UI and wakes it so it repaints promptly.
pub fn send(tx: &Sender<Event>, event: Event, wake: &Wake) {
    let _ = tx.send(event);
    wake();
}

/// Asks the OS for a free loopback port by briefly binding to port 0, then
/// releasing it so the engine can bind it moments later. There's a small
/// theoretical race if something else grabs the exact same port in between,
/// same as any "find a free port" trick — if it happens, the engine just
/// fails to bind and that surfaces in the log pane like any other failure.
pub fn ephemeral_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr().map(|a| a.port())
}

/// Set once a reader has seen the end of its pipe: every line the program wrote is in the log.
pub type ReaderDone = Arc<AtomicBool>;

/// Forwards each line of a child's output to the log, on a thread of its own. The returned flag is
/// set when the pipe has been read to its end.
pub fn spawn_reader<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    evt_tx: Sender<Event>,
    wake: Wake,
) -> ReaderDone {
    let done = Arc::new(AtomicBool::new(false));
    let Some(pipe) = pipe else {
        done.store(true, Ordering::SeqCst);
        return done;
    };
    let flag = done.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(pipe);
        for line in reader.lines() {
            match line {
                Ok(line) => send(&evt_tx, Event::Log(line), &wake),
                Err(_) => break,
            }
        }
        flag.store(true, Ordering::SeqCst);
    });
    done
}

/// Waits until every reader is done or `limit` has passed (a grandchild that kept the pipe open
/// must not hold up the end of a run).
pub fn wait_for_readers(readers: &[ReaderDone], limit: Duration) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline && !readers.iter().all(|r| r.load(Ordering::SeqCst)) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What a console program started by a GUI program needs not to open a window of its own.
#[cfg(any(windows, test))]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The process creation flags of every spawn on Windows. A pure function so that the choice is
/// tested on every platform, although only Windows uses it.
#[cfg(any(windows, test))]
pub fn windows_creation_flags() -> u32 {
    CREATE_NO_WINDOW
}

/// The environment a child is started with, as changes to what this process has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChildEnv {
    /// Variables to take out of the inherited environment.
    pub remove: Vec<String>,
    /// Variables to set, in order (a later one replaces an earlier one).
    pub set: Vec<(String, String)>,
}

/// Works out the child's environment from the plan and the environment this process has (`get`).
/// A program that was downloaded (the plan says `scrub_loader_env`) first loses the loader
/// variables that point into QuickAI's own package; the plan's operations then apply to what is
/// left, so a `prepend` goes in front of the scrubbed value.
pub fn child_env(spawn: &SpawnPlan, get: &dyn Fn(&str) -> Option<String>) -> ChildEnv {
    let changes = if spawn.scrub_loader_env {
        loaderenv::scrub(get)
    } else {
        Vec::new()
    };
    let inherited = |name: &str| match changes.iter().find(|c| c.name == name) {
        Some(change) => change.value.clone(),
        None => get(name),
    };
    let mut env = ChildEnv::default();
    for change in &changes {
        match &change.value {
            Some(value) => env.set.push((change.name.clone(), value.clone())),
            None => env.remove.push(change.name.clone()),
        }
    }
    env.set.extend(apply_env(&spawn.env, inherited));
    env
}

/// Takes the loader variables of QuickAI's own package (an AppImage's or a snap's library
/// directories) out of the environment of `cmd`: for a program that was downloaded, which has
/// libraries of its own.
pub fn without_package_loader_env(cmd: &mut Command) {
    for change in loaderenv::scrub(&|name| std::env::var(name).ok()) {
        match change.value {
            Some(value) => cmd.env(change.name, value),
            None => cmd.env_remove(change.name),
        };
    }
}

/// What a program that was run to completion said and how it ended.
pub struct Captured {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Why `run_captured` has no answer.
pub enum RunFailure {
    /// The program could not be started.
    Start(std::io::Error),
    /// It had not finished when the time was up, and was killed.
    TimedOut,
    /// It could not be waited for.
    Wait(std::io::Error),
}

/// Runs `cmd` to its end with no input, collecting what it prints (the first 64 KB of each stream)
/// and killing it when `timeout` passes. A program that was written a moment ago may be busy for a
/// few milliseconds, which `spawn_retrying` allows for.
pub fn run_captured(cmd: &mut Command, timeout: Duration) -> Result<Captured, RunFailure> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_retrying(cmd).map_err(RunFailure::Start)?;
    fn capture<R: std::io::Read + Send + 'static>(
        pipe: Option<R>,
    ) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(64 * 1024).read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    }
    let stdout = capture(child.stdout.take());
    let stderr = capture(child.stderr.take());

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunFailure::TimedOut);
            }
            Err(e) => return Err(RunFailure::Wait(e)),
        }
    };
    Ok(Captured {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// A program QuickAI started, and everything that comes with it.
pub struct Daemon {
    child: Child,
    /// It leads a process group of its own (Unix); signals go to the group.
    group: bool,
    /// Set once the child has been reaped: its id may be reused from then on.
    exited: bool,
    #[cfg(windows)]
    job: Option<job::Job>,
}

impl Daemon {
    /// Starts the program of `spawn` with `env`, output piped. On a failure nothing was started.
    pub fn spawn(spawn: &SpawnPlan, env: &ChildEnv) -> std::io::Result<Daemon> {
        let mut cmd = Command::new(&spawn.program.value);
        cmd.args(spawn.args.iter().map(|a| &a.value))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in &env.remove {
            cmd.env_remove(name);
        }
        cmd.envs(env.set.iter().map(|(k, v)| (k, v)));

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            if spawn.group {
                // A program in a group of its own is a background group: it must not read the
                // terminal QuickAI may have been started from.
                cmd.stdin(Stdio::null());
                cmd.process_group(0);
            }
            #[cfg(target_os = "linux")]
            // SAFETY: the closure runs between fork and exec and calls only prctl, getppid and
            // their error conversion, which are safe there.
            unsafe {
                let parent = libc::getpid();
                cmd.pre_exec(move || follow_the_parent(parent));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(windows_creation_flags());
        }

        let child = spawn_retrying(&mut cmd)?;
        Ok(Daemon {
            #[cfg(windows)]
            job: job::Job::around(&child),
            child,
            group: cfg!(unix) && spawn.group,
            exited: false,
        })
    }

    #[cfg(test)]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn take_output(&mut self) -> (Option<ChildStdout>, Option<ChildStderr>) {
        (self.child.stdout.take(), self.child.stderr.take())
    }

    /// `Some` once the program has exited.
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.exited = true;
        }
        Ok(status)
    }

    /// Waits for the program to exit.
    pub fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.exited = true;
        Ok(status)
    }

    /// Asks it to leave: SIGTERM to its process group, or to the process when it leads none.
    /// Windows has no such signal for a GUI program to send, so there this ends it.
    pub fn terminate(&mut self) {
        #[cfg(unix)]
        self.signal(libc::SIGTERM);
        #[cfg(not(unix))]
        self.kill();
    }

    /// Ends it and everything in its group (SIGKILL) or job.
    pub fn kill(&mut self) {
        #[cfg(unix)]
        {
            self.signal(libc::SIGKILL);
            if !self.exited {
                let _ = self.child.kill();
            }
        }
        #[cfg(windows)]
        {
            if let Some(job) = &self.job {
                job.terminate();
            }
            if !self.exited {
                let _ = self.child.kill();
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = self.child.kill();
        }
    }

    /// Ends whatever the program left running after it exited: its group, or its job. Called as
    /// soon as the exit is seen, while the group's id cannot have been given to anything else.
    pub fn sweep(&mut self) {
        #[cfg(unix)]
        if self.group {
            // SAFETY: a plain signal to a process group; ESRCH (nothing left) is the usual answer.
            unsafe {
                libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
    }

    #[cfg(unix)]
    fn signal(&self, signal: libc::c_int) {
        let pid = self.child.id() as libc::pid_t;
        let target = match (self.group, self.exited) {
            (true, _) => -pid,
            // a process that has been reaped has no id any more
            (false, true) => return,
            (false, false) => pid,
        };
        // SAFETY: a plain signal to a process or group this Daemon started.
        unsafe {
            libc::kill(target, signal);
        }
    }
}

/// Starts `cmd`, trying again for a moment when the program is "busy". A file that was written a
/// moment ago (an installer's output, a test's script) can still be open for writing in a process
/// that another thread has just forked and that has not yet executed its program, and Unix
/// refuses to execute a file that is open for writing (ETXTBSY) until that child lets go, which
/// takes milliseconds.
pub fn spawn_retrying(cmd: &mut Command) -> std::io::Result<Child> {
    let mut tries = 0;
    loop {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

/// Runs in the child between fork and exec on Linux: ask the kernel to send SIGTERM when the
/// parent thread dies, and give up if the parent has gone already, since the signal would never
/// come.
#[cfg(target_os = "linux")]
fn follow_the_parent(parent: libc::pid_t) -> std::io::Result<()> {
    // SAFETY: prctl and getppid take plain integers and have no preconditions.
    unsafe {
        if libc::prctl(
            libc::PR_SET_PDEATHSIG,
            libc::SIGTERM as libc::c_ulong,
            0,
            0,
            0,
        ) == -1
        {
            return Err(std::io::Error::last_os_error());
        }
        if libc::getppid() != parent {
            return Err(std::io::Error::other(
                "QuickAI exited while starting the program",
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
mod job {
    //! A job object that ends everything in it when the last handle to it closes.

    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct Job(HANDLE);

    // SAFETY: a job handle is just a kernel object reference; any thread may use it.
    unsafe impl Send for Job {}

    impl Job {
        /// A job holding `child`, or `None` when Windows would not make one (the program then
        /// runs as it would have without: stopped by Stop, not by the end of QuickAI).
        pub fn around(child: &Child) -> Option<Job> {
            // SAFETY: plain calls of the Win32 API with a handle this function owns; the struct is
            // zeroed, which is its documented initial state.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let job = Job(handle);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                let assigned = AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE);
                (set != 0 && assigned != 0).then_some(job)
            }
        }

        pub fn terminate(&self) {
            // SAFETY: the handle is the one this Job owns.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closing the handle this Job owns, once; with KILL_ON_JOB_CLOSE this ends
            // whatever is still in the job.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::plan::{EnvMode, EnvOp, Traced};

    fn plan_with(env: Vec<EnvOp>, scrub: bool) -> SpawnPlan {
        SpawnPlan {
            program: Traced {
                value: "p".into(),
                origin: "t".into(),
            },
            args: vec![],
            env,
            group: false,
            scrub_loader_env: scrub,
            identity: None,
        }
    }

    fn op(name: &str, mode: EnvMode) -> EnvOp {
        EnvOp {
            name: name.into(),
            mode,
            origin: "t".into(),
        }
    }

    fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    /// K08. That no console window opens is A8, which only a Windows run can observe.
    #[test]
    fn p3_k08_every_spawn_carries_create_no_window_on_windows_and_the_flag_set_is_pure() {
        // checked on every platform; only Windows passes the flags on (A8 is the observation)
        assert_eq!(
            windows_creation_flags() & CREATE_NO_WINDOW,
            CREATE_NO_WINDOW
        );
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    }

    #[test]
    fn the_plans_variables_are_set_and_nothing_is_removed_when_nothing_is_scrubbed() {
        let plan = plan_with(
            vec![
                op("A", EnvMode::Set { value: "1".into() }),
                op(
                    "LD_LIBRARY_PATH",
                    EnvMode::Prepend {
                        value: "/opt/llama".into(),
                        sep: ":".into(),
                    },
                ),
            ],
            false,
        );
        let env = child_env(
            &plan,
            &environment(&[
                ("APPDIR", "/tmp/.mount_x"),
                ("LD_LIBRARY_PATH", "/tmp/.mount_x/usr/lib:/usr/lib"),
            ]),
        );
        // llama.cpp keeps what it always did: its directory goes in front of everything inherited
        assert!(env.remove.is_empty());
        assert_eq!(
            env.set,
            [
                ("A".to_string(), "1".to_string()),
                (
                    "LD_LIBRARY_PATH".to_string(),
                    "/opt/llama:/tmp/.mount_x/usr/lib:/usr/lib".to_string()
                )
            ]
        );
    }

    #[test]
    fn a_downloaded_program_does_not_inherit_the_packages_libraries() {
        let plan = plan_with(
            vec![op(
                "LD_LIBRARY_PATH",
                EnvMode::Prepend {
                    value: "/opt/ollama/lib".into(),
                    sep: ":".into(),
                },
            )],
            true,
        );
        let env = child_env(
            &plan,
            &environment(&[
                ("APPDIR", "/tmp/.mount_x"),
                ("LD_LIBRARY_PATH", "/tmp/.mount_x/usr/lib:/usr/lib"),
                ("LD_PRELOAD", "/tmp/.mount_x/usr/lib/libx.so"),
            ]),
        );
        assert_eq!(env.remove, ["LD_PRELOAD"]);
        assert_eq!(
            env.set,
            [
                ("LD_LIBRARY_PATH".to_string(), "/usr/lib".to_string()),
                (
                    "LD_LIBRARY_PATH".to_string(),
                    "/opt/ollama/lib:/usr/lib".to_string()
                )
            ],
            "the prepend goes in front of the scrubbed value, and comes last so it wins"
        );
    }

    #[test]
    fn a_downloaded_program_outside_a_package_is_left_alone() {
        let plan = plan_with(vec![op("A", EnvMode::Set { value: "1".into() })], true);
        let env = child_env(&plan, &environment(&[("LD_LIBRARY_PATH", "/usr/lib")]));
        assert_eq!(env.remove, Vec::<String>::new());
        assert_eq!(env.set, [("A".to_string(), "1".to_string())]);
    }

    /// K09: Windows has no process groups to signal, so what keeps a daemon's runner from outliving
    /// it is the job object. These tests run on Windows (CI); the daemon is this very test binary
    /// started with a filter that makes one of the helper tests below run.
    #[cfg(windows)]
    mod windows {
        use super::*;
        use crate::testkit::procs::{gone_within, pid_alive, read_pid};
        use crate::testkit::scratch;
        use quickai_core::plan::Traced;

        const DIR_VARIABLE: &str = "QUICKAI_K09_DIR";
        const FAMILY: &str = "runtime::process::tests::windows::k09_helper_family";
        const SLEEPER: &str = "runtime::process::tests::windows::k09_helper_sleeper";

        fn helper_plan(test: &str, dir: &std::path::Path) -> SpawnPlan {
            SpawnPlan {
                program: Traced {
                    value: std::env::current_exe().unwrap().display().to_string(),
                    origin: "t".into(),
                },
                args: [test, "--exact", "--ignored", "--nocapture"]
                    .iter()
                    .map(|a| Traced {
                        value: a.to_string(),
                        origin: "t".into(),
                    })
                    .collect(),
                env: vec![EnvOp {
                    name: DIR_VARIABLE.into(),
                    mode: EnvMode::Set {
                        value: dir.display().to_string(),
                    },
                    origin: "t".into(),
                }],
                group: true,
                scrub_loader_env: false,
                identity: None,
            }
        }

        /// The daemon: starts a runner of its own, writes both ids down and waits.
        #[test]
        #[ignore = "runs only as the daemon of p3_k09"]
        fn k09_helper_family() {
            let Some(dir) = std::env::var_os(DIR_VARIABLE) else {
                return;
            };
            let dir = std::path::PathBuf::from(dir);
            let mut runner = Command::new(std::env::current_exe().unwrap())
                .args([SLEEPER, "--exact", "--ignored", "--nocapture"])
                .env(DIR_VARIABLE, &dir)
                .spawn()
                .unwrap();
            std::fs::write(dir.join("runner.pid"), runner.id().to_string()).unwrap();
            std::fs::write(dir.join("parent.pid"), std::process::id().to_string()).unwrap();
            // like a daemon: stay for as long as the runner does
            let _ = runner.wait();
        }

        /// The runner.
        #[test]
        #[ignore = "runs only as the runner of p3_k09"]
        fn k09_helper_sleeper() {
            if std::env::var_os(DIR_VARIABLE).is_some() {
                std::thread::sleep(Duration::from_secs(300));
            }
        }

        fn running_family() -> (crate::testkit::Scratch, Daemon, u32, u32) {
            let dir = scratch("job");
            let plan = helper_plan(FAMILY, &dir);
            let env = child_env(&plan, &|name| std::env::var(name).ok());
            let daemon = Daemon::spawn(&plan, &env).expect("the test binary starts");
            let parent = read_pid(&dir.join("parent.pid"), Duration::from_secs(60));
            let runner = read_pid(&dir.join("runner.pid"), Duration::from_secs(60));
            assert!(pid_alive(parent) && pid_alive(runner));
            (dir, daemon, parent, runner)
        }

        #[test]
        fn p3_k09_the_daemon_is_in_a_job_and_neither_it_nor_its_child_outlives_stop_or_the_app() {
            // Stop
            let (_dir, mut daemon, parent, runner) = running_family();
            daemon.terminate();
            assert!(
                gone_within(parent, Duration::from_secs(30)),
                "the daemon survived Stop"
            );
            assert!(
                gone_within(runner, Duration::from_secs(30)),
                "the daemon's child survived Stop"
            );
            drop(daemon);

            // the end of the application: the last handle to the job closes
            let (_dir, daemon, parent, runner) = running_family();
            drop(daemon);
            assert!(
                gone_within(parent, Duration::from_secs(30)),
                "the daemon outlived the application"
            );
            assert!(
                gone_within(runner, Duration::from_secs(30)),
                "the daemon's child outlived the application"
            );
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::testkit::procs::{gone_within, pid_alive, read_pid, write_script};
        use crate::testkit::scratch;
        use quickai_core::plan::Traced;

        fn script_plan(path: &std::path::Path, group: bool) -> SpawnPlan {
            SpawnPlan {
                program: Traced {
                    value: path.display().to_string(),
                    origin: "t".into(),
                },
                args: vec![],
                env: vec![],
                group,
                scrub_loader_env: false,
                identity: None,
            }
        }

        /// A parent that starts a child of its own, writes both pids down and waits; `trap` is what
        /// it does on SIGTERM (nothing, for a program that ignores it: the child then ignores it
        /// too, so the trap is set before the child starts).
        fn family(dir: &std::path::Path, trap: &str) -> std::path::PathBuf {
            write_script(
                dir,
                "family",
                &format!(
                    "d=$(dirname \"$0\")\ntrap '{trap}' TERM\nsleep 300 &\nrunner=$!\n\
                     echo $$ > \"$d/parent.pid\"\necho $runner > \"$d/runner.pid\"\n\
                     while kill -0 $runner 2>/dev/null; do wait $runner; done\n"
                ),
            )
        }

        #[test]
        fn a_group_is_led_by_the_program_and_ended_with_it() {
            let dir = scratch("daemon");
            let script = family(&dir, "kill $runner; exit 0");
            let mut daemon = Daemon::spawn(&script_plan(&script, true), &ChildEnv::default())
                .expect("the script starts");
            let (parent, runner) = (
                read_pid(&dir.join("parent.pid"), Duration::from_secs(5)),
                read_pid(&dir.join("runner.pid"), Duration::from_secs(5)),
            );
            assert_eq!(parent, daemon.id());
            assert_eq!(
                crate::testkit::procs::process_group(parent),
                Some(parent),
                "the program leads its own group"
            );
            assert_eq!(crate::testkit::procs::process_group(runner), Some(parent));

            daemon.terminate();
            let status = wait_until_exit(&mut daemon, Duration::from_secs(5)).expect("it left");
            assert!(status.success(), "it obeyed SIGTERM: {status}");
            daemon.sweep();
            assert!(gone_within(parent, Duration::from_secs(2)));
            assert!(gone_within(runner, Duration::from_secs(2)));
        }

        #[test]
        fn a_kill_takes_a_program_that_ignores_sigterm_and_its_child() {
            let dir = scratch("daemon");
            let script = family(&dir, "");
            let mut daemon = Daemon::spawn(&script_plan(&script, true), &ChildEnv::default())
                .expect("the script starts");
            let (parent, runner) = (
                read_pid(&dir.join("parent.pid"), Duration::from_secs(5)),
                read_pid(&dir.join("runner.pid"), Duration::from_secs(5)),
            );
            daemon.terminate();
            std::thread::sleep(Duration::from_millis(150));
            assert!(
                daemon.try_wait().unwrap().is_none(),
                "it ignored SIGTERM, so it is still there"
            );
            daemon.kill();
            wait_until_exit(&mut daemon, Duration::from_secs(5)).expect("it was killed");
            assert!(gone_within(parent, Duration::from_secs(2)));
            assert!(gone_within(runner, Duration::from_secs(2)));
        }

        #[test]
        fn a_sweep_after_the_exit_ends_what_the_program_left_behind() {
            let dir = scratch("daemon");
            // the parent leaves at once and does not take its child with it
            let script = write_script(
                &dir,
                "leaver",
                "d=$(dirname \"$0\")\nsleep 300 &\necho $! > \"$d/runner.pid\"\nexit 0\n",
            );
            let mut daemon = Daemon::spawn(&script_plan(&script, true), &ChildEnv::default())
                .expect("the script starts");
            wait_until_exit(&mut daemon, Duration::from_secs(5)).expect("it left");
            let runner = read_pid(&dir.join("runner.pid"), Duration::from_secs(5));
            assert!(pid_alive(runner), "the child outlived its parent");
            daemon.sweep();
            assert!(
                gone_within(runner, Duration::from_secs(2)),
                "the sweep left the child running"
            );
        }

        #[test]
        fn a_program_that_leads_no_group_is_signalled_alone() {
            let dir = scratch("daemon");
            let script = family(&dir, "exit 0");
            let mut daemon = Daemon::spawn(&script_plan(&script, false), &ChildEnv::default())
                .expect("the script starts");
            let (parent, runner) = (
                read_pid(&dir.join("parent.pid"), Duration::from_secs(5)),
                read_pid(&dir.join("runner.pid"), Duration::from_secs(5)),
            );
            assert_ne!(
                crate::testkit::procs::process_group(parent),
                Some(parent),
                "without `group` the program shares QuickAI's group"
            );
            daemon.kill();
            wait_until_exit(&mut daemon, Duration::from_secs(5)).expect("it was killed");
            assert!(gone_within(parent, Duration::from_secs(2)));
            // llama.cpp's way: one process, killed at once, and nothing else is touched
            assert!(pid_alive(runner));
            crate::testkit::procs::kill_quietly(runner);
        }

        #[test]
        fn the_child_gets_the_environment_it_was_given() {
            let dir = scratch("daemon");
            let script = write_script(
                &dir,
                "envdump",
                "d=$(dirname \"$0\")\nenv > \"$d/env.record\"\n",
            );
            // HOME is in every test environment this runs in, and the script needs nothing from it
            let env = ChildEnv {
                remove: vec!["HOME".into()],
                set: vec![("QUICKAI_TEST_SET".into(), "yes".into())],
            };
            let mut daemon = Daemon::spawn(&script_plan(&script, false), &env).unwrap();
            wait_until_exit(&mut daemon, Duration::from_secs(5)).expect("it ran");
            let record = std::fs::read_to_string(dir.join("env.record")).unwrap();
            assert!(
                record.lines().any(|l| l == "QUICKAI_TEST_SET=yes"),
                "{record}"
            );
            assert!(
                !record.lines().any(|l| l.starts_with("HOME=")),
                "an inherited variable the plan removes reached the program: {record}"
            );
        }

        #[test]
        fn a_program_that_is_busy_for_a_moment_is_started_when_it_is_not() {
            let dir = scratch("daemon");
            let script = write_script(&dir, "busy", "exit 0");
            // a file open for writing cannot be executed: this is what a script written a moment
            // ago looks like while a forked child still holds a copy of its descriptor
            let writer = std::fs::OpenOptions::new()
                .write(true)
                .open(&script)
                .unwrap();
            let mut cmd = Command::new(&script);
            let err = cmd.spawn().expect_err("a file open for writing is busy");
            assert_eq!(err.kind(), std::io::ErrorKind::ExecutableFileBusy, "{err}");

            let release = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(120));
                drop(writer);
            });
            let started = Instant::now();
            let mut child = spawn_retrying(&mut cmd).expect("it starts once it is not busy");
            assert!(started.elapsed() >= Duration::from_millis(100));
            assert!(child.wait().unwrap().success());
            release.join().unwrap();

            // any other failure is reported at once, not retried
            let started = Instant::now();
            assert!(spawn_retrying(&mut Command::new("/nonexistent/quickai-daemon")).is_err());
            assert!(started.elapsed() < Duration::from_millis(100));
        }

        #[test]
        fn a_program_that_does_not_exist_is_an_error_not_a_panic() {
            let plan = script_plan(std::path::Path::new("/nonexistent/quickai-daemon"), true);
            assert!(Daemon::spawn(&plan, &ChildEnv::default()).is_err());
        }

        fn wait_until_exit(daemon: &mut Daemon, limit: Duration) -> Option<ExitStatus> {
            let deadline = Instant::now() + limit;
            while Instant::now() < deadline {
                if let Some(status) = daemon.try_wait().unwrap() {
                    return Some(status);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        }
    }
}
