//! The programs that listen on ports of this computer: which one holds a port, and stopping it so that a run can
//! have the port (docs/remote-runs.md section 3.2: Override, and Stop it, for a program QuickAI did not start). A
//! program of the person is asked to end, then ended. A system service (an Ollama installed as one) is stopped by its
//! service manager through `sudo -n` when that needs no password; otherwise what stops it is said, for the person to
//! run. `quickai-run free` does it on a computer the window set up; the window does it here.

use std::process::Command;
use std::time::{Duration, Instant};

use serde::Serialize;

/// What was found on a port, and what was done.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Freed {
    pub port: u16,
    /// The program that listened on it, when it could be told.
    pub program: Option<String>,
    pub pid: Option<u32>,
    /// Nothing listens on it now.
    pub free: bool,
    /// What to do when it could not be freed here: the command to run as the administrator.
    pub hint: Option<String>,
}

/// The program that listens on `port`, in a few words (`ollama (process 1234)`), when it can be told.
pub fn holder(port: u16) -> Option<String> {
    #[cfg(unix)]
    let found = listener(port).map(|l| (l.pid, Some(l.name)));
    #[cfg(windows)]
    let found = windows_listener(port);
    let (pid, name) = found?;
    Some(match name {
        Some(name) => format!("{name} (process {pid})"),
        None => format!("process {pid}"),
    })
}

