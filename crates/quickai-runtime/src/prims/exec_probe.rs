//! `exec.probe@1`: run a program and read one fact from what it prints, such as the version of an
//! installed engine (docs/architecture.md, section 8; docs/phase3-test-plan.md, group L).
//!
//! The program is started, so this runs only for an executable the application has found (its own
//! `locate` strategies, or one the person chose), never for a name nothing vouches for, and the
//! answer is remembered per file so that asking again does not start it again (`VersionCache`).
//!
//! The application reads the version of the engine it runs once the engine is chosen in Settings
//! (work package 3.7); until then only the tests ask.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use regex::Regex;

use crate::runtime::process::{run_captured, without_package_loader_env};

/// How long a program has to say its version: it is asked while the window is drawn from what it
/// says, so a program that hangs must not hold anything up for long.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs `binary args...` and returns the first capture group of `pattern` found in its standard
/// output, or failing that its standard error. `None` when the program cannot be started, does
/// not finish within `timeout` (it is killed), or prints nothing the pattern matches. The exit
/// status does not matter: the real `ollama --version` with no server up says what it knows on
/// stdout and exits 0, and a program that has the fact has it.
pub fn probe_version(
    binary: &Path,
    args: &[&str],
    pattern: &str,
    timeout: Duration,
) -> Option<String> {
    let pattern = Regex::new(pattern).ok()?;
    let mut command = Command::new(binary);
    command.args(args);
    // a downloaded program has libraries of its own, not the ones of QuickAI's package
    without_package_loader_env(&mut command);
    let ran = run_captured(&mut command, timeout).ok()?;
    let read = |text: &str| {
        pattern
            .captures(text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
    };
    read(&ran.stdout).or_else(|| read(&ran.stderr))
}

/// What identifies an answer: the same file (by size and modification time), asked the same way.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Asked {
    binary: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
    args: Vec<String>,
    pattern: String,
}

/// Versions already read, by file and modification time: a new build in the same place is asked
/// again, the same one is not.
#[derive(Default)]
pub struct VersionCache {
    known: Mutex<HashMap<Asked, String>>,
}

impl VersionCache {
    /// The version `binary` reports, from what was remembered when the file is unchanged. A file
    /// that is not there has no version, and an answer that is `None` is not remembered (a program
    /// that was busy a moment ago may answer the next time).
    pub fn version(
        &self,
        binary: &Path,
        args: &[&str],
        pattern: &str,
        timeout: Duration,
    ) -> Option<String> {
        let meta = std::fs::metadata(binary).ok()?;
        let asked = Asked {
            binary: binary.to_path_buf(),
            modified: meta.modified().ok(),
            len: meta.len(),
            args: args.iter().map(|a| a.to_string()).collect(),
            pattern: pattern.to_string(),
        };
        if let Some(version) = self.known.lock().unwrap().get(&asked) {
            return Some(version.clone());
        }
        let version = probe_version(binary, args, pattern, timeout)?;
        self.known.lock().unwrap().insert(asked, version.clone());
        Some(version)
    }
}
