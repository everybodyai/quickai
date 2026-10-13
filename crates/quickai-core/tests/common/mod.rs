//! What the tests of the look-up share: the documents the application ships, read from where they
//! are (so a test of the real thing breaks when the real thing changes), the answers recorded from the
//! real sites, and the rules a look-up reads what it found with.

#![allow(dead_code)]

use std::path::Path;

use quickai_core::config::{load, Layer, LayerSource, LoadOptions, MemorySource};
use quickai_core::diag::Severity;
use quickai_core::lookup::candidate::Candidate;
use quickai_core::lookup::huggingface::{self, Hit};
use quickai_core::lookup::present::OwnedRules;
use quickai_core::models::{bundles, lookups};
use quickai_core::registry::{KnownPrimitives, Registry};

/// Every primitive the shipped documents ask for.
pub const PROVIDED: &[&str] = &[
    "spawn@1",
    "probe.http@1",
    "locate.setting@1",
    "locate.next_to_exe@1",
    "locate.dev_tree@1",
    "locate.installed@1",
    "locate.path_search@1",
    "locate.known_paths@1",
    "exec.probe@1",
    "feed.github_release@1",
    "feed.pypi@1",
    "install.archive@1",
    "install.python_app@1",
    "attach@1",
    "http_call@1",
    "decode@1",
    "lookup.ollama_library@1",
    "lookup.huggingface_gguf@1",
    "prerequisite@1",
];

/// A file of the application's shipped documents (`crates/quickai-runtime/manifests/<rel>`).
pub fn manifest(rel: &str) -> String {
    read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../quickai-runtime/manifests")
            .join(rel),
    )
}

/// The catalog documents the application ships, `(file name, text)` in the order of the names.
pub fn shipped_catalogs() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../quickai-runtime/manifests/catalog");
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let text = read_file(&dir.join(&n));
            (n, text)
        })
        .collect()
}

/// A recorded answer of Hugging Face (`crates/quickai-runtime/tests/fixtures/huggingface/<rel>`).
pub fn fixture(rel: &str) -> String {
    read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../quickai-runtime/tests/fixtures/huggingface")
            .join(rel),
    )
}

fn read(path: &Path) -> String {
    read_file(path)
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The registry the shipped documents make, with the person's own documents above them.
pub fn registry(user: &[(&str, &str)]) -> Registry {
    registry_with(PROVIDED, user)
}

pub fn registry_with(provided: &[&str], user: &[(&str, &str)]) -> Registry {
    build_registry(provided, user, false)
}

/// The registry of everything the application ships, its recommended list included, with the
/// person's own documents above it.
pub fn shipped_registry(user: &[(&str, &str)]) -> Registry {
    build_registry(PROVIDED, user, true)
}

fn build_registry(provided: &[&str], user: &[(&str, &str)], with_catalogs: bool) -> Registry {
    Shape {
        provided,
        user,
        with_catalogs,
        ..Shape::default()
    }
    .build()
}

/// What a test of the configurations varies in the registry the shipped documents make: the
/// version of the application, a package's documents, the person's, and safe mode.
#[derive(Default)]
pub struct Shape<'a> {
    pub provided: &'a [&'a str],
    pub user: &'a [(&'a str, &'a str)],
    pub bundle: &'a [(&'a str, &'a str)],
    pub with_catalogs: bool,
    pub app_version: Option<&'a str>,
    pub safe_mode: bool,
}

