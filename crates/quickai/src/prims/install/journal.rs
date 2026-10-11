//! What an install of an application keeps of itself while it runs (docs/comfyui.md, "If an install is
//! interrupted"): a journal of the steps it has done and a lock that says it is going.
//!
//! An install of ComfyUI takes twenty minutes and writes seventeen gigabytes. The window it was started
//! from can stop answering, be closed or be killed, and the person must not lose what was done, nor have
//! to do anything by hand to a directory to go on. So the staging directory (`<label>.partial`) says in
//! `.quickai-install.json` which steps are done and for which plan, and the next install of the same plan
//! goes on from there. A directory with a journal is an install that is not finished, whatever it is
//! called: it is not a build (a directory that was renamed by hand to the build's name is still one that
//! is not whole), and it is offered for continuing or deleting. The journal is the last thing to go, after
//! the directory is in place under the build's name, so a build that is whole has none.
//!
//! A lock on a file of the directory (`.quickai-install.lock`) is held for as long as an install works in
//! it: another window of QuickAI neither clears it nor continues it, and does not list it as stopped.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The record of the install, in the directory it works in.
pub const JOURNAL: &str = ".quickai-install.json";
/// The file whose lock says that an install is working in the directory.
pub const LOCK: &str = ".quickai-install.lock";
const SCHEMA: u32 = 1;

/// One step of an install, as the journal and the line under the bar name it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepRecord {
    pub id: String,
    pub label: String,
}

/// Where the file the build is made from came from: what going on with an install needs when the document
/// alone does not say (the address of a wheel is the package index's, not something made up from its version).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRecord {
    pub url: String,
    pub host_digest: Option<String>,
    pub pin: Option<String>,
    pub size: Option<u64>,
}

/// The steps of an install and which of them are done.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Journal {
    pub schema: u32,
    /// The build this is the install of.
    pub label: String,
    /// What the steps were made for (the source, the Python, the packages): a journal of another plan
    /// is not continued, since the environment it describes is not the one this install would make.
    pub fingerprint: String,
    /// When the install began (seconds since 1970).
    pub started: u64,
    pub steps: Vec<StepRecord>,
    /// The ids of the steps that are done, in the order they were.
    pub done: Vec<String>,
    /// The file the build is made from; a journal of an earlier version has none.
    #[serde(default)]
    pub source: Option<SourceRecord>,
}

impl Journal {
    pub fn new(label: &str, fingerprint: String, steps: Vec<StepRecord>) -> Journal {
        Journal {
            schema: SCHEMA,
            label: label.to_string(),
            fingerprint,
            started: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            steps,
            done: Vec::new(),
            source: None,
        }
    }

    /// The journal in `dir`, if it has one this version reads.
    pub fn read(dir: &Path) -> Option<Journal> {
        let text = std::fs::read_to_string(dir.join(JOURNAL)).ok()?;
        let journal: Journal = serde_json::from_str(&text).ok()?;
        (journal.schema == SCHEMA).then_some(journal)
    }

    /// Writes it into `dir` whole or not at all: a journal that was cut short by a kill would be
    /// believed about nothing.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let temporary = dir.join(format!("{JOURNAL}.new"));
        std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, dir.join(JOURNAL)))
            .map_err(|e| {
                format!(
                    "could not write the record of the install in {}: {e}",
                    dir.display()
                )
            })
    }

    pub fn is_done(&self, id: &str) -> bool {
        self.done.iter().any(|d| d == id)
    }

    pub fn mark(&mut self, id: &str) {
        if !self.is_done(id) {
            self.done.push(id.to_string());
        }
    }

    /// Takes `id` and every step after it out of the done ones: what is on disk for it is not there
    /// (or not whole), and what came after was made on top of it.
    pub fn undo_from(&mut self, id: &str) {
        let Some(at) = self.steps.iter().position(|s| s.id == id) else {
            return;
        };
        let later: Vec<&str> = self.steps[at..].iter().map(|s| s.id.as_str()).collect();
        self.done.retain(|d| !later.contains(&d.as_str()));
    }

    /// The first step that is not done.
    pub fn next(&self) -> Option<&StepRecord> {
        self.steps.iter().find(|s| !self.is_done(&s.id))
    }
}

