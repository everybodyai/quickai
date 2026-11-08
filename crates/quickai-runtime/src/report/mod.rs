//! Problem reports on this computer (C202): what QuickAI finds out for a report (`environment`, `machine`), the panic
//! hook that writes a crash's report to `~/.quickai/reports` (`crash`), and the reports a crash left for the next start.
//!
//! Nothing here sends anything: the window shows a report, and the person copies it and posts it where they choose.
//! The telemetry of 0.2.1 to 0.2.5 is gone; what it left on this computer, its install id among it, is deleted at the
//! start (`forget_telemetry`).

pub mod crash;
pub mod environment;
pub mod machine;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use quickai_core::report::clean::Cleaner;
use quickai_core::report::Environment;

pub use crash::{crash_report, frames, write_report, CrashContext};

/// The folder of the data folder (`~/.quickai`) a crash's report is written to.
pub const REPORTS_DIR: &str = "reports";

/// A crash's report not shown within this long is deleted (30 days).
pub const CRASHES_KEPT_FOR: u64 = 30 * 86_400;

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Installs the panic hook that writes a crash's report to `dir` (after which the hook that was there runs), and gives
/// back the context the window keeps up to date for it.
pub fn install_panic_hook(
    dir: PathBuf,
    environment: Environment,
    cleaner: Cleaner,
) -> Arc<Mutex<CrashContext>> {
    let context = Arc::new(Mutex::new(CrashContext::default()));
    let hook = crash::Hook {
        dir,
        environment,
        cleaner,
        context: context.clone(),
        started: now(),
        clock: Arc::new(now),
    };
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        hook.record(info);
        previous(info);
    }));
    context
}

/// The crash reports in `dir`, the newest first; those older than `CRASHES_KEPT_FOR` are deleted.
pub fn pending_crashes(dir: &Path) -> Vec<PathBuf> {
    let now = std::time::SystemTime::now();
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "txt")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
        })
        .map(|p| {
            let modified = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, p)
        })
        .collect();
    files.retain(|(modified, path)| {
        let old = now
            .duration_since(*modified)
            .is_ok_and(|age| age.as_secs() > CRASHES_KEPT_FOR);
        if old {
            let _ = std::fs::remove_file(path);
        }
        !old
    });
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().map(|(_, p)| p).collect()
}

/// Deletes what the telemetry of QuickAI 0.2.1 to 0.2.5 kept in `data_dir/telemetry`: its install id, the events that
/// waited, its log and the crash reports it kept. Nothing of it is used any more.
pub fn forget_telemetry(data_dir: &Path) {
    let dir = data_dir.join("telemetry");
    if dir.is_dir() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::report::{Kind, Report};

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quickai-report-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_crash_reports_left_are_found_the_newest_first_and_nothing_else() {
        let dir = scratch("pending");
        let report = Report::new(Kind::Crash, Environment::default());
        let first = write_report(&dir, &report, 1).unwrap();
        // a later one (the file names differ by their seconds)
        let second = write_report(&dir, &report, 2).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&second)
            .unwrap()
            .set_modified(later)
            .unwrap();
        std::fs::write(dir.join("notes.txt"), "not a report").unwrap();
        std::fs::write(dir.join("crash-3-4.txt.part"), "half").unwrap();
        assert_eq!(pending_crashes(&dir), vec![second, first]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crash_report_older_than_a_month_is_deleted() {
        let dir = scratch("old");
        let path =
            write_report(&dir, &Report::new(Kind::Crash, Environment::default()), 1).unwrap();
        let long_ago = std::time::SystemTime::now()
            - std::time::Duration::from_secs(CRASHES_KEPT_FOR + 86_400);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        assert!(pending_crashes(&dir).is_empty());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_telemetry_left_is_deleted_with_its_install_id() {
        let data = scratch("forget");
        let old = data.join("telemetry");
        std::fs::create_dir_all(old.join("crashes")).unwrap();
        std::fs::write(
            old.join("install-id"),
            "3f1c9a2e-7b44-4d0e-9c1a-0b6f1e2a3c4d",
        )
        .unwrap();
        std::fs::write(old.join("queue.jsonl"), "{}").unwrap();
        std::fs::write(data.join("settings.json"), "{}").unwrap();
        forget_telemetry(&data);
        assert!(!old.exists());
        assert!(data.join("settings.json").exists(), "nothing else goes");
        // and again, with nothing there
        forget_telemetry(&data);
        let _ = std::fs::remove_dir_all(&data);
    }
}
