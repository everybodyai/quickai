//! Locates the `llama-server` binary and the two models directories. In a
//! packaged build the binary and the *bundled* models live right next to
//! this executable (see the build scripts in the `quickai_base` repository);
//! in the macOS app the models are in the app's `Contents/Resources` instead
//! (`packaging_dir`); under `cargo run` neither does, so we fall back to the
//! paths this repo's own `llama.cpp`/`models` directories live at, resolved at
//! compile time via `CARGO_MANIFEST_DIR` (this crate is always at
//! `<repo>/crates/quickai-runtime`).
//!
//! Downloaded models are kept separate, under `~/.quickai/models`, rather
//! than next to the executable: a packaged build's own directory can be
//! read-only (e.g. an AppImage's squashfs mount) or owned by another user
//! (e.g. an install location on Windows/Linux), so writing there isn't
//! reliable. The user's home directory always is.

use std::path::{Path, PathBuf};

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.to_path_buf())
}

pub(crate) fn dev_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Where a package keeps what is not a program, for an executable in `exe_dir`: that folder itself,
/// or, for the program of a macOS app (`QuickAI.app/Contents/MacOS/quickai`), the app's
/// `Contents/Resources`. macOS keeps an app's data there: the `MacOS` folder of a signed app holds
/// programs only (`build/macos.sh` of the `quickai_base` repository lays the app out so).
pub fn packaging_dir(exe_dir: &Path) -> PathBuf {
    if exe_dir.ends_with("Contents/MacOS") {
        let resources = exe_dir.with_file_name("Resources");
        if resources.is_dir() {
            return resources;
        }
    }
    exe_dir.to_path_buf()
}

/// Where the models this package shipped with live: next to the executable
/// in a packaged build (in the macOS app, its `Contents/Resources`), or this
/// repo's own `models/` under `cargo run`.
/// Read from, never written to — a packaged build's own directory may not
/// be writable (see module docs).
pub fn bundled_models_dir() -> PathBuf {
    if let Some(dir) = exe_dir() {
        let candidate = packaging_dir(&dir).join("models");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quickai-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_program_of_a_macos_app_finds_its_data_in_the_apps_resources() {
        let app = scratch("app").join("QuickAI.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
        assert_eq!(
            packaging_dir(&app.join("Contents/MacOS")),
            app.join("Contents/Resources")
        );
    }

    #[test]
    fn any_other_program_finds_its_data_beside_it() {
        let dir = scratch("plain");
        assert_eq!(packaging_dir(&dir), dir);
        // the shape of an app without its Resources folder is not an app
        let app = dir.join("QuickAI.app/Contents/MacOS");
        std::fs::create_dir_all(&app).unwrap();
        assert_eq!(packaging_dir(&app), app);
    }
}
