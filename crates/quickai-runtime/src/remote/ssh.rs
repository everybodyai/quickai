//! The transport to another computer (`remote.ssh`, WP4.33.4, docs/remote-runs.md section 5): a command run there and
//! a file copied there, by the person's own `ssh` and `scp`, so that their `~/.ssh/config`, keys and agent are used as
//! they are. Neither is ever asked a question (BatchMode), as the tunnel is not: a password prompt would have nowhere
//! to go. What ssh says when it fails is put in a sentence a person can act on (`explain`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::{explain, RemoteSpec};

/// The options of every connection: no question asked, a computer's key taken the first time and checked after (as
/// ssh's own first connection does once the person says yes), and a connection that does not come up given up.
pub(super) const NO_QUESTIONS: [&str; 6] = [
    "-o",
    "BatchMode=yes",
    "-o",
    "StrictHostKeyChecking=accept-new",
    "-o",
    "ConnectTimeout=15",
];

/// What is said when there is no ssh (or scp) to run.
pub(super) const NO_SSH: &str =
    "No ssh program was found. Install OpenSSH's client (it comes with Windows 10 and later, macOS and most Linux).";

/// What a command there said, and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    /// Its exit code (`None` when it was ended by a signal).
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Said {
    /// It ended well.
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    /// Its output's lines, with the ends of lines Windows writes taken off.
    pub fn lines(&self) -> Vec<&str> {
        self.stdout
            .lines()
            .map(|l| l.trim_end_matches('\r'))
            .collect()
    }
}

/// The program that copies files: the system's `scp`, or another a test names in `QUICKAI_SCP`.
pub fn scp_program() -> PathBuf {
    std::env::var_os("QUICKAI_SCP")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("scp"))
}

/// The port and the key file of `spec`, as `flag` (`-p` for ssh, `-P` for scp) and `-i` say them.
pub(super) fn port_and_key(spec: &RemoteSpec, port_flag: &str) -> Vec<String> {
    let mut args = Vec::new();
    if spec.ssh_port != 22 {
        args.push(port_flag.to_string());
        args.push(spec.ssh_port.to_string());
    }
    if let Some(identity) = spec
        .identity
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
    {
        args.push("-i".into());
        args.push(identity.to_string());
    }
    args
}

/// The arguments of ssh that run `command` on `spec`, by the shell of the person there. The address comes after `--`,
/// so that nothing in it is read as an option.
pub fn command_args(spec: &RemoteSpec, command: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["-T".into()];
    args.extend(NO_QUESTIONS.iter().map(|s| s.to_string()));
    args.extend(port_and_key(spec, "-p"));
    args.push("--".into());
    args.push(spec.host.trim().to_string());
    args.push(command.to_string());
    args
}

/// The arguments of scp that copy `local` to `remote` on `spec` (a path there relative to the person's home, which
/// both the POSIX and the Windows servers of OpenSSH take as it is).
pub fn copy_args(spec: &RemoteSpec, local: &Path, remote: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["-q".into(), "-B".into()];
    args.extend(NO_QUESTIONS.iter().map(|s| s.to_string()));
    args.extend(port_and_key(spec, "-P"));
    args.push("--".into());
    args.push(local.display().to_string());
    args.push(format!("{}:{remote}", spec.host.trim()));
    args
}

/// Runs `command` on `spec` with ssh (`program`) and waits for it, `timeout` at most. What the command said comes back
/// however it ended; the connection itself failing (ssh's own exit code, 255) is said as why.
pub fn run(
    program: &Path,
    spec: &RemoteSpec,
    command: &str,
    timeout: Duration,
) -> Result<Said, String> {
    if let Some(problem) = spec.problem() {
        return Err(problem);
    }
    let mut ssh = Command::new(program);
    ssh.args(command_args(spec, command));
    let said = wait_for(ssh, timeout, &spec.host)?;
    if said.code == Some(255) {
        let lines: Vec<String> = said.stderr.lines().map(str::to_string).collect();
        return Err(explain(&lines));
    }
    Ok(said)
}

/// Copies `local` to `remote` on `spec` with scp (`program`), `timeout` at most.
pub fn copy(
    program: &Path,
    spec: &RemoteSpec,
    local: &Path,
    remote: &str,
    timeout: Duration,
) -> Result<(), String> {
    if let Some(problem) = spec.problem() {
        return Err(problem);
    }
    let mut scp = Command::new(program);
    scp.args(copy_args(spec, local, remote));
    let said = wait_for(scp, timeout, &spec.host)?;
    if said.ok() {
        return Ok(());
    }
    let lines: Vec<String> = said.stderr.lines().map(str::to_string).collect();
    Err(explain(&lines))
}

/// Starts `command` with nothing to read, reads what it writes, and waits for it, `timeout` at most (it is ended
/// then). `host` names the computer in what is said.
fn wait_for(mut command: Command, timeout: Duration, host: &str) -> Result<Said, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            NO_SSH.to_string()
        } else {
            format!("Could not start ssh: {e}")
        }
    })?;
    let stdout = read_all(&mut child, true);
    let stderr = read_all(&mut child, false);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} did not answer in {} seconds.",
                    host.trim(),
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("Could not wait for ssh: {e}")),
        }
    };
    Ok(Said {
        code: status.code(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// What the child writes to its output (`out`) or its error stream, read to its end on a thread of its own, so that
/// neither fills while the other is waited for.
fn read_all(child: &mut Child, out: bool) -> std::thread::JoinHandle<String> {
    let pipe: Option<Box<dyn Read + Send>> = if out {
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>)
    } else {
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>)
    };
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `text` as one word of a POSIX shell's command line: in single quotes, a single quote in it closed, escaped and
/// opened again.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_and_a_copy_are_asked_of_ssh_and_scp_with_no_questions() {
        let mut spec = RemoteSpec::new("Studio", "me@studio.local");
        spec.ssh_port = 2222;
        spec.identity = Some("/home/me/.ssh/studio".into());
        let args = command_args(&spec, "uname -sm");
        assert!(args.contains(&"BatchMode=yes".to_string()));
        assert_eq!(
            args[args.iter().position(|a| a == "-p").unwrap() + 1],
            "2222"
        );
        assert_eq!(
            &args[args.len() - 3..],
            ["--", "me@studio.local", "uname -sm"]
        );
        let copy = copy_args(&spec, Path::new("/tmp/q"), ".quickai/bin/q");
        assert_eq!(
            copy[copy.iter().position(|a| a == "-P").unwrap() + 1],
            "2222"
        );
        assert_eq!(
            &copy[copy.len() - 3..],
            ["--", "/tmp/q", "me@studio.local:.quickai/bin/q"]
        );
        assert!(copy.contains(&"-B".to_string()));
    }

    #[test]
    fn a_word_is_quoted_for_a_posix_shell() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(sh_quote("$HOME; rm"), "'$HOME; rm'");
    }
}
