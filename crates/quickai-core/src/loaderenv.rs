//! Keeps the loader environment of QuickAI's own packaging out of a program it starts
//! (docs/architecture.md, section 11.2; docs/phase3-test-plan.md, group K).
//!
//! An AppImage sets `LD_LIBRARY_PATH` to directories inside itself and a snap sets it to
//! directories of the snap; a downloaded Ollama that inherits them would load QuickAI's libraries
//! instead of its own. The entries that lie under `$APPDIR` or `$SNAP` are dropped; everything
//! else is left alone.
//!
//! Pure: the environment comes in as a function, the changes go out as data, and the caller
//! (`runtime::process`) applies them to the command it is about to run.

/// The new value of a variable, or `None` to remove it (nothing was left).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvChange {
    pub name: String,
    pub value: Option<String>,
}

/// The variables that tell the dynamic loader where to look.
const LOADER_VARIABLES: [&str; 2] = ["LD_LIBRARY_PATH", "LD_PRELOAD"];

/// The variables that name the root of a package whose loader environment must not leak.
const PACKAGE_ROOTS: [&str; 2] = ["APPDIR", "SNAP"];

/// What to change in the environment of a child, given the environment this process has (`get`).
/// Only variables that need changing are returned. `LD_LIBRARY_PATH` and `LD_PRELOAD` are looked
/// at, entries are split at `:`, and an entry is under a root when it is that directory or a
/// path inside it (`/tmp/.mount_x` does not cover `/tmp/.mount_xyz`).
pub fn scrub(get: &dyn Fn(&str) -> Option<String>) -> Vec<EnvChange> {
    let roots: Vec<String> = PACKAGE_ROOTS
        .iter()
        .filter_map(|name| get(name))
        .map(|root| root.trim_end_matches('/').to_string())
        // An empty root (or `/`, which trims to nothing) would match every entry.
        .filter(|root| !root.is_empty())
        .collect();
    if roots.is_empty() {
        return Vec::new();
    }

    let mut changes = Vec::new();
    for name in LOADER_VARIABLES {
        let Some(value) = get(name) else { continue };
        let entries: Vec<&str> = value.split(':').collect();
        let kept: Vec<&str> = entries
            .iter()
            .copied()
            .filter(|entry| !roots.iter().any(|root| is_inside(entry, root)))
            .collect();
        if kept.len() == entries.len() {
            continue;
        }
        changes.push(EnvChange {
            name: name.to_string(),
            value: (!kept.is_empty()).then(|| kept.join(":")),
        });
    }
    changes
}

/// Whether `entry` is `root` or a path below it.
fn is_inside(entry: &str, root: &str) -> bool {
    match entry.strip_prefix(root) {
        Some("") => true,
        Some(rest) => rest.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_is_inside_itself_and_below_but_not_beside() {
        assert!(is_inside("/a/b", "/a/b"));
        assert!(is_inside("/a/b/c", "/a/b"));
        assert!(is_inside("/a/b/", "/a/b"));
        assert!(!is_inside("/a/bc", "/a/b"));
        assert!(!is_inside("/a", "/a/b"));
    }

    #[test]
    fn a_root_with_a_trailing_slash_still_covers_what_is_below_it() {
        let env = |name: &str| match name {
            "APPDIR" => Some("/tmp/.mount_x/".to_string()),
            "LD_LIBRARY_PATH" => Some("/tmp/.mount_x/usr/lib:/usr/lib".to_string()),
            _ => None,
        };
        assert_eq!(
            scrub(&env),
            [EnvChange {
                name: "LD_LIBRARY_PATH".into(),
                value: Some("/usr/lib".into())
            }]
        );
    }

    #[test]
    fn a_root_of_slash_matches_nothing_because_it_would_match_everything() {
        let env = |name: &str| match name {
            "APPDIR" => Some("/".to_string()),
            "LD_LIBRARY_PATH" => Some("/usr/lib".to_string()),
            _ => None,
        };
        assert!(scrub(&env).is_empty());
    }

    #[test]
    fn empty_entries_are_left_where_they_are() {
        // an empty entry means the current directory to the loader; it is not ours to remove
        let env = |name: &str| match name {
            "SNAP" => Some("/snap/q/1".to_string()),
            "LD_LIBRARY_PATH" => Some("/snap/q/1/lib::/usr/lib".to_string()),
            _ => None,
        };
        assert_eq!(
            scrub(&env),
            [EnvChange {
                name: "LD_LIBRARY_PATH".into(),
                value: Some(":/usr/lib".into())
            }]
        );
    }
}
