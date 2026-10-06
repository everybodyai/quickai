//! Loading configuration: layers of documents merged into one set (docs/architecture.md, 5.2).
//!
//! The application has its defaults embedded; a package may add documents next to the executable;
//! the user may add and override in their config directory; tests point an override directory at
//! their own. This module takes whatever each layer offers as text (through `LayerSource`, so it
//! never touches a filesystem itself), checks each document, merges documents of the same
//! `(kind, id)` from lower to higher layers, resolves `extends`, and reports every problem as a
//! diagnostic with file and line. A broken document is skipped; the rest keeps working.

pub mod locate;
pub mod merge;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::diag::{Diagnostic, Severity};
use crate::doc::{Envelope, Kind, PrimitiveRef, Tier};
use crate::docpath::DocPath;

/// Where a document came from. Later layers override earlier ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// Compiled into the application.
    Embedded,
    /// Next to the executable, put there by the package.
    Bundle,
    /// Downloaded content packs. Data documents only.
    Remote,
    /// The user's own configuration directory.
    User,
    /// A directory named by `QUICKAI_CONFIG_DIR`; what tests use.
    Override,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Layer::Embedded => "embedded",
            Layer::Bundle => "bundle",
            Layer::Remote => "remote",
            Layer::User => "user",
            Layer::Override => "override",
        }
    }

    /// Whether documents of `kind` may come from this layer. Remote content is data only: a
    /// document that names a program or a host must come from somewhere the user controls.
    pub fn may_define(self, kind: Kind) -> bool {
        match self {
            Layer::Remote => kind.tier() == Tier::Data,
            _ => true,
        }
    }
}

/// One document as a layer offers it.
#[derive(Clone, Debug)]
pub struct SourceDoc {
    /// How diagnostics name it: `embedded:engines/llamacpp.toml`, or a path.
    pub name: String,
    pub text: String,
}

/// What a layer has: its documents, and anything that went wrong reading them (a file that could
/// not be read is a diagnostic, not a reason to stop).
#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub docs: Vec<SourceDoc>,
    pub problems: Vec<Diagnostic>,
}

/// A layer's documents. The application implements this over the filesystem and over its
/// embedded files; tests use `MemorySource`.
pub trait LayerSource {
    fn read(&self) -> Listing;
}

/// A layer held in memory.
#[derive(Clone, Debug, Default)]
pub struct MemorySource(pub Vec<SourceDoc>);

impl MemorySource {
    pub fn new(docs: &[(&str, &str)]) -> Self {
        MemorySource(
            docs.iter()
                .map(|(name, text)| SourceDoc {
                    name: (*name).to_string(),
                    text: (*text).to_string(),
                })
                .collect(),
        )
    }
}

