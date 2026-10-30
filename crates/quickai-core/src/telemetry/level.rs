//! The four telemetry levels (VS Code's `telemetry.telemetryLevel`), and the level in force once the caps a package,
//! an organization or the environment set are applied.

/// What QuickAI sends, from nothing to everything. The order is the order of what they allow: a level allows the
/// events of its own level and of every level below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Off,
    Crash,
    Error,
    All,
}

impl Level {
    /// The levels as the notice and Settings list them, the most first.
    pub const SHOWN: [Level; 4] = [Level::All, Level::Error, Level::Crash, Level::Off];

    /// The id kept in the settings and sent with every batch.
    pub fn id(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Crash => "crash",
            Level::Error => "error",
            Level::All => "all",
        }
    }

    /// The level an id names (as `id` writes it, any case; `crashes` and `errors` are taken too).
    pub fn parse(text: &str) -> Option<Level> {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Level::Off),
            "crash" | "crashes" => Some(Level::Crash),
            "error" | "errors" => Some(Level::Error),
            "all" => Some(Level::All),
            _ => None,
        }
    }

    /// The name a person reads.
    pub fn label(self) -> &'static str {
        match self {
            Level::Off => "Off",
            Level::Crash => "Crashes only",
            Level::Error => "Errors and crashes",
            Level::All => "All",
        }
    }

    /// What the level sends, in a sentence.
    pub fn describe(self) -> &'static str {
        match self {
            Level::Off => "Nothing is sent.",
            Level::Crash => "A report when QuickAI stops because of an error.",
            Level::Error => {
                "Crash reports, and reports of what failed: an engine that stopped, a download or an install that \
                 failed, a window that stopped answering."
            }
            Level::All => {
                "Crash and error reports, and counts of what the app did: which engine ran, whether a model started, \
                 which windows were opened."
            }
        }
    }

    /// Whether an event of level `event` may be sent at this level. Off allows nothing.
    pub fn allows(self, event: Level) -> bool {
        event != Level::Off && event <= self
    }
}

/// The level in force: what the person chose (`None`, not answered yet, is Off) held under every cap.
pub fn in_force(chosen: Option<Level>, caps: &[Level]) -> Level {
    caps.iter()
        .fold(chosen.unwrap_or(Level::Off), |level, cap| level.min(*cap))
}

/// The cap the environment sets: `QUICKAI_TELEMETRY=off|crash|error|all`, and `DO_NOT_TRACK` set to anything but
/// empty or `0`, which means off. A value that names no level is ignored. The values are passed in: this crate reads
/// no environment.
pub fn env_cap(quickai_telemetry: Option<&str>, do_not_track: Option<&str>) -> Option<Level> {
    let dnt = do_not_track
        .map(str::trim)
        .is_some_and(|v| !v.is_empty() && v != "0");
    let named = quickai_telemetry.and_then(Level::parse);
    match (dnt, named) {
        (true, _) => Some(Level::Off),
        (false, named) => named,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_allows_its_own_events_and_those_below() {
        assert!(Level::All.allows(Level::All));
        assert!(Level::All.allows(Level::Crash));
        assert!(Level::Error.allows(Level::Crash));
        assert!(!Level::Error.allows(Level::All));
        assert!(!Level::Crash.allows(Level::Error));
        for level in Level::SHOWN {
            assert!(!Level::Off.allows(level));
            assert!(!level.allows(Level::Off), "no event is of level Off");
        }
    }

    #[test]
    fn not_answered_is_off_and_every_cap_holds() {
        assert_eq!(in_force(None, &[]), Level::Off);
        assert_eq!(in_force(Some(Level::All), &[]), Level::All);
        assert_eq!(in_force(Some(Level::All), &[Level::Error]), Level::Error);
        assert_eq!(
            in_force(Some(Level::All), &[Level::Error, Level::Crash]),
            Level::Crash
        );
        assert_eq!(in_force(Some(Level::Crash), &[Level::All]), Level::Crash);
    }

    #[test]
    fn the_environment_caps_and_do_not_track_means_off() {
        assert_eq!(env_cap(None, None), None);
        assert_eq!(env_cap(Some("crash"), None), Some(Level::Crash));
        assert_eq!(env_cap(Some("OFF"), None), Some(Level::Off));
        assert_eq!(env_cap(Some("loud"), None), None);
        assert_eq!(env_cap(Some("all"), Some("1")), Some(Level::Off));
        assert_eq!(env_cap(None, Some("true")), Some(Level::Off));
        assert_eq!(env_cap(None, Some("0")), None);
        assert_eq!(env_cap(None, Some("")), None);
    }

    #[test]
    fn ids_read_back() {
        for level in Level::SHOWN {
            assert_eq!(Level::parse(level.id()), Some(level));
        }
        assert_eq!(Level::parse("errors"), Some(Level::Error));
        assert_eq!(Level::parse("sometimes"), None);
    }
}
