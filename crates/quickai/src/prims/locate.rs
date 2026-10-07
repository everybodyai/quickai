//! The `locate.*` primitives (docs/architecture.md, section 8): finding an engine's executable
//! without touching the network. They replace the hard-coded search in `paths.rs`; the engine
//! document says which to try and in what order, and the first that yields a path wins.
//!
//! What a strategy needs to know about the machine it is handed (`LocateEnv`): the data directory
//! and the system from the host facts, the environment variables as a function, so that a test can
//! play any machine without touching its own.

use std::path::{Path, PathBuf};

use quickai_core::acquire::render_template;
use quickai_core::doc::engine::{LocateStrategy, Pick};
use quickai_core::plan::HostFacts;

use super::install::is_leftover;

/// What the strategies need to know about this run.
pub struct LocateEnv<'a> {
    /// The directory of the running executable (a packaged build keeps `llama-server` beside it).
    pub exe_dir: Option<&'a Path>,
    /// The source checkout this build came from, for `cargo run`.
    pub dev_root: &'a Path,
    /// A setting field's value as the user entered it, `None` when it is unset.
    pub setting: &'a dyn Fn(&str) -> Option<String>,
    /// This machine: the system whose lists and paths apply, `${data}` for a template, the home
    /// directory for `~`.
    pub facts: &'a HostFacts,
    /// An environment variable (`PATH`, `PATHEXT`, `LOCALAPPDATA`), `None` when it is not set.
    pub var: &'a dyn Fn(&str) -> Option<String>,
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
/// - `installed`, `path_search` and `known_paths` yield a file only when one is there: they look
///   at the machine, and a guess about where something might be is not a finding.
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
            LocateStrategy::Installed { root, pick, binary } => {
                installed(root, *pick, binary, env).map(|p| found(p, "installed"))
            }
            LocateStrategy::PathSearch { name } => {
                path_search(name, env).map(|p| found(p, "path_search"))
            }
            LocateStrategy::KnownPaths {
                linux,
                macos,
                windows,
            } => {
                // a system the document lists no places for has none, and the next strategy is tried
                let places: &[String] = match env.facts.os.as_str() {
                    "linux" => linux,
                    "macos" => macos,
                    "windows" => windows,
                    _ => &[],
                };
                known_paths(places, env).map(|p| found(p, "known_paths"))
            }
        };
        if result.is_some() {
            return result;
        }
    }
    None
}

/// The newest build below the root that has the executable: one directory per build, the
/// executable at the path the document gives for this system. What an install in progress leaves
/// (`.partial`, `.part`) and a directory without the executable are not builds, and a root that is
/// not there has none.
fn installed(
    root: &str,
    pick: Pick,
    binary: &std::collections::BTreeMap<String, String>,
    env: &LocateEnv,
) -> Option<PathBuf> {
    let Pick::Newest = pick;
    let relative = binary.get(&env.facts.os)?;
    let root = PathBuf::from(render_template(root, env.facts)?);
    std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .filter(|entry| !is_leftover(&entry.file_name().to_string_lossy()))
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let executable = entry.path().join(relative);
            let modified = entry.metadata().and_then(|m| m.modified()).ok();
            executable
                .is_file()
                .then(|| (modified, entry.file_name(), executable))
        })
        // the latest modification wins; builds with the same time are told apart by name, so the
        // answer does not depend on the order the directory lists them in
        .max_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)))
        .map(|(_, _, executable)| executable)
}

/// The first program of this name in a directory of the `PATH`: an absolute directory (an empty or
/// relative entry means the current directory, which is not where to look for a program), a file
/// that is not a directory, and on Unix one that can be executed. On Windows the extensions in
/// `PATHEXT` are tried after the name.
fn path_search(name: &str, env: &LocateEnv) -> Option<PathBuf> {
    let path = (env.var)("PATH")?;
    let windows = env.facts.os == "windows";
    let extensions: Vec<String> = if windows {
        let pathext = (env.var)("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
        pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    let has_extension = extensions
        .iter()
        .any(|e| name.to_ascii_lowercase().ends_with(&e.to_ascii_lowercase()));
    // the names to try in each directory, in order: as written, then with each extension
    let mut names = vec![name.to_string()];
    if windows && !has_extension {
        for extension in &extensions {
            names.push(format!("{name}{extension}"));
            names.push(format!("{name}{}", extension.to_ascii_lowercase()));
        }
    }
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        for candidate in &names {
            let file = directory.join(candidate);
            if can_run(&file, windows) {
                return Some(file);
            }
        }
    }
    None
}

/// A file (not a directory) that a program can be started from: on Unix, one with an execute bit.
fn can_run(path: &Path, windows: bool) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    if !windows {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    }
    let _ = windows;
    true
}

/// The first of the places that has a file, after `~` and `%NAME%` are filled in: a place that
/// needs a variable that is not set cannot be formed, and is skipped.
fn known_paths(places: &[String], env: &LocateEnv) -> Option<PathBuf> {
    places
        .iter()
        .filter_map(|place| expand_place(place, env))
        .find(|path| path.is_file())
}

fn expand_place(place: &str, env: &LocateEnv) -> Option<PathBuf> {
    let mut text = place.to_string();
    if text == "~" || text.starts_with("~/") || text.starts_with("~\\") {
        if env.facts.home.is_empty() {
            return None;
        }
        text = format!("{}{}", env.facts.home, &text[1..]);
    }
    // `%NAME%`, as Windows writes a variable
    while let Some(start) = text.find('%') {
        let end = text[start + 1..].find('%')? + start + 1;
        let value = (env.var)(&text[start + 1..end])?;
        text.replace_range(start..=end, &value);
    }
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
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
            facts: &HostFacts::default(),
            var: &|_| None,
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
            facts: &HostFacts::default(),
            var: &|_| None,
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
            facts: &HostFacts::default(),
            var: &|_| None,
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
            facts: &HostFacts::default(),
            var: &|_| None,
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
            facts: &HostFacts::default(),
            var: &|_| None,
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
