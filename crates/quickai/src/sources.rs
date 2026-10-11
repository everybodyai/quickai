//! Where the files of the models folders were fetched from, so that a configuration can say where a model
//! can be got when it is given to someone else (docs/configurations.md, section 5).
//!
//! A file fetched from a look-up has no entry of the recommended list to name it by, and the record beside
//! the weights (`bundles`) exists only for the files that go together or are started in a mode. The
//! application writes one small file in its own data directory instead, `model-sources.json`: the name of
//! a file, its size, and the kind of place and the name in it (`huggingface:owner/repo`) it was fetched
//! from. Nothing is written into a models folder for it, so what is there is what it always was. It is a
//! memory and no more: a name that the size no longer agrees with is not believed, and what is read from
//! it is held to the shape a place may have.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

const FILE: &str = "model-sources.json";
/// The most files it remembers: it is a memory and not an archive.
const MOST: usize = 4000;

#[derive(Serialize, Deserialize, Default)]
struct Index {
    version: u32,
    files: BTreeMap<String, Source>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Source {
    size: u64,
    source: String,
}

fn read(data_dir: &Path) -> Index {
    std::fs::read_to_string(data_dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Index>(&text).ok())
        .filter(|index| index.version == 1)
        .unwrap_or_default()
}

/// Remembers that `file`, of this size, was fetched from `source`. Whole or not at all.
pub fn remember(data_dir: &Path, file: &str, size: u64, source: &str) -> std::io::Result<()> {
    if !quickai_core::profile::valid_from(source) || !quickai_core::profile::valid_file_name(file) {
        return Ok(());
    }
    let mut index = read(data_dir);
    index.version = 1;
    if index.files.len() >= MOST && !index.files.contains_key(file) {
        return Ok(());
    }
    index.files.insert(
        file.to_string(),
        Source {
            size,
            source: source.to_string(),
        },
    );
    std::fs::create_dir_all(data_dir)?;
    let path = data_dir.join(FILE);
    let temporary = path.with_extension("json.part");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&index).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(&temporary, &path)
}

/// Where `file`, which is this big now, was fetched from, when it is remembered and the file is the one
/// that was fetched.
pub fn recall(data_dir: &Path, file: &str, size: u64) -> Option<String> {
    let found = read(data_dir).files.get(file)?.clone();
    (found.size == size && quickai_core::profile::valid_from(&found.source)).then_some(found.source)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testkit::scratch;

    #[test]
    fn a_file_remembers_where_it_came_from_while_it_is_the_same_file() {
        let dir = scratch("sources-same");
        assert_eq!(recall(&dir, "m.gguf", 10), None);
        remember(&dir, "m.gguf", 10, "huggingface:acme/Tiny-GGUF").unwrap();
        assert_eq!(
            recall(&dir, "m.gguf", 10).as_deref(),
            Some("huggingface:acme/Tiny-GGUF")
        );
        // another file of the same name is not believed to be the one that was fetched
        assert_eq!(recall(&dir, "m.gguf", 11), None);
        assert_eq!(recall(&dir, "other.gguf", 10), None);
        // fetched again from somewhere else: the later place stands
        remember(&dir, "m.gguf", 12, "huggingface:me/Mine").unwrap();
        assert_eq!(
            recall(&dir, "m.gguf", 12).as_deref(),
            Some("huggingface:me/Mine")
        );
        // it is one file of the data directory, written whole
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["model-sources.json"]);
    }

    #[test]
    fn what_is_not_a_place_or_a_name_is_not_remembered_and_what_is_found_is_not_believed() {
        let dir = scratch("sources-shape");
        for (file, source) in [
            ("m.gguf", "https://evil.example/m.gguf"),
            ("m.gguf", "huggingface:../x"),
            ("../m.gguf", "huggingface:a/b"),
            ("a/b.gguf", "huggingface:a/b"),
        ] {
            remember(&dir, file, 1, source).unwrap();
        }
        assert!(
            !dir.join("model-sources.json").exists(),
            "nothing was written"
        );
        // a file that someone wrote by hand with a place that is an address
        std::fs::write(
            dir.join("model-sources.json"),
            r#"{"version":1,"files":{"m.gguf":{"size":1,"source":"https://evil.example/x"}}}"#,
        )
        .unwrap();
        assert_eq!(recall(&dir, "m.gguf", 1), None);
        // one that is not ours, or damaged
        std::fs::write(dir.join("model-sources.json"), "{").unwrap();
        assert_eq!(recall(&dir, "m.gguf", 1), None);
        std::fs::write(
            dir.join("model-sources.json"),
            r#"{"version":2,"files":{}}"#,
        )
        .unwrap();
        assert_eq!(recall(&dir, "m.gguf", 1), None);
    }
}