impl LayerSource for MemorySource {
    fn read(&self) -> Listing {
        Listing {
            docs: self.0.clone(),
            problems: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LoadOptions {
    /// `QUICKAI_SAFE=1`: ignore the user layer.
    pub safe_mode: bool,
    /// This application's version, for `requires.app`.
    pub app_version: Option<String>,
}

/// One file that contributed to a merged document, with what it said.
#[derive(Clone, Debug)]
pub struct Contributor {
    pub layer: Layer,
    pub file: String,
    pub text: Arc<str>,
    pub envelope: Envelope,
    /// The document's keys without the envelope.
    pub body: toml::Table,
}

/// A document after merging every layer that defined it.
#[derive(Clone, Debug)]
pub struct MergedDoc {
    pub envelope: Envelope,
    /// Everything but the envelope.
    pub body: toml::Table,
    /// The files it was built from, lowest layer first.
    pub contributors: Vec<Contributor>,
}

impl MergedDoc {
    /// Folds `contributors` (lowest layer first) into one document. A contributor that says
    /// `replace = true` discards the earlier ones of its own `(kind, id)`; the rest patch what is
    /// below them. The registry uses this again to rebuild a document without its highest layer
    /// when that layer turns out to be invalid.
    pub fn from_contributors(contributors: Vec<Contributor>) -> MergedDoc {
        let mut effective: Vec<Contributor> = Vec::new();
        for c in contributors {
            if c.envelope.replace {
                effective.retain(|e| {
                    e.envelope.kind != c.envelope.kind || e.envelope.id != c.envelope.id
                });
            }
            effective.push(c);
        }
        let last = effective
            .last()
            .expect("a document has at least one contributor")
            .envelope
            .clone();
        let mut body = toml::Table::new();
        let mut envelope = last.clone();
        envelope.extends = None;
        envelope.requires_primitives = Vec::new();
        envelope.requires_app = None;
        envelope.schema = 1;
        for c in &effective {
            merge::merge_tables(&mut body, c.body.clone());
            if c.envelope.id == last.id && c.envelope.extends.is_some() {
                envelope.extends = c.envelope.extends.clone();
            }
            if c.envelope.requires_app.is_some() {
                envelope.requires_app = c.envelope.requires_app.clone();
            }
            for p in &c.envelope.requires_primitives {
                if !envelope.requires_primitives.contains(p) {
                    envelope.requires_primitives.push(p.clone());
                }
            }
            envelope.schema = envelope.schema.max(c.envelope.schema);
        }
        MergedDoc {
            envelope,
            body,
            contributors: effective,
        }
    }

    /// The file and line that define `path`: the highest layer that has it. When no file has the
    /// value itself, the nearest enclosing part of the path in the highest file that has any.
    pub fn locate(&self, path: &DocPath) -> (Option<String>, Option<u32>) {
        let mut fallback: Option<(String, u32)> = None;
        for c in self.contributors.iter().rev() {
            match locate::locate_in_text(&c.text, path) {
                Some(found) if found.exact => return (Some(c.file.clone()), Some(found.line)),
                Some(found) if fallback.is_none() => fallback = Some((c.file.clone(), found.line)),
                _ => {}
            }
        }
        match fallback {
            Some((file, line)) => (Some(file), Some(line)),
            None => (self.contributors.last().map(|c| c.file.clone()), None),
        }
    }

    /// `message` as a diagnostic of this document at `path`, with file and line filled in.
    pub fn diagnostic(
        &self,
        severity: Severity,
        path: &DocPath,
        message: impl Into<String>,
    ) -> Diagnostic {
        let mut d = match severity {
            Severity::Info => Diagnostic::info(message),
            Severity::Warning => Diagnostic::warning(message),
            Severity::Error => Diagnostic::error(message),
        };
        let (file, line) = self.locate(path);
        if let Some(file) = file {
            d = d.in_file(file);
        }
        if let Some(line) = line {
            d = d.at_line(line);
        }
        if !path.is_empty() {
            d = d.at_path(path.to_string());
        }
        d
    }
}

/// The result of loading: the merged documents and everything that went wrong on the way.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub docs: BTreeMap<(Kind, String), MergedDoc>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Loads and merges the documents of `layers` (in any order; they are applied from the lowest layer
/// to the highest).
pub fn load(layers: &[(Layer, &dyn LayerSource)], options: &LoadOptions) -> Loaded {
    let mut diagnostics = Vec::new();
    let mut groups: BTreeMap<(Kind, String), Vec<Contributor>> = BTreeMap::new();

    let mut ordered: Vec<&(Layer, &dyn LayerSource)> = layers.iter().collect();
    ordered.sort_by_key(|(layer, _)| *layer);

    for (layer, source) in ordered {
        if options.safe_mode && *layer == Layer::User {
            diagnostics.push(Diagnostic::info(
                "safe mode: the user configuration is ignored",
            ));
            continue;
        }
        let listing = source.read();
        diagnostics.extend(listing.problems);
        for doc in listing.docs {
            ingest(*layer, doc, options, &mut groups, &mut diagnostics);
        }
    }

    let docs = resolve_extends(groups, &mut diagnostics);
    Loaded { docs, diagnostics }
}

fn ingest(
    layer: Layer,
    doc: SourceDoc,
    options: &LoadOptions,
    groups: &mut BTreeMap<(Kind, String), Vec<Contributor>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let text: Arc<str> = Arc::from(doc.text.as_str());
    let at = |d: Diagnostic| d.in_file(doc.name.clone());

    let table: toml::Table = match toml::from_str(&doc.text) {
        Ok(t) => t,
        Err(e) => {
            let mut d = at(Diagnostic::error(format!(
                "not valid TOML: {}",
                e.message().trim()
            )));
            if let Some(span) = e.span() {
                d = d.at_line(locate::line_of_offset(&doc.text, span.start));
            }
            diagnostics.push(d.with_hint("the document was skipped"));
            return;
        }
    };

    let (envelope, body) = match Envelope::split(table) {
        Ok(split) => split,
        Err(problems) => {
            for p in problems {
                let path = DocPath::default().key(p.path.split('.').next().unwrap_or_default());
                let mut d = at(Diagnostic::error(p.message)).at_path(p.path.clone());
                if let Some(found) = locate::locate_in_text(&doc.text, &path) {
                    d = d.at_line(found.line);
                }
                d = d.with_hint(
                    p.hint
                        .unwrap_or_else(|| "the document was skipped".to_string()),
                );
                diagnostics.push(d);
            }
            return;
        }
    };

    if !layer.may_define(envelope.kind) {
        diagnostics.push(
            at(Diagnostic::error(format!(
                "{} document may not come from the {} layer",
                envelope.kind.with_article(),
                layer.name()
            )))
            .with_hint("documents that can run programs or reach hosts load only from the application, its package and the user's own directory"),
        );
        return;
    }

    if let (Some(required), Some(have)) = (&envelope.requires_app, &options.app_version) {
        match (
            semver::VersionReq::parse(required),
            crate::expr::funcs::parse_version(have),
        ) {
            (Ok(req), Ok(version)) => {
                if !req.matches(&version) {
                    diagnostics.push(
                        at(Diagnostic::error(format!(
                            "the document needs QuickAI {required}, this is {have}"
                        )))
                        .with_hint("update QuickAI, or remove the document"),
                    );
                    return;
                }
            }
            (Err(_), _) => {
                diagnostics.push(
                    at(Diagnostic::error(format!(
                        "`requires.app` is `{required}`, which is not a version requirement"
                    )))
                    .at_path("requires.app")
                    .with_hint("write it like `>=0.2`"),
                );
                return;
            }
            (_, Err(_)) => {} // an unreadable application version cannot rule a document out
        }
    }

    groups
        .entry((envelope.kind, envelope.id.clone()))
        .or_default()
        .push(Contributor {
            layer,
            file: doc.name.clone(),
            text,
            envelope,
            body,
        });
}

/// Applies `extends`: a document derived from another starts as a copy of it and is patched by its
/// own keys. A missing base or a cycle drops the document, with a diagnostic.
fn resolve_extends(
    mut groups: BTreeMap<(Kind, String), Vec<Contributor>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<(Kind, String), MergedDoc> {
    let keys: Vec<(Kind, String)> = groups.keys().cloned().collect();
    let mut done: BTreeMap<(Kind, String), Option<MergedDoc>> = BTreeMap::new();
    for key in &keys {
        resolve_one(key, &mut groups, &mut done, &mut Vec::new(), diagnostics);
    }
    done.into_iter()
        .filter_map(|(key, doc)| doc.map(|d| (key, d)))
        .collect()
}

fn resolve_one(
    key: &(Kind, String),
    groups: &mut BTreeMap<(Kind, String), Vec<Contributor>>,
    done: &mut BTreeMap<(Kind, String), Option<MergedDoc>>,
    stack: &mut Vec<(Kind, String)>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MergedDoc> {
    if let Some(known) = done.get(key) {
        return known.clone();
    }
    if stack.contains(key) {
        return None; // the cycle is reported by the document that closed it
    }
    let own = groups.remove(key)?;
    let this = MergedDoc::from_contributors(own.clone());
    stack.push(key.clone());

    let merged = match this.envelope.extends.clone() {
        None => Some(this.clone()),
        Some(base_id) => {
            let base_key = (key.0, base_id.clone());
            let extends_path = DocPath::default().key("extends");
            if stack.contains(&base_key) {
                diagnostics.push(
                    this.diagnostic(
                        Severity::Error,
                        &extends_path,
                        format!("`{}` extends `{base_id}`, which extends it back", key.1),
                    )
                    .with_hint("the document was skipped"),
                );
                None
            } else if !groups.contains_key(&base_key) && !done.contains_key(&base_key) {
                diagnostics.push(
                    this.diagnostic(
                        Severity::Error,
                        &extends_path,
                        format!(
                            "`{}` extends `{base_id}`, but there is no {} document with that id",
                            key.1,
                            key.0.name()
                        ),
                    )
                    .with_hint("the document was skipped"),
                );
                None
            } else {
                match resolve_one(&base_key, groups, done, stack, diagnostics) {
                    Some(base) => {
                        let mut contributors = base.contributors.clone();
                        contributors.extend(own);
                        Some(MergedDoc::from_contributors(contributors))
                    }
                    None => {
                        diagnostics.push(
                            this.diagnostic(
                                Severity::Error,
                                &extends_path,
                                format!(
                                    "`{}` extends `{base_id}`, which could not be loaded",
                                    key.1
                                ),
                            )
                            .with_hint("the document was skipped"),
                        );
                        None
                    }
                }
            }
        }
    };
    stack.pop();
    done.insert(key.clone(), merged.clone());
    merged
}

/// Primitives required by any loaded document, deduplicated, for checks against what the
/// application provides.
pub fn required_primitives(loaded: &Loaded) -> BTreeSet<PrimitiveRef> {
    loaded
        .docs
        .values()
        .flat_map(|d| d.envelope.requires_primitives.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\
[engine]\nname = \"llama.cpp\"\n\
[launch.spawn]\nprogram = \"${runtime.binary}\"\nargs = [\"--model\", \"${model.main.path}\"]\n\
[[field]]\nid = \"port\"\ntype = \"port\"\ndefault = 8080\n\
[[field]]\nid = \"ctx_size\"\ntype = \"int\"\ndefault = 4096\n";

    fn embedded() -> MemorySource {
        MemorySource::new(&[("embedded:engines/llamacpp.toml", BASE)])
    }

    fn load_layers(layers: &[(Layer, &dyn LayerSource)]) -> Loaded {
        load(layers, &LoadOptions::default())
    }

    fn engine<'a>(loaded: &'a Loaded, id: &str) -> &'a MergedDoc {
        &loaded.docs[&(Kind::Engine, id.to_string())]
    }

    fn errors(loaded: &Loaded) -> Vec<String> {
        loaded
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.to_string())
            .collect()
    }

    #[test]
    fn one_layer_loads_as_it_is() {
        let e = embedded();
        let loaded = load_layers(&[(Layer::Embedded, &e)]);
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        let doc = engine(&loaded, "llamacpp");
        assert_eq!(doc.envelope.kind, Kind::Engine);
        assert!(doc.body.contains_key("engine") && !doc.body.contains_key("schema"));
        assert_eq!(doc.contributors.len(), 1);
    }

    #[test]
    fn a_higher_layer_patches_the_lower_one() {
        let e = embedded();
        let u = MemorySource::new(&[(
            "/home/u/.config/quickai/engines/llamacpp.toml",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\
             [engine]\nname = \"My llama\"\n\
             [[field]]\nid = \"ctx_size\"\ndefault = 8192\n\
             [[field]]\nid = \"threads\"\ntype = \"int\"\n",
        )]);
        // the layers may be given in any order
        let loaded = load_layers(&[(Layer::User, &u), (Layer::Embedded, &e)]);
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        let doc = engine(&loaded, "llamacpp");
        assert_eq!(doc.body["engine"]["name"].as_str(), Some("My llama"));
        let fields = doc.body["field"].as_array().unwrap();
        let ids: Vec<&str> = fields.iter().map(|f| f["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["port", "ctx_size", "threads"]);
        assert_eq!(fields[1]["default"].as_integer(), Some(8192));
        assert_eq!(
            fields[1]["type"].as_str(),
            Some("int"),
            "untouched keys of a patched field stay"
        );
        assert_eq!(doc.contributors.len(), 2);
        assert_eq!(doc.contributors[0].layer, Layer::Embedded);
        assert_eq!(doc.contributors[1].layer, Layer::User);
    }

    #[test]
    fn replace_discards_the_lower_document() {
        let e = embedded();
        let u = MemorySource::new(&[(
            "user.toml",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\nreplace = true\n[engine]\nname = \"Only mine\"\n",
        )]);
        let loaded = load_layers(&[(Layer::Embedded, &e), (Layer::User, &u)]);
        let doc = engine(&loaded, "llamacpp");
        assert!(!doc.body.contains_key("launch"));
        assert_eq!(doc.contributors.len(), 1);
    }

    #[test]
    fn extends_derives_one_document_from_another() {
        let e = MemorySource::new(&[
            ("a.toml", BASE),
            (
                "b.toml",
                "schema = 1\nkind = \"engine\"\nid = \"fork\"\nextends = \"llamacpp\"\n\
                 [engine]\nname = \"Fork\"\n[[field]]\nid = \"port\"\ndefault = 9090\n",
            ),
        ]);
        let loaded = load_layers(&[(Layer::Embedded, &e)]);
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        let fork = engine(&loaded, "fork");
        assert_eq!(fork.body["engine"]["name"].as_str(), Some("Fork"));
        assert_eq!(
            fork.body["launch"]["spawn"]["program"].as_str(),
            Some("${runtime.binary}")
        );
        assert_eq!(
            fork.body["field"].as_array().unwrap()[0]["default"].as_integer(),
            Some(9090)
        );
        // the base is untouched
        assert_eq!(
            engine(&loaded, "llamacpp").body["engine"]["name"].as_str(),
            Some("llama.cpp")
        );
        assert_eq!(
            fork.contributors.len(),
            2,
            "both files are searched when a line is looked up"
        );
    }

    #[test]
    fn extends_problems_skip_the_document_and_say_why() {
        let missing = MemorySource::new(&[(
            "m.toml",
            "schema = 1\nkind = \"engine\"\nid = \"orphan\"\nextends = \"nowhere\"\n[engine]\nname = 'x'\n",
        )]);
        let loaded = load_layers(&[(Layer::Embedded, &missing)]);
        assert!(loaded.docs.is_empty());
        let errs = errors(&loaded);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("m.toml:4"), "{}", errs[0]);
        assert!(
            errs[0].contains("no engine document with that id"),
            "{}",
            errs[0]
        );

        let cycle = MemorySource::new(&[
            ("a.toml", "schema = 1\nkind = \"engine\"\nid = \"a\"\nextends = \"b\"\n[engine]\nname = 'a'\n"),
            ("b.toml", "schema = 1\nkind = \"engine\"\nid = \"b\"\nextends = \"a\"\n[engine]\nname = 'b'\n"),
        ]);
        let loaded = load_layers(&[(Layer::Embedded, &cycle)]);
        assert!(loaded.docs.is_empty(), "{:?}", loaded.docs.keys());
        assert!(!errors(&loaded).is_empty());
        assert!(
            errors(&loaded)
                .iter()
                .any(|e| e.contains("extends it back")),
            "{:?}",
            errors(&loaded)
        );
    }

    #[test]
    fn a_broken_user_document_is_skipped_and_the_embedded_one_stays() {
        let e = embedded();
        let u = MemorySource::new(&[(
            "/home/u/.config/quickai/engines/llamacpp.toml",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[engine\nname = 'broken'\n",
        )]);
        let loaded = load_layers(&[(Layer::Embedded, &e), (Layer::User, &u)]);
        let doc = engine(&loaded, "llamacpp");
        assert_eq!(doc.body["engine"]["name"].as_str(), Some("llama.cpp"));
        let errs = errors(&loaded);
        assert_eq!(errs.len(), 1);
        assert!(
            errs[0].starts_with(
                "error: /home/u/.config/quickai/engines/llamacpp.toml:4: not valid TOML"
            ),
            "{}",
            errs[0]
        );
        assert!(errs[0].contains("the document was skipped"));
    }

    #[test]
    fn envelope_problems_carry_file_line_and_hint() {
        let u = MemorySource::new(&[("u.toml", "# comment\nkind = \"engin\"\nid = \"x\"\n")]);
        let loaded = load_layers(&[(Layer::User, &u)]);
        assert!(loaded.docs.is_empty());
        let messages: Vec<String> = loaded.diagnostics.iter().map(|d| d.to_string()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("u.toml:2") && m.contains("unknown kind `engin`")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("no `schema` number") && m.contains("schema = 1")),
            "{messages:?}"
        );
    }

    #[test]
    fn remote_content_may_not_define_privileged_documents() {
        let remote = MemorySource::new(&[
            ("pack/engine.toml", BASE),
            (
                "pack/catalog.toml",
                "schema = 1\nkind = \"catalog\"\nid = \"extra\"\n[[model]]\nid = \"m\"\n",
            ),
        ]);
        let loaded = load_layers(&[(Layer::Remote, &remote)]);
        assert!(loaded
            .docs
            .contains_key(&(Kind::Catalog, "extra".to_string())));
        assert!(!loaded
            .docs
            .contains_key(&(Kind::Engine, "llamacpp".to_string())));
        let errs = errors(&loaded);
        assert_eq!(errs.len(), 1);
        assert!(
            errs[0].contains("an engine document may not come from the remote layer"),
            "{}",
            errs[0]
        );
    }

    #[test]
    fn safe_mode_ignores_the_user_layer_only() {
        let e = embedded();
        let u = MemorySource::new(&[(
            "u.toml",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[engine]\nname = 'mine'\n",
        )]);
        let o = MemorySource::new(&[(
            "o.toml",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[provides]\ndefault_port = 1\n",
        )]);
        let options = LoadOptions {
            safe_mode: true,
            ..Default::default()
        };
        let loaded = load(
            &[
                (Layer::Embedded, &e),
                (Layer::User, &u),
                (Layer::Override, &o),
            ],
            &options,
        );
        let doc = engine(&loaded, "llamacpp");
        assert_eq!(doc.body["engine"]["name"].as_str(), Some("llama.cpp"));
        assert_eq!(
            doc.body["provides"]["default_port"].as_integer(),
            Some(1),
            "the override layer still applies"
        );
        assert!(loaded
            .diagnostics
            .iter()
            .any(|d| d.message.contains("safe mode")));
        assert_eq!(doc.contributors.len(), 2);
    }

    #[test]
    fn requires_app_is_checked_against_the_version() {
        let u = MemorySource::new(&[
            ("new.toml", "schema = 1\nkind = \"engine\"\nid = \"n\"\nrequires = { app = \">=0.5\" }\n[engine]\nname = 'n'\n"),
            ("ok.toml", "schema = 1\nkind = \"engine\"\nid = \"o\"\nrequires = { app = \">=0.1\" }\n[engine]\nname = 'o'\n"),
            ("bad.toml", "schema = 1\nkind = \"engine\"\nid = \"b\"\nrequires = { app = \"soon\" }\n[engine]\nname = 'b'\n"),
        ]);
        let options = LoadOptions {
            app_version: Some("0.1.0".into()),
            ..Default::default()
        };
        let loaded = load(&[(Layer::User, &u)], &options);
        assert!(loaded.docs.contains_key(&(Kind::Engine, "o".to_string())));
        assert!(!loaded.docs.contains_key(&(Kind::Engine, "n".to_string())));
        assert!(!loaded.docs.contains_key(&(Kind::Engine, "b".to_string())));
        let errs = errors(&loaded);
        assert!(
            errs.iter()
                .any(|e| e.contains("needs QuickAI >=0.5, this is 0.1.0")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("not a version requirement")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_diagnostic_points_at_the_highest_layer_that_defines_the_value() {
        let e = embedded();
        let u = MemorySource::new(&[(
            "user.toml",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\n[[field]]\nid = \"ctx_size\"\ndefault = 8192\n",
        )]);
        let loaded = load_layers(&[(Layer::Embedded, &e), (Layer::User, &u)]);
        let doc = engine(&loaded, "llamacpp");

        // `default` of ctx_size is set by both; the user file wins
        let d = doc.diagnostic(
            Severity::Error,
            &DocPath::default()
                .key("field")
                .id("ctx_size")
                .key("default"),
            "bad default",
        );
        assert_eq!(
            d.to_string(),
            "error: user.toml:7: field[ctx_size].default: bad default"
        );
        // `type` is only in the embedded file
        let d = doc.diagnostic(
            Severity::Warning,
            &DocPath::default().key("field").id("ctx_size").key("type"),
            "odd type",
        );
        assert_eq!(d.file.as_deref(), Some("embedded:engines/llamacpp.toml"));
        assert_eq!(d.line, Some(15));
        // a value nobody has points at the nearest enclosing part
        let d = doc.diagnostic(
            Severity::Error,
            &DocPath::default().key("launch").key("spawn").key("stop"),
            "missing",
        );
        assert_eq!(d.file.as_deref(), Some("embedded:engines/llamacpp.toml"));
        assert_eq!(d.line, Some(6));
    }

    #[test]
    fn required_primitives_are_collected_across_documents() {
        let e = MemorySource::new(&[
            ("a.toml", "schema=1\nkind='engine'\nid='a'\nrequires={primitives=['spawn@1','locate.setting@1']}\n[engine]\nname='a'\n"),
            ("b.toml", "schema=1\nkind='engine'\nid='b'\nrequires={primitives=['spawn@1']}\n[engine]\nname='b'\n"),
        ]);
        let loaded = load_layers(&[(Layer::Embedded, &e)]);
        let names: Vec<String> = required_primitives(&loaded)
            .iter()
            .map(|p| p.to_string())
            .collect();
        assert_eq!(names, vec!["locate.setting@1", "spawn@1"]);
    }
}
