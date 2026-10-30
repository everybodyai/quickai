//! The check of a configuration shared on quickai.everybodyai.dev (docs/reports-and-community.md,
//! section 11.6), and what the site shows of it.
//!
//! The community's Worker runs this as WebAssembly, so that a file is read by the rules the app
//! reads it by, over the documents the app ships: what the app would refuse or drop on import is
//! refused here, with the reason, and the identity that keeps one copy of each configuration
//! (`profile::canonical`) is the one the app computes. Then, for a file that is shared with
//! everyone: no word or line that looks like a secret, no path of a computer, no line of
//! environment a file may not set, and models only from public places (the recommended list,
//! Ollama's library, Hugging Face).
//!
//! `check` is the whole of it; `check_json` is the same through the WebAssembly boundary.

use std::sync::OnceLock;

use include_dir::{Dir, DirEntry};
use quickai_core::config::{load, Layer, LayerSource, Listing, LoadOptions, SourceDoc};
use quickai_core::doc::engine::Share;
use quickai_core::profile::canonical::{identity, CANON};
use quickai_core::profile::check::open_text;
use quickai_core::profile::state::field_of;
use quickai_core::profile::{label_of, screen, shown, Configuration, ModelSource};
use quickai_core::registry::{KnownPrimitives, Registry};
use quickai_core::value::Value;
use serde_json::{json, Value as Json};

static MANIFESTS: Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../quickai-runtime/manifests");

/// The version of QuickAI whose documents this is: a file that asks for a later one is refused.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Embedded;

impl LayerSource for Embedded {
    fn read(&self) -> Listing {
        fn walk(dir: &Dir<'static>, out: &mut Vec<SourceDoc>) {
            for entry in dir.entries() {
                match entry {
                    DirEntry::Dir(d) => walk(d, out),
                    DirEntry::File(f) if f.path().extension().is_some_and(|e| e == "toml") => {
                        out.push(SourceDoc {
                            name: format!("embedded:{}", f.path().display()),
                            text: f.contents_utf8().unwrap_or_default().to_string(),
                        });
                    }
                    DirEntry::File(_) => {}
                }
            }
        }
        let mut docs = Vec::new();
        walk(&MANIFESTS, &mut docs);
        docs.sort_by(|a, b| a.name.cmp(&b.name));
        Listing {
            docs,
            problems: Vec::new(),
        }
    }
}

/// The registry of the documents the app ships. Nothing here runs a program, so every primitive a
/// document asks for is taken as provided.
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let loaded = load(
            &[(Layer::Embedded, &Embedded as &dyn LayerSource)],
            &LoadOptions {
                safe_mode: true,
                app_version: Some(APP_VERSION.to_string()),
            },
        );
        let named: Vec<String> = loaded
            .docs
            .values()
            .flat_map(|d| d.envelope.requires_primitives.iter().map(|p| p.to_string()))
            .collect();
        let named: Vec<&str> = named.iter().map(String::as_str).collect();
        Registry::build(loaded, &KnownPrimitives::new(&named))
    })
}

