//! The vocabulary of a registry: the APIs engines serve and how the proxy reads each one (work
//! package 3.6), and the types of model and classes of size a look-up sorts by (WP4.9).
//! Vocabulary documents are checked here, and an engine that names an API no vocabulary defines is
//! a load diagnostic (docs/phase3-test-plan.md, D08).

use std::collections::{BTreeMap, BTreeSet};

use crate::config::MergedDoc;
use crate::decode::{ApiDecoder, ChatShape, Framing, PartsShape};
use crate::diag::{Diagnostic, Severity};
use crate::doc::vocab::{ApiDef, DecodeSpec, VocabDoc};
use crate::docpath::DocPath;
use crate::expr::types::suggest;
use crate::expr::{Expr, Template, Type};
use crate::lookup::size::parse_size;
use crate::lookup::types::{SizeClassDef, TypeDef, TypeMatch};
use crate::profile::notes;

/// An API the vocabulary defines.
#[derive(Clone, Debug)]
pub struct Api {
    pub id: String,
    pub capabilities: Vec<String>,
    /// How the proxy reads it; `None` when the vocabulary only names it.
    pub decoder: Option<ApiDecoder>,
    /// Curl examples for the API panel, in the order the document lists them.
    pub examples: Vec<CompiledExample>,
    /// The vocabulary document it is defined in.
    pub vocab: String,
}

/// One example: shown when `when` holds (always, without one), `curl` (and `python` and `javascript`, when given)
/// a template over the `api` of the engine (`${api.base_url}`, `${api.model}`).
#[derive(Clone, Debug)]
pub struct CompiledExample {
    pub label: String,
    pub when: Option<Expr>,
    pub curl: Template,
    pub python: Option<Template>,
    pub javascript: Option<Template>,
}

/// What one vocabulary document defines, once checked.
#[derive(Clone, Debug, Default)]
pub(super) struct VocabParts {
    pub apis: Vec<Api>,
    /// The `(api, path)` pairs the APIs claim.
    pub paths: Vec<(String, String)>,
    pub types: Vec<TypeDef>,
    pub sizes: Vec<SizeClassDef>,
    /// The kinds of input and output a model can have, as `(id, label)`.
    pub modalities: Vec<(String, String)>,
    /// The sentences of the notes about a configuration, by the id of the note.
    pub findings: Vec<(String, String)>,
    /// Names of environment variables a configuration may not set, besides the ones the code refuses.
    pub deny_env: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Vocabulary {
    apis: BTreeMap<String, Api>,
    /// Which API each path belongs to, for telling two definitions of one path apart.
    paths: BTreeMap<String, String>,
    /// The types of model, in the order the documents list them (the first whose rule holds wins).
    types: Vec<TypeDef>,
    /// The classes of size, in the order they are written: from the smallest up.
    sizes: Vec<SizeClassDef>,
    /// The modalities, `(id, label)`, in the order the documents list them.
    modalities: Vec<(String, String)>,
    /// The sentence of each note about a configuration, by the id of the note.
    findings: BTreeMap<String, String>,
    /// Names of environment variables a configuration may not set, besides the ones the code refuses.
    deny_env: Vec<String>,
    /// The vocabulary document each type and each size class came from, for the message of a clash.
    owners: BTreeMap<String, String>,
    /// At least one vocabulary document was loaded (a registry of one engine document alone has
    /// nothing to hold the engine's API names to).
    present: bool,
}

impl Vocabulary {
    /// True once a vocabulary document has been built into it.
    pub fn is_present(&self) -> bool {
        self.present
    }

    pub fn api(&self, id: &str) -> Option<&Api> {
        self.apis.get(id)
    }

    pub fn apis(&self) -> impl Iterator<Item = &Api> {
        self.apis.values()
    }

    /// The types of model a look-up sorts into, in the order their rules are tried.
    pub fn model_types(&self) -> &[TypeDef] {
        &self.types
    }

    /// The classes of size, from the smallest up.
    pub fn size_classes(&self) -> &[SizeClassDef] {
        &self.sizes
    }

    /// The kinds of input and output a model can have (`text`, `image`), as `(id, label)`.
    pub fn modalities(&self) -> &[(String, String)] {
        &self.modalities
    }

