//! The reference pages for engine and vocabulary documents are generated from the types, and these
//! tests keep them honest (docs/implementation-plan.md, WP2.7 and WP3.6): each checked-in page is
//! the generated one, every table lists exactly the fields of the type behind it, every field type
//! is listed, and the keys of the shapes that are enum variants (or that a table of its own does not
//! cover) are ones the typed schema accepts.

use std::path::{Path, PathBuf};

use quickai_core::doc::catalog::CatalogDoc;
use quickai_core::doc::engine::EngineDoc;
use quickai_core::doc::read_body;
use quickai_core::doc::reference::{
    render, render_catalog, render_vocab, type_name, CATALOG_GROUPS, GROUPS, TYPES, VOCAB_GROUPS,
};
use quickai_core::doc::vocab::VocabDoc;

const ENGINE_RS: &str = include_str!("../src/doc/engine.rs");
const VOCAB_RS: &str = include_str!("../src/doc/vocab.rs");
const CATALOG_RS: &str = include_str!("../src/doc/catalog.rs");

fn page_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("reference")
        .join(name)
}

#[test]
fn the_checked_in_page_is_the_generated_one() {
    check_golden("engine-fields.md", render());
}

#[test]
fn the_checked_in_vocabulary_page_is_the_generated_one() {
    check_golden("vocab-fields.md", render_vocab());
}

#[test]
fn the_checked_in_catalog_page_is_the_generated_one() {
    check_golden("catalog-fields.md", render_catalog());
}

fn check_golden(name: &str, generated: String) {
    let path = page_path(name);
    if std::env::var_os("QUICKAI_UPDATE_GOLDEN").is_some_and(|v| !v.is_empty() && v != "0") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| {
            panic!(
                "{} is unreadable ({e}); generate it with QUICKAI_UPDATE_GOLDEN=1",
                path.display()
            )
        })
        .replace("\r\n", "\n");
    assert!(
        on_disk == generated,
        "{} is out of date; run `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` and read the diff",
        path.display()
    );
}

