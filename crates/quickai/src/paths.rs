//! Locates the `llama-server` binary and the two models directories. In a
//! packaged build the binary and the *bundled* models live right next to
//! this executable (see the build scripts in the `quickai_base` repository);
//! under `cargo run` neither does, so we fall back to the paths this repo's
//! own `llama.cpp`/`models` directories live at, resolved at compile time via
//! `CARGO_MANIFEST_DIR` (this crate is always at `<repo>/crates/quickai`).
//!
//! Downloaded models are kept separate, under `~/.quickai/models`, rather
//! than next to the executable: a packaged build's own directory can be
//! read-only (e.g. an AppImage's squashfs mount) or owned by another user
//! (e.g. an install location on Windows/Linux), so writing there isn't
//! reliable. The user's home directory always is.

use std::path::PathBuf;

#[cfg(windows)]
pub(crate) const LLAMA_SERVER_NAME: &str = "llama-server.exe";
#[cfg(not(windows))]
pub(crate) const LLAMA_SERVER_NAME: &str = "llama-server";

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.to_path_buf())
}

fn dev_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

pub fn llama_server_bin() -> PathBuf {
    if let Some(dir) = exe_dir() {
        let candidate = dir.join(LLAMA_SERVER_NAME);
        if candidate.is_file() {
            return candidate;
        }
    }
    dev_repo_root()
        .join("llama.cpp")
        .join("build")
        .join("bin")
        .join(LLAMA_SERVER_NAME)
}

/// Where the models this package shipped with live: next to the executable
/// in a packaged build, or this repo's own `models/` under `cargo run`.
/// Read from, never written to — a packaged build's own directory may not
/// be writable (see module docs).
pub fn bundled_models_dir() -> PathBuf {
    if let Some(dir) = exe_dir() {
        let candidate = dir.join("models");
        if candidate.is_dir() {
            return candidate;
        }
    }
    dev_repo_root().join("models")
}

/// Where downloaded models are written and, alongside `bundled_models_dir`,
/// scanned from. Always under the user's home directory so it's writable
/// regardless of where the app itself is installed/mounted from.
pub fn user_models_dir() -> PathBuf {
    home_dir().join(".quickai").join("models")
}

fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
}

/// Parent of every per-build directory `llama_bin_install_dir` creates.
/// `llama_install::list_installed` scans this so a build downloaded in an
/// earlier run still shows up (and can be re-selected or deleted) even
/// after it's aged out of "Look up llama.cpp builds"' GitHub results,
/// which only ever shows a handful of the most recent releases.
pub fn llama_bin_root_dir() -> PathBuf {
    home_dir().join(".quickai").join("llama-bin")
}

/// Where a `llama-server` build fetched via ⚙ Settings' "Look up llama.cpp
/// builds" gets installed, one subdirectory per build (`label` — see
/// `llama_releases::LlamaBuild::install_label`) since the binary needs its
/// shared libraries sitting right next to it and different builds' same-
/// named libs (`libggml.so`, etc.) must not clobber each other.
pub fn llama_bin_install_dir(label: &str) -> PathBuf {
    llama_bin_root_dir().join(label)
}
