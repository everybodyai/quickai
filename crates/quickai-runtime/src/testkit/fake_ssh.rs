//! A stand-in for `ssh -N -L 127.0.0.1:<here>:127.0.0.1:<there> -- <host>` (WP4.27): it forwards the port here to the
//! port there on this machine, as a real tunnel to a computer whose engine is on its loopback does, and fails the way
//! ssh fails for the hosts named after the failure (`denied`, `nosuch`, `refused`). It is a Python script, run by the
//! code under test as it would run ssh; a test that has no python3 is skipped.

use std::path::{Path, PathBuf};

const SCRIPT: &str = r#"#!/usr/bin/env python3
import socket, sys, threading
args = sys.argv[1:]
host = args[-1]
if host == "denied":
    sys.stderr.write("me@denied: Permission denied (publickey,password).\n"); sys.exit(255)
if host == "nosuch":
    sys.stderr.write("ssh: Could not resolve hostname nosuch: Name or service not known\n"); sys.exit(255)
if host == "refused":
    sys.stderr.write("ssh: connect to host refused port 22: Connection refused\n"); sys.exit(255)
spec = args[args.index("-L") + 1]
_, here, _, there = spec.split(":")
with open(__file__ + ".argv", "w") as f:
    f.write("\n".join(args))
server = socket.socket(); server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", int(here))); server.listen(16)
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
def serve(client):
    try:
        upstream = socket.create_connection(("127.0.0.1", int(there)))
    except OSError:
        sys.stderr.write("channel 2: open failed: connect failed: Connection refused\n"); sys.stderr.flush()
        client.close(); return
    threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
    pipe(upstream, client)
while True:
    client, _ = server.accept()
    threading.Thread(target=serve, args=(client,), daemon=True).start()
"#;

/// Writes the stand-in into `dir` as `ssh`; `None` when this machine has no python3 to run it.
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
    let path = dir.join("ssh");
    std::fs::write(&path, SCRIPT).ok()?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(path)
}

/// The arguments the stand-in was last run with, one a line.
pub fn last_args(program: &Path) -> Vec<String> {
    let mut record = program.as_os_str().to_os_string();
    record.push(".argv");
    std::fs::read_to_string(PathBuf::from(record))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}