/// The keys of `struct <name>` in `source` as a document spells them (a `rename = "..."` in a
/// `#[serde(...)]` wins), read from the source of the types.
fn struct_keys(source: &str, name: &str) -> Vec<String> {
    let start = source
        .lines()
        .position(|l| l.starts_with(&format!("pub struct {name} {{")))
        .unwrap_or_else(|| panic!("no `pub struct {name}` in the types"));
    let mut keys = Vec::new();
    let mut rename: Option<String> = None;
    let mut flattened = false;
    for line in source.lines().skip(start + 1) {
        if line == "}" {
            break;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("#[serde(") && trimmed.contains("flatten") {
            // the keys of a flattened field are another type's: the field is no key of its own
            flattened = true;
        } else if trimmed.starts_with("#[serde(") && trimmed.contains("rename = \"") {
            rename = trimmed
                .split("rename = \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(str::to_string);
        } else if let Some(rest) = trimmed.strip_prefix("pub ") {
            let field = rest.split(':').next().unwrap().trim().to_string();
            let key = rename.take().unwrap_or(field);
            if !std::mem::take(&mut flattened) {
                keys.push(key);
            }
        }
    }
    keys
}

#[test]
fn p3_e19_every_table_lists_exactly_the_fields_of_its_type_in_both_directions() {
    for (types, groups) in [
        (ENGINE_RS, GROUPS),
        (VOCAB_RS, VOCAB_GROUPS),
        (CATALOG_RS, CATALOG_GROUPS),
    ] {
        for group in groups {
            let Some(source) = group.source else { continue };
            let mut documented: Vec<String> = group
                .attributes
                .iter()
                .map(|a| a.name.to_string())
                .collect();
            let mut actual = struct_keys(types, source);
            documented.sort();
            actual.sort();
            assert_eq!(
                documented, actual,
                "the table `{}` and `struct {source}` disagree: add or remove a row in doc/reference.rs",
                group.heading
            );
        }
    }
}

#[test]
fn the_vocabulary_page_has_a_table_for_every_type_of_the_document() {
    let types: Vec<&str> = VOCAB_RS
        .lines()
        .filter_map(|l| l.strip_prefix("pub struct "))
        .filter_map(|rest| rest.split(' ').next())
        .collect();
    let described: Vec<&str> = VOCAB_GROUPS.iter().filter_map(|g| g.source).collect();
    for ty in &types {
        // the document itself is its two tables
        if *ty == "VocabDoc" {
            continue;
        }
        assert!(
            described.contains(ty),
            "`struct {ty}` of doc/vocab.rs has no table in VOCAB_GROUPS"
        );
    }
    for group in VOCAB_GROUPS {
        assert!(group.source.is_some(), "{}", group.heading);
    }
}

/// A vocabulary document that says every key there is.
const EVERY_VOCAB_KEY: &str = r#"
[[modality]]
id = "image"
label = "Images"

[[api]]
id = "acme.chat"
paths = ["/chat"]
capabilities = ["chat"]

[api.decode]
framing = ["sse", "ndjson", "json"]
delta = ["/d"]
final = ["/f"]
error = ["/e"]

[api.decode.chat]
messages = "/messages"
role = "/role"
content = "/content"
skip_roles = ["system"]
images = "/images"

[api.decode.chat.parts]
type = "/type"
text = "/text"

[[api.examples]]
label = "Chat with an image"
when = "has(model.modalities.input, 'image')"
curl = "curl http://localhost/chat"

[[api]]
id = "acme.complete"
paths = ["/complete"]

[api.decode]
framing = ["json"]
prompt = "/prompt"

[[model_type]]
id = "vision"
label = "Vision"
help = "Reads pictures as well as text."
order = 30
default = false

[model_type.match]
capabilities = ["vision"]
pipelines = ["image-text-to-text"]
tags = ["multimodal"]
words = ["vl"]
companions = ["mmproj"]

[[model_type]]
id = "chat"
default = true

[[size_class]]
id = "small"
label = "Small (< 4 GB)"
help = "Runs on most machines."
below = "4GiB"

[[size_class]]
id = "large"
"#;

#[test]
fn every_key_the_vocabulary_page_lists_is_one_the_schema_accepts() {
    let table: toml::Table = toml::from_str(EVERY_VOCAB_KEY).unwrap();
    let typed =
        read_body::<VocabDoc>(&table).unwrap_or_else(|e| panic!("{}: {}", e.path, e.message));
    assert!(
        typed.unknown.is_empty(),
        "keys the schema does not know: {:?}",
        typed.unknown
    );
    for group in VOCAB_GROUPS {
        for attribute in group.attributes {
            // a key is said as `key = ...` or, for a table, as a header ending in `.key]`
            let (as_value, as_table) = (
                format!("{} =", attribute.name),
                format!(".{}]", attribute.name),
            );
            assert!(
                EVERY_VOCAB_KEY.contains(&as_value) || EVERY_VOCAB_KEY.contains(&as_table),
                "`{}` of `{}` is not exercised by the document above",
                attribute.name,
                group.heading
            );
        }
    }
    // and what a key that is not on the page does
    let table: toml::Table = toml::from_str("[[api]]\nid = \"x\"\npahts = []\n").unwrap();
    let typed = read_body::<VocabDoc>(&table).unwrap();
    assert_eq!(typed.unknown.len(), 1, "{:?}", typed.unknown);
}

/// A catalog document that says every key there is.
const EVERY_CATALOG_KEY: &str = r#"
title = "Chat"
verified = "2026-10-07"

[[model]]
id = "alpha-4b"
name = "Alpha 4B"
by = "Acme"
type = "chat"
note = "A small chat model."
license = "Apache-2.0"
params = "4B"
context = 131072
input = ["text", "image"]
features = ["thinking", "tools"]
start = true
bundled = false
hidden = false

[model.run.llamacpp]
repo = "acme/Alpha-4B-GGUF"
rev = "0123456789abcdef0123456789abcdef01234567"
mode = "embedding"
place = "checkpoints"
files = [ { file = "Alpha-4B-Q4_K_M.gguf", size = 2500000000, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", label = "Q4_K_M", best = true } ]

[model.run.llamacpp.companions]
mmproj = { file = "mmproj-Alpha-4B-F16.gguf", size = 800000000, sha256 = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" }

[model.run.ollama]
ref = "alpha:4b"
size = 3300000000
"#;

#[test]
fn every_key_the_catalog_page_lists_is_one_the_schema_accepts() {
    let table: toml::Table = toml::from_str(EVERY_CATALOG_KEY).unwrap();
    let typed =
        read_body::<CatalogDoc>(&table).unwrap_or_else(|e| panic!("{}: {}", e.path, e.message));
    assert!(
        typed.unknown.is_empty(),
        "keys the schema does not know: {:?}",
        typed.unknown
    );
    for group in CATALOG_GROUPS {
        for attribute in group.attributes {
            // a key is said as `key = ...`, as a header ending in `.key]`, as the header `[[key]]` of a list, or inside a header (`[model.run.x]`)
            let (as_value, as_table, as_list, as_inner) = (
                format!("{} =", attribute.name),
                format!(".{}]", attribute.name),
                format!("[[{}]]", attribute.name),
                format!(".{}.", attribute.name),
            );
            assert!(
                EVERY_CATALOG_KEY.contains(&as_value)
                    || EVERY_CATALOG_KEY.contains(&as_table)
                    || EVERY_CATALOG_KEY.contains(&as_list)
                    || EVERY_CATALOG_KEY.contains(&as_inner),
                "`{}` of `{}` is not exercised by the document above",
                attribute.name,
                group.heading
            );
        }
    }
    // and a key that is not on the page is reported, not read
    let table: toml::Table = toml::from_str("[[model]]\nid = \"x\"\nfeatures_ = []\n").unwrap();
    let typed = read_body::<CatalogDoc>(&table).unwrap();
    assert_eq!(typed.unknown.len(), 1, "{:?}", typed.unknown);
}

#[test]
fn the_catalog_page_has_a_table_for_every_type_of_the_document() {
    let types: Vec<&str> = CATALOG_RS
        .lines()
        .filter_map(|l| l.strip_prefix("pub struct "))
        .filter_map(|rest| rest.split(' ').next())
        .collect();
    let described: Vec<&str> = CATALOG_GROUPS.iter().filter_map(|g| g.source).collect();
    for ty in &types {
        assert!(
            described.contains(ty),
            "`struct {ty}` of doc/catalog.rs has no table in CATALOG_GROUPS"
        );
    }
}

fn snake(camel: &str) -> String {
    let mut out = String::new();
    for (i, c) in camel.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

#[test]
fn every_field_type_is_listed_once() {
    let start = ENGINE_RS
        .lines()
        .position(|l| l.starts_with("pub enum FieldType {"))
        .expect("pub enum FieldType");
    let mut actual: Vec<String> = ENGINE_RS
        .lines()
        .skip(start + 1)
        .take_while(|l| *l != "}")
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .map(|l| snake(l.trim_end_matches(',')))
        .collect();
    let mut listed: Vec<String> = TYPES.iter().map(|(ty, _, _)| type_name(*ty)).collect();
    actual.sort();
    listed.sort();
    assert_eq!(listed, actual, "FieldType and the TYPES table disagree");
    let mut unique = listed.clone();
    unique.dedup();
    assert_eq!(unique.len(), listed.len(), "a type is listed twice");
}

#[test]
fn the_keys_of_the_variant_shapes_are_ones_the_schema_accepts() {
    let keywords = r#"
[engine]
name = "x"
[locate]
strategies = [ { use = "setting", field = "f" }, { use = "next_to_exe", file = "f" }, { use = "dev_tree", path = "p" }, { use = "installed", root = "${data}/x", pick = "newest", binary = { linux = "bin/x", windows = "x.exe" } }, { use = "path_search", name = "x" }, { use = "known_paths", linux = ["/usr/bin/x"], macos = ["/opt/x"], windows = ["%LOCALAPPDATA%\\x.exe"] } ]
[runtime]
version = { exec = ["${binary}", "--version"], regex = 'version is (\S+)', as = "version" }
[acquire]
root = "${data}/x"
filter_hint = "h"
feed = { use = "github_release", repo = "a/b", per_page = 10, show = 5, prereleases = true, api = "https://api.example.com", digest = true, archive = "https://mirror.example.com" }
assets = { by = "keywords", ext = [".zip"], exclude = ["x"], os = { linux = ["l"] }, arch = { x86_64 = ["x"] }, label = "${tag}" }
install = { use = "archive", binary = { name = "x", search_depth = 4 }, chmod = true, verify = "digest", smoke = ["${binary}", "--version"], skip = [ { when = "gpu.known", glob = "lib/cuda_*" } ] }
backends = [ { match = "a", label = "A", needs_gpu = "amd" } ]
"#;
    let names = r#"
[engine]
name = "x"
[acquire]
assets = { by = "names", rules = [ { when = "os == 'linux'", name = "n" } ], label = "${tag}" }
"#;
    let python = r#"
[engine]
name = "x"
[acquire]
root = "${data}/x"
feed = { use = "github_release", repo = "a/b" }
assets = { by = "source", label = "${tag}" }
install = { use = "python_app", binary = { name = "x" }, entry = "main.py", python = { versions = ["3.13", "3.12"] }, verify = "digest", smoke = ["${binary}", "--quick-test-for-ci"], smoke_timeout = "300s", space = "30GiB", pip = [ { id = "torch", label = "PyTorch", when = "has(gpu.vendors, 'amd')", packages = ["torch"], requirements = "requirements.txt", index = "https://download.example.com/whl/rocm", weight = 10 } ], nodes = [ { id = "gguf", label = "GGUF", when = "true", url = "https://example.com/a.tar.gz", sha256 = "sha256:0000000000000000000000000000000000000000000000000000000000000000", into = "custom_nodes/gguf", requirements = "requirements.txt", license = "MIT" } ] }
"#;
    let lookups = r#"
[engine]
name = "x"
[models]
lookup = [
  { id = "library", use = "ollama_library", site = "https://ollama.example", bytes_per_parameter = 0.6, show_variants = 16, types = ["chat"] },
  { id = "hub", use = "huggingface_gguf", site = "https://hub.example", popular = 30, keyword = 20, similar = 20, pipelines = ["text-to-video"], words = ["encoder"], prefer = ["Q4_K_M"], skip = ["*-of-*"], show_variants = 4, types = ["chat", "vision"], alone = ["mmproj"] },
]
acquire = { use = "file", dir = "${data}/x", place = "p", places = ["q"] }
bundle = [ { id = "vision", label = "with projector", help = "h", parts = [ { kind = "mmproj", label = "projector", match = ["mmproj*.gguf"], prefer = ["F16"] } ] } ]
mode = [ { id = "embedding", label = "embedding server", help = "h", arg = ["--embeddings"] } ]
[components.mmproj]
order = 80
bind = { arg = ["--mmproj", "${component.path}"] }
"#;
    for text in [keywords, names, python, lookups] {
        let table: toml::Table = toml::from_str(text).unwrap();
        let typed =
            read_body::<EngineDoc>(&table).unwrap_or_else(|e| panic!("{}: {}", e.path, e.message));
        assert!(
            typed.unknown.is_empty(),
            "keys the schema does not know: {:?}",
            typed.unknown
        );
    }
    // and the page documents every one of them
    for group in GROUPS.iter().filter(|g| g.source.is_none()) {
        for attribute in group.attributes {
            let key = format!("{} =", attribute.name);
            assert!(
                keywords.contains(&key)
                    || names.contains(&key)
                    || python.contains(&key)
                    || lookups.contains(&key),
                "`{}` of `{}` is not exercised by the variant document above",
                attribute.name,
                group.heading
            );
        }
    }
}