/// Checks the text of a `.toml` shared with the community. The answer is JSON: `{ "ok": false,
/// "problems": [...] }` with a sentence for each reason it is refused, or `{ "ok": true, ... }` with
/// its identity and what the site shows of it.
pub fn check(text: &str) -> Json {
    let registry = registry();
    let vocabulary = registry.vocabulary();
    let opened = match open_text(registry, text, APP_VERSION) {
        Ok(opened) => opened,
        Err(note) => return refused(vec![note.text(vocabulary)]),
    };
    let configuration = &opened.configuration;
    // what the app would drop on import is a reason here: a shared file holds only what runs
    let mut problems: Vec<String> = opened.notes.iter().map(|n| n.text(vocabulary)).collect();
    let Some(engine) = registry
        .engine(&configuration.engine)
        .filter(|e| e.doc.engine.companion_of.is_none())
    else {
        problems.push(format!(
            "QuickAI has no engine called `{}`.",
            configuration.engine
        ));
        return refused(problems);
    };

    let mut settings = Vec::new();
    let (mut needs_review, mut custom_args, mut custom_env) = (false, false, false);
    let deny = vocabulary.deny_env();
    for (key, value) in &configuration.values {
        let Some((owner, field)) = field_of(registry, key) else {
            continue;
        };
        let label = label_of(field);
        if let Value::Str(text) = value {
            if field.bind.arg_split {
                custom_args |= !text.trim().is_empty();
                if screen::drop_secret_args(text).1 > 0 {
                    problems.push(format!("{label}: a word looks like a password or a token. A shared file holds none."));
                }
            }
            if let Some(lines) = &field.bind.env_lines {
                custom_env |= !text.trim().is_empty();
                if screen::drop_secret_lines(text).1 > 0 {
                    problems.push(format!("{label}: a line looks like a password or a token. A shared file holds none."));
                }
                for name in screen::env_lines(owner, &lines.protect, text, deny).refused {
                    problems.push(format!("{label}: a file may not set {name}."));
                }
                let parsed = quickai_core::kvlines::parse(text, &[], false);
                if !parsed.problems.is_empty() {
                    problems.push(format!("{label}: {}", parsed.problems_text()));
                }
            }
            if let Some(path) = screen::names_a_path(text) {
                problems.push(format!(
                    "{label} names a folder or a file of a computer ({path}). A shared file names none."
                ));
            }
        }
        let review = field.share() == Some(Share::Review);
        needs_review |= review;
        settings.push(json!({
            "key": key,
            "engine": owner.id,
            "label": label,
            "value": shown(value),
            "review": review,
            "text": matches!(value, Value::Str(_)) && review,
        }));
    }

    let mut models = Vec::new();
    for pick in &configuration.models {
        let mut label = pick.display().to_string();
        let (source, link) = match &pick.source {
            ModelSource::Catalog { id, .. } => match registry.catalog().model(id) {
                Some(model) => {
                    if pick.name.is_none() {
                        label = model.name.clone();
                    }
                    ("QuickAI's recommended list".to_string(), None)
                }
                None => {
                    problems.push(format!(
                        "The model `{id}` is not on QuickAI's recommended list."
                    ));
                    continue;
                }
            },
            ModelSource::Named { kind, name } if kind == "ollama" => {
                let library = name.split(':').next().unwrap_or(name).to_ascii_lowercase();
                (
                    "Ollama's library".to_string(),
                    Some(format!("https://ollama.com/library/{library}")),
                )
            }
            ModelSource::Named { kind, .. } => (kind.clone(), None),
            ModelSource::File { name } => match pick.from.as_deref() {
                Some(from) if from.starts_with("huggingface:") => {
                    let repo = &from["huggingface:".len()..];
                    (
                        "Hugging Face".to_string(),
                        Some(format!("https://huggingface.co/{repo}")),
                    )
                }
                Some(_) | None if pick.url.is_some() => {
                    problems.push(format!(
                        "The model {name} is fetched from an address. A shared model comes from QuickAI's recommended list, Ollama's library or Hugging Face."
                    ));
                    continue;
                }
                _ => {
                    problems.push(format!(
                        "Where the model {name} comes from is not known. A shared model comes from QuickAI's recommended list, Ollama's library or Hugging Face."
                    ));
                    continue;
                }
            },
        };
        models.push(json!({
            "role": pick.role,
            "label": label,
            "source": source,
            "link": link,
            "size": pick.size,
        }));
    }
    if configuration.models.is_empty() {
        problems.push(
            "It names no model. A shared configuration says which model it runs.".to_string(),
        );
    }
    if !problems.is_empty() {
        return refused(problems);
    }

    let id = identity(registry, configuration);
    json!({
        "ok": true,
        "canon": CANON,
        "identity": id.identity,
        "family": id.family,
        "id": configuration.id,
        "name": configuration.name,
        "note": configuration.meta.note,
        "made_with": configuration.meta.made_with,
        "made_on": made_on(configuration),
        "community": configuration.meta.community.as_ref().map(|c| json!({ "id": c.id, "version": c.version })),
        "engine": configuration.engine,
        "engine_name": engine.doc.engine.name,
        "build": configuration.runtime.build,
        "companions": configuration
            .runtime
            .companions
            .iter()
            .map(|(id, runtime)| json!({
                "id": id,
                "name": registry.engine(id).map_or(id.as_str(), |e| e.doc.engine.name.as_str()),
                "build": runtime.build,
            }))
            .collect::<Vec<_>>(),
        "models": models,
        "settings": settings,
        "needs": configuration.needs.iter().map(|n| n.label()).collect::<Vec<_>>(),
        "needs_review": needs_review,
        "custom_args": custom_args,
        "custom_env": custom_env,
    })
}

