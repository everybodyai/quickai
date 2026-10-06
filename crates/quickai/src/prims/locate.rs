//! The `locate.*` primitives (docs/architecture.md, section 8): finding an engine's executable
//! without touching the network. They replace the hard-coded search in `paths.rs`; the engine
//! document says which to try and in what order, and the first that yields a path wins.

use std::path::{Path, PathBuf};

use quickai_core::doc::engine::LocateStrategy;

/// What the strategies need to know about this run.
pub struct LocateEnv<'a> {
    /// The directory of the running executable (a packaged build keeps `llama-server` beside it).
    pub exe_dir: Option<&'a Path>,
    /// The source checkout this build came from, for `cargo run`.
    pub dev_root: &'a Path,
    /// A setting field's value as the user entered it, `None` when it is unset.
    pub setting: &'a dyn Fn(&str) -> Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub path: PathBuf,
    /// Whether a file is there. A path can win without one (see `locate`), and the application
    /// then says "not found at ..." rather than looking further.
    pub exists: bool,
    /// Which strategy found it.
    pub strategy: &'static str,
}

/// A file name with the platform's executable suffix (`.exe` on Windows), so documents name
/// programs the same way on every system.
fn exe_name(file: &str) -> String {
    let suffix = std::env::consts::EXE_SUFFIX;
    if suffix.is_empty() || file.ends_with(suffix) {
        file.to_string()
    } else {
        format!("{file}{suffix}")
    }
}

fn found(path: PathBuf, strategy: &'static str) -> Located {
    Located {
        exists: path.is_file(),
        path,
        strategy,
    }
}

/// Tries `strategies` in order.
///
/// - `setting` yields the path the user entered, even when no file is there: it is an explicit
///   choice, and the application reports a missing file rather than quietly using another.
/// - `next_to_exe` yields a file beside the executable only when it exists.
/// - `dev_tree` always yields its path below the source checkout, so there is something to show
///   when nothing else was found.
pub fn locate(strategies: &[LocateStrategy], env: &LocateEnv) -> Option<Located> {
    for strategy in strategies {
        let result = match strategy {
            LocateStrategy::Setting { field } => (env.setting)(field)
                .filter(|v| !v.trim().is_empty())
                .map(|v| found(PathBuf::from(v), "setting")),
            LocateStrategy::NextToExe { file } => env
                .exe_dir
                .map(|dir| dir.join(exe_name(file)))
                .filter(|p| p.is_file())
                .map(|p| found(p, "next_to_exe")),
            LocateStrategy::DevTree { path } => {
                Some(found(env.dev_root.join(exe_name(path)), "dev_tree"))
            }
        };
        if result.is_some() {
            return result;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quickai-locate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn strategies() -> Vec<LocateStrategy> {
        vec![
            LocateStrategy::Setting {
                field: "bin_override".into(),
            },
            LocateStrategy::NextToExe {
                file: "llama-server".into(),
            },
            LocateStrategy::DevTree {
                path: "llama.cpp/build/bin/llama-server".into(),
            },
        ]
    }

    fn touch(dir: &Path, relative: &str) -> PathBuf {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"x").unwrap();
        path
    }

    #[test]
    fn a_setting_wins_even_when_the_file_is_gone() {
        let dir = scratch("setting");
        let exe = dir.join("bin");
        touch(&exe, &exe_name("llama-server"));
        let set = |f: &str| (f == "bin_override").then(|| "/nowhere/llama-server".to_string());
        let env = LocateEnv {
            exe_dir: Some(&exe),
            dev_root: &dir,
            setting: &set,
        };
        let got = locate(&strategies(), &env).unwrap();
        assert_eq!(got.path, PathBuf::from("/nowhere/llama-server"));
        assert!(!got.exists);
        assert_eq!(got.strategy, "setting");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_blank_setting_is_unset() {
        let dir = scratch("blank");
        let exe = dir.join("bin");
        let beside = touch(&exe, &exe_name("llama-server"));
        let set = |_: &str| Some("   ".to_string());
        let env = LocateEnv {
            exe_dir: Some(&exe),
            dev_root: &dir,
            setting: &set,
        };
        let got = locate(&strategies(), &env).unwrap();
        assert_eq!((got.path, got.strategy), (beside, "next_to_exe"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_beside_the_executable_beats_the_dev_tree() {
        let dir = scratch("beside");
        let exe = dir.join("bin");
        let beside = touch(&exe, &exe_name("llama-server"));
        touch(&dir, &exe_name("llama.cpp/build/bin/llama-server"));
        let none = |_: &str| None;
        let env = LocateEnv {
            exe_dir: Some(&exe),
            dev_root: &dir,
            setting: &none,
        };
        let got = locate(&strategies(), &env).unwrap();
        assert_eq!(
            (got.path, got.exists, got.strategy),
            (beside, true, "next_to_exe")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_file_beside_it_the_dev_tree_path_is_given_even_if_missing() {
        let dir = scratch("dev");
        let exe = dir.join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        let none = |_: &str| None;
        let env = LocateEnv {
            exe_dir: Some(&exe),
            dev_root: &dir,
            setting: &none,
        };
        let got = locate(&strategies(), &env).unwrap();
        assert_eq!(
            got.path,
            dir.join(exe_name("llama.cpp/build/bin/llama-server"))
        );
        assert!(!got.exists);
        assert_eq!(got.strategy, "dev_tree");

        touch(&dir, &exe_name("llama.cpp/build/bin/llama-server"));
        assert!(locate(&strategies(), &env).unwrap().exists);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_strategy_means_nothing_found() {
        let none = |_: &str| None;
        let env = LocateEnv {
            exe_dir: None,
            dev_root: Path::new("/x"),
            setting: &none,
        };
        assert_eq!(locate(&[], &env), None);
        assert_eq!(
            locate(&[LocateStrategy::NextToExe { file: "a".into() }], &env),
            None
        );
    }

    #[test]
    fn executable_suffixes_are_added_once() {
        let suffix = std::env::consts::EXE_SUFFIX;
        assert_eq!(exe_name("llama-server"), format!("llama-server{suffix}"));
        assert_eq!(
            exe_name(&format!("llama-server{suffix}")),
            format!("llama-server{suffix}")
        );
    }
}
