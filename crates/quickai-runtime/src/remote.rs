//! Remote computers (WP4.27): a computer with more memory or a bigger graphics card runs the models, and this one
//! chooses, pulls and chats with them. The engine there listens on its own loopback, as every engine QuickAI starts
//! does; an SSH tunnel brings its port to one of this computer's, so that everything the window does with an engine
//! that is already running (attach, list, pull, load, remove, chat) is done with it unchanged, and no port is
//! opened to the network on either side.
//!
//! The person's own `ssh` does the connecting, so their `~/.ssh/config` (aliases, users, keys, jump hosts) and their
//! agent are used as they are. It is never asked a question: a password prompt would have nowhere to go
//! (`BatchMode`), and a computer's key is taken the first time and checked after (`StrictHostKeyChecking=accept-new`,
//! as ssh's own first connection does once the person says yes).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};

use crate::runtime::process::{ephemeral_port, Daemon};

/// The port of an engine there when neither the person nor the engine's document gives one.
pub const DEFAULT_ENGINE_PORT: u16 = 11434;

/// How long a tunnel has to come up: the connection, the login and the forward.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(25);

/// A computer the models can run on, as the person described it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSpec {
    /// What the window calls it (`Studio PC`), and what it is known by in the settings.
    pub id: String,
    pub name: String,
    /// `host`, `user@host`, or an alias of `~/.ssh/config`.
    pub host: String,
    /// The port of its SSH server.
    #[serde(default = "ssh_port")]
    pub ssh_port: u16,
    /// The port the engine listens on there, on its loopback; `None` is the engine's own (Ollama's 11434, ComfyUI's 8188).
    #[serde(default)]
    pub engine_port: Option<u16>,
    /// A key file to log in with, when it is not one ssh finds by itself.
    #[serde(default)]
    pub identity: Option<String>,
}

fn ssh_port() -> u16 {
    22
}

impl RemoteSpec {
    /// A remote of this name and host with the usual ports, its id made from its name.
    pub fn new(name: &str, host: &str) -> RemoteSpec {
        RemoteSpec {
            id: slug(name),
            name: name.trim().to_string(),
            host: host.trim().to_string(),
            ssh_port: ssh_port(),
            engine_port: None,
            identity: None,
        }
    }

    /// The port of the engine there: the one given, else the engine's own.
    pub fn port_for(&self, engine_default: u16) -> u16 {
        self.engine_port.unwrap_or(engine_default)
    }

    /// What is wrong with it, in a sentence, before anything is tried.
    pub fn problem(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("Give it a name.".into());
        }
        let host = self.host.trim();
        if host.is_empty() {
            return Some(
                "Give its address: a host name, user@host, or an alias of your SSH config.".into(),
            );
        }
        // what ssh would read as an option, or as more than one word, is not an address
        if host.starts_with('-') || host.chars().any(char::is_whitespace) {
            return Some(format!("\"{host}\" is not an address."));
        }
        if self.ssh_port == 0 || self.engine_port == Some(0) {
            return Some("A port is a number from 1 to 65535.".into());
        }
        None
    }
}

/// `Studio PC` as `studio-pc`.
pub fn slug(name: &str) -> String {
    let mut id = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.ends_with('-') && !id.is_empty() {
            id.push('-');
        }
    }
    let id = id.trim_end_matches('-').to_string();
    if id.is_empty() {
        "remote".into()
    } else {
        id
    }
}

/// The program that makes the tunnel: the system's `ssh`, or another a test names in `QUICKAI_SSH`.
pub fn ssh_program() -> PathBuf {
    std::env::var_os("QUICKAI_SSH")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ssh"))
}

