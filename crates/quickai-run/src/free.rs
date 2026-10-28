//! `quickai-run free PORT...`: stops the programs that listen on these ports of this computer, so that a run can have
//! them (docs/remote-runs.md section 3.2: Override, and Stop it, for a program QuickAI did not start). A program of
//! the person who runs this is asked to end, then ended. A system service (an Ollama installed as one) is stopped by
//! its service manager through `sudo -n` when that needs no password; otherwise what stops it is said, for the person
//! to run there. What was done is said for each port, as sentences or as JSON (`--json`). The doing is the runtime's
//! (`quickai_runtime::ports`), which the window uses for this computer.

use quickai_runtime::ports::{free_port, Freed};

/// Frees each of `ports`, and says what was done. Exit code 0 when each is free, 8 when one is not.
pub fn run(ports: &[u16], json: bool) -> i32 {
    let freed: Vec<Freed> = ports.iter().map(|port| free_port(*port)).collect();
    if json {
        match serde_json::to_string(&freed) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("quickai-run: {e}");
                return 1;
            }
        }
    } else {
        for f in &freed {
            let who = match (&f.program, f.pid) {
                (Some(program), Some(pid)) => format!("{program} (process {pid})"),
                (Some(program), None) => program.clone(),
                _ => "a program".to_string(),
            };
            if f.free {
                println!("Port {} is free.", f.port);
            } else {
                println!(
                    "Port {} is held by {who}; {}",
                    f.port,
                    f.hint
                        .as_deref()
                        .unwrap_or("it could not be stopped from here.")
                );
            }
        }
    }
    if freed.iter().all(|f| f.free) {
        0
    } else {
        8
    }
}
