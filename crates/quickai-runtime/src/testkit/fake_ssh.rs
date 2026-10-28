//! A stand-in for `ssh` and `scp` (WP4.27, WP4.33): another computer that is a folder of this one. The same script is
//! installed as both, and does what its name says:
//!
//! - `ssh ... -L 127.0.0.1:<here>:127.0.0.1:<there> ... -- <host>`: forwards each port here to the port there on this
//!   machine, as a real tunnel to a computer whose engine is on its loopback does (a port here that is taken fails as
//!   ssh's `ExitOnForwardFailure` does);
//! - `ssh ... -- <host> <command>`: runs the command with `sh` in the other computer's home (`remote_home`, its
//!   `HOME`), with the variables `set_remote_env` names, and ends with its code;
//! - `scp ... -- <file> <host>:<path>`: copies the file to that path of the other computer's home.
//!
//! It fails the way ssh fails for the hosts named after the failure (`denied`, `nosuch`, `refused`). Each call is kept
//! beside it (`calls`). It is a Python script, run by the code under test as it would run ssh; a test that has no
//! python3 is skipped.

use std::path::{Path, PathBuf};

const SCRIPT: &str = r#"#!/usr/bin/env python3
import json, os, shutil, socket, subprocess, sys, threading
me = os.path.basename(sys.argv[0])
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
with open(os.path.join(here, me + ".argv"), "w") as f:
    f.write("\n".join(args))
with open(os.path.join(here, "calls.jsonl"), "a") as f:
    f.write(json.dumps([me] + args) + "\n")

# the options that take a value, the forwards, and what follows them
VALUED = {"-o", "-p", "-P", "-i", "-L", "-l", "-F", "-J", "-c", "-S"}
forwards, rest, i = [], [], 0
while i < len(args):
    a = args[i]
    if a == "--":
        rest = args[i + 1:]
        break
    if a in VALUED:
        if a == "-L" and i + 1 < len(args):
            forwards.append(args[i + 1])
        i += 2
        continue
    if a.startswith("-"):
        i += 1
        continue
    rest = args[i:]
    break

def fail_for(host, code):
    bare = host.rsplit("@", 1)[-1]
    if bare == "denied":
        sys.stderr.write(host + ": Permission denied (publickey,password).\n"); sys.exit(code)
    if bare == "nosuch":
        sys.stderr.write("ssh: Could not resolve hostname nosuch: Name or service not known\n"); sys.exit(code)
    if bare == "refused":
        sys.stderr.write("ssh: connect to host refused port 22: Connection refused\n"); sys.exit(code)

def read(name, default):
    try:
        with open(os.path.join(here, name)) as f:
            return f.read()
    except OSError:
        return default

home = read("remote.home", os.path.join(here, "remote-home")).strip()
os.makedirs(home, exist_ok=True)
env = dict(os.environ)
env["HOME"] = home
for line in read("remote.env", "").splitlines():
    if "=" in line:
        key, value = line.split("=", 1)
        env[key] = value

if me.startswith("scp"):
    local, target = rest[0], rest[1]
    host, path = target.split(":", 1)
    fail_for(host, 1)
    dest = path if os.path.isabs(path) else os.path.join(home, path)
    os.makedirs(os.path.dirname(dest) or home, exist_ok=True)
    shutil.copyfile(local, dest)
    sys.exit(0)

host = rest[0] if rest else ""
fail_for(host, 255)
command = " ".join(rest[1:])
if not forwards:
    sys.exit(subprocess.run(["sh", "-c", command], cwd=home, env=env).returncode)

def pipe(a, b):
    # one direction: what a sends goes to b, and a's end of it (a half-close) is passed on as b's, as ssh does;
    # the other direction goes on until it ends too
    try:
        while True:
            data = a.recv(65536)
            if not data:
                break
            b.sendall(data)
    except OSError:
        pass
    finally:
        try: b.shutdown(socket.SHUT_WR)
        except OSError: pass

def serve(client, there):
    try:
        upstream = socket.create_connection(("127.0.0.1", int(there)))
    except OSError:
        sys.stderr.write("channel 2: open failed: connect failed: Connection refused\n"); sys.stderr.flush()
        client.close(); return
    threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
    pipe(upstream, client)

def listen(server, there):
    while True:
        client, _ = server.accept()
        threading.Thread(target=serve, args=(client, there), daemon=True).start()

servers = []
for spec in forwards:
    _, port_here, _, there = spec.split(":")
    server = socket.socket(); server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        server.bind(("127.0.0.1", int(port_here)))
    except OSError:
        sys.stderr.write("bind [127.0.0.1]:%s: Address already in use\nchannel_setup_fwd_listener_tcpip: cannot listen to port: %s\nCould not request local forwarding.\n" % (port_here, port_here))
        sys.exit(255)
    server.listen(16)
    servers.append((server, there))
for server, there in servers:
    threading.Thread(target=listen, args=(server, there), daemon=True).start()
threading.Event().wait()
"#;

/// Writes the stand-in into `dir` as `ssh` and as `scp`, the other computer's home beside them (`remote_home`):
/// the path of `ssh`; `None` when this machine has no python3 to run it.
#[cfg(unix)]
pub fn install(dir: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let python = std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !python {
        return None;
    }
    std::fs::create_dir_all(dir).ok()?;
    for name in ["ssh", "scp"] {
        let path = dir.join(name);
        std::fs::write(&path, SCRIPT).ok()?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).ok()?;
    }
    Some(dir.join("ssh"))
}

/// The `scp` of the stand-in that `ssh` is.
pub fn scp_of(ssh: &Path) -> PathBuf {
    ssh.with_file_name("scp")
}

/// The home of the other computer that the stand-in `ssh` reaches: a folder beside it.
pub fn remote_home(ssh: &Path) -> PathBuf {
    ssh.with_file_name("remote-home")
}

/// The variables the commands run on the other computer are given, besides its `HOME` (where the fakes are, for a
/// `quickai-run` there).
pub fn set_remote_env(ssh: &Path, vars: &[(&str, &str)]) {
    let text: String = vars.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    std::fs::write(ssh.with_file_name("remote.env"), text).expect("write remote.env");
}

/// The arguments the stand-in (`ssh` or `scp`) was last run with, one a line.
pub fn last_args(program: &Path) -> Vec<String> {
    let mut record = program.as_os_str().to_os_string();
    record.push(".argv");
    std::fs::read_to_string(PathBuf::from(record))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Every call of the stand-ins of `ssh`'s folder, in order: the program's name, then its arguments.
pub fn calls(ssh: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(ssh.with_file_name("calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