fn refused(mut problems: Vec<String>) -> Json {
    problems.dedup();
    json!({ "ok": false, "problems": problems })
}

fn made_on(configuration: &Configuration) -> Json {
    match &configuration.meta.made_on {
        Some(on) => json!({
            "os": on.os,
            "arch": on.arch,
            "gpu": on.gpu,
            "memory_gb": on.memory_gb,
        }),
        None => Json::Null,
    }
}

// ---- the WebAssembly boundary ----------------------------------------------------------------------
//
// The Worker writes the file's bytes into memory it got from `block`, calls `check_json`, reads the
// answer at the address and length it returns (address in the high 32 bits, length in the low), and
// gives both back with `release`.

fn layout(len: usize) -> std::alloc::Layout {
    std::alloc::Layout::array::<u8>(len.max(1)).expect("a block of bytes fits the address space")
}

/// A block of `len` bytes for the caller to write into.
#[no_mangle]
pub extern "C" fn block(len: usize) -> *mut u8 {
    // SAFETY: the layout is never of size zero
    unsafe { std::alloc::alloc(layout(len)) }
}

/// Gives back a block of `block` or an answer of `check_json`.
///
/// # Safety
/// `at` and `len` are those of one block this module gave and that was not given back yet.
#[no_mangle]
pub unsafe extern "C" fn release(at: *mut u8, len: usize) {
    std::alloc::dealloc(at, layout(len));
}

/// `check` of the UTF-8 text at `at`, as JSON in a block of its own.
///
/// # Safety
/// `at` and `len` are a block of `block` that holds the text.
#[no_mangle]
pub unsafe extern "C" fn check_json(at: *const u8, len: usize) -> u64 {
    let bytes = std::slice::from_raw_parts(at, len);
    let answer = match std::str::from_utf8(bytes) {
        Ok(text) => check(text),
        Err(_) => refused(vec!["It is not text in UTF-8.".to_string()]),
    }
    .to_string();
    let out = block(answer.len());
    std::ptr::copy_nonoverlapping(answer.as_ptr(), out, answer.len());
    ((out as u64) << 32) | answer.len() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
schema = 1
kind = "profile"
id = "fast-chat"
name = "Fast chat"
engine = "llamacpp"

[[model]]
catalog = "bonsai-2-4b"

[values.llamacpp]
ctx_size = 8192
extra_args = "-t 8 --temp=0.7"
"#;

    #[test]
    fn a_good_file_has_an_identity() {
        let answer = check(GOOD);
        assert_eq!(answer["ok"], true, "{answer}");
        assert_eq!(answer["identity"].as_str().map(str::len), Some(64));
        assert_eq!(answer["custom_args"], true);
    }

    #[test]
    fn the_port_and_the_spelling_of_the_arguments_do_not_change_the_identity() {
        let other = GOOD.replace(
            "extra_args = \"-t 8 --temp=0.7\"",
            "extra_args = \"--temp 0.7 --threads 8\"\nport = 9000",
        );
        assert_eq!(check(GOOD)["identity"], check(&other)["identity"]);
    }

    #[test]
    fn a_secret_is_refused() {
        let bad = GOOD.replace("-t 8", "--hf-token hf_abcdefghijklmnopqrstuvwxyz0123");
        let answer = check(&bad);
        assert_eq!(answer["ok"], false, "{answer}");
    }

    #[test]
    fn a_model_of_unknown_source_is_refused() {
        let bad = GOOD.replace("catalog = \"bonsai-2-4b\"", "file = \"mine.gguf\"");
        assert_eq!(check(&bad)["ok"], false);
    }
}