/// A fingerprint of what an install was made for: the same parts, in the same order, give the same one.
pub fn fingerprint(parts: &[String]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A directory with the record of an install is one that is not finished.
pub fn is_unfinished(dir: &Path) -> bool {
    dir.join(JOURNAL).is_file()
}

// ---- the lock ----------------------------------------------------------------------------------------------

/// The right to work in a directory: held until it is dropped, or until the process ends (the system
/// lets go of a lock with the file).
pub struct Claim {
    _file: Option<File>,
}

/// Why a directory cannot be worked in.
#[derive(Debug, PartialEq, Eq)]
pub struct Busy;

/// How many times a lock that is held is asked for, and how long between. A lock belongs to the open
/// file, not to the process, and a program that this process starts holds a copy of every file that is
/// open until it has run its first instruction: the lock of an install that has just ended can stay
/// held for a few milliseconds by a pip that another thread was starting.
const ASKS: u32 = 5;
const BETWEEN: std::time::Duration = std::time::Duration::from_millis(20);

/// Takes the lock of `dir`. A file system that has no locks is not a reason to refuse: the install
/// goes on without one, as it did before there were any.
pub fn claim(dir: &Path) -> Result<Claim, Busy> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK));
    let Ok(file) = file else {
        return Ok(Claim { _file: None });
    };
    for ask in 0..ASKS {
        match file.try_lock() {
            Ok(()) => return Ok(Claim { _file: Some(file) }),
            Err(TryLockError::WouldBlock) if ask + 1 < ASKS => std::thread::sleep(BETWEEN),
            Err(TryLockError::WouldBlock) => return Err(Busy),
            Err(TryLockError::Error(_)) => return Ok(Claim { _file: None }),
        }
    }
    Err(Busy)
}

/// Whether an install is working in `dir` now (in this process or another).
pub fn is_busy(dir: &Path) -> bool {
    let Ok(file) = File::open(dir.join(LOCK)) else {
        return false;
    };
    for ask in 0..ASKS {
        match file.try_lock_shared() {
            Err(TryLockError::WouldBlock) if ask + 1 < ASKS => std::thread::sleep(BETWEEN),
            Err(TryLockError::WouldBlock) => return true,
            _ => return false,
        }
    }
    true
}

// ---- what is stopped ------------------------------------------------------------------------------------------

/// An install that stopped before it was whole, as the runtime manager lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unfinished {
    pub label: String,
    /// Where it is, whatever it is called.
    pub dir: PathBuf,
    pub size_bytes: u64,
    pub done: usize,
    pub total: usize,
    /// What the next step is.
    pub next: Option<String>,
    /// When it last did something.
    pub touched: Option<SystemTime>,
    /// Where the file the build is made from came from, as the install that stopped recorded it.
    pub source: Option<SourceRecord>,
}

/// The directories below `root` with a journal that no install is working in, most recently touched
/// first.
pub fn list_unfinished(root: &Path) -> Vec<Unfinished> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<Unfinished> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir() && !is_busy(dir))
        .filter_map(|dir| {
            let journal = Journal::read(&dir)?;
            let touched = std::fs::metadata(dir.join(JOURNAL))
                .and_then(|m| m.modified())
                .ok();
            Some(Unfinished {
                size_bytes: super::dir_size(&dir),
                done: journal.done.len(),
                total: journal.steps.len(),
                next: journal.next().map(|s| s.label.clone()),
                source: journal.source,
                label: journal.label,
                dir,
                touched,
            })
        })
        .collect();
    found.sort_by_key(|u| std::cmp::Reverse(u.touched));
    found
}

/// The directory below `root` whose journal is the install of `label`, other than `except` (the
/// staging directory, which the caller has looked at already).
pub fn unfinished_named(root: &Path, label: &str, except: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir() && dir != except)
        .filter(|dir| Journal::read(dir).is_some_and(|j| j.label == label))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Empties `dir`, keeping the file of its lock (which is held).
