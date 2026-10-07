//! The worked examples in `docs/examples/` are the design written as documents. They are not loaded
//! by the application (they describe more than this version interprets), but they must stay valid
//! TOML with a valid envelope whose kind matches the directory it sits in, so the documentation
//! cannot quietly rot. The llama.cpp example is also held to the typed engine schema, with the
//! sections this version does not interpret yet reported as unknown keys.

use std::path::{Path, PathBuf};

use quickai_core::doc::engine::EngineDoc;
use quickai_core::doc::{read_body, Envelope, Kind};

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("examples")
}

fn toml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable examples directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            toml_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "toml") {
            out.push(path);
        }
    }
}

fn read(path: &Path) -> toml::Table {
    let text = std::fs::read_to_string(path).expect("readable example");
    toml::from_str(&text).unwrap_or_else(|e| panic!("{} is not valid TOML: {e}", path.display()))
}

#[test]
fn every_example_is_valid_toml_with_a_valid_envelope_in_the_right_directory() {
    let mut files = Vec::new();
    toml_files(&examples_dir(), &mut files);
    assert!(files.len() >= 8, "the examples went missing: {files:?}");
    for path in files {
        let (envelope, _) = Envelope::split(read(&path)).unwrap_or_else(|problems| {
            panic!(
                "{}: {}",
                path.display(),
                problems
                    .iter()
                    .map(|p| format!("{}: {}", p.path, p.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        });
        let dir = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
            .unwrap();
        assert_eq!(
            envelope.kind.dir(),
            dir,
            "{} says kind `{}` but sits in `{dir}`",
            path.display(),
            envelope.kind.name()
        );
    }
}

#[test]
fn p3_e20_the_llamacpp_example_fits_the_typed_engine_schema_and_nothing_asserts_sections_are_ignored(
) {
    // what is left over is p3_schema's to say (E18): only `components`, which wait for Phase 4
    let path = examples_dir().join("engines").join("llamacpp.toml");
    let (envelope, body) = Envelope::split(read(&path)).unwrap();
    assert_eq!(envelope.kind, Kind::Engine);
    let typed = read_body::<EngineDoc>(&body)
        .unwrap_or_else(|e| panic!("{}: {}: {}", path.display(), e.path, e.message));
    assert_eq!(typed.value.engine.name, "llama.cpp");
    assert!(typed.value.fields.len() >= 7);
}
