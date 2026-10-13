//! What the computer has to have for a configuration to work (`[[prerequisite]]` of an engine's document,
//! `[[needs]] system = "..."` of a configuration, docs/headless.md section 12): which of the document's
//! prerequisites a configuration asks for, which of them apply to this computer, and the names of the
//! packages that bring one. Looking at the computer and installing are the application's; everything here
//! is a function of the documents and the facts it is given.

use std::time::Duration;

use crate::doc::engine::{Fix, PackageManager, Prerequisite};
use crate::expr::eval;
use crate::plan::resolve::base_context;
use crate::plan::{HostFacts, Selection};
use crate::profile::{Configuration, NeedKind};
use crate::registry::engine::{CompiledPrerequisite, DEFAULT_PREREQUISITE_TIMEOUT};
use crate::registry::{Engine, Registry};

/// One prerequisite that a configuration needs, with why the configuration says so.
#[derive(Clone, Copy, Debug)]
pub struct Wanted<'a> {
    pub engine: &'a Engine,
    pub item: &'a Prerequisite,
    pub compiled: &'a CompiledPrerequisite,
    /// The configuration's own words about it (`why` of its `[[needs]]`).
    pub why: Option<&'a str>,
}

impl Wanted<'_> {
    /// How long a script of it may take.
    pub fn timeout(&self) -> Duration {
        self.compiled
            .timeout
            .unwrap_or(DEFAULT_PREREQUISITE_TIMEOUT)
    }
}

/// The prerequisites `configuration` needs that apply to a computer with `facts`, in the order the
/// configuration lists them, each once. A need the engine's documents do not know is left out (the
/// configuration was read against them, so it does not happen); one whose condition does not hold here, or
/// cannot be evaluated, is left out too: nothing is asked of a computer on a guess.
pub fn wanted<'a>(
    registry: &'a Registry,
    configuration: &'a Configuration,
    facts: &HostFacts,
) -> Vec<Wanted<'a>> {
    let context = base_context(&Selection::default(), facts);
    let mut out: Vec<Wanted<'a>> = Vec::new();
    for need in &configuration.needs {
        let NeedKind::System { id } = &need.kind else {
            continue;
        };
        if out.iter().any(|w| w.item.id == *id) {
            continue;
        }
        let found = crate::profile::family(registry.engine_map(), &configuration.engine)
            .into_iter()
            .find_map(|engine| {
                engine
                    .doc
                    .prerequisites
                    .iter()
                    .zip(&engine.prerequisites)
                    .find(|(item, _)| item.id == *id)
                    .map(|(item, compiled)| (engine, item, compiled))
            });
        let Some((engine, item, compiled)) = found else {
            continue;
        };
        let applies = match &compiled.when {
            None => true,
            Some(when) => eval(when, &context).is_ok_and(|v| v.truthy()),
        };
        if applies {
            out.push(Wanted {
                engine,
                item,
                compiled,
                why: need.why.as_deref(),
            });
        }
    }
    out
}

/// The packages of `item` for a package manager, with `${python.version}` and `${python.digits}` replaced
/// by the Python of the environment (`(3, 12)`). `None` when the document names none for the manager.
/// A name that still has a word in it after that was not one the document load let through, and is left out.
pub fn packages(
    fix: &Fix,
    manager: PackageManager,
    python: Option<(u32, u32)>,
) -> Option<Vec<String>> {
    let names = fix.packages(manager);
    if names.is_empty() {
        return None;
    }
    let version = python.map(|(major, minor)| format!("{major}.{minor}"));
    let digits = python.map(|(major, minor)| format!("{major}{minor}"));
    let mut out = Vec::new();
    for name in names {
        let mut name = name.clone();
        if name.contains("${python.") {
            match (&version, &digits) {
                (Some(version), Some(digits)) => {
                    name = name
                        .replace("${python.version}", version)
                        .replace("${python.digits}", digits);
                }
                // a package named for a Python nobody knows is not a package that can be named
                _ => return None,
            }
        }
        if name.contains('$') {
            return None;
        }
        out.push(name);
    }
    Some(out)
}

/// Whether `found` (`595.58.03`) is `wanted` (`580`) or newer, number by number; a number that is missing
/// counts as 0, and a version that is not numbers is older than anything.
pub fn version_at_least(found: &str, wanted: &str) -> bool {
    let numbers = |text: &str| -> Option<Vec<u64>> {
        text.trim()
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect()
    };
    let (Some(found), Some(wanted)) = (numbers(found), numbers(wanted)) else {
        return false;
    };
    for i in 0..found.len().max(wanted.len()) {
        let (f, w) = (
            found.get(i).copied().unwrap_or(0),
            wanted.get(i).copied().unwrap_or(0),
        );
        if f != w {
            return f > w;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_compared_number_by_number() {
        assert!(version_at_least("595.58.03", "580"));
        assert!(version_at_least("580", "580"));
        assert!(version_at_least("580.65.06", "580.65"));
        assert!(version_at_least("580", "580.0.0"));
        assert!(!version_at_least("575.64", "580"));
        assert!(!version_at_least("580.64", "580.65"));
        // 1000 is more than 999, which a comparison of text would not say
        assert!(version_at_least("1000", "999"));
        assert!(!version_at_least("", "580"));
        assert!(!version_at_least("NVRM", "580"));
    }
}
