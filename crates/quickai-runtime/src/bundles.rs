//! What keeps a model's weights and the files that go with them together: a small record beside the
//! weights, `<weights>.bundle.json` (docs/lookup-and-bundles.md, section 4).
//!
//! The record is written after the last file of a bundle is in place and names the files, so the
//! weights need no particular name for the application to know what goes with them and the model
//! picker lists weights only. What it names is held to what it says: a file name that is not a plain
//! name in the same folder is not believed, and a companion that has gone is not an error but a
//! model that starts without it.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use quickai_core::lookup::present::{is_place, safe_file_name, Download};
use serde::{Deserialize, Serialize};

/// What is added to the weights' file name to make the name of the record.
pub const RECORD_SUFFIX: &str = ".bundle.json";

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    /// The bundle the files were fetched as; empty for a model that has a mode and no companion.
    #[serde(default)]
    bundle: String,
    source: String,
    parts: Vec<RecordPart>,
    /// The id of the engine's `[[models.mode]]` the model is started in. Only the id is kept, never
    /// the words: what reaches the command line is the engine document's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct RecordPart {
    kind: String,
    label: String,
    /// The name it is kept under, in the folder of the weights or, with `dir`, in the folder of that
    /// name beside the weights' own.
    file: String,
    /// The name it has where it came from.
    name: String,
    /// The folder of the models folder it is kept in (`text_encoders`), for an engine that finds its
    /// files by the folder they are in; absent when it is beside the weights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dir: Option<String>,
}

/// A file that goes with a model's weights.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Companion {
    /// The kind of component it is (`mmproj`), which the engine's document says how to attach.
    pub kind: String,
    /// What the window calls it (`projector`).
    pub label: String,
    /// The name it has where it came from.
    pub name: String,
    /// Where it is.
    pub path: PathBuf,
}

impl Companion {
    pub fn exists(&self) -> bool {
        self.path.is_file()
    }
}

/// The record of these weights: `models/m.gguf` has `models/m.gguf.bundle.json`.
pub fn record_path(weights: &Path) -> PathBuf {
    let mut name = weights.file_name().unwrap_or_default().to_os_string();
    name.push(RECORD_SUFFIX);
    weights.with_file_name(name)
}

/// Writes the record of a download, whole or not at all (a temporary file, then a rename). A download
/// of the weights alone, started the usual way, has none.
pub fn write(weights: &Path, download: &Download) -> std::io::Result<()> {
    if download.bundle.is_none() && download.mode.is_none() {
        return Ok(());
    }
    let record = Record {
        version: 1,
        bundle: download.bundle.clone().unwrap_or_default(),
        mode: download.mode.clone(),
        source: download.source.clone(),
        parts: download
            .files
            .iter()
            .filter_map(|piece| {
                piece.kind.as_ref().map(|kind| RecordPart {
                    kind: kind.clone(),
                    label: piece.label.clone(),
                    file: piece.store_as.clone(),
                    name: piece.name.clone(),
                    dir: piece.place.clone(),
                })
            })
            .collect(),
    };
    let path = record_path(weights);
    let temporary = path.with_extension("json.part");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(
        serde_json::to_string_pretty(&record)
            .map_err(std::io::Error::other)?
            .as_bytes(),
    )?;
    file.write_all(b"\n")?;
    drop(file);
    std::fs::rename(&temporary, &path)
}

fn read_record(weights: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(record_path(weights)).ok()?;
    let record: Record = serde_json::from_str(&text).ok()?;
    (record.version == 1).then_some(record)
}

/// The mode the record beside `weights` says the model is started in: the id of one of the engine's
/// `[[models.mode]]`. None when there is no record, it is not one this version reads, or the id is
/// not a plain name (a record is a file in a folder the person can write to, and is not believed
/// beyond what it may say).
pub fn mode(weights: &Path) -> Option<String> {
    let record = read_record(weights)?;
    record.mode.filter(|m| valid_mode_id(m))
}

fn valid_mode_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
}

/// The companions the record beside `weights` names, whether or not they are still there; none when
/// there is no record, it is not one this version reads, or a name in it is not a plain name.
pub fn companions(weights: &Path) -> Vec<Companion> {
    let Some(record) = read_record(weights) else {
        return Vec::new();
    };
    let folder = weights.parent().map(Path::to_path_buf).unwrap_or_default();
    // the models folder, when the weights are in a folder of it (the folders of one engine are side by side)
    let root = folder.parent().map(Path::to_path_buf).unwrap_or_default();
    record
        .parts
        .into_iter()
        // a name of a record is a file of this folder, or of a folder beside it that is a plain name:
        // whatever else it says is not believed
        .filter(|part| {
            safe_file_name(&part.file) == part.file
                && !part.kind.is_empty()
                && part.dir.as_deref().is_none_or(is_place)
        })
        .map(|part| Companion {
            path: match &part.dir {
                Some(dir) => root.join(dir).join(&part.file),
                None => folder.join(&part.file),
            },
            kind: part.kind,
            label: part.label,
            name: part.name,
        })
        .collect()
}

