//! Scans a models directory for GGUF files and attaches display metadata for
//! the ones we know about. Anything else dropped into that directory still
//! shows up (with just its filename) — that's how "other models" get picked
//! up without touching this file.

use std::path::{Path, PathBuf};

pub struct KnownModel {
    pub filename: &'static str,
    pub display: &'static str,
    pub note: &'static str,
    /// Bundled-by-default models get a subtle highlight in the picker, so
    /// "pick a lighter one if this machine is slow" has an obvious next
    /// step even before Model URL/Look up online are touched.
    pub bundled: bool,
}

pub const KNOWN_MODELS: &[KnownModel] = &[
    KnownModel {
        filename: "Bonsai-8B-Q1_0.gguf",
        display: "Bonsai 8B (1-bit Q1_0)",
        note: "1-bit quantized, ~1.1 GB, Apache-2.0. Bundled with this package.",
        bundled: true,
    },
    KnownModel {
        filename: "Bonsai-1.7B-Q1_0.gguf",
        display: "Bonsai 1.7B (1-bit Q1_0)",
        note: "1-bit quantized, ~0.25 GB, Apache-2.0. Bundled with this package — the \
               lighter pick if the 8B runs slowly on this machine.",
        bundled: true,
    },
    KnownModel {
        filename: "bonsai-1.7b-q1_0.gguf",
        display: "Bonsai 1.7B (1-bit Q1_0)",
        note: "1-bit quantized, ~0.25 GB, Apache-2.0.",
        bundled: true,
    },
    KnownModel {
        filename: "qwen3-1.7b-q8_0.gguf",
        display: "Qwen3 1.7B (Q8_0)",
        note: "Instruction-tuned chat model, small and fast.",
        bundled: false,
    },
    KnownModel {
        filename: "qwen3-4b-q4_k_m.gguf",
        display: "Qwen3 4B (Q4_K_M)",
        note: "Instruction-tuned chat model, larger and slower than the 1.7B.",
        bundled: false,
    },
];

#[derive(Clone)]
pub struct ModelEntry {
    pub filename: String,
    pub path: PathBuf,
    pub display: String,
    pub note: String,
    pub bundled: bool,
    pub size_bytes: u64,
}

/// Scans both the bundled and user (downloaded-into) models directories and
/// merges the results into one alphabetically-sorted list, so the picker
/// shows everything regardless of which of the two a given file lives in.
/// A filename present in both wins from `bundled_dir` (shouldn't normally
/// happen, but bundled metadata is the more trustworthy of the two).
pub fn discover_all(bundled_dir: &Path, user_dir: &Path) -> Vec<ModelEntry> {
    let mut models = discover_models(bundled_dir);
    for m in discover_models(user_dir) {
        if !models.iter().any(|existing| existing.filename == m.filename) {
            models.push(m);
        }
    }
    models.sort_by(|a, b| a.filename.cmp(&b.filename));
    models
}

pub fn discover_models(models_dir: &Path) -> Vec<ModelEntry> {
    let Ok(read_dir) = std::fs::read_dir(models_dir) else {
        return Vec::new();
    };

    let mut paths: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("gguf"))
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let known = KNOWN_MODELS.iter().find(|m| m.filename == filename);
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            ModelEntry {
                filename: filename.clone(),
                path,
                display: known.map(|m| m.display.to_string()).unwrap_or(filename),
                note: known.map(|m| m.note.to_string()).unwrap_or_default(),
                bundled: known.map(|m| m.bundled).unwrap_or(false),
                size_bytes,
            }
        })
        .collect()
}