impl Shape<'_> {
    pub fn build(&self) -> Registry {
        let llama = manifest("engines/llamacpp.toml");
        let ollama = manifest("engines/ollama.toml");
        let comfyui = manifest("engines/comfyui.toml");
        let openwebui = manifest("engines/openwebui.toml");
        let vocab = manifest("vocab/core.toml");
        let default_profile = manifest("profiles/default.toml");
        let catalogs = if self.with_catalogs {
            shipped_catalogs()
        } else {
            Vec::new()
        };
        let mut docs: Vec<(String, &str)> = vec![
            ("embedded:engines/llamacpp.toml".to_string(), &llama),
            ("embedded:engines/ollama.toml".to_string(), &ollama),
            ("embedded:engines/comfyui.toml".to_string(), &comfyui),
            ("embedded:engines/openwebui.toml".to_string(), &openwebui),
            ("embedded:vocab/core.toml".to_string(), &vocab),
            (
                "embedded:profiles/default.toml".to_string(),
                &default_profile,
            ),
        ];
        docs.extend(
            catalogs
                .iter()
                .map(|(n, t)| (format!("embedded:catalog/{n}"), t.as_str())),
        );
        let docs: Vec<(&str, &str)> = docs.iter().map(|(n, t)| (n.as_str(), *t)).collect();
        let embedded = MemorySource::new(&docs);
        let package = MemorySource::new(self.bundle);
        let person = MemorySource::new(self.user);
        let mut layers: Vec<(Layer, &dyn LayerSource)> = vec![(Layer::Embedded, &embedded)];
        if !self.bundle.is_empty() {
            layers.push((Layer::Bundle, &package));
        }
        if !self.user.is_empty() {
            layers.push((Layer::User, &person));
        }
        let provided = if self.provided.is_empty() {
            PROVIDED
        } else {
            self.provided
        };
        Registry::build(
            load(
                &layers,
                &LoadOptions {
                    safe_mode: self.safe_mode,
                    app_version: self.app_version.map(str::to_string),
                },
            ),
            &KnownPrimitives::new(provided),
        )
    }
}

/// A registry of documents only (no shipped ones), for the tests of a document that is wrong.
pub fn registry_of(docs: &[(&str, &str)]) -> Registry {
    let source = MemorySource::new(docs);
    Registry::build(
        load(
            &[(Layer::Embedded, &source as &dyn LayerSource)],
            &LoadOptions::default(),
        ),
        &KnownPrimitives::new(PROVIDED),
    )
}

pub fn texts(registry: &Registry, severity: Severity) -> Vec<String> {
    registry
        .diagnostics
        .iter()
        .filter(|d| d.severity == severity)
        .map(|d| d.to_string())
        .collect()
}

/// What a look-up reads with: the types and sizes of the vocabulary, the engine's bundles and the
/// settings of one of its look-ups.
pub type Owned = OwnedRules;

/// The rules of the look-up `lookup` of the engine `engine` in the registry.
pub fn owned(registry: &Registry, engine: &str, lookup: &str) -> Owned {
    let engine = registry
        .engine(engine)
        .unwrap_or_else(|| panic!("no engine `{engine}`: {:#?}", registry.diagnostics));
    let found = lookups(engine)
        .iter()
        .find(|l| l.id == lookup)
        .unwrap_or_else(|| panic!("no look-up `{lookup}`"));
    OwnedRules {
        types: registry.vocabulary().model_types().to_vec(),
        sizes: registry.vocabulary().size_classes().to_vec(),
        bundles: bundles(engine).to_vec(),
        settings: found.kind.read().clone(),
        places: quickai_core::models::places(engine),
    }
}

/// The candidate a recorded repository makes: the hit of the recorded popular list (or a plain one
/// when the repository is not on it) and the recorded files.
pub fn hub_candidate(id: &str) -> Candidate {
    let hits = huggingface::parse_search(&fixture("popular.json")).expect("the recorded list");
    let hit = hits.into_iter().find(|h| h.id == id).unwrap_or(Hit {
        id: id.to_string(),
        downloads: 0,
        likes: 0,
        pipeline: None,
        tags: Vec::new(),
    });
    let repo = huggingface::parse_repo(&fixture(&format!("repos/{}.json", id.replace('/', "__"))))
        .unwrap_or_else(|e| panic!("{id}: {e}"));
    huggingface::candidate("https://huggingface.co", &hit, &repo)
}

/// A small engine document of its own (`acme`) whose `[models]` and `[components]` are `extra`: for the
/// tests of what is wrong in them, where the shipped documents' own sections would merge with it.
pub fn acme(extra: &str) -> String {
    format!(
        r#"schema = 1
kind = "engine"
id = "acme"
requires = {{ primitives = ["spawn@1", "probe.http@1", "locate.setting@1", "lookup.huggingface_gguf@1"] }}

[engine]
name = "Acme"

[provides]
apis = ["acme.talk"]
default_port = 9000

[locate]
strategies = [ {{ use = "setting", field = "bin_override" }} ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${{runtime.binary}}"
args = ["serve"]

[launch.ready]
probe = {{ use = "http", path = "/health", expect = 200 }}

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"

[components.mmproj]
bind.arg = ["--mmproj", "${{component.path}}"]

{extra}
"#
    )
}