/// The names of every companion any record in `folder` names, that is kept beside the weights. (One
/// that is kept in a folder of its own is not in this folder to be taken for a model.)
pub fn referenced(folder: &Path) -> BTreeSet<String> {
    let Ok(read_dir) = std::fs::read_dir(folder) else {
        return BTreeSet::new();
    };
    let mut names = BTreeSet::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(weights) = name.strip_suffix(RECORD_SUFFIX) {
            for companion in companions(&folder.join(weights)) {
                if companion.path.parent() != Some(folder) {
                    continue;
                }
                if let Some(file) = companion.path.file_name() {
                    names.insert(file.to_string_lossy().into_owned());
                }
            }
        }
    }
    names
}

/// The paths of every companion any record in any of `folders` names.
fn referenced_paths(folders: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    for folder in folders {
        let Ok(read_dir) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(weights) = name.strip_suffix(RECORD_SUFFIX) {
                paths.extend(
                    companions(&folder.join(weights))
                        .into_iter()
                        .map(|c| c.path),
                );
            }
        }
    }
    paths
}

/// What deleting a model took away.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Removed {
    /// Files that are gone, by name: the weights, its record and the companions nothing else uses.
    pub removed: Vec<String>,
    /// Companions that stay because another model's record names them.
    pub kept: Vec<String>,
    /// Why the weights themselves could not be removed; nothing else is touched then.
    pub weights_error: Option<String>,
    /// What else could not be removed, and why.
    pub failed: Vec<String>,
}

