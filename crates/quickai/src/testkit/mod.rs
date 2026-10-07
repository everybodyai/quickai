//! Test infrastructure for the Phase 3 tests (docs/phase3-test-plan.md, section 2): fakes of the
//! four things QuickAI talks to (Ollama, GitHub's release API, the library at ollama.com, Hugging Face), generators for the archives an
//! engine is installed from, helpers for the processes a session starts, and the recordings of a
//! real Ollama that the fakes are held to.
//!
//! Nothing here is part of the application; it is compiled for tests only. It has tests of its own
//! (in each file): a fake that is wrong makes every test that uses it wrong in the same way, so it
//! is checked against the real thing (`recorded`) first.

#![allow(dead_code)] // a toolbox: each test uses some of it

pub mod archives;
pub mod fake_github;
pub mod fake_hub;
pub mod fake_library;
pub mod fake_ollama;
pub mod files;
pub mod http;
pub mod mini_http;
pub mod procs;
pub mod recorded;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// `crates/quickai/tests/fixtures`.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// An empty directory of this test's own, removed when it goes out of scope (set `QUICKAI_TEST_KEEP`
/// to keep what a failing test left, for looking at). It derefs to its path.
pub struct Scratch(Option<PathBuf>);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        self.0
            .as_ref()
            .expect("a scratch directory that was given away")
    }
}

impl AsRef<std::path::Path> for Scratch {
    fn as_ref(&self) -> &std::path::Path {
        self
    }
}

impl Scratch {
    /// The path, and the duty to remove it: for a world that cleans up after itself.
    pub fn into_path(mut self) -> PathBuf {
        self.0
            .take()
            .expect("a scratch directory that was given away")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(dir) = &self.0 {
            if std::env::var_os("QUICKAI_TEST_KEEP").is_none() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }
}

/// An empty directory of this test's own (below the temporary directory, named by the process and
/// a counter, so tests running side by side never meet).
pub fn scratch(name: &str) -> Scratch {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "quickai-p3-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create a scratch directory");
    Scratch(Some(dir))
}

/// Polls `condition` until it holds; panics with `what` when it does not within `timeout`.
pub fn wait_until(timeout: Duration, what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// How the proxy reads the APIs of the embedded vocabulary: the decoders of all the APIs that
/// have one.
pub fn decoders() -> Vec<quickai_core::decode::ApiDecoder> {
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let ids: Vec<String> = registry.vocabulary().apis().map(|a| a.id.clone()).collect();
    registry.decoders_for(&ids)
}

/// Where `free_port` draws from: below the range the kernel hands out for `bind(0)` (32768 and up on
/// Linux, 49152 and up on Windows and macOS), which is where the fakes get their ports.
const PORT_BASE: u32 = 20_000;
const PORT_SPAN: u32 = 10_000;

/// A free loopback port, for a test that uses it later: a daemon that will listen there, a fake
/// bound once the script has started, a port that has to stay silent.
///
/// Asking the kernel is no help (the lesson of two flaky Phase 1 tests, and again of one in three
/// hundred runs of the session tests): a port it hands out and gets back is as free for the next
/// `bind(0)` of another test as for the test that asked, and the test that waits meets the other
/// test's fake on its port, "a daemon that never answers" included. So the ports come from below
/// the kernel's range, one for each request, so no fake and no other request can be given the same
/// one; the start depends on the process, so two test runs side by side do not draw the same ports
/// either. A port that something is listening on at the moment is skipped.
pub fn free_port() -> u16 {
    static NEXT: std::sync::OnceLock<AtomicUsize> = std::sync::OnceLock::new();
    let next = NEXT.get_or_init(|| {
        // consecutive process ids are common for runs started together: spread them
        AtomicUsize::new(
            (std::process::id() as usize).wrapping_mul(2_654_435_761) % PORT_SPAN as usize,
        )
    });
    for _ in 0..PORT_SPAN {
        let offset = next.fetch_add(1, Ordering::Relaxed) as u32 % PORT_SPAN;
        let port = (PORT_BASE + offset) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    panic!(
        "no free port between {PORT_BASE} and {}",
        PORT_BASE + PORT_SPAN
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_port_is_never_given_twice_and_is_never_one_a_fake_gets() {
        let ports: Vec<u16> = (0..300).map(|_| free_port()).collect();
        let distinct: std::collections::BTreeSet<_> = ports.iter().collect();
        assert_eq!(
            distinct.len(),
            ports.len(),
            "a port was given twice: {ports:?}"
        );
        assert!(
            ports
                .iter()
                .all(|p| (PORT_BASE..PORT_BASE + PORT_SPAN).contains(&(*p as u32))),
            "a port outside the range reserved for tests: {ports:?}"
        );
        // what a fake binds (port 0) is drawn from the kernel's own range, never from ours
        let fake = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = fake.local_addr().unwrap().port() as u32;
        assert!(
            !(PORT_BASE..PORT_BASE + PORT_SPAN).contains(&port),
            "the kernel handed out {port}, inside the range kept for free_port"
        );
    }

    #[test]
    fn a_port_that_is_taken_is_skipped() {
        let first = free_port();
        // take the port the next request would draw, and see that it is not given
        let next = if first as u32 + 1 < PORT_BASE + PORT_SPAN {
            first + 1
        } else {
            PORT_BASE as u16
        };
        let _held = std::net::TcpListener::bind(("127.0.0.1", next));
        // other tests draw ports too, so the next one need not be `next`: what matters is that
        // `next` is never given while it is held
        for _ in 0..50 {
            assert_ne!(free_port(), next, "a port that is listened on was given");
        }
    }
}
