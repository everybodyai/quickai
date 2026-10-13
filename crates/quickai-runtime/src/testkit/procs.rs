//! Helpers for tests that start real processes: scripts, pid files, and asking whether a process
//! (or a whole process group) is still there.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::time::Duration;

use super::wait_until;

/// Writes an executable shell script `dir/name` and returns its path (Unix).
#[cfg(unix)]
pub fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write a script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// Waits for a file a script writes its pid into, and reads it.
pub fn read_pid(path: &Path, timeout: Duration) -> u32 {
    wait_until(timeout, &format!("{} to appear", path.display()), || {
        std::fs::read_to_string(path).is_ok_and(|t| t.trim().parse::<u32>().is_ok())
    });
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// Whether the process exists and is not a zombie (a child nobody has waited for yet looks alive to
/// `kill -0` for as long as it is).
#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            // "pid (comm) S ...": the state follows the last `)`, which `comm` may itself contain
            Ok(stat) => stat
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.trim_start().chars().next())
                .is_some_and(|state| state != 'Z' && state != 'X'),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// Whether the process exists (Windows): `tasklist` lists it by its id.
#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\"")))
}

/// Waits for `pid` to be gone; `false` if it is still there after `timeout`.
pub fn gone_within(pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    !pid_alive(pid)
}

/// The process group of `pid` (Unix): from `/proc` on Linux, from `ps` where there is none (macOS).
#[cfg(unix)]
pub fn process_group(pid: u32) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = stat.rsplit_once(')')?.1;
        // fields after `comm`: state ppid pgrp ...
        rest.split_whitespace().nth(2)?.parse().ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = std::process::Command::new("ps")
            .args(["-o", "pgid=", "-p", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        String::from_utf8(out.stdout).ok()?.trim().parse().ok()
    }
}

/// Kills whatever is left of a test's processes, so a failed assertion does not leave a `sleep 300`
/// behind.
#[cfg(unix)]
pub fn kill_quietly(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status();
}

/// Runs one `#[ignore]`d test of this very binary in a process of its own, so the test can kill
/// that process (a `SIGKILL` of the application mid-install, a parent that dies under a running
/// engine). The test it runs does nothing unless `QUICKAI_P3_CHILD` is set, so a plain
/// `cargo test -- --ignored` never runs it for nothing.
pub fn spawn_test_child(test: &str, env: &[(&str, &str)]) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            test,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("QUICKAI_P3_CHILD", "1")
        .envs(env.iter().copied())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("start the child test process")
}

/// True in the process `spawn_test_child` made.
pub fn is_test_child() -> bool {
    std::env::var_os("QUICKAI_P3_CHILD").is_some()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testkit::scratch;

    #[test]
    fn a_script_runs_and_a_dead_process_is_reported_gone() {
        let dir = scratch("procs");
        let script = write_script(&dir, "s.sh", "echo $$ > \"$1\"\nexec sleep 30");
        let pidfile = dir.join("pid");
        // a script written a moment ago can be "busy" while another test thread's fork still holds it
        let mut child = crate::runtime::process::spawn_retrying(
            std::process::Command::new(&script).arg(&pidfile),
        )
        .unwrap();
        let pid = read_pid(&pidfile, Duration::from_secs(5));
        assert_eq!(pid, child.id());
        assert!(pid_alive(pid));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(gone_within(pid, Duration::from_secs(5)));
        assert!(!pid_alive(pid));
    }

    #[test]
    fn the_process_group_of_this_process_is_known() {
        let me = std::process::id();
        assert!(process_group(me).is_some_and(|g| g > 0));
        assert_eq!(process_group(u32::MAX - 1), None);
    }
}
