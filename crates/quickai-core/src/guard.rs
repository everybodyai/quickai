//! The foreign-program guard (docs/architecture.md, section 12.1; decision C33): a document that
//! says `program = ...` is code. An engine document of the application's own, or one that starts only
//! what the application found or installed, runs without a question; a document from the person's
//! configuration that starts a program the application did not find asks once, and is remembered by
//! the hash of what the merged document says, not by how it is written.
//!
//! Pure: the machine's answer to "is this program one the engine's own strategies find?" comes in as
//! a function, and where the answers are kept is the application's.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::config::Layer;
use crate::registry::Engine;

/// A program the person has not yet said may run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Foreign {
    /// What the run plan would start.
    pub program: String,
    /// The hash of the merged document (64 hexadecimal digits): what is remembered.
    pub hash: String,
    /// The files above the application's own layer that make the document what it is.
    pub files: Vec<String>,
}

/// What the guard says about a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to ask: the document is the application's own, or the program is one it found.
    Trusted,
    /// The person said yes to exactly this document before.
    Confirmed,
    /// Nothing starts until the person says yes.
    NeedsConfirmation(Foreign),
}

/// Whether the guard has anything to look at: some layer above the application's own contributed.
fn user_files(engine: &Engine) -> Vec<String> {
    engine
        .merged
        .contributors
        .iter()
        .filter(|c| !matches!(c.layer, Layer::Embedded | Layer::Bundle))
        .map(|c| c.file.clone())
        .collect()
}

/// The verdict on starting `program` with this engine's document. `trusted` says whether a program
/// is one the application found by the strategies of its own document or installed itself;
/// `confirmed` are the hashes the person has said yes to.
pub fn judge(
    engine: &Engine,
    program: &str,
    trusted: &dyn Fn(&str) -> bool,
    confirmed: &[String],
) -> Verdict {
    let files = user_files(engine);
    if files.is_empty() || trusted(program) {
        return Verdict::Trusted;
    }
    let hash = document_hash(engine);
    if confirmed.contains(&hash) {
        return Verdict::Confirmed;
    }
    Verdict::NeedsConfirmation(Foreign {
        program: program.to_string(),
        hash,
        files,
    })
}

/// The hash of what the merged document says: the same for a document written another way (other
/// spacing, comments, the order of its keys) and different for any change of meaning.
pub fn document_hash(engine: &Engine) -> String {
    let mut text = format!("engine {}\n", engine.id);
    canonical(&toml::Value::Table(engine.merged.body.clone()), &mut text);
    let mut hex = String::new();
    for byte in Sha256::digest(text.as_bytes()) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// A text for a value that does not depend on how it was written: tables by key in order, lists as
/// they are, strings quoted.
fn canonical(value: &toml::Value, out: &mut String) {
    match value {
        toml::Value::String(s) => {
            let _ = write!(out, "{s:?}");
        }
        toml::Value::Integer(n) => {
            let _ = write!(out, "{n}");
        }
        toml::Value::Float(f) => {
            let _ = write!(out, "{f:?}");
        }
        toml::Value::Boolean(b) => {
            let _ = write!(out, "{b}");
        }
        toml::Value::Datetime(d) => {
            let _ = write!(out, "{d}");
        }
        toml::Value::Array(items) => {
            out.push('[');
            for item in items {
                canonical(item, out);
                out.push(',');
            }
            out.push(']');
        }
        toml::Value::Table(table) => {
            let mut keys: Vec<&String> = table.keys().collect();
            keys.sort();
            out.push('{');
            for key in keys {
                let _ = write!(out, "{key:?}:");
                canonical(&table[key], out);
                out.push(',');
            }
            out.push('}');
        }
    }
}
