//! Scans a models directory for GGUF files and attaches display metadata for
//! the ones we know about. Anything else dropped into that directory still
//! shows up (with just its filename) — that's how "other models" get picked
//! up without touching this file.

use std::path::{Path, PathBuf};

use quickai_core::lookup::glob;
use quickai_core::plan::ModelInfo;

use crate::bundles::{self, Companion};
use crate::prims::engine_models::ModelRow;

pub struct KnownModel {
    pub filename: &'static str,
    pub display: &'static str,
    pub note: &'static str,
    /// The model the packages ship: its row of the list says `bundled`.
    pub bundled: bool,
    /// How the engine is started for it, as the id of one of its `[[models.mode]]`, when no record beside
    /// the weights says (the model the packages ship has none).
    pub mode: Option<&'static str>,
}

pub const KNOWN_MODELS: &[KnownModel] = &[
    KnownModel {
        filename: "Ternary-Bonsai-2-4B-Q2_0.gguf",
        display: "Bonsai 2 4B (ternary Q2_0)",
        note: "Ternary weights, ~1.2 GB, Apache-2.0, made by Prism ML from Qwen3.5-4B. Bundled with this package.",
        bundled: true,
        mode: Some("direct"),
    },
    KnownModel {
        filename: "qwen3-1.7b-q8_0.gguf",
        display: "Qwen3 1.7B (Q8_0)",
        note: "Instruction-tuned chat model, small and fast.",
        bundled: false,
        mode: None,
    },
    KnownModel {
        filename: "qwen3-4b-q4_k_m.gguf",
        display: "Qwen3 4B (Q4_K_M)",
        note: "Instruction-tuned chat model, larger and slower than the 1.7B.",
        bundled: false,
        mode: None,
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
    /// The files that were fetched with the weights and go with them (a vision model's projector), as
    /// the record beside the weights names them; they may have been removed since.
    pub companions: Vec<Companion>,
    /// How the engine is started for this model, as the id of one of its `[[models.mode]]`, when the
    /// record beside the weights says (a model fetched from the recommended list that is served
    /// differently, an embedding model).
    pub mode: Option<String>,
}

impl ModelEntry {
    /// A model an engine lists by name, as the picker holds one: there is no file, so `path` is
    /// empty, `filename` is the name the engine knows it by (what is remembered, and what a request
    /// names), and the size is the one the listing gave, which the context-size heuristic and the
    /// speed estimate then use as they use a file's.
    #[allow(dead_code)] // the model box for engines that list their models arrives with 3.7
    pub fn from_row(row: &ModelRow) -> ModelEntry {
        ModelEntry {
            filename: row.id.clone(),
            path: PathBuf::new(),
            display: row.display.clone(),
            note: row.capabilities.join(", "),
            bundled: false,
            size_bytes: row.size_bytes.unwrap_or(0),
            companions: Vec::new(),
            mode: None,
        }
    }

    /// The model as the form and the planner know one.
    pub fn info(&self) -> ModelInfo {
        ModelInfo {
            id: self.filename.clone(),
            display: self.display.clone(),
            path: self.path.display().to_string(),
            size_bytes: self.size_bytes,
        }
    }
}

/// Scans both the bundled and user (downloaded-into) models directories and
/// merges the results into one alphabetically-sorted list, so the picker
/// shows everything regardless of which of the two a given file lives in.
/// A filename present in both wins from `bundled_dir` (shouldn't normally
/// happen, but bundled metadata is the more trustworthy of the two).
/// `hide` are the patterns of the files that are companions and not models
/// (`mmproj*.gguf`, from the engine's bundles).
pub fn discover_all(bundled_dir: &Path, user_dir: &Path, hide: &[String]) -> Vec<ModelEntry> {
    let mut models = discover_models(bundled_dir, hide);
    for m in discover_models(user_dir, hide) {
        if !models
            .iter()
            .any(|existing| existing.filename == m.filename)
        {
            models.push(m);
        }
    }
    models.sort_by(|a, b| a.filename.cmp(&b.filename));
    models
}

/// What the recommended list calls the weights it offers, by the name they are kept under: the
/// name of the model and of the quantization, and what the list says it is good for.
pub fn names_from_list(
    entries: &[quickai_core::lookup::present::Entry],
) -> std::collections::BTreeMap<String, (String, String)> {
    use quickai_core::lookup::present::Fetch;
    let mut names = std::collections::BTreeMap::new();
    for entry in entries {
        for offer in &entry.offers {
            let Fetch::Files(download) = &offer.fetch else {
                continue;
            };
            if let Some(weights) = download.weights() {
                let quantization = offer.label.split(" · ").next().unwrap_or_default();
                names.entry(weights.store_as.clone()).or_insert_with(|| {
                    (
                        format!("{} ({quantization})", entry.name),
                        entry.description.clone(),
                    )
                });
            }
        }
    }
    names
}

/// Gives the models the names the list knows them by, except those this file already names: a
/// model fetched from the list is `Qwen3.5 4B (Q4_K_M)` in the picker and not a file name.
pub fn name_from_list(
    models: &mut [ModelEntry],
    known: &std::collections::BTreeMap<String, (String, String)>,
) {
    for model in models {
        if model.display != model.filename {
            continue;
        }
        if let Some((display, note)) = known.get(&model.filename) {
            model.display = display.clone();
            model.note = if model.note.is_empty() {
                note.clone()
            } else {
                format!("{note} {}", model.note)
            };
        }
    }
}

/// Says in the note of a model how it is started, when its record names a mode: with the label the
/// engine's document gives the mode, or that it has none.
pub fn say_modes(models: &mut [ModelEntry], labels: &[(String, String)]) {
    for model in models {
        let Some(mode) = &model.mode else { continue };
        let sentence = match labels.iter().find(|(id, _)| id == mode) {
            Some((_, label)) => format!("Starts as an {label}."),
            None => format!(
                "It is meant to start in the mode `{mode}`, which this engine does not have."
            ),
        };
        if !model.note.is_empty() {
            model.note.push(' ');
        }
        model.note.push_str(&sentence);
    }
}

/// The models of one folder: its `.gguf` files, but for the ones that are not models: a file that a
/// record beside some weights names as a companion, and a file whose name `hide` says is one (a
/// projector dropped into the folder by hand is not a model either).
pub fn discover_models(models_dir: &Path, hide: &[String]) -> Vec<ModelEntry> {
    discover_models_of(models_dir, &[".gguf".to_string()], hide)
}

/// The models of the places an engine's document names (`[[models.sources]]`), each with the endings
/// of its own, in the order the document gives them, a file name once.
pub fn discover_in_sources(
    sources: &[quickai_core::models::FileSource],
    hide: &[String],
) -> Vec<ModelEntry> {
    let mut models: Vec<ModelEntry> = Vec::new();
    for source in sources {
        for model in discover_models_of(Path::new(&source.path), &source.ext, hide) {
            if !models.iter().any(|m| m.filename == model.filename) {
                models.push(model);
            }
        }
    }
    models.sort_by(|a, b| a.filename.cmp(&b.filename));
    models
}

/// The models of one folder that end in one of `exts` (with the dot, in lower case; any when none).
fn discover_models_of(models_dir: &Path, exts: &[String], hide: &[String]) -> Vec<ModelEntry> {
    let Ok(read_dir) = std::fs::read_dir(models_dir) else {
        return Vec::new();
    };

    let named = bundles::referenced(models_dir);
    let mut paths: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            exts.is_empty()
                || p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| exts.contains(&format!(".{}", e.to_ascii_lowercase())))
        })
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            !named.contains(name) && !glob::matches_any(hide, name)
        })
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
            let companions = bundles::companions(&path);
            let mode =
                bundles::mode(&path).or_else(|| known.and_then(|m| m.mode).map(str::to_string));
            let mut note = known.map(|m| m.note.to_string()).unwrap_or_default();
            for companion in &companions {
                if !note.is_empty() {
                    note.push(' ');
                }
                note.push_str(&if companion.exists() {
                    format!("With its {}: {}.", companion.label, companion.name)
                } else {
                    format!(
                        "Its {} ({}) is not there any more: it starts without it.",
                        companion.label, companion.name
                    )
                });
            }
            ModelEntry {
                filename: filename.clone(),
                path,
                display: known.map(|m| m.display.to_string()).unwrap_or(filename),
                note,
                bundled: known.map(|m| m.bundled).unwrap_or(false),
                size_bytes,
                companions,
                mode,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_two_files_of_the_recommended_list_are_kept_under_the_same_name() {
        // a file kept under another's name overwrites it, and its record (a projector) is then the other's
        use quickai_core::lookup::present::Fetch;
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        for engine in registry.engines() {
            let mut kept: std::collections::BTreeMap<String, Option<String>> = Default::default();
            for entry in quickai_core::models::recommended(&registry, engine) {
                for offer in &entry.offers {
                    let Fetch::Files(download) = &offer.fetch else {
                        continue;
                    };
                    for piece in &download.files {
                        let key = match &piece.place {
                            Some(place) => format!("{place}/{}", piece.store_as),
                            None => piece.store_as.clone(),
                        };
                        if let Some(before) = kept.insert(key.clone(), piece.sha256.clone()) {
                            assert_eq!(
                                before, piece.sha256,
                                "{}: two files are kept as {key} ({})",
                                engine.id, entry.name
                            );
                        }
                    }
                }
            }
        }
    }

    use super::*;
    use crate::testkit::scratch;
    use quickai_core::lookup::present::{Download, Piece};

    fn hide() -> Vec<String> {
        vec!["mmproj*.gguf".to_string(), "*mmproj*.gguf".to_string()]
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    fn names(models: &[ModelEntry]) -> Vec<&str> {
        models.iter().map(|m| m.filename.as_str()).collect()
    }

    fn pair(dir: &Path, weights: &str, projector: &str) {
        let piece = |kind: Option<&str>, label: &str, file: &str, name: &str| Piece {
            kind: kind.map(str::to_string),
            label: label.to_string(),
            url: String::new(),
            store_as: file.to_string(),
            name: name.to_string(),
            size: None,
            sha256: None,
            place: None,
        };
        crate::bundles::write(
            &dir.join(weights),
            &Download {
                files: vec![
                    piece(None, "model", weights, weights),
                    piece(Some("mmproj"), "projector", projector, "mmproj-f16.gguf"),
                ],
                bundle: Some("vision".into()),
                source: "huggingface:o/r".into(),
                mode: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn p3_b16_the_picker_lists_weights_and_not_the_files_that_go_with_them() {
        let dir = scratch("b16");
        for f in [
            "a-Q4_K_M.gguf",
            "b.gguf",
            "mmproj-by-hand.gguf",
            "o--r--mmproj-f16.gguf",
            "notes.txt",
        ] {
            touch(&dir, f);
        }
        pair(&dir, "a-Q4_K_M.gguf", "o--r--mmproj-f16.gguf");

        // with what the engine's bundles say: the projector dropped in by hand is not a model either,
        // and neither is the one a record names, whatever it is called
        let models = discover_models(&dir, &hide());
        assert_eq!(names(&models), ["a-Q4_K_M.gguf", "b.gguf"]);
        let paired = &models[0];
        assert_eq!(paired.companions.len(), 1);
        assert_eq!(paired.companions[0].kind, "mmproj");
        assert!(
            paired.note.contains("With its projector: mmproj-f16.gguf."),
            "{}",
            paired.note
        );
        assert!(models[1].companions.is_empty() && models[1].note.is_empty());

        // a record's file is hidden even where nothing says its name looks like one
        assert_eq!(
            names(&discover_models(&dir, &[])),
            ["a-Q4_K_M.gguf", "b.gguf", "mmproj-by-hand.gguf"]
        );
        // a projector that has gone is said, and the model is still listed
        std::fs::remove_file(dir.join("o--r--mmproj-f16.gguf")).unwrap();
        let models = discover_models(&dir, &hide());
        assert_eq!(names(&models), ["a-Q4_K_M.gguf", "b.gguf"]);
        assert!(
            models[0].note.contains("is not there any more"),
            "{}",
            models[0].note
        );
        assert!(!models[0].companions[0].exists());
    }

    #[test]
    fn p3_b16b_both_folders_are_scanned_the_same_way_and_a_known_models_note_stays() {
        let bundled = scratch("b16-bundled");
        let user = scratch("b16-user");
        touch(&bundled, "Ternary-Bonsai-2-4B-Q2_0.gguf");
        touch(&bundled, "mmproj-bundled.gguf");
        touch(&user, "z.gguf");
        touch(&user, "mmproj-user.gguf");
        let models = discover_all(&bundled, &user, &hide());
        assert_eq!(names(&models), ["Ternary-Bonsai-2-4B-Q2_0.gguf", "z.gguf"]);
        assert!(
            models[0].bundled && models[0].note.contains("Bundled with this package"),
            "{}",
            models[0].note
        );
        // the model the packages ship starts with its thinking off, with no record beside it to say so
        assert_eq!(models[0].mode.as_deref(), Some("direct"));
        assert_eq!(models[1].mode, None);
        // a folder that is not there is no models
        assert!(discover_models(&bundled.join("nope"), &hide()).is_empty());
        // no patterns: what a folder holds is what is listed, as it was before bundles
        assert_eq!(names(&discover_all(&bundled, &user, &[])).len(), 4);
    }
}