/// Whether something listens on `port` of the loopback.
pub fn taken(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// Waits a little for `port` to be let go of.
fn freed_within(port: u16, wait: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < wait {
        if !taken(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    !taken(port)
}

/// Runs `program` with `args`, and whether it ended well.
fn ran(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// What `program` with `args` wrote, when it ended well.
fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Stops the program that listens on `port`, and says what was done.
#[cfg(unix)]
pub fn free_port(port: u16) -> Freed {
    let mut freed = Freed {
        port,
        program: None,
        pid: None,
        free: !taken(port),
        hint: None,
    };
    if freed.free {
        return freed;
    }
    let found = listener(port);
    freed.pid = found.as_ref().map(|l| l.pid);
    freed.program = found.as_ref().map(|l| l.name.clone());
    match found {
        // a service: its manager stops it, or it would start it again
        Some(Listener {
            service: Some(unit),
            ..
        }) => {
            #[cfg(target_os = "linux")]
            let (stop, said) = (
                ["-n", "systemctl", "stop", unit.as_str()],
                format!("sudo systemctl stop {unit}"),
            );
            #[cfg(not(target_os = "linux"))]
            let (stop, said) = (
                ["-n", "launchctl", "bootout", unit.as_str()],
                format!("sudo launchctl bootout {unit}"),
            );
            if ran("sudo", &stop) && freed_within(port, Duration::from_secs(20)) {
                freed.free = true;
            } else {
                freed.hint = Some(format!(
                    "it is the service {unit}: stop it as the administrator ({said}), then start again."
                ));
            }
        }
        // a program of this person: asked to end, then ended
        Some(listener) if listener.own => {
            // SAFETY: a signal to a process of this person, by its id
            unsafe { libc::kill(listener.pid as libc::pid_t, libc::SIGTERM) };
            if !freed_within(port, Duration::from_secs(10)) {
                unsafe { libc::kill(listener.pid as libc::pid_t, libc::SIGKILL) };
            }
            freed.free = freed_within(port, Duration::from_secs(5));
            if !freed.free {
                freed.hint = Some(format!("process {} would not end.", listener.pid));
            }
        }
        Some(listener) => {
            if ran("sudo", &["-n", "kill", &listener.pid.to_string()])
                && freed_within(port, Duration::from_secs(10))
            {
                freed.free = true;
            } else {
                freed.hint = Some(format!(
                    "it is a program of another user: stop it as the administrator (sudo kill {}), then start again.",
                    listener.pid
                ));
            }
        }
        None => {
            freed.hint = Some(format!(
                "the program that holds it could not be told: stop it as the administrator (on Linux, \
                 sudo ss -ltnp 'sport = :{port}' names it), then start again."
            ));
        }
    }
    freed
}

/// A program that listens on a port.
#[cfg(unix)]
struct Listener {
    pid: u32,
    name: String,
    /// It is this person's.
    own: bool,
    /// The service it belongs to, when a service manager started it (`ollama.service`).
    service: Option<String>,
}

/// The program that listens on `port`: this person's are found by `/proc` on Linux and `lsof` on macOS; another
/// user's only when `sudo -n` may ask (`ss` on Linux, `lsof` on macOS).
#[cfg(unix)]
fn listener(port: u16) -> Option<Listener> {
    let own_uid = unsafe { libc::getuid() };
    #[cfg(target_os = "linux")]
    let pid = linux_own_listener(port).or_else(|| {
        let said = output("sudo", &["-n", "ss", "-ltnpH", &format!("sport = :{port}")])?;
        // users:(("ollama",pid=1234,fd=3))
        let at = said.find("pid=")? + 4;
        said[at..]
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    })?;
    #[cfg(not(target_os = "linux"))]
    let pid = {
        let args = ["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"];
        let said = output("lsof", &args).or_else(|| {
            let mut sudo = vec!["-n", "lsof"];
            sudo.extend(args);
            output("sudo", &sudo)
        })?;
        said.lines().next()?.trim().parse().ok()?
    };
    Some(Listener {
        pid,
        name: process_name(pid).unwrap_or_else(|| format!("process {pid}")),
        own: process_uid(pid) == Some(own_uid),
        service: service_of(pid),
    })
}

/// The process of this person that listens on `port`, from the sockets `/proc/net/tcp` lists and the files of the
/// processes it may read.
#[cfg(target_os = "linux")]
fn linux_own_listener(port: u16) -> Option<u32> {
    let mut inodes = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let words: Vec<&str> = line.split_whitespace().collect();
            // local address, state 0A (listen), inode
            let (Some(local), Some(state), Some(inode)) =
                (words.get(1), words.get(3), words.get(9))
            else {
                continue;
            };
            let here = local
                .rsplit_once(':')
                .and_then(|(_, p)| u16::from_str_radix(p, 16).ok());
            if *state == "0A" && here == Some(port) {
                inodes.push(format!("socket:[{inode}]"));
            }
        }
    }
    if inodes.is_empty() {
        return None;
    }
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path()) {
                if inodes.iter().any(|i| target.to_string_lossy() == *i) {
                    return Some(pid);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_name(pid: u32) -> Option<String> {
    output("ps", &["-o", "comm=", "-p", &pid.to_string()])
        .map(|s| s.trim().rsplit('/').next().unwrap_or_default().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "linux")]
fn process_uid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_uid(pid: u32) -> Option<u32> {
    output("ps", &["-o", "uid=", "-p", &pid.to_string()])?
        .trim()
        .parse()
        .ok()
}

/// The systemd service a process belongs to, from its control group (`/system.slice/ollama.service`); a service of
/// the person's own session (`/user.slice/...`) is not one to stop as the administrator.
#[cfg(target_os = "linux")]
fn service_of(pid: u32) -> Option<String> {
    let groups = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    groups.lines().find_map(|line| {
        let path = line.rsplit(':').next()?;
        let unit = path.strip_prefix("/system.slice/")?.split('/').next()?;
        unit.ends_with(".service").then(|| unit.to_string())
    })
}

/// A launchd service a process belongs to, when it is one of the system's (`system/com.example.ollama`).
#[cfg(all(unix, not(target_os = "linux")))]
fn service_of(pid: u32) -> Option<String> {
    let said = output("launchctl", &["procinfo", &pid.to_string()])?;
    said.lines()
        .find_map(|l| l.trim().strip_prefix("domain = "))
        .filter(|d| d.starts_with("system"))
        .and_then(|_| {
            said.lines()
                .find_map(|l| l.trim().strip_prefix("label = "))
                .map(|label| format!("system/{label}"))
        })
}

/// The process that listens on `port`, and its name when `tasklist` says it.
#[cfg(windows)]
fn windows_listener(port: u16) -> Option<(u32, Option<String>)> {
    // TCP    127.0.0.1:11434    0.0.0.0:0    LISTENING    1234
    let pid: u32 = output("netstat", &["-ano", "-p", "TCP"]).and_then(|said| {
        said.lines().find_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let listening = words.get(3) == Some(&"LISTENING");
            let here = words
                .get(1)
                .and_then(|a| a.rsplit_once(':'))
                .and_then(|(_, p)| p.parse::<u16>().ok());
            (listening && here == Some(port))
                .then(|| words.get(4)?.parse().ok())
                .flatten()
        })
    })?;
    // "ollama.exe","1234","Console","1","50,000 K"
    let name = output(
        "tasklist",
        &["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"],
    )
    .and_then(|said| {
        said.lines()
            .next()
            .and_then(|l| l.split(',').next())
            .map(|n| n.trim_matches('"').to_string())
    })
    .filter(|n| !n.is_empty() && !n.starts_with("INFO"));
    Some((pid, name))
}

/// Stops the program that listens on `port`, and says what was done.
#[cfg(windows)]
pub fn free_port(port: u16) -> Freed {
    let mut freed = Freed {
        port,
        program: None,
        pid: None,
        free: !taken(port),
        hint: None,
    };
    if freed.free {
        return freed;
    }
    let Some((pid, name)) = windows_listener(port) else {
        freed.hint = Some(
            "the program that holds it could not be told: netstat -ano names it, then stop it as the administrator."
                .to_string(),
        );
        return freed;
    };
    freed.pid = Some(pid);
    freed.program = name;
    let pid_text = pid.to_string();
    // asked to end, then ended
    let asked =
        ran("taskkill", &["/PID", &pid_text]) && freed_within(port, Duration::from_secs(10));
    if asked
        || (ran("taskkill", &["/F", "/PID", &pid_text])
            && freed_within(port, Duration::from_secs(5)))
    {
        freed.free = true;
    } else {
        freed.hint = Some(format!(
            "stop it as the administrator (taskkill /F /PID {pid}, or Stop-Service for a service), then start again."
        ));
    }
    freed
}