/// The arguments of the tunnel from `local_port` here to `remote_port` on `spec`'s loopback.
pub fn ssh_args(spec: &RemoteSpec, local_port: u16, remote_port: u16) -> Vec<String> {
    let mut args: Vec<String> = [
        "-N",
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push("-L".into());
    args.push(format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"));
    if spec.ssh_port != 22 {
        args.push("-p".into());
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
    // the address last, after `--`: nothing in it is read as an option
    args.push("--".into());
    args.push(spec.host.trim().to_string());
    args
}

/// An open tunnel: the port of this computer it listens on, and the thread that holds its `ssh`. The program is
/// started, watched and stopped by that one thread, which lives as long as the tunnel: on Linux a program QuickAI starts
/// is told to leave when the *thread* that started it ends (the parent-death signal, decision C38), so a tunnel opened
/// by a thread that then ended was closed at once.
pub struct Tunnel {
    pub local_port: u16,
    alive: Arc<AtomicBool>,
    close: Sender<()>,
    keeper: Option<std::thread::JoinHandle<()>>,
    said: Arc<Mutex<Vec<String>>>,
}

impl Tunnel {
    /// Whether `ssh` still holds it.
    pub fn alive(&mut self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// What `ssh` said, its last meaningful line first explained (see `explain`).
    pub fn why_closed(&self) -> String {
        explain(&self.said.lock().map(|l| l.clone()).unwrap_or_default())
    }

    /// Closes it, and waits (a little) for its `ssh` to be gone.
    pub fn close(&mut self) {
        let _ = self.close.send(());
        if let Some(keeper) = self.keeper.take() {
            let _ = keeper.join();
        }
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.close();
    }
}

/// Opens a tunnel to `remote_port` of `spec` and waits until it listens here, or says why it could not. Blocks for up
/// to `timeout`: call it from a thread of its own. `program` is ssh (`ssh_program`, or a test's stand-in).
pub fn open_with(
    program: &Path,
    spec: &RemoteSpec,
    remote_port: u16,
    timeout: Duration,
) -> Result<Tunnel, String> {
    if let Some(problem) = spec.problem() {
        return Err(problem);
    }
    let local_port = ephemeral_port().map_err(|e| format!("No free port on this computer: {e}"))?;
    let mut command = Command::new(program);
    command.args(ssh_args(spec, local_port, remote_port));
    let said: Arc<Mutex<Vec<String>>> = Arc::default();
    let alive = Arc::new(AtomicBool::new(true));
    let (started_tx, started_rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
    let (close_tx, close_rx) = crossbeam_channel::bounded::<()>(1);
    let (into, flag) = (said.clone(), alive.clone());
    let keeper = std::thread::spawn(move || {
        let mut daemon = match Daemon::start(command, true) {
            Ok(daemon) => daemon,
            Err(e) => {
                flag.store(false, Ordering::SeqCst);
                let _ = started_tx.send(Err(if e.kind() == std::io::ErrorKind::NotFound {
                    "No ssh program was found. Install OpenSSH's client (it comes with Windows 10 and later, macOS and most Linux).".to_string()
                } else {
                    format!("Could not start ssh: {e}")
                }));
                return;
            }
        };
        let (stdout, stderr) = daemon.take_output();
        drop(stdout);
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut lines) = into.lock() {
                        lines.push(line);
                    }
                }
            });
        }
        let _ = started_tx.send(Ok(()));
        // held until it is closed, or ssh leaves by itself
        loop {
            match close_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    daemon.terminate();
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < deadline && matches!(daemon.try_wait(), Ok(None)) {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    if matches!(daemon.try_wait(), Ok(None)) {
                        daemon.kill();
                    }
                    let _ = daemon.wait();
                    break;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if !matches!(daemon.try_wait(), Ok(None)) {
                        break;
                    }
                }
            }
        }
        flag.store(false, Ordering::SeqCst);
    });
    let mut tunnel = Tunnel {
        local_port,
        alive,
        close: close_tx,
        keeper: Some(keeper),
        said,
    };
    match started_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(why)) => return Err(why),
        Err(_) => return Err("ssh could not be started.".to_string()),
    }
    let address = SocketAddr::from(([127, 0, 0, 1], local_port));
    let deadline = Instant::now() + timeout;
    loop {
        if !tunnel.alive() {
            // what it said may still be on its way from the reader
            std::thread::sleep(Duration::from_millis(150));
            return Err(tunnel.why_closed());
        }
        if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
            return Ok(tunnel);
        }
        if Instant::now() > deadline {
            tunnel.close();
            return Err(format!(
                "The connection to {} took more than {} seconds and was given up.",
                spec.host,
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// What ssh's own words mean for a person: the usual failures in a sentence with what to do, anything else as it
/// said it.
pub fn explain(lines: &[String]) -> String {
    let all = lines.join("\n");
    let lower = all.to_lowercase();
    let known: [(&str, &str); 7] = [
        ("permission denied", "The remote computer refused the login. QuickAI logs in with an SSH key (a password cannot be asked for here): add your public key to its ~/.ssh/authorized_keys, or name the key file."),
        ("could not resolve hostname", "That address was not found. Check the host name, or use the computer's IP address."),
        ("connection refused", "The remote computer refused the connection: its SSH server is not running, or listens on another port."),
        ("connection timed out", "The remote computer did not answer. Is it on, and on the same network (or VPN)?"),
        ("no route to host", "There is no way to the remote computer from here. Is it on, and on the same network (or VPN)?"),
        ("host key verification failed", "The remote computer's key is not the one it had before. If it was reinstalled, remove its old key with: ssh-keygen -R <host>"),
        ("address already in use", "The port QuickAI chose here was taken at the last moment. Try again."),
    ];
    for (needle, sentence) in known {
        if lower.contains(needle) {
            return sentence.to_string();
        }
    }
    let last = lines
        .iter()
        .rev()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with("Warning: Permanently added"));
    match last {
        Some(line) => format!("ssh stopped: {line}"),
        None => "ssh stopped without saying why.".to_string(),
    }
}

/// What answers on `local_port` says it is: a `GET` of `path`, and the string at the JSON pointer `pointer` of the reply
/// (the engine's own identity, as its document names it: Ollama's `/api/version` and `/version`). `None` when nothing
/// answers there, or what answers is not the engine.
pub fn identity(local_port: u16, path: &str, pointer: &str) -> Option<String> {
    let address = SocketAddr::from(([127, 0, 0, 1], local_port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(8))).ok()?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{local_port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    let (head, body) = reply.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 2") && !head.starts_with("HTTP/1.0 2") {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(body.trim()).ok().or_else(|| {
        // a chunked body: the JSON is the line that starts with `{`
        body.lines()
            .find(|l| l.trim_start().starts_with('{'))
            .and_then(|l| serde_json::from_str(l).ok())
    })?;
    json.pointer(pointer)?.as_str().map(str::to_string)
}

/// What a test of `spec` found for an engine that answers `path` with its identity at `pointer`: the tunnel opens, and
/// the engine answers through it (what it says it is), or why not. `engine` names it in the sentence.
pub fn check_with(
    program: &Path,
    spec: &RemoteSpec,
    remote_port: u16,
    engine: &str,
    path: &str,
    pointer: &str,
) -> Result<String, String> {
    check_steps(program, spec, remote_port, engine, path, pointer).into_result()
}

/// What a test of a computer found, step by step, for the window to show as a list: it was reached over SSH (or why
/// not), and then the engine answered on its port there (what it says it is), or why not. The engine is not asked when
/// the computer was not reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    pub reached: Result<(), String>,
    pub engine: Option<Result<String, String>>,
}

