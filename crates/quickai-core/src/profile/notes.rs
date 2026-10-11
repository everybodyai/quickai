//! What a person is told about a configuration: the notes, and what each one is allowed to do.
//!
//! The design is in docs/configurations.md, section 7. A note has a stable **id**, a **level** and
//! the **holes** of its sentence. The sentence is not here: it is the vocabulary's (`[[finding]]`),
//! so it can be worded again, translated, or made plainer without a change of code. The level *is*
//! here, and cannot be changed by a document: a note that stops an import or asks a question is
//! behaviour, and a document, which may come from anyone, must not be able to turn a question into a
//! statement.

use serde::Serialize;

use crate::docpath::DocPath;
use crate::registry::Vocabulary;

/// How much a note matters, from the one that matters most. The order of the variants is the order
/// a list of notes is read in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// The file cannot be used here; nothing is stored.
    Stops,
    /// A yes is needed before something is taken.
    Asks,
    /// It differs from what the file was made on; it can be used.
    Warns,
    /// Information.
    Says,
}

impl Level {
    /// The word a window puts in front of the sentence.
    pub fn label(self) -> &'static str {
        match self {
            Level::Stops => "Stops",
            Level::Asks => "Asks",
            Level::Warns => "Warns",
            Level::Says => "Says",
        }
    }
}

/// One kind of note: the name the code and the vocabulary know it by, its level, and the words of
/// its sentence that the code fills in (`{needs}` in `Made by a newer QuickAI (it needs {needs})`).
pub struct NoteSpec {
    pub id: &'static str,
    pub level: Level,
    pub holes: &'static [&'static str],
}

const fn spec(id: &'static str, level: Level, holes: &'static [&'static str]) -> NoteSpec {
    NoteSpec { id, level, holes }
}

/// Every note this version can make. The vocabulary must give each one a sentence (a vocabulary
/// that gives some and not all is a load error), and a sentence may use only the holes listed here.
pub const NOTES: &[NoteSpec] = &[
    spec("not_a_configuration", Level::Stops, &["why"]),
    spec("too_new", Level::Stops, &["needs", "this"]),
    spec("unknown_engine", Level::Stops, &["engine"]),
    spec("free_text", Level::Asks, &["field", "engine"]),
    spec("wider_access", Level::Asks, &["field", "engine", "value"]),
    spec("name_taken", Level::Asks, &["name"]),
    spec("refused_line", Level::Warns, &["name", "field"]),
    spec("names_a_path", Level::Warns, &["field", "path"]),
    spec("unknown_setting", Level::Warns, &["setting"]),
    spec("private_setting", Level::Warns, &["setting"]),
    spec("bad_value", Level::Warns, &["field", "value", "why"]),
    spec("unknown_keys", Level::Warns, &["keys"]),
    spec("other_system", Level::Warns, &["made", "this"]),
    spec("other_gpu", Level::Warns, &["text"]),
    spec("memory", Level::Warns, &["model", "need", "have"]),
    spec("model_to_get", Level::Warns, &["model", "from"]),
    spec("model_unknown", Level::Warns, &["model"]),
    spec("model_unseen", Level::Says, &["model", "engine"]),
    spec("build_missing", Level::Warns, &["build", "have"]),
    spec("build_none", Level::Warns, &["build"]),
    spec("port_taken", Level::Warns, &["field", "port"]),
    spec("ports_clash", Level::Warns, &["fields", "port"]),
    spec("companion_missing", Level::Warns, &["companion"]),
    spec(
        "companion_address",
        Level::Warns,
        &["companion", "engine", "first", "now"],
    ),
    spec("will_change", Level::Says, &["list"]),
    spec("left_out_private", Level::Says, &["count"]),
    spec("left_out_secrets", Level::Says, &["count"]),
    spec("left_out_source", Level::Says, &["model"]),
];

/// The kind of note called `id`.
pub fn spec_of(id: &str) -> Option<&'static NoteSpec> {
    NOTES.iter().find(|n| n.id == id)
}

/// One thing a person is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: &'static str,
    pub level: Level,
    /// The words that fill the holes of its sentence.
    pub holes: Vec<(&'static str, String)>,
    /// Where in the file it points, when it points at a place.
    pub path: Option<DocPath>,
    /// What it is about, so that a window can offer what to do about it: the key of a setting
    /// (`ollama.extra_env`), the id of a model, the label of a build.
    pub subject: Option<String>,
    /// A sentence of the engine's own document (a field's `share_note`) that says it instead of the
    /// vocabulary's, because the document knows what the setting does.
    pub sentence: Option<String>,
}

