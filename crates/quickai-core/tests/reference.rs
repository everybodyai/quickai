//! The reference page for engine documents is generated from the types, and these tests keep it
//! honest (docs/implementation-plan.md, WP2.7): the checked-in page is the generated one, every
//! table lists exactly the fields of the type behind it, every field type is listed, and the keys of
//! the shapes that are enum variants are ones the typed schema accepts.

use std::path::{Path, PathBuf};

use quickai_core::doc::engine::EngineDoc;
use quickai_core::doc::read_body;
use quickai_core::doc::reference::{render, type_name, GROUPS, TYPES};

const ENGINE_RS: &str = include_str!("../src/doc/engine.rs");

fn page_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("reference")
        .join("engine-fields.md")
}

#[test]
fn the_checked_in_page_is_the_generated_one() {
    let generated = render();
    let path = page_path();
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

/// The keys of `struct <name>` as a document spells them (a `#[serde(rename = "...")]` wins), read
/// from the source of the types.
fn struct_keys(name: &str) -> Vec<String> {
    let start = ENGINE_RS
        .lines()
        .position(|l| l.starts_with(&format!("pub struct {name} {{")))
        .unwrap_or_else(|| panic!("no `pub struct {name}` in doc/engine.rs"));
    let mut keys = Vec::new();
    let mut rename: Option<String> = None;
    for line in ENGINE_RS.lines().skip(start + 1) {
        if line == "}" {
            break;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#[serde(rename = \"") {
            rename = rest.split('"').next().map(str::to_string);
        } else if let Some(rest) = trimmed.strip_prefix("pub ") {
            let field = rest.split(':').next().unwrap().trim().to_string();
            keys.push(rename.take().unwrap_or(field));
        }
    }
    keys
}

#[test]
fn every_table_lists_exactly_the_fields_of_its_type() {
    for group in GROUPS {
        let Some(source) = group.source else { continue };
        let mut documented: Vec<String> = group
            .attributes
            .iter()
            .map(|a| a.name.to_string())
            .collect();
        let mut actual = struct_keys(source);
        documented.sort();
        actual.sort();
        assert_eq!(
            documented, actual,
            "the table `{}` and `struct {source}` disagree: add or remove a row in doc/reference.rs",
            group.heading
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
strategies = [ { use = "setting", field = "f" }, { use = "next_to_exe", file = "f" }, { use = "dev_tree", path = "p" } ]
[acquire]
root = "${data}/x"
filter_hint = "h"
feed = { use = "github_release", repo = "a/b", per_page = 10, show = 5, prereleases = true }
assets = { by = "keywords", ext = [".zip"], exclude = ["x"], os = { linux = ["l"] }, arch = { x86_64 = ["x"] }, label = "${tag}" }
install = { use = "archive", binary = { name = "x", search_depth = 4 }, chmod = true }
backends = [ { match = "a", label = "A", needs_gpu = "amd" } ]
"#;
    let names = r#"
[engine]
name = "x"
[acquire]
assets = { by = "names", rules = [ { when = "os == 'linux'", name = "n" } ], label = "${tag}" }
"#;
    for text in [keywords, names] {
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
                keywords.contains(&key) || names.contains(&key),
                "`{}` of `{}` is not exercised by the variant document above",
                attribute.name,
                group.heading
            );
        }
    }
}