/// Deletes the weights, the record beside them, and the companions that no other record names (a
/// projector is shared by the quantizations of its repository). The weights go first: what it cannot
/// remove is what it says.
pub fn remove_model(weights: &Path) -> Removed {
    let mut report = Removed::default();
    let name_of = |p: &Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let companions = companions(weights);
    match std::fs::remove_file(weights) {
        Ok(()) => report.removed.push(name_of(weights)),
        Err(e) => {
            report.weights_error = Some(e.to_string());
            return report;
        }
    }
    let record = record_path(weights);
    if record.exists() {
        match std::fs::remove_file(&record) {
            Ok(()) => report.removed.push(name_of(&record)),
            Err(e) => report.failed.push(format!("{}: {e}", name_of(&record))),
        }
    }
    let folder = weights.parent().map(Path::to_path_buf).unwrap_or_default();
    // A companion kept in a folder of its own may be named by the record of weights in any folder
    // beside this one (a VAE is shared by the models of a family, whichever folder their weights are in).
    let mut folders = vec![folder.clone()];
    if companions.iter().any(|c| c.path.parent() != Some(&folder)) {
        if let Some(root) = folder.parent() {
            folders.extend(
                std::fs::read_dir(root)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && *p != folder),
            );
        }
    }
    let still_named = referenced_paths(&folders);
    for companion in companions {
        let file = name_of(&companion.path);
        if still_named.contains(&companion.path) {
            report.kept.push(file);
        } else if companion.path.exists() {
            match std::fs::remove_file(&companion.path) {
                Ok(()) => report.removed.push(file),
                Err(e) => report.failed.push(format!("{file}: {e}")),
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::scratch;
    use quickai_core::lookup::present::Piece;

    fn piece(kind: Option<&str>, label: &str, store_as: &str, name: &str) -> Piece {
        Piece {
            kind: kind.map(str::to_string),
            label: label.to_string(),
            url: format!("http://x/{name}"),
            store_as: store_as.to_string(),
            name: name.to_string(),
            size: Some(1),
            sha256: None,
            place: None,
        }
    }

    fn vision() -> Download {
        Download {
            files: vec![
                piece(None, "model", "m-Q4_K_M.gguf", "m-Q4_K_M.gguf"),
                piece(
                    Some("mmproj"),
                    "projector",
                    "o--r--mmproj-f16.gguf",
                    "mmproj-f16.gguf",
                ),
            ],
            bundle: Some("vision".into()),
            source: "huggingface:o/r".into(),
            mode: None,
        }
    }

    #[test]
    fn p3_b09_the_record_beside_the_weights_names_the_files_that_go_with_them() {
        let dir = scratch("record");
        let weights = dir.join("m-Q4_K_M.gguf");
        std::fs::write(&weights, b"w").unwrap();
        // none yet
        assert!(companions(&weights).is_empty());
        write(&weights, &vision()).unwrap();
        assert_eq!(record_path(&weights), dir.join("m-Q4_K_M.gguf.bundle.json"));
        let text = std::fs::read_to_string(record_path(&weights)).unwrap();
        assert!(
            text.contains("\"bundle\": \"vision\"") && text.contains("huggingface:o/r"),
            "{text}"
        );
        assert!(
            !dir.join("m-Q4_K_M.gguf.bundle.json.part").exists(),
            "written whole"
        );
        let found = companions(&weights);
        assert_eq!(
            found,
            [Companion {
                kind: "mmproj".into(),
                label: "projector".into(),
                name: "mmproj-f16.gguf".into(),
                path: dir.join("o--r--mmproj-f16.gguf"),
            }]
        );
        assert!(
            !found[0].exists(),
            "a companion that is not there is named all the same"
        );
        std::fs::write(dir.join("o--r--mmproj-f16.gguf"), b"p").unwrap();
        assert!(companions(&weights)[0].exists());
        // the weights alone have no record, and a record is replaced whole
        let alone = Download {
            files: vec![piece(None, "model", "x.gguf", "x.gguf")],
            bundle: None,
            source: "huggingface:o/r".into(),
            mode: None,
        };
        write(&dir.join("x.gguf"), &alone).unwrap();
        assert!(!record_path(&dir.join("x.gguf")).exists());
        write(&weights, &alone_bundle()).unwrap();
        assert_eq!(
            companions(&weights).len(),
            0,
            "a bundle with no companion names none"
        );
    }

    fn alone_bundle() -> Download {
        Download {
            files: vec![piece(None, "model", "m-Q4_K_M.gguf", "m-Q4_K_M.gguf")],
            bundle: Some("empty".into()),
            source: "huggingface:o/r".into(),
            mode: None,
        }
    }

    #[test]
    fn p3_j14_a_mode_is_kept_by_its_id_with_or_without_a_companion_and_is_not_believed_beyond_that()
    {
        let dir = scratch("j14");
        let weights = dir.join("e-Q8_0.gguf");
        let alone = |mode: Option<&str>| Download {
            files: vec![piece(None, "model", "e-Q8_0.gguf", "e-Q8_0.gguf")],
            bundle: None,
            source: "huggingface:o/r".into(),
            mode: mode.map(str::to_string),
        };
        // no companion and no mode: no record, as always; a mode alone is worth one
        write(&weights, &alone(None)).unwrap();
        assert!(!record_path(&weights).exists());
        assert_eq!(mode(&weights), None);
        write(&weights, &alone(Some("embedding"))).unwrap();
        assert!(record_path(&weights).is_file());
        assert_eq!(mode(&weights).as_deref(), Some("embedding"));
        assert!(companions(&weights).is_empty(), "a mode names no file");
        // the record has the id and nothing of the words it stands for
        let text = std::fs::read_to_string(record_path(&weights)).unwrap();
        assert!(text.contains("\"mode\": \"embedding\"") && !text.contains("--embeddings"));
        // a bundle and a mode together
        let both = Download {
            mode: Some("rerank".into()),
            ..vision()
        };
        let paired = dir.join("m-Q4_K_M.gguf");
        write(&paired, &both).unwrap();
        assert_eq!(mode(&paired).as_deref(), Some("rerank"));
        assert_eq!(companions(&paired).len(), 1);
        // a record of the first version of this work has no mode and is read as it was
        let old = dir.join("old.gguf");
        std::fs::write(
            record_path(&old),
            r#"{"version":1,"bundle":"vision","source":"huggingface:o/r","parts":[{"kind":"mmproj","label":"projector","file":"o--r--p.gguf","name":"p.gguf"}]}"#,
        )
        .unwrap();
        assert_eq!(mode(&old), None);
        assert_eq!(companions(&old).len(), 1);
        // a record is a file in a folder a person can write to: what is not a plain id is not a mode
        for hostile in [
            "--embeddings --api-key x",
            "../x",
            "Embedding",
            "",
            "a b",
            &"x".repeat(80),
        ] {
            std::fs::write(
                record_path(&old),
                format!(r#"{{"version":1,"source":"s","parts":[],"mode":"{hostile}"}}"#),
            )
            .unwrap();
            assert_eq!(mode(&old), None, "{hostile:?}");
        }
        // an unknown version is not read at all
        std::fs::write(
            record_path(&old),
            r#"{"version":2,"source":"s","parts":[],"mode":"embedding"}"#,
        )
        .unwrap();
        assert_eq!(mode(&old), None);
    }

    #[test]
    fn p3_j16_deleting_a_model_takes_a_record_that_has_only_a_mode() {
        let dir = scratch("j16");
        let weights = dir.join("e-Q8_0.gguf");
        std::fs::write(&weights, b"w").unwrap();
        let alone = Download {
            files: vec![piece(None, "model", "e-Q8_0.gguf", "e-Q8_0.gguf")],
            bundle: None,
            source: "huggingface:o/r".into(),
            mode: Some("embedding".into()),
        };
        write(&weights, &alone).unwrap();
        let report = remove_model(&weights);
        assert!(
            report.weights_error.is_none() && report.failed.is_empty(),
            "{report:?}"
        );
        assert_eq!(report.removed, ["e-Q8_0.gguf", "e-Q8_0.gguf.bundle.json"]);
        assert!(!weights.exists() && !record_path(&weights).exists());
    }

    #[test]
    fn p3_b10_a_record_is_not_believed_beyond_what_it_may_say() {
        let dir = scratch("hostile");
        let weights = dir.join("m.gguf");
        let record = |body: &str| std::fs::write(record_path(&weights), body).unwrap();
        // not JSON, an unknown version, the wrong shape
        for body in [
            "",
            "not json",
            "{}",
            r#"{"version":2,"bundle":"b","source":"s","parts":[]}"#,
            "[1,2]",
            "null",
        ] {
            record(body);
            assert!(companions(&weights).is_empty(), "{body}");
        }
        // names that climb out of the folder, or are not names, are left out; a plain one stays
        record(
            r#"{"version":1,"bundle":"b","source":"s","parts":[
                {"kind":"mmproj","label":"p","file":"../../etc/passwd","name":"x"},
                {"kind":"mmproj","label":"p","file":"/abs/path.gguf","name":"x"},
                {"kind":"mmproj","label":"p","file":"a/b.gguf","name":"x"},
                {"kind":"mmproj","label":"p","file":"","name":"x"},
                {"kind":"","label":"p","file":"ok.gguf","name":"x"},
                {"kind":"mmproj","label":"p","file":"ok.gguf","name":"x"}
            ]}"#,
        );
        let found = companions(&weights);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, dir.join("ok.gguf"));
    }

    #[test]
    fn p3_b11_deleting_a_model_takes_its_record_and_the_companions_nothing_else_uses() {
        let dir = scratch("remove");
        for name in [
            "a-Q4_K_M.gguf",
            "a-Q8_0.gguf",
            "o--r--mmproj-f16.gguf",
            "other.gguf",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let download = |weights: &str| Download {
            files: vec![
                piece(None, "model", weights, weights),
                piece(
                    Some("mmproj"),
                    "projector",
                    "o--r--mmproj-f16.gguf",
                    "mmproj-f16.gguf",
                ),
            ],
            bundle: Some("vision".into()),
            source: "huggingface:o/r".into(),
            mode: None,
        };
        write(&dir.join("a-Q4_K_M.gguf"), &download("a-Q4_K_M.gguf")).unwrap();
        write(&dir.join("a-Q8_0.gguf"), &download("a-Q8_0.gguf")).unwrap();
        assert_eq!(
            referenced(&dir),
            BTreeSet::from(["o--r--mmproj-f16.gguf".to_string()])
        );

        // the projector is the other quantization's too: it stays
        let first = remove_model(&dir.join("a-Q4_K_M.gguf"));
        assert_eq!(
            first.removed,
            ["a-Q4_K_M.gguf", "a-Q4_K_M.gguf.bundle.json"]
        );
        assert_eq!(first.kept, ["o--r--mmproj-f16.gguf"]);
        assert!(first.failed.is_empty());
        assert!(dir.join("o--r--mmproj-f16.gguf").exists() && dir.join("a-Q8_0.gguf").exists());
        // the last one takes it
        let last = remove_model(&dir.join("a-Q8_0.gguf"));
        assert_eq!(
            last.removed,
            [
                "a-Q8_0.gguf",
                "a-Q8_0.gguf.bundle.json",
                "o--r--mmproj-f16.gguf"
            ]
        );
        assert!(!dir.join("o--r--mmproj-f16.gguf").exists());
        // a model with no record is just its file; one whose weights are gone says so and touches nothing
        let plain = remove_model(&dir.join("other.gguf"));
        assert_eq!(plain.removed, ["other.gguf"]);
        let missing = remove_model(&dir.join("gone.gguf"));
        assert!(
            missing.removed.is_empty() && missing.weights_error.is_some(),
            "{missing:?}"
        );
    }
}