    /// The sentence of the note called `id`, when a vocabulary gives one.
    pub fn finding(&self, id: &str) -> Option<&str> {
        self.findings.get(id).map(String::as_str)
    }

    /// Whether any sentence of the notes about a configuration is defined.
    pub fn has_findings(&self) -> bool {
        !self.findings.is_empty()
    }

    /// The names of environment variables a configuration may not set, besides the ones the
    /// application always refuses (`profile::notes::DENIED_ENV`).
    pub fn deny_env(&self) -> &[String] {
        &self.deny_env
    }

    /// The decoders of the APIs named, in that order, for those that have one.
    pub fn decoders_for<'a>(&self, ids: impl IntoIterator<Item = &'a String>) -> Vec<ApiDecoder> {
        ids.into_iter()
            .filter_map(|id| self.apis.get(id).and_then(|a| a.decoder.clone()))
            .collect()
    }

    /// Takes in what one document defines. A document that defines an API, a path, a type or a size
    /// class that another one already has is refused as a whole, with the diagnostics that say which.
    pub(super) fn add(
        &mut self,
        merged: &MergedDoc,
        parts: VocabParts,
    ) -> Result<(), Vec<Diagnostic>> {
        let mut problems = Vec::new();
        for api in &parts.apis {
            if let Some(other) = self.apis.get(&api.id) {
                problems.push(merged.diagnostic(
                    Severity::Error,
                    &DocPath::default().key("api").id(&api.id),
                    format!(
                        "the API `{}` is already defined by the vocabulary `{}`",
                        api.id, other.vocab
                    ),
                ));
            }
        }
        for (api, path) in &parts.paths {
            if let Some(other) = self.paths.get(path) {
                problems.push(merged.diagnostic(
                    Severity::Error,
                    &DocPath::default().key("api").id(api).key("paths"),
                    format!("the path `{path}` already belongs to the API `{other}`"),
                ));
            }
        }
        for ty in &parts.types {
            if let Some(owner) = self.owners.get(&format!("type:{}", ty.id)) {
                problems.push(merged.diagnostic(
                    Severity::Error,
                    &DocPath::default().key("model_type").id(&ty.id),
                    format!(
                        "the model type `{}` is already defined by the vocabulary `{owner}`",
                        ty.id
                    ),
                ));
            }
            if ty.default {
                if let Some(other) = self.types.iter().find(|t| t.default) {
                    problems.push(
                        merged.diagnostic(
                            Severity::Error,
                            &DocPath::default()
                                .key("model_type")
                                .id(&ty.id)
                                .key("default"),
                            format!("`{}` is already the default type", other.id),
                        ),
                    );
                }
            }
        }
        for (id, _) in &parts.findings {
            if let Some(owner) = self.owners.get(&format!("finding:{id}")) {
                problems.push(merged.diagnostic(
                    Severity::Error,
                    &DocPath::default().key("finding").id(id),
                    format!(
                        "the note `{id}` already has its sentence from the vocabulary `{owner}`"
                    ),
                ));
            }
        }
        for class in &parts.sizes {
            if let Some(owner) = self.owners.get(&format!("size:{}", class.id)) {
                problems.push(merged.diagnostic(
                    Severity::Error,
                    &DocPath::default().key("size_class").id(&class.id),
                    format!(
                        "the size class `{}` is already defined by the vocabulary `{owner}`",
                        class.id
                    ),
                ));
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let vocab_id = merged.envelope.id.clone();
        for (api, path) in &parts.paths {
            self.paths.insert(path.clone(), api.clone());
        }
        for api in parts.apis {
            self.apis.insert(api.id.clone(), api);
        }
        for ty in parts.types {
            self.owners
                .insert(format!("type:{}", ty.id), vocab_id.clone());
            self.types.push(ty);
        }
        for class in parts.sizes {
            self.owners
                .insert(format!("size:{}", class.id), vocab_id.clone());
            self.sizes.push(class);
        }
        for modality in parts.modalities {
            if !self.modalities.iter().any(|(id, _)| *id == modality.0) {
                self.modalities.push(modality);
            }
        }
        for (id, text) in parts.findings {
            self.owners
                .insert(format!("finding:{id}"), vocab_id.clone());
            self.findings.insert(id, text);
        }
        for name in parts.deny_env {
            if !self.deny_env.contains(&name) {
                self.deny_env.push(name);
            }
        }
        self.present = true;
        Ok(())
    }

    /// Warnings for the model types a look-up of the engine says it runs (`types`) that no vocabulary
    /// defines: the type would never be shown, and the person who wrote it should hear. Nothing is said
    /// when no vocabulary defines any type (a registry of one engine document alone).
    pub(super) fn check_lookup_types(
        &self,
        merged: &MergedDoc,
        lookups: &[crate::registry::engine::CompiledLookup],
    ) -> Vec<Diagnostic> {
        if self.types.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (i, lookup) in lookups.iter().enumerate() {
            for (n, id) in lookup.kind.read().types.iter().enumerate() {
                if self.types.iter().any(|t| &t.id == id) {
                    continue;
                }
                let mut d = merged.diagnostic(
                    Severity::Warning,
                    &DocPath::default()
                        .key("models")
                        .key("lookup")
                        .index(i)
                        .key("types")
                        .index(n),
                    format!("`{id}` is not a model type any vocabulary defines"),
                );
                if let Some(hint) = suggest(id, self.types.iter().map(|t| t.id.as_str())) {
                    d = d.with_hint(format!("did you mean `{hint}`?"));
                }
                out.push(d);
            }
        }
        out
    }

    /// The problems with the API names an engine lists under `section.key` (`provides.apis`,
    /// `traffic.apis`): each must be defined by a vocabulary.
    pub(super) fn check_engine_apis(
        &self,
        merged: &MergedDoc,
        section: &str,
        key: &str,
        names: &[String],
    ) -> Vec<Diagnostic> {
        if !self.present {
            return Vec::new();
        }
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| !self.apis.contains_key(*name))
            .map(|(i, name)| {
                let mut d = merged.diagnostic(
                    Severity::Error,
                    &DocPath::default().key(section).key(key).index(i),
                    format!("`{name}` is not an API any vocabulary defines"),
                );
                if let Some(hint) = suggest(name, self.apis.keys().map(String::as_str)) {
                    d = d.with_hint(format!("did you mean `{hint}`?"));
                }
                d
            })
            .collect()
    }
}

/// Reads and checks the APIs of one vocabulary document: the compiled APIs, and the `(api, path)`
/// pairs they claim. With any error the document has no APIs.
pub(super) fn compile(merged: &MergedDoc, doc: &VocabDoc) -> (VocabParts, Vec<Diagnostic>) {
    let mut problems = Vec::new();
    let mut apis = Vec::new();
    let mut paths: Vec<(String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    let vocab_id = merged.envelope.id.clone();

    for api in &doc.apis {
        let at = DocPath::default().key("api").id(&api.id);
        let mut error = |path: DocPath, message: String, hint: Option<&str>| {
            let mut d = merged.diagnostic(Severity::Error, &path, message);
            if let Some(hint) = hint {
                d = d.with_hint(hint);
            }
            problems.push(d);
        };
        if api.id.is_empty()
            || !api
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        {
            error(
                at.clone().key("id"),
                format!("`{}` is not a valid API id", api.id),
                Some("lower case letters, digits and . _ -"),
            );
        }
        if !seen.insert(api.id.clone()) {
            error(
                at.clone(),
                format!("the API id `{}` is used twice", api.id),
                None,
            );
        }
        for (i, path) in api.paths.iter().enumerate() {
            if !path.starts_with('/') || path.contains(['?', '#']) {
                error(
                    at.clone().key("paths").index(i),
                    format!("`{path}` is not a path: it starts with `/` and has no query"),
                    None,
                );
            } else if paths.iter().any(|(a, p)| a == &api.id && p == path) {
                error(
                    at.clone().key("paths").index(i),
                    format!("the path `{path}` is listed twice"),
                    None,
                );
            } else {
                paths.push((api.id.clone(), path.clone()));
            }
        }
        let decoder = api
            .decode
            .as_ref()
            .map(|spec| decode(merged, &mut problems, &at, api, spec));
        let examples = compile_examples(merged, &mut problems, &at, api);
        apis.push(Api {
            id: api.id.clone(),
            capabilities: api.capabilities.clone(),
            decoder: decoder.flatten(),
            examples,
            vocab: vocab_id.clone(),
        });
    }

    let types = compile_types(merged, doc, &mut problems);
    let sizes = compile_sizes(merged, doc, &mut problems);
    let modalities = compile_modalities(merged, doc, &mut problems);
    let findings = compile_findings(merged, doc, &mut problems);
    let deny_env = compile_deny_env(merged, doc, &mut problems);

    if problems.iter().any(Diagnostic::is_error) {
        return (VocabParts::default(), problems);
    }
    (
        VocabParts {
            apis,
            paths,
            types,
            sizes,
            modalities,
            findings,
            deny_env,
        },
        problems,
    )
}

/// The sentences of the notes about a configuration, checked: each is for a note this version
/// makes (else a warning: it may be for a newer one), is written once, says something, and uses
/// only the holes its note fills in.
fn compile_findings(
    merged: &MergedDoc,
    doc: &VocabDoc,
    problems: &mut Vec<Diagnostic>,
) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for def in &doc.findings {
        let at = DocPath::default().key("finding").id(&def.id);
        let Some(spec) = notes::spec_of(&def.id) else {
            let mut d = merged.diagnostic(
                Severity::Warning,
                &at,
                format!(
                    "`{}` is not a note this version of QuickAI makes and is ignored",
                    def.id
                ),
            );
            if let Some(hint) = suggest(&def.id, notes::NOTES.iter().map(|n| n.id)) {
                d = d.with_hint(format!("did you mean `{hint}`?"));
            }
            problems.push(d);
            continue;
        };
        if found.iter().any(|(id, _)| *id == def.id) {
            problems.push(merged.diagnostic(
                Severity::Error,
                &at,
                format!("the note `{}` is given a sentence twice", def.id),
            ));
            continue;
        }
        if def.text.trim().is_empty() {
            problems.push(merged.diagnostic(
                Severity::Error,
                &at.key("text"),
                format!("the sentence of `{}` is empty", def.id),
            ));
            continue;
        }
        let unknown: Vec<String> = notes::holes_of(&def.text)
            .into_iter()
            .filter(|hole| !spec.holes.contains(&hole.as_str()))
            .collect();
        if !unknown.is_empty() {
            problems.push(
                merged
                    .diagnostic(
                        Severity::Error,
                        &at.key("text"),
                        format!(
                            "the sentence of `{}` uses {} which the note does not fill in",
                            def.id,
                            unknown
                                .iter()
                                .map(|h| format!("`{{{h}}}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                    .with_hint(format!(
                        "it has {}",
                        spec.holes
                            .iter()
                            .map(|h| format!("`{{{h}}}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
            );
            continue;
        }
        found.push((def.id.clone(), def.text.clone()));
    }
    found
}

/// The names of `[profile] deny_env`: each a name of an environment variable, with a trailing `*`
/// allowed.
fn compile_deny_env(
    merged: &MergedDoc,
    doc: &VocabDoc,
    problems: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let Some(rules) = &doc.profile else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for (i, name) in rules.deny_env.iter().enumerate() {
        let body = name.strip_suffix('*').unwrap_or(name);
        let at = DocPath::default().key("profile").key("deny_env").index(i);
        if name.is_empty() || !body.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            problems.push(
                merged
                    .diagnostic(
                        Severity::Error,
                        &at,
                        format!("`{name}` is not the name of an environment variable"),
                    )
                    .with_hint("letters, digits and `_`, with `*` allowed at the end"),
            );
            continue;
        }
        if name == "*" {
            problems.push(merged.diagnostic(
                Severity::Error,
                &at,
                "`*` alone would refuse every variable",
            ));
            continue;
        }
        names.push(name.clone());
    }
    names
}

/// The modalities of one document: each an id, written once.
fn compile_modalities(
    merged: &MergedDoc,
    doc: &VocabDoc,
    problems: &mut Vec<Diagnostic>,
) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for def in &doc.modalities {
        let at = DocPath::default().key("modality").id(&def.id);
        if !valid_id(&def.id) {
            problems.push(
                merged
                    .diagnostic(
                        Severity::Error,
                        &at.key("id"),
                        format!("`{}` is not a valid modality id", def.id),
                    )
                    .with_hint("lower case letters, digits and . _ -"),
            );
            continue;
        }
        if found.iter().any(|(id, _)| *id == def.id) {
            problems.push(merged.diagnostic(
                Severity::Error,
                &at,
                format!("the modality `{}` is written twice", def.id),
            ));
            continue;
        }
        found.push((
            def.id.clone(),
            def.label
                .clone()
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| def.id.clone()),
        ));
    }
    found
}

/// What an id may be: lower case letters, digits and `.` `_` `-`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
}

/// The types of model of one document, checked: the ids are names, no id is used twice, at most one
/// type is the default, every list entry says something, and a type that is not the default has a
/// rule (else nothing could ever be of it).
fn compile_types(
    merged: &MergedDoc,
    doc: &VocabDoc,
    problems: &mut Vec<Diagnostic>,
) -> Vec<TypeDef> {
    let mut types: Vec<TypeDef> = Vec::new();
    for (index, def) in doc.model_types.iter().enumerate() {
        let at = DocPath::default().key("model_type").id(&def.id);
        let mut error = |path: DocPath, message: String, hint: Option<&str>| {
            let mut d = merged.diagnostic(Severity::Error, &path, message);
            if let Some(hint) = hint {
                d = d.with_hint(hint);
            }
            problems.push(d);
        };
        if !valid_id(&def.id) {
            error(
                at.clone().key("id"),
                format!("`{}` is not a valid model type id", def.id),
                Some("lower case letters, digits and . _ -"),
            );
            continue;
        }
        if types.iter().any(|t| t.id == def.id) {
            error(
                at.clone(),
                format!("the model type `{}` is written twice", def.id),
                None,
            );
            continue;
        }
        if def.default && types.iter().any(|t| t.default) {
            error(
                at.clone().key("default"),
                "only one model type is the default".to_string(),
                None,
            );
            continue;
        }
        let mut rule = TypeMatch::default();
        for (key, list, into) in [
            (
                "capabilities",
                &def.rule.capabilities,
                &mut rule.capabilities,
            ),
            ("pipelines", &def.rule.pipelines, &mut rule.pipelines),
            ("tags", &def.rule.tags, &mut rule.tags),
            ("words", &def.rule.words, &mut rule.words),
            ("companions", &def.rule.companions, &mut rule.companions),
        ] {
            for (i, entry) in list.iter().enumerate() {
                if entry.trim().is_empty() {
                    error(
                        at.clone().key("match").key(key).index(i),
                        format!("an empty entry in `match.{key}` matches nothing"),
                        None,
                    );
                } else {
                    into.push(entry.trim().to_lowercase());
                }
            }
        }
        if !def.default && rule.is_empty() {
            problems.push(merged.diagnostic(
                Severity::Warning,
                &at,
                format!(
                    "the model type `{}` has no `match` and is not the `default`: nothing is ever of it",
                    def.id
                ),
            ));
        }
        types.push(TypeDef {
            id: def.id.clone(),
            label: def
                .label
                .clone()
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| def.id.clone()),
            help: def.help.clone().unwrap_or_default(),
            order: def.order.unwrap_or(100 + index as i64),
            default: def.default,
            rule,
        });
    }
    types
}

/// The size classes of one document, checked: the ids are names and are not repeated, every bound is
/// a size, the bounds go up, and the class with no bound (at most one) is the last.
fn compile_sizes(
    merged: &MergedDoc,
    doc: &VocabDoc,
    problems: &mut Vec<Diagnostic>,
) -> Vec<SizeClassDef> {
    let mut classes: Vec<SizeClassDef> = Vec::new();
    for def in &doc.size_classes {
        let at = DocPath::default().key("size_class").id(&def.id);
        let mut error = |path: DocPath, message: String, hint: Option<&str>| {
            let mut d = merged.diagnostic(Severity::Error, &path, message);
            if let Some(hint) = hint {
                d = d.with_hint(hint);
            }
            problems.push(d);
        };
        if !valid_id(&def.id) {
            error(
                at.clone().key("id"),
                format!("`{}` is not a valid size class id", def.id),
                Some("lower case letters, digits and . _ -"),
            );
            continue;
        }
        if classes.iter().any(|c| c.id == def.id) {
            error(
                at.clone(),
                format!("the size class `{}` is written twice", def.id),
                None,
            );
            continue;
        }
        let below = match def.below.as_deref() {
            None => None,
            Some(text) => match parse_size(text) {
                Some(bytes) if bytes > 0 => Some(bytes),
                _ => {
                    error(
                        at.clone().key("below"),
                        format!("`{text}` is not a size"),
                        Some("a number and a unit: 4GiB, 500MB"),
                    );
                    continue;
                }
            },
        };
        if let Some(last) = classes.last() {
            match (last.below, below) {
                (None, _) => {
                    error(
                        at.clone(),
                        format!(
                            "`{}` has no `below`, so it takes every size left and must be the last",
                            last.id
                        ),
                        Some("write the classes from the smallest to the largest"),
                    );
                    continue;
                }
                (Some(before), Some(now)) if now <= before => {
                    error(
                        at.clone().key("below"),
                        format!("`{}` ends at or below where `{}` does", def.id, last.id),
                        Some("write the classes from the smallest to the largest"),
                    );
                    continue;
                }
                _ => {}
            }
        }
        classes.push(SizeClassDef {
            id: def.id.clone(),
            label: def
                .label
                .clone()
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| def.id.clone()),
            help: def.help.clone().unwrap_or_default(),
            below,
        });
    }
    classes
}

/// The examples of an API, in curl, Python and JavaScript: each template must only name `api.base_url` and
/// `api.model`, and the condition only what an engine's context has.
fn compile_examples(
    merged: &MergedDoc,
    problems: &mut Vec<Diagnostic>,
    at: &DocPath,
    api: &ApiDef,
) -> Vec<CompiledExample> {
    let code_schema = Type::object([(
        "api",
        Type::object([("base_url", Type::Str), ("model", Type::Str)]),
    )]);
    let when_schema = crate::registry::context::schema(
        &crate::doc::engine::EngineDoc::default(),
        crate::registry::context::Site::Engine,
    );
    let mut compiled = Vec::new();
    for (i, example) in api.examples.iter().enumerate() {
        let at = at.clone().key("examples").index(i);
        let mut error = |path: DocPath, message: String| {
            problems.push(merged.diagnostic(Severity::Error, &path, message));
        };
        // each language's text is a template over the same two values
        let mut code = |key: &str, text: &str| match Template::parse(text) {
            Ok(t) => {
                let found = t.check(&code_schema);
                for e in &found {
                    error(at.clone().key(key), e.message.clone());
                }
                found.is_empty().then_some(t)
            }
            Err(e) => {
                error(at.clone().key(key), e.describe(text));
                None
            }
        };
        let curl = code("curl", &example.curl);
        // a language that does not compile is left out (and said); the example stays with the others
        let python = example.python.as_deref().and_then(|t| code("python", t));
        let javascript = example
            .javascript
            .as_deref()
            .and_then(|t| code("javascript", t));
        let when = match example.when.as_deref() {
            None => Some(None),
            Some(text) => match crate::expr::parse(text) {
                Ok(e) => {
                    let (ty, found) = crate::expr::check(&e, &when_schema);
                    for p in &found {
                        error(
                            at.clone().key("when"),
                            format!("in `{text}`: {}", p.message),
                        );
                    }
                    if found.is_empty() && !ty.is_condition() {
                        error(
                            at.clone().key("when"),
                            format!("`{text}` must be a condition, but it is {}", ty.name()),
                        );
                        None
                    } else {
                        found.is_empty().then_some(Some(e))
                    }
                }
                Err(e) => {
                    error(
                        at.clone().key("when"),
                        format!("`{text}`: {}", e.describe(text)),
                    );
                    None
                }
            },
        };
        if let (Some(curl), Some(when)) = (curl, when) {
            compiled.push(CompiledExample {
                label: example.label.clone(),
                when,
                curl,
                python,
                javascript,
            });
        }
    }
    compiled
}

fn decode(
    merged: &MergedDoc,
    problems: &mut Vec<Diagnostic>,
    at: &DocPath,
    api: &ApiDef,
    spec: &DecodeSpec,
) -> Option<ApiDecoder> {
    let at = at.clone().key("decode");
    let mut error = |path: DocPath, message: String, hint: Option<&str>| {
        let mut d = merged.diagnostic(Severity::Error, &path, message);
        if let Some(hint) = hint {
            d = d.with_hint(hint);
        }
        problems.push(d);
    };
    let pointer = |error: &mut dyn FnMut(DocPath, String, Option<&str>), path: DocPath, p: &str| {
        let ok = p.starts_with('/');
        if !ok {
            error(
                path,
                format!("`{p}` is not a JSON pointer"),
                Some("a pointer starts with a slash: /choices/0/delta/content"),
            );
        }
        ok
    };

    if api.paths.is_empty() {
        error(
            at.clone(),
            format!(
                "the API `{}` is decoded but has no `paths`: nothing would be read",
                api.id
            ),
            None,
        );
    }
    let mut framing = Vec::new();
    if spec.framing.is_empty() {
        error(
            at.clone().key("framing"),
            "`framing` says how a reply comes".to_string(),
            Some("framing = [\"sse\", \"json\"]"),
        );
    }
    for (i, name) in spec.framing.iter().enumerate() {
        match Framing::parse(name) {
            Some(f) => framing.push(f),
            None => error(
                at.clone().key("framing").index(i),
                format!("`{name}` is not a framing"),
                Some("the framings are sse, ndjson and json"),
            ),
        }
    }
    let mut ok = !framing.is_empty() && !api.paths.is_empty();
    for (key, list) in [
        ("delta", &spec.delta),
        ("final", &spec.final_text),
        ("error", &spec.error),
    ] {
        for (i, p) in list.iter().enumerate() {
            ok &= pointer(&mut error, at.clone().key(key).index(i), p);
        }
    }
    match (&spec.chat, &spec.prompt) {
        (Some(_), Some(_)) => {
            error(
                at.clone(),
                "a request is read as a chat (`chat`) or by its `prompt`, not both".to_string(),
                None,
            );
            ok = false;
        }
        (None, None) => {
            error(
                at.clone(),
                "a decoded API says where the text of a request is: `prompt`, or a `chat` table"
                    .to_string(),
                Some("prompt = \"/prompt\""),
            );
            ok = false;
        }
        _ => {}
    }
    if let Some(p) = &spec.prompt {
        ok &= pointer(&mut error, at.clone().key("prompt"), p);
    }
    let chat = spec.chat.as_ref().map(|c| {
        let cat = at.clone().key("chat");
        for (key, p) in [
            ("messages", &c.messages),
            ("role", &c.role),
            ("content", &c.content),
        ] {
            ok &= pointer(&mut error, cat.clone().key(key), p);
        }
        if let Some(images) = &c.images {
            ok &= pointer(&mut error, cat.clone().key("images"), images);
        }
        if let Some(parts) = &c.parts {
            ok &= pointer(
                &mut error,
                cat.clone().key("parts").key("type"),
                &parts.kind,
            );
            ok &= pointer(
                &mut error,
                cat.clone().key("parts").key("text"),
                &parts.text,
            );
        }
        ChatShape {
            messages: c.messages.clone(),
            role: c.role.clone(),
            content: c.content.clone(),
            skip_roles: c.skip_roles.clone(),
            parts: c.parts.as_ref().map(|p| PartsShape {
                kind: p.kind.clone(),
                text: p.text.clone(),
            }),
            images: c.images.clone(),
        }
    });
    ok.then(|| ApiDecoder {
        id: api.id.clone(),
        paths: api.paths.clone(),
        framing,
        prompt: spec.prompt.clone(),
        delta: spec.delta.clone(),
        final_text: spec.final_text.clone(),
        error: spec.error.clone(),
        chat,
    })
}