pub fn clear_except_lock(dir: &Path) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("could not read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        if entry.file_name() == LOCK {
            continue;
        }
        let path = entry.path();
        let gone = if path.is_dir() && !path.is_symlink() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        gone.map_err(|e| format!("could not clear {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quickai-journal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn steps(ids: &[&str]) -> Vec<StepRecord> {
        ids.iter()
            .map(|id| StepRecord {
                id: id.to_string(),
                label: format!("the {id} step"),
            })
            .collect()
    }

    #[test]
    fn a_journal_is_read_back_as_written_and_only_when_it_is_whole() {
        let dir = scratch("roundtrip");
        let mut journal = Journal::new("v1", "abc".into(), steps(&["a", "b", "c"]));
        journal.mark("a");
        journal.mark("a");
        journal.write(&dir).unwrap();
        assert_eq!(Journal::read(&dir), Some(journal.clone()));
        assert_eq!(journal.done, ["a"], "a step is done once");
        assert_eq!(journal.next().map(|s| s.id.as_str()), Some("b"));
        assert!(is_unfinished(&dir));
        // no half-written file is left beside it
        assert!(!dir.join(format!("{JOURNAL}.new")).exists());

        // one that was cut short, or is another schema, is not believed
        std::fs::write(dir.join(JOURNAL), b"{\"schema\":1,\"label\":\"v1\"").unwrap();
        assert_eq!(Journal::read(&dir), None);
        journal.schema = 2;
        journal.write(&dir).unwrap();
        assert_eq!(Journal::read(&dir), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn undoing_a_step_undoes_what_was_made_on_top_of_it() {
        let mut journal = Journal::new("v1", "abc".into(), steps(&["a", "b", "c", "d"]));
        for id in ["a", "b", "c"] {
            journal.mark(id);
        }
        journal.undo_from("b");
        assert_eq!(journal.done, ["a"]);
        journal.undo_from("not-a-step");
        assert_eq!(journal.done, ["a"]);
    }

    #[test]
    fn a_fingerprint_changes_with_any_part_and_its_order() {
        let p = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(fingerprint(&p(&["a", "b"])), fingerprint(&p(&["a", "b"])));
        assert_ne!(fingerprint(&p(&["a", "b"])), fingerprint(&p(&["b", "a"])));
        assert_ne!(
            fingerprint(&p(&["ab", "c"])),
            fingerprint(&p(&["a", "bc"])),
            "where one part ends and the next begins counts"
        );
    }

    #[test]
    fn a_directory_is_busy_while_a_claim_on_it_is_held_and_not_after() {
        let dir = scratch("claim");
        assert!(!is_busy(&dir), "nothing has claimed it");
        let first = claim(&dir).expect("the first claim");
        assert!(is_busy(&dir));
        assert_eq!(claim(&dir).err(), Some(Busy), "a second claim is refused");
        drop(first);
        assert!(!is_busy(&dir));
        assert!(claim(&dir).is_ok(), "and can be made again");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_stopped_is_listed_with_its_steps_and_what_is_going_on_is_not() {
        let root = scratch("list");
        let made = |name: &str, label: &str, done: &[&str]| {
            let dir = root.join(name);
            std::fs::create_dir_all(dir.join("venv")).unwrap();
            std::fs::write(dir.join("venv/x"), vec![0u8; 100]).unwrap();
            let mut journal = Journal::new(label, "f".into(), steps(&["a", "b", "c"]));
            for id in done {
                journal.mark(id);
            }
            journal.write(&dir).unwrap();
            dir
        };
        let stopped = made("v1.partial", "v1", &["a"]);
        made("renamed", "v2", &["a", "b"]);
        let going = made("v3.partial", "v3", &[]);
        // a build that is whole has no journal, and a folder of models is not an install
        std::fs::create_dir_all(root.join("v0")).unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        let _held = claim(&going).unwrap();

        let listed = list_unfinished(&root);
        let mut names: Vec<(&str, usize, usize, Option<&str>)> = listed
            .iter()
            .map(|u| (u.label.as_str(), u.done, u.total, u.next.as_deref()))
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                ("v1", 1, 3, Some("the b step")),
                ("v2", 2, 3, Some("the c step")),
            ],
            "v3 is being installed"
        );
        let first = listed.iter().find(|u| u.label == "v1").unwrap();
        assert_eq!(first.dir, stopped);
        assert!(first.size_bytes >= 100, "{}", first.size_bytes);
        assert!(first.touched.is_some());

        // the install of a build finds the directory of its journal under whatever name
        assert_eq!(
            unfinished_named(&root, "v2", &root.join("v2.partial")),
            Some(root.join("renamed"))
        );
        assert_eq!(unfinished_named(&root, "v2", &root.join("renamed")), None);
        assert_eq!(unfinished_named(&root, "nothing", &root.join("x")), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clearing_a_directory_keeps_the_file_of_the_lock_and_nothing_else() {
        let dir = scratch("clear");
        let held = claim(&dir).unwrap();
        std::fs::create_dir_all(dir.join("venv/lib")).unwrap();
        std::fs::write(dir.join("venv/lib/x"), b"x").unwrap();
        std::fs::write(dir.join("y"), b"y").unwrap();
        clear_except_lock(&dir).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [LOCK]);
        drop(held);
        let _ = std::fs::remove_dir_all(dir);
    }
}