impl Checked {
    /// Every step passed.
    pub fn passed(&self) -> bool {
        self.reached.is_ok() && matches!(self.engine, Some(Ok(_)))
    }

    /// The engine's identity, or the first step's reason it is not there.
    pub fn into_result(self) -> Result<String, String> {
        self.reached?;
        self.engine
            .unwrap_or_else(|| Err("The engine was not asked.".to_string()))
    }
}

/// `check_with`, step by step.
pub fn check_steps(
    program: &Path,
    spec: &RemoteSpec,
    remote_port: u16,
    engine: &str,
    path: &str,
    pointer: &str,
) -> Checked {
    let mut tunnel = match open_with(program, spec, remote_port, OPEN_TIMEOUT) {
        Ok(tunnel) => tunnel,
        Err(why) => {
            return Checked {
                reached: Err(why),
                engine: None,
            }
        }
    };
    let found = identity(tunnel.local_port, path, pointer);
    tunnel.close();
    Checked {
        reached: Ok(()),
        engine: Some(found.ok_or_else(|| not_answering(spec, engine, remote_port))),
    }
}

/// The command that fixes why `spec` could not be reached, when there is one to run on this computer: a login with no
/// key there is the key copied over once, and a changed key of the computer is the old one forgotten.
pub fn login_fix(spec: &RemoteSpec, why: &str) -> Option<String> {
    let host = spec.host.trim();
    let port = if spec.ssh_port == 22 {
        String::new()
    } else {
        format!("-p {} ", spec.ssh_port)
    };
    if why.contains("refused the login") {
        return Some(format!("ssh-copy-id {port}{host}"));
    }
    if why.contains("key is not the one it had before") {
        let bare = host.rsplit('@').next().unwrap_or(host);
        return Some(format!("ssh-keygen -R {bare}"));
    }
    None
}

