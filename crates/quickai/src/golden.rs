//! Golden-file assertions for the parity tests (docs/implementation-plan.md, WP0.2).
//!
//! A golden file is a checked-in snapshot of behaviour that must not change by accident: the
//! `llama-server` command line, the shape of `settings.json`. `assert_golden` compares a freshly
//! produced text with `tests/golden/<name>`. A deliberate change is made by running the tests with
//! `QUICKAI_UPDATE_GOLDEN=1`, which rewrites the files, and then reading the diff in review.

use std::path::{Path, PathBuf};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

fn update_requested() -> bool {
    std::env::var_os("QUICKAI_UPDATE_GOLDEN").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Panics unless `actual` equals the golden file `name` (or rewrites it, see the module docs).
pub fn assert_golden(name: &str, actual: &str) {
    check(&golden_dir(), name, actual, update_requested());
}

/// The text of a golden file, for tests that compare *another* behaviour against it (the real-spawn
/// proof reads the launch matrix this way).
pub fn read_golden(name: &str) -> String {
    let path = golden_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| {
            panic!(
                "golden file {} is unreadable ({e}); create it with QUICKAI_UPDATE_GOLDEN=1",
                path.display()
            )
        })
        .replace("\r\n", "\n")
}

fn check(dir: &Path, name: &str, actual: &str, update: bool) {
    let path = dir.join(name);
    if update {
        std::fs::create_dir_all(dir).expect("create the golden directory");
        std::fs::write(&path, actual).expect("write the golden file");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| {
            panic!(
                "golden file {} is unreadable ({e}); create it with QUICKAI_UPDATE_GOLDEN=1",
                path.display()
            )
        })
        // A checkout with autocrlf must not turn every golden into a failure.
        .replace("\r\n", "\n");
    if expected != actual {
        panic!("{}", mismatch_message(&path, &expected, actual));
    }
}

fn mismatch_message(path: &Path, expected: &str, actual: &str) -> String {
    let (mut want, mut got) = (expected.lines(), actual.lines());
    let mut line = 1;
    loop {
        match (want.next(), got.next()) {
            (Some(w), Some(g)) if w == g => line += 1,
            (None, None) => {
                return format!(
                    "golden mismatch in {}: the texts differ only in trailing whitespace",
                    path.display()
                )
            }
            (w, g) => {
                return format!(
                    "golden mismatch in {} at line {line}:\n  expected: {}\n  actual:   {}\n\
                     If the change is intended, rerun with QUICKAI_UPDATE_GOLDEN=1 and review the diff.",
                    path.display(),
                    w.unwrap_or("<end of file>"),
                    g.unwrap_or("<end of file>")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quickai-golden-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn matching_text_passes_and_crlf_is_ignored() {
        let dir = scratch("match");
        check(&dir, "a.txt", "one\ntwo\n", true);
        check(&dir, "a.txt", "one\ntwo\n", false);
        std::fs::write(dir.join("a.txt"), "one\r\ntwo\r\n").unwrap();
        check(&dir, "a.txt", "one\ntwo\n", false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_difference_names_the_first_differing_line() {
        let dir = scratch("diff");
        check(&dir, "a.txt", "one\ntwo\nthree\n", true);
        let err = std::panic::catch_unwind(|| check(&dir, "a.txt", "one\nTWO\nthree\n", false))
            .expect_err("a changed text must fail");
        let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(msg.contains("at line 2"), "{msg}");
        assert!(msg.contains("expected: two"), "{msg}");
        assert!(msg.contains("actual:   TWO"), "{msg}");
        assert!(msg.contains("QUICKAI_UPDATE_GOLDEN=1"), "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_fails_unless_updating() {
        let dir = scratch("missing");
        let err = std::panic::catch_unwind(|| check(&dir, "nope.txt", "x", false))
            .expect_err("a missing golden must fail");
        let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(msg.contains("QUICKAI_UPDATE_GOLDEN=1"), "{msg}");
        check(&dir, "nope.txt", "x", true);
        assert_eq!(std::fs::read_to_string(dir.join("nope.txt")).unwrap(), "x");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_length_difference_is_reported_as_end_of_file() {
        let dir = scratch("len");
        check(&dir, "a.txt", "one\ntwo\n", true);
        let err = std::panic::catch_unwind(|| check(&dir, "a.txt", "one\n", false))
            .expect_err("a shorter text must fail");
        let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(msg.contains("actual:   <end of file>"), "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
