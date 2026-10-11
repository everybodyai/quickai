//! The configurations a registry holds: the `profile` documents of every layer, read and held to the
//! engines of the registry (docs/configurations.md, section 3).
//!
//! A document that cannot be used (it has no engine, a model with two sources, a name that is too
//! long) is left out with a diagnostic that names the file and line, and the document of a lower
//! layer stays in force, as with every kind. A document that can be used is kept with what is odd
//! about it: a setting the engine does not have is a warning here, and a note when the same file is
//! checked for another computer (`profile::check`). Whether the configuration can be used *on this
//! computer* is not decided here: an engine this build lacks, or a build that is not installed, are
//! for the person to be told when they choose it.

use std::collections::BTreeMap;

use crate::config::{Layer, MergedDoc};
use crate::diag::{Diagnostic, Severity};
use crate::docpath::DocPath;
use crate::profile::{self, Configuration};
use crate::registry::{Engine, Vocabulary};

/// A configuration of the registry and where it came from.
#[derive(Clone, Debug)]
pub struct ProfileEntry {
    pub configuration: Configuration,
    /// What is odd about the file, apart from the keys it has that this version does not know.
    pub notes: Vec<profile::notes::Note>,
    /// The highest layer that has a file for it: where the person would look for it.
    pub layer: Layer,
    /// The file that layer has, as diagnostics name it.
    pub file: String,
}

/// One document: its configuration, with the places in the file that are wrong as diagnostics.
pub(super) fn try_build(
    merged: &MergedDoc,
    vocabulary: &Vocabulary,
    engines: &BTreeMap<String, Engine>,
) -> (Option<ProfileEntry>, Vec<Diagnostic>) {
    let mut problems = Vec::new();
    let read = match profile::read(&merged.envelope.id, &merged.body, engines) {
        Ok(read) => read,
        Err(fatal) => {
            for f in fatal {
                let mut d = merged.diagnostic(Severity::Error, &f.path, f.message);
                if let Some(hint) = f.hint {
                    d = d.with_hint(hint);
                }
                problems.push(d);
            }
            return (None, problems);
        }
    };
    for path in &read.unknown {
        problems.push(merged.diagnostic(
            Severity::Warning,
            path,
            format!("`{path}` is not used by this version of QuickAI and is ignored"),
        ));
    }
    for note in read.notes.iter().filter(|n| n.id != "unknown_keys") {
        problems.push(merged.diagnostic(
            Severity::Warning,
            note.path.as_ref().unwrap_or(&DocPath::default()),
            note.text(vocabulary),
        ));
    }
    let last = merged
        .contributors
        .last()
        .expect("a document has at least one contributor");
    (
        Some(ProfileEntry {
            configuration: read.configuration,
            notes: read.notes,
            layer: last.layer,
            file: last.file.clone(),
        }),
        problems,
    )
}