/// A `Host` of the person's `~/.ssh/config`, for the address box to offer: the name to give ssh, and where it goes
/// (`nigo@studio.local · port 2222`) as far as the file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshHost {
    pub alias: String,
    pub goes_to: String,
}

/// The hosts `~/.ssh/config` names, in its order: each name of a `Host` line that is not a pattern, with the
/// `HostName`, `User` and `Port` given under it. What ssh reads beyond that (`Match`, `Include`, the defaults of
/// `Host *`) is not needed to offer a name: ssh itself reads the file when it connects.
pub fn ssh_config_hosts(text: &str) -> Vec<SshHost> {
    struct Entry {
        alias: String,
        hostname: Option<String>,
        user: Option<String>,
        port: Option<String>,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut current: std::ops::Range<usize> = 0..0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (
                k.to_ascii_lowercase(),
                v.trim().trim_start_matches('=').trim(),
            ),
            None => continue,
        };
        let value = value.trim_matches('"').to_string();
        match key.as_str() {
            "host" => {
                let start = entries.len();
                for name in value.split_whitespace() {
                    if name.contains(['*', '?', '!']) || entries.iter().any(|e| e.alias == name) {
                        continue;
                    }
                    entries.push(Entry {
                        alias: name.to_string(),
                        hostname: None,
                        user: None,
                        port: None,
                    });
                }
                current = start..entries.len();
            }
            "match" => current = entries.len()..entries.len(),
            "hostname" | "user" | "port" => {
                for entry in &mut entries[current.clone()] {
                    let slot = match key.as_str() {
                        "hostname" => &mut entry.hostname,
                        "user" => &mut entry.user,
                        _ => &mut entry.port,
                    };
                    // the first value given is the one ssh uses
                    slot.get_or_insert_with(|| value.clone());
                }
            }
            _ => {}
        }
    }
    entries
        .into_iter()
        .map(|e| {
            let host = e.hostname.unwrap_or_else(|| e.alias.clone());
            let mut goes_to = match e.user {
                Some(user) => format!("{user}@{host}"),
                None => host,
            };
            if let Some(port) = e.port.filter(|p| p != "22") {
                goes_to.push_str(&format!(" · port {port}"));
            }
            SshHost {
                alias: e.alias,
                goes_to,
            }
        })
        .collect()
}

/// `ssh_config_hosts` of the person's own `~/.ssh/config`; none when there is no such file.
pub fn read_ssh_config_hosts() -> Vec<SshHost> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".ssh").join("config"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| ssh_config_hosts(&text))
        .unwrap_or_default()
}