impl Note {
    /// A note of kind `id` with these holes. A kind this version does not have is a mistake in the
    /// code, not in a document, so it panics (a test makes every kind).
    pub fn new(id: &'static str, holes: &[(&'static str, String)]) -> Note {
        let spec = spec_of(id).unwrap_or_else(|| panic!("there is no note called `{id}`"));
        for (name, _) in holes {
            assert!(
                spec.holes.contains(name),
                "the note `{id}` has no hole called `{name}`"
            );
        }
        Note {
            id: spec.id,
            level: spec.level,
            holes: holes.to_vec(),
            path: None,
            subject: None,
            sentence: None,
        }
    }

    /// Says it with a sentence of the document that knows (`share_note`).
    pub fn said(mut self, sentence: Option<&str>) -> Note {
        self.sentence = sentence.map(str::to_string);
        self
    }

    pub fn at(mut self, path: DocPath) -> Note {
        self.path = Some(path);
        self
    }

    pub fn about(mut self, subject: impl Into<String>) -> Note {
        self.subject = Some(subject.into());
        self
    }

    /// The hole called `name`.
    pub fn hole(&self, name: &str) -> Option<&str> {
        self.holes
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The sentence: the vocabulary's words with the holes filled in. A vocabulary that has no
    /// sentence for the note (a registry built to test something else) gives the id and the holes, so
    /// nothing is ever lost.
    pub fn text(&self, vocabulary: &Vocabulary) -> String {
        if let Some(sentence) = &self.sentence {
            return fill(sentence, &self.holes);
        }
        match vocabulary.finding(self.id) {
            Some(template) => fill(template, &self.holes),
            None => {
                let parts: Vec<String> = self
                    .holes
                    .iter()
                    .map(|(n, v)| format!("{n}: {v}"))
                    .collect();
                format!("{} ({})", self.id, parts.join(", "))
            }
        }
    }
}

/// `{name}` in `template` replaced by its hole. A hole the code did not fill stays as it is written,
/// so that a sentence with a misspelt hole shows it.
pub fn fill(template: &str, holes: &[(&'static str, String)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match holes.iter().find(|(n, _)| *n == name) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The holes a sentence uses: the words between `{` and `}`.
pub fn holes_of(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                found.push(after[..close].to_string());
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    found
}

/// Sorts notes as they are read: by level, then in the order they were made (the order of the
/// file), so that a person who reads them twice reads the same.
pub fn sort(notes: &mut [Note]) {
    notes.sort_by_key(|n| n.level);
}

/// Names a file may never set in the environment of a program, whatever the engine's document says
/// and whatever a vocabulary adds: they make a program load other code. A trailing `*` stands for
/// any ending. This is a floor that no document can lower; the vocabulary's `[profile] deny_env`
/// adds to it.
pub const DENIED_ENV: &[&str] = &["LD_*", "DYLD_*", "PATH", "PYTHON*"];

/// Whether `name` is a name that may not be set from a file: one of `DENIED_ENV` or of `extra`, in
/// any case.
pub fn env_name_denied(name: &str, extra: &[String]) -> bool {
    let upper = name.trim().to_ascii_uppercase();
    DENIED_ENV
        .iter()
        .copied()
        .chain(extra.iter().map(String::as_str))
        .any(|pattern| match pattern.strip_suffix('*') {
            Some(prefix) => upper.starts_with(&prefix.to_ascii_uppercase()),
            None => upper == pattern.to_ascii_uppercase(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sentence_has_its_holes_filled_in_and_a_strange_one_stays_visible() {
        let holes = [("needs", "0.3".to_string()), ("this", "0.1".to_string())];
        assert_eq!(
            fill("it needs {needs}; this is {this}.", &holes),
            "it needs 0.3; this is 0.1."
        );
        assert_eq!(fill("it needs {nope}.", &holes), "it needs {nope}.");
        assert_eq!(fill("open { brace", &holes), "open { brace");
        assert_eq!(fill("no holes", &holes), "no holes");
        assert_eq!(holes_of("a {x} b {y} {x"), vec!["x", "y"]);
    }

    #[test]
    fn every_kind_of_note_can_be_made_and_has_distinct_holes() {
        let mut seen = std::collections::BTreeSet::new();
        for spec in NOTES {
            assert!(seen.insert(spec.id), "{} is listed twice", spec.id);
            let holes: Vec<(&'static str, String)> =
                spec.holes.iter().map(|h| (*h, "x".to_string())).collect();
            let note = Note::new(spec.id, &holes);
            assert_eq!(note.level, spec.level);
        }
    }

    #[test]
    fn levels_are_read_from_the_one_that_stops_to_the_one_that_says() {
        let mut notes = vec![
            Note::new("will_change", &[("list", "a".into())]),
            Note::new("unknown_engine", &[("engine", "x".into())]),
            Note::new(
                "free_text",
                &[("field", "f".into()), ("engine", "e".into())],
            ),
            Note::new("unknown_keys", &[("keys", "k".into())]),
        ];
        sort(&mut notes);
        let order: Vec<&str> = notes.iter().map(|n| n.id).collect();
        assert_eq!(
            order,
            vec!["unknown_engine", "free_text", "unknown_keys", "will_change"]
        );
    }

    #[test]
    fn the_names_a_file_may_not_set_are_refused_in_any_case() {
        for name in [
            "LD_PRELOAD",
            "ld_library_path",
            "DYLD_INSERT_LIBRARIES",
            "Path",
            "PYTHONPATH",
            "pythonstartup",
        ] {
            assert!(env_name_denied(name, &[]), "{name}");
        }
        for name in [
            "OLLAMA_FLASH_ATTENTION",
            "HF_TOKEN",
            "CUDA_VISIBLE_DEVICES",
            "MYPATH",
        ] {
            assert!(!env_name_denied(name, &[]), "{name}");
        }
        assert!(env_name_denied(
            "NODE_OPTIONS",
            &["NODE_OPTIONS".to_string()]
        ));
        assert!(env_name_denied("JAVA_ANYTHING", &["java_*".to_string()]));
    }
}
