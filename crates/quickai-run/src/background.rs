//! A run that lives until it is stopped (WP4.33.2, docs/remote-runs.md section 3.1): `up --detach` starts this
//! program again as a run of its own, out of the terminal or the SSH connection that asked for it, and returns once
//! it has started; `status` says what runs; `down` stops it. What runs is said by the run's folder
//! (`quickai_runtime::run_file`), the same on Linux, macOS and Windows.

use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use quickai_runtime::run_file::{self, RunFile};

/// How long `up --detach` waits for the run it started to get past its plan, before it returns anyway.
const START_WAIT: Duration = Duration::from_secs(60);

/// How long `down` waits for the run to stop (a program's grace is at most this).
const STOP_WAIT: Duration = Duration::from_secs(120);

/// What is said when another run goes on.
pub fn busy(run: Option<&RunFile>) -> String {
    match run {
        Some(run) if !run.configuration.is_empty() => format!(
            "\"{}\" runs on this computer ({}, {}); stop it first with `quickai-run down`",
            run.configuration,
            run.state,
            for_how_long(run.started)
        ),
        _ => "another run goes on on this computer; stop it first with `quickai-run down`".into(),
    }
}

/// `up --detach`: this program again, with `args` but in the background, its events written to the run's folder
/// `dir`. Returns once the run has made its plan and gone on, or has ended (with its code), or after a minute.
pub fn start(args: &[String], dir: &Path) -> i32 {
    if let Some(run) = run_file::running(dir) {
        eprintln!("quickai-run: {}", busy(Some(&run)));
        return 7;
    }
    let files = std::fs::create_dir_all(dir).and_then(|()| {
        Ok((
            File::create(run_file::events_path(dir))?,
            File::create(dir.join("stderr.log"))?,
        ))
    });
    let (events, errors) = match files {
        Ok(files) => files,
        Err(e) => {
            eprintln!("quickai-run: could not write in {}: {e}", dir.display());
            return 1;
        }
    };
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(e) => {
            eprintln!("quickai-run: could not find this program to start it again: {e}");
            return 1;
        }
    };
    let mut child_args: Vec<String> = args.iter().filter(|a| *a != "--detach").cloned().collect();
    child_args.extend(["--detached".to_string(), "--json".to_string()]);
    let mut command = Command::new(program);
    command
        .args(&child_args)
        .stdin(Stdio::null())
        .stdout(events)
        .stderr(errors);
    let mut child = match spawn_detached(command) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("quickai-run: could not start the run in the background: {e}");
            return 1;
        }
    };
    let asked = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            // it ended before it went on: what it said is the reason
            let last = run_file::last(dir);
            let why = last
                .as_ref()
                .and_then(|l| l.message.clone())
                .unwrap_or_else(|| {
                    format!(
                        "it stopped before it ran; see {} and {}",
                        run_file::events_path(dir).display(),
                        dir.join("stderr.log").display()
                    )
                });
            eprintln!("quickai-run: {why}");
            return last.map_or_else(|| status.code().unwrap_or(1), |l| l.code);
        }
        let run = run_file::read(dir).filter(|r| r.pid == child.id());
        if let Some(run) = run.filter(|r| r.state != "starting") {
            println!(
                "\"{}\" runs in the background (process {}), until `quickai-run down` stops it. \
                 `quickai-run status` says how it goes; what it says is in {}.",
                run.configuration,
                run.pid,
                run_file::events_path(dir).display()
            );
            return 0;
        }
        if asked.elapsed() > START_WAIT {
            println!(
                "The run was started in the background (process {}) and is still making its plan. \
                 `quickai-run status` says how it goes.",
                child.id()
            );
            return 0;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Starts `command` as a program of its own: on Linux and macOS in a session of its own, so that the end of the
/// terminal or the connection is not its end.
#[cfg(unix)]
fn spawn_detached(mut command: Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    // SAFETY: `setsid` is async-signal-safe, and nothing else is done between the fork and the exec
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn()
}

/// Starts `command` as a program of its own: on Windows with no console, in a group of its own, and out of the job
/// Windows' OpenSSH server puts a connection's programs in (the job is closed with the connection). A job that does
/// not let a program out is asked again without it.
#[cfg(windows)]
fn spawn_detached(mut command: Command) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
    };
    let own = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    command.creation_flags(own | CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            command.creation_flags(own);
            command.spawn()
        }
        spawned => spawned,
    }
}

/// `status`: what runs on this computer, or how the last run ended; as JSON (`run_file::Status`), with the last
/// events of a run in the background.
pub fn status(dir: &Path, json: bool) -> i32 {
    let said = run_file::status(dir, 20);
    if json {
        match serde_json::to_string(&said) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("quickai-run: {e}");
                return 1;
            }
        }
        return 0;
    }
    match (&said.running, &said.last) {
        (Some(run), _) => {
            let place = if run.detached {
                "in the background"
            } else {
                "in a terminal"
            };
            println!(
                "\"{}\" runs {place} (process {}, {}), {}.",
                run.configuration,
                run.pid,
                run.state,
                for_how_long(run.started)
            );
            for endpoint in &run.endpoints {
                println!(
                    "  {} ({}): {}",
                    endpoint.program, endpoint.role, endpoint.url
                );
            }
            if let Some(model) = &run.model {
                println!("  the model: {model}");
            }
        }
        (None, Some(last)) => {
            println!("Nothing runs.");
            let why = last
                .message
                .as_deref()
                .map(|m| format!(": {m}"))
                .unwrap_or_default();
            println!(
                "The last run, of \"{}\", ended {} ago ({}{why}).",
                last.configuration,
                how_long(run_file::now().saturating_sub(last.ended)),
                last.outcome
            );
        }
        (None, None) => println!("Nothing runs."),
    }
    0
}

/// `down`: asks the run to stop, and waits until it has (`wait`).
pub fn down(dir: &Path, wait: bool) -> i32 {
    let Some(run) = run_file::running(dir) else {
        println!("Nothing runs.");
        return 0;
    };
    if let Err(e) = run_file::ask_stop(dir) {
        eprintln!("quickai-run: could not ask the run to stop: {e}");
        return 1;
    }
    if !wait {
        println!("\"{}\" was asked to stop.", run.configuration);
        return 0;
    }
    let asked = Instant::now();
    while run_file::running(dir).is_some() {
        if asked.elapsed() > STOP_WAIT {
            eprintln!(
                "quickai-run: \"{}\" has not stopped in {} seconds; it goes on stopping.",
                run.configuration,
                STOP_WAIT.as_secs()
            );
            return 1;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("\"{}\" stopped.", run.configuration);
    0
}

/// `for 3 minutes`, since `started` (seconds since 1970).
fn for_how_long(started: u64) -> String {
    format!("for {}", how_long(run_file::now().saturating_sub(started)))
}

/// A length of time in words: `40 seconds`, `3 minutes`, `2 hours`, `5 days`.
fn how_long(seconds: u64) -> String {
    let (n, unit) = match seconds {
        0..=59 => (seconds, "second"),
        60..=3599 => (seconds / 60, "minute"),
        3600..=86_399 => (seconds / 3600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    if n == 1 {
        format!("1 {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_of_time_is_said_in_words() {
        assert_eq!(how_long(1), "1 second");
        assert_eq!(how_long(40), "40 seconds");
        assert_eq!(how_long(180), "3 minutes");
        assert_eq!(how_long(7200), "2 hours");
        assert_eq!(how_long(86_400 * 5), "5 days");
    }
}