/// What a computer is called when nobody named it, from its address: `me@studio.local` is `Studio`.
pub fn name_from_address(host: &str) -> String {
    let bare = host.trim().rsplit('@').next().unwrap_or_default();
    let first = bare.split('.').next().unwrap_or_default();
    // an IP address stays whole
    let word = if bare.parse::<std::net::IpAddr>().is_ok() {
        bare
    } else {
        first
    };
    let mut chars = word.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The sentence for a computer that was reached but has no such engine answering on the port.
pub fn not_answering(spec: &RemoteSpec, engine: &str, port: u16) -> String {
    format!(
        "Connected to {}, but no {engine} answers on its port {port}. Start it there (its own service, or \
         `quickai-run up` with a configuration of {engine}), or set the port it uses.",
        spec.host
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tunnel_is_asked_of_ssh_with_no_questions_and_no_port_open_on_either_side() {
        let mut spec = RemoteSpec::new("Studio PC", "me@studio.local");
        assert_eq!(spec.id, "studio-pc");
        let args = ssh_args(&spec, 40123, 11434);
        let at = |opt: &str| args.iter().position(|a| a == opt);
        assert!(
            args.contains(&"BatchMode=yes".to_string()),
            "a password prompt would have nowhere to go"
        );
        assert!(args.contains(&"ExitOnForwardFailure=yes".to_string()));
        assert!(args.contains(&"StrictHostKeyChecking=accept-new".to_string()));
        let forward = &args[at("-L").unwrap() + 1];
        assert_eq!(
            forward, "127.0.0.1:40123:127.0.0.1:11434",
            "loopback on both sides"
        );
        assert_eq!(
            &args[args.len() - 2..],
            ["--", "me@studio.local"],
            "the address last, after --"
        );
        assert!(
            at("-p").is_none() && at("-i").is_none(),
            "the usual port and ssh's own keys say nothing"
        );

        spec.ssh_port = 2222;
        spec.identity = Some("/home/me/.ssh/studio".into());
        let args = ssh_args(&spec, 40123, 11500);
        assert_eq!(
            args[args.iter().position(|a| a == "-p").unwrap() + 1],
            "2222"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "-i").unwrap() + 1],
            "/home/me/.ssh/studio"
        );
        assert!(args.contains(&"127.0.0.1:40123:127.0.0.1:11500".to_string()));
        // the port there is the engine's own unless one is given
        assert_eq!(spec.port_for(8188), 8188);
        spec.engine_port = Some(9000);
        assert_eq!(spec.port_for(8188), 9000);
    }

    #[test]
    fn what_is_not_an_address_is_refused_before_anything_runs() {
        assert!(RemoteSpec::new("", "host").problem().is_some());
        assert!(RemoteSpec::new("PC", "  ").problem().is_some());
        assert!(RemoteSpec::new("PC", "-oProxyCommand=evil")
            .problem()
            .is_some());
        assert!(RemoteSpec::new("PC", "two words").problem().is_some());
        assert_eq!(RemoteSpec::new("PC", "pc.local").problem(), None);
        assert_eq!(RemoteSpec::new("PC", "me@10.0.0.5").problem(), None);
        assert_eq!(slug("  My GPU box!! "), "my-gpu-box");
        assert_eq!(slug("!!"), "remote");
    }

    #[test]
    fn what_ssh_says_is_put_in_words_a_person_can_act_on() {
        let said = |line: &str| explain(&[line.to_string()]);
        assert!(said("me@pc: Permission denied (publickey,password).").contains("SSH key"));
        assert!(
            said("ssh: Could not resolve hostname nosuch: Name or service not known")
                .contains("not found")
        );
        assert!(said("ssh: connect to host pc port 22: Connection refused").contains("SSH server"));
        assert!(said("Host key verification failed.").contains("ssh-keygen -R"));
        assert_eq!(
            said("something else entirely"),
            "ssh stopped: something else entirely"
        );
        assert_eq!(explain(&[]), "ssh stopped without saying why.");
        // the note of a first connection is not the reason
        assert_eq!(
            explain(&[
                "kex_exchange_identification: read: Connection reset by peer".into(),
                "Warning: Permanently added 'pc' (ED25519) to the list of known hosts.".into(),
            ]),
            "ssh stopped: kex_exchange_identification: read: Connection reset by peer"
        );
    }

    #[cfg(unix)]
    mod tunnel {
        use super::super::*;
        use crate::testkit::fake_ollama::FakeOllama;
        use crate::testkit::{fake_ssh, scratch};

        #[test]
        fn a_tunnel_brings_the_engine_of_the_remote_computer_here_and_closing_it_takes_it_away() {
            let dir = scratch("remote-tunnel");
            let Some(ssh) = fake_ssh::install(&dir) else {
                eprintln!("no python3: the stand-in for ssh cannot run");
                return;
            };
            let fake = FakeOllama::start();
            let spec = RemoteSpec::new("Studio", "studio.local");
            let mut tunnel =
                open_with(&ssh, &spec, fake.port(), OPEN_TIMEOUT).expect("the tunnel opens");
            assert_ne!(tunnel.local_port, fake.port());
            assert!(fake_ssh::last_args(&ssh).contains(&format!(
                "127.0.0.1:{}:127.0.0.1:{}",
                tunnel.local_port,
                fake.port()
            )));
            // what answers here is the Ollama there
            assert_eq!(
                identity(tunnel.local_port, "/api/version", "/version").as_deref(),
                Some("0.35.1")
            );
            assert_eq!(
                check_with(
                    &ssh,
                    &spec,
                    fake.port(),
                    "Ollama",
                    "/api/version",
                    "/version"
                )
                .as_deref(),
                Ok("0.35.1")
            );
            // what answers, but not as the engine says it would, is not it
            assert_eq!(
                identity(
                    tunnel.local_port,
                    "/system_stats",
                    "/system/comfyui_version"
                ),
                None
            );
            tunnel.close();
            assert!(!tunnel.alive());
            assert!(
                identity(tunnel.local_port, "/api/version", "/version").is_none(),
                "the port is closed with it"
            );
        }

        #[test]
        fn a_tunnel_opened_by_a_thread_that_ends_stays_open() {
            // the window opens a tunnel on a thread of its own, which ends once it is open; on Linux a program is told to
            // leave when the thread that started it ends, and the tunnel was gone before the run could use it
            let dir = scratch("remote-thread");
            let Some(ssh) = fake_ssh::install(&dir) else {
                return;
            };
            let fake = FakeOllama::start();
            let port = fake.port();
            let spec = RemoteSpec::new("Studio", "studio.local");
            let mut tunnel = std::thread::spawn(move || open_with(&ssh, &spec, port, OPEN_TIMEOUT))
                .join()
                .unwrap()
                .expect("the tunnel opens");
            std::thread::sleep(Duration::from_millis(300));
            assert!(
                tunnel.alive(),
                "the tunnel went with the thread that opened it"
            );
            assert_eq!(
                identity(tunnel.local_port, "/api/version", "/version").as_deref(),
                Some("0.35.1")
            );
        }

        #[test]
        fn a_computer_that_cannot_be_reached_or_has_no_engine_says_so_in_words() {
            let dir = scratch("remote-fail");
            let Some(ssh) = fake_ssh::install(&dir) else {
                return;
            };
            let open = |host: &str| {
                open_with(&ssh, &RemoteSpec::new("X", host), 11434, OPEN_TIMEOUT)
                    .err()
                    .unwrap()
            };
            let refused = open("denied");
            assert!(refused.contains("refused the login"), "{refused}");
            let nosuch = open("nosuch");
            assert!(nosuch.contains("not found"), "{nosuch}");
            // reached, but nothing listens on the engine's port there
            let spec = RemoteSpec::new("Empty", "empty.local");
            let port = crate::testkit::free_port();
            let empty = check_with(&ssh, &spec, port, "Ollama", "/api/version", "/version")
                .err()
                .unwrap();
            assert!(empty.contains("no Ollama answers"), "{empty}");
            // and what is not an address is never handed to ssh
            let bad = open("-oProxyCommand=x");
            assert!(bad.contains("not an address"), "{bad}");
        }
    }

    #[test]
    fn the_hosts_of_an_ssh_config_are_offered_with_where_they_go() {
        let text = "\
# my machines
Host studio
    HostName studio.local
    User nigo
Host gpu-box rented
  Hostname=203.0.113.7
  Port 2222
  User root
Host *.internal *
    User someone
Match host foo
    User other
Host studio
    User ignored
";
        let hosts = ssh_config_hosts(text);
        let said: Vec<(&str, &str)> = hosts
            .iter()
            .map(|h| (h.alias.as_str(), h.goes_to.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("studio", "nigo@studio.local"),
                ("gpu-box", "root@203.0.113.7 · port 2222"),
                ("rented", "root@203.0.113.7 · port 2222"),
            ]
        );
        assert!(ssh_config_hosts("").is_empty());
    }

    #[test]
    fn a_name_is_made_from_an_address_and_a_failed_login_has_its_fix() {
        assert_eq!(name_from_address("me@studio.local"), "Studio");
        assert_eq!(name_from_address("gpu-box"), "Gpu-box");
        assert_eq!(name_from_address("root@203.0.113.7"), "203.0.113.7");
        let mut spec = RemoteSpec::new("Studio", "me@studio.local");
        let refused = explain(&["me@studio.local: Permission denied (publickey).".into()]);
        assert_eq!(
            login_fix(&spec, &refused).as_deref(),
            Some("ssh-copy-id me@studio.local")
        );
        spec.ssh_port = 2222;
        assert_eq!(
            login_fix(&spec, &refused).as_deref(),
            Some("ssh-copy-id -p 2222 me@studio.local")
        );
        let changed = explain(&["Host key verification failed.".into()]);
        assert_eq!(
            login_fix(&spec, &changed).as_deref(),
            Some("ssh-keygen -R studio.local")
        );
        assert_eq!(login_fix(&spec, "That address was not found."), None);
    }
}
