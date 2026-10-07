//! Group E of the Phase 3 test plan: the engine documents of `docs/examples/` are held to the typed
//! schema with nothing left over (work packages 3.7 and 3.11): the sections `runtime`, `models`,
//! `traffic`, `api_info` and `diagnostic` and the keys of `launch` and `acquire` are typed, so the
//! Ollama example leaves nothing out and the llama.cpp example only `components`, the adapters of
//! Phase 4.

use std::path::{Path, PathBuf};

use quickai_core::doc::engine::EngineDoc;
use quickai_core::doc::{read_body, Envelope};

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("examples")
        .join("engines")
        .join(name)
}

/// What the typed schema does not know of the example: an error if it does not even load, else the
/// keys it ignores.
fn leftovers(name: &str) -> Result<Vec<String>, String> {
    let path = example(name);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let table: toml::Table = toml::from_str(&text).map_err(|e| e.to_string())?;
    let (_, body) = Envelope::split(table).map_err(|p| format!("{p:?}"))?;
    let typed = read_body::<EngineDoc>(&body).map_err(|e| format!("{}: {}", e.path, e.message))?;
    let mut unknown: Vec<String> = typed.unknown.iter().map(|p| p.to_string()).collect();
    unknown.sort();
    unknown.dedup();
    Ok(unknown)
}

#[test]
fn p3_e17_the_ollama_example_fits_the_typed_engine_schema_with_nothing_ignored() {
    match leftovers("ollama.toml") {
        Ok(unknown) => assert!(unknown.is_empty(), "the typed schema ignores {unknown:#?}"),
        Err(e) => panic!("the example does not load: {e}"),
    }
}

#[test]
fn p3_e18_the_llamacpp_example_is_typed_but_for_the_keys_phase_4_adds_to_its_components() {
    // a component is attached by its `bind.arg` from WP4.9; what is left is the rest of the design of
    // Phase 4: a flag the build must have, the fields of a component (offload, scale), several
    // attachments joined into one argument, and a change while running
    match leftovers("llamacpp.toml") {
        Ok(unknown) => assert_eq!(
            unknown,
            [
                "components.draft.fields",
                "components.draft.requires_flag",
                "components.lora.bind.each",
                "components.lora.bind.sep",
                "components.lora.live",
                "components.lora.requires_flag",
                "components.mmproj.fields",
                "components.mmproj.requires_flag",
            ],
            "`runtime`, `models`, `traffic`, `api_info`, `diagnostic` and the `bind.arg` of a component are \
             interpreted; the keys above are Phase 4"
        ),
        Err(e) => panic!("the example does not load: {e}"),
    }
}
