//! The catalog of a registry: the models the application recommends, read from every catalog
//! document and held to the vocabulary and to the engines they name (docs/catalog.md). A document
//! that says something that cannot be right (a type no vocabulary has, a file that climbs out of its
//! repository, a companion the engine cannot attach, a hash that is not one) is refused as a whole,
//! with where and what to write, and the document below it stays in force.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::MergedDoc;
use crate::diag::{Diagnostic, Severity};
use crate::doc::catalog::{CatalogDoc, FileDoc, ModelDoc, RunDoc};
use crate::docpath::DocPath;
use crate::expr::types::suggest;
use crate::lookup::huggingface::valid_repo_id;

use super::engine::Engine;
use super::vocab::Vocabulary;

/// The most features a model shows.
pub const MAX_FEATURES: usize = 8;
/// The longest a feature word is.
pub const MAX_FEATURE_CHARS: usize = 24;
/// The longest a note is before it is called too long.
pub const MAX_NOTE_CHARS: usize = 600;
/// The longest a tagline is before it is called too long: it is one line under a name.
pub const MAX_TAGLINE_CHARS: usize = 80;

/// One file of a repository the catalog names, with what is known of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogFile {
    /// Its path in the repository.
    pub path: String,
    pub size: Option<u64>,
    /// Lower case hexadecimal.
    pub sha256: Option<String>,
    pub label: Option<String>,
    pub best: bool,
    /// The name it is kept under, when the list gives one (`store_as`); else its own.
    pub store_as: Option<String>,
}

/// What an engine whose models are files does with a model: where the files are and which.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilesRun {
    pub repo: String,
    pub rev: String,
    /// The engine's `[[models.mode]]` the model is started in.
    pub mode: Option<String>,
    /// The weights on offer, in the order the document gives them.
    pub files: Vec<CatalogFile>,
    /// The files that go with the weights, by kind, in the order of the parts of `bundle`.
    pub companions: Vec<(String, CatalogFile)>,
    /// The engine's bundle that pairs the weights with the companions; `None` without companions.
    pub bundle: Option<String>,
    /// The folder of the models folder the weights are kept in, when this model's are not where the
    /// engine keeps weights (a checkpoint, not a diffusion model).
    pub place: Option<String>,
}

/// What an engine that keeps its own models does: pulls a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceRun {
    pub reference: String,
    /// What the pull downloads.
    pub size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    Files(FilesRun),
    Reference(ReferenceRun),
}

/// A model of the list, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub by: String,
    /// An id of the vocabulary's `[[model_type]]`.
    pub kind: String,
    pub note: String,
    /// What it is in a few words; empty when the list says none.
    pub tagline: String,
    pub license: String,
    pub params: String,
    pub context: Option<u64>,
    /// Ids of the vocabulary's `[[modality]]`.
    pub input: Vec<String>,
    pub features: Vec<String>,
    /// Ids of the vocabulary's `[[task]]` it does, in the order of the tasks: the ones it says and the
    /// ones the tasks' rules give it.
    pub tasks: Vec<String>,
    pub start: bool,
    pub bundled: bool,
    /// The catalog document it is written in.
    pub document: String,
    /// By the id of the engine.
    pub runs: BTreeMap<String, Run>,
}

/// What one catalog document says, once checked.
#[derive(Clone, Debug, Default)]
pub(super) struct CatalogParts {
    pub models: Vec<CatalogModel>,
}

/// One document of the list, for the window to say how old it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogPage {
    pub id: String,
    pub title: String,
    /// `YYYY-MM-DD`, as the document says.
    pub verified: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    models: Vec<CatalogModel>,
    /// The document each model id is written in, for the message of a clash.
    owners: BTreeMap<String, String>,
    pages: Vec<CatalogPage>,
}

impl Catalog {
    /// The models of the list, in the order the documents give them (documents by their id).
    pub fn models(&self) -> &[CatalogModel] {
        &self.models
    }

    pub fn model(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// The documents the list is made of.
    pub fn pages(&self) -> &[CatalogPage] {
        &self.pages
    }

    /// The day the list was checked: that of its oldest page that says one.
    pub fn verified(&self) -> Option<&str> {
        self.pages
            .iter()
            .filter_map(|p| p.verified.as_deref())
            .min()
    }

    /// The models an engine offers, in the order of the list.
    pub fn for_engine<'a>(&'a self, engine: &'a str) -> impl Iterator<Item = &'a CatalogModel> {
        self.models
            .iter()
            .filter(move |m| m.runs.contains_key(engine))
    }

    /// Takes in what one document says. A model id another document already has is refused, with
    /// the document that has it named.
    pub(super) fn add(
        &mut self,
        merged: &MergedDoc,
        title: String,
        verified: Option<String>,
        parts: CatalogParts,
    ) -> Result<(), Vec<Diagnostic>> {
        let mut problems = Vec::new();
        for id in parts.models.iter().map(|m| &m.id) {
            if let Some(owner) = self.owners.get(id) {
                problems.push(
                    merged
                        .diagnostic(
                            Severity::Error,
                            &DocPath::default().key("model").id(id),
                            format!("the model `{id}` is already written in the catalog `{owner}`"),
                        )
                        .with_hint(format!(
                            "to change it, write a document with `id = \"{owner}\"` and the model's id there"
                        )),
                );
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let document = merged.envelope.id.clone();
        for id in parts.models.iter().map(|m| &m.id) {
            self.owners.insert(id.clone(), document.clone());
        }
        self.models.extend(parts.models);
        self.pages.push(CatalogPage {
            id: document,
            title,
            verified,
        });
        Ok(())
    }
}

/// What a catalog is held to.
pub(super) struct Context<'a> {
    pub vocabulary: &'a Vocabulary,
    /// The engines that were built.
    pub engines: &'a BTreeMap<String, Engine>,
    /// The ids of every engine there is a document for, built or not.
    pub engine_documents: &'a BTreeSet<String>,
}

/// What an id may be: lower case letters, digits and `.` `_` `-`, beginning with a letter or digit.
pub fn valid_id(id: &str) -> bool {
    id.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().count() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
}

/// Whether `text` is a name an engine can pull: `name` or `name:tag`, the name of lower case
/// letters, digits and `.` `_` `-` with `/` between parts, the tag of letters, digits and `.` `_` `-`.
pub fn valid_reference(text: &str) -> bool {
    let (name, tag) = match text.rsplit_once(':') {
        Some((name, tag)) => (name, Some(tag)),
        None => (text, None),
    };
    let name_ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && !name.contains("..")
        && !name.ends_with('/')
        && !name.contains("//")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c));
    let tag_ok = tag.is_none_or(|t| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    });
    text.chars().count() <= 128 && name_ok && tag_ok
}

/// A commit or branch a file is fetched at: letters, digits and `.` `_` `-`.
fn valid_revision(rev: &str) -> bool {
    !rev.is_empty()
        && rev.chars().count() <= 100
        && rev
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// A path that stays inside the repository it is in.
fn valid_file_path(path: &str) -> bool {
    !path.is_empty()
        && path.chars().count() <= 200
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Reads and checks one document. With any error the document has no models.
pub(super) fn compile(
    merged: &MergedDoc,
    doc: &CatalogDoc,
    ctx: &Context<'_>,
) -> (CatalogParts, Vec<Diagnostic>) {
    let mut problems = Vec::new();
    let mut parts = CatalogParts::default();
    let mut seen = BTreeSet::new();
    let document = merged.envelope.id.clone();

    for model in &doc.models {
        let at = DocPath::default().key("model").id(&model.id);
        if !valid_id(&model.id) {
            problems.push(
                merged
                    .diagnostic(
                        Severity::Error,
                        &at.clone().key("id"),
                        format!("`{}` is not a valid model id", model.id),
                    )
                    .with_hint("lower case letters, digits and . _ -, such as `qwen3.5-4b`"),
            );
            continue;
        }
        if !seen.insert(model.id.clone()) {
            problems.push(merged.diagnostic(
                Severity::Error,
                &at,
                format!("the model `{}` is written twice", model.id),
            ));
            continue;
        }
        // a model a person hides is no model of the list: it is not checked and takes no name
        if model.hidden {
            continue;
        }
        if let Some(compiled) = compile_model(merged, ctx, model, &at, &document, &mut problems) {
            parts.models.push(compiled);
        }
    }

    if problems.iter().any(Diagnostic::is_error) {
        return (CatalogParts::default(), problems);
    }
    (parts, problems)
}

fn report(
    merged: &MergedDoc,
    problems: &mut Vec<Diagnostic>,
    path: DocPath,
    message: String,
    hint: Option<String>,
) {
    let mut d = merged.diagnostic(Severity::Error, &path, message);
    if let Some(hint) = hint {
        d = d.with_hint(hint);
    }
    problems.push(d);
}

fn compile_model(
    merged: &MergedDoc,
    ctx: &Context<'_>,
    model: &ModelDoc,
    at: &DocPath,
    document: &str,
    problems: &mut Vec<Diagnostic>,
) -> Option<CatalogModel> {
    // the type: one the vocabulary has, else the vocabulary's default
    let types = ctx.vocabulary.model_types();
    let kind = match model
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(kind) => {
            if !types.is_empty() && !types.iter().any(|t| t.id == kind) {
                report(
                    merged,
                    problems,
                    at.clone().key("type"),
                    format!("`{kind}` is not a model type any vocabulary defines"),
                    Some(match suggest(kind, types.iter().map(|t| t.id.as_str())) {
                        Some(near) => format!("did you mean `{near}`?"),
                        None => format!(
                            "the types: {}",
                            types
                                .iter()
                                .map(|t| t.id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }),
                );
                return None;
            }
            kind.to_string()
        }
        None => types
            .iter()
            .find(|t| t.default)
            .map(|t| t.id.clone())
            .unwrap_or_default(),
    };

    // what it reads
    let known: Vec<&str> = ctx
        .vocabulary
        .modalities()
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    let mut input: Vec<String> = Vec::new();
    for (i, word) in model.input.iter().enumerate() {
        let word = word.trim();
        if !known.is_empty() && !known.contains(&word) {
            report(
                merged,
                problems,
                at.clone().key("input").index(i),
                format!("`{word}` is not a modality any vocabulary defines"),
                Some(match suggest(word, known.iter().copied()) {
                    Some(near) => format!("did you mean `{near}`?"),
                    None => format!("the modalities: {}", known.join(", ")),
                }),
            );
            return None;
        }
        if !input.iter().any(|w| w == word) {
            input.push(word.to_string());
        }
    }
    if input.is_empty() {
        input.push("text".to_string());
    }

    // what else is worth knowing
    let mut features: Vec<String> = Vec::new();
    for (i, feature) in model.features.iter().enumerate() {
        let feature = feature.trim();
        if feature.is_empty()
            || feature.chars().count() > MAX_FEATURE_CHARS
            || feature != feature.to_lowercase()
        {
            report(
                merged,
                problems,
                at.clone().key("features").index(i),
                format!("`{feature}` is not a short lower case word or two"),
                Some(format!(
                    "at most {MAX_FEATURE_CHARS} characters, lower case"
                )),
            );
            return None;
        }
        if !features.iter().any(|f| f == feature) {
            features.push(feature.to_string());
        }
    }
    if features.len() > MAX_FEATURES {
        report(
            merged,
            problems,
            at.clone().key("features"),
            format!(
                "a model shows at most {MAX_FEATURES} features, this has {}",
                features.len()
            ),
            None,
        );
        return None;
    }
    // what a person can want it for: the tasks it says it does, then the ones whose rule holds
    let defined = ctx.vocabulary.tasks();
    let mut said: Vec<&str> = Vec::new();
    for (i, task) in model.tasks.iter().enumerate() {
        let task = task.trim();
        if !defined.iter().any(|t| t.id == task) {
            report(
                merged,
                problems,
                at.clone().key("tasks").index(i),
                format!("`{task}` is not a task any vocabulary defines"),
                Some(match suggest(task, defined.iter().map(|t| t.id.as_str())) {
                    Some(near) => format!("did you mean `{near}`?"),
                    None if defined.is_empty() => "no vocabulary defines a `[[task]]`".to_string(),
                    None => format!(
                        "the tasks: {}",
                        defined
                            .iter()
                            .map(|t| t.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }),
            );
            return None;
        }
        said.push(task);
    }
    let tasks: Vec<String> = defined
        .iter()
        .filter(|t| said.contains(&t.id.as_str()) || t.holds(&kind, &input, &features))
        .map(|t| t.id.clone())
        .collect();
    if model.context == Some(0) {
        report(
            merged,
            problems,
            at.clone().key("context"),
            "a context of 0 tokens is not one".to_string(),
            None,
        );
        return None;
    }

    // how each engine gets it
    let mut runs = BTreeMap::new();
    let mut ok = true;
    for (engine_id, run) in &model.run {
        let run_at = at.clone().key("run").key(engine_id);
        let Some(engine) = ctx.engines.get(engine_id) else {
            // an engine whose document did not build is not here to run anything, and has said why
            if ctx.engine_documents.contains(engine_id) {
                continue;
            }
            report(
                merged,
                problems,
                run_at,
                format!("there is no engine called `{engine_id}`"),
                Some(
                    match suggest(engine_id, ctx.engines.keys().map(String::as_str)) {
                        Some(near) => format!("did you mean `{near}`?"),
                        None => format!(
                            "the engines: {}",
                            ctx.engines.keys().cloned().collect::<Vec<_>>().join(", ")
                        ),
                    },
                ),
            );
            ok = false;
            continue;
        };
        match compile_run(merged, engine, run, &run_at, problems) {
            Some(run) => {
                runs.insert(engine_id.clone(), run);
            }
            None => ok = false,
        }
    }
    if !ok {
        return None;
    }
    if runs.is_empty() {
        problems.push(merged.diagnostic(
            Severity::Warning,
            &at.clone().key("run"),
            format!(
                "no engine offers the model `{}`: it has no `[model.run.<engine>]`",
                model.id
            ),
        ));
    }

    let note = model.note.clone().unwrap_or_default();
    if note.chars().count() > MAX_NOTE_CHARS {
        problems.push(merged.diagnostic(
            Severity::Warning,
            &at.clone().key("note"),
            format!(
                "the note of `{}` is longer than {MAX_NOTE_CHARS} characters",
                model.id
            ),
        ));
    }
    let tagline = model
        .tagline
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    if tagline.chars().count() > MAX_TAGLINE_CHARS {
        problems.push(merged.diagnostic(
            Severity::Warning,
            &at.clone().key("tagline"),
            format!(
                "the tagline of `{}` is longer than {MAX_TAGLINE_CHARS} characters: it is one line under the name",
                model.id
            ),
        ));
    }
    Some(CatalogModel {
        id: model.id.clone(),
        name: model
            .name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| model.id.clone()),
        by: model.by.clone().unwrap_or_default(),
        kind,
        note,
        tagline,
        license: model.license.clone().unwrap_or_default(),
        params: model.params.clone().unwrap_or_default(),
        context: model.context,
        input,
        features,
        tasks,
        start: model.start,
        bundled: model.bundled,
        document: document.to_string(),
        runs,
    })
}

/// What an engine accepts: files (it has `formats`), names (it has `refs`), or nothing said.
enum Style {
    Files(Vec<String>),
    Reference,
    Unsaid,
}

fn style_of(engine: &Engine) -> Style {
    match &engine.doc.provides.accepts {
        Some(a) if !a.formats.is_empty() => Style::Files(a.formats.clone()),
        Some(a) if !a.refs.is_empty() => Style::Reference,
        _ => Style::Unsaid,
    }
}

fn compile_run(
    merged: &MergedDoc,
    engine: &Engine,
    run: &RunDoc,
    at: &DocPath,
    problems: &mut Vec<Diagnostic>,
) -> Option<Run> {
    let mut error = |path: DocPath, message: String, hint: Option<String>| {
        let mut d = merged.diagnostic(Severity::Error, &path, message);
        if let Some(hint) = hint {
            d = d.with_hint(hint);
        }
        problems.push(d);
    };
    let by_files = run.repo.is_some() || !run.files.is_empty() || !run.companions.is_empty();
    let by_name = run.reference.is_some();
    let (files_style, formats) = match style_of(engine) {
        Style::Files(formats) => (true, formats),
        Style::Reference => (false, Vec::new()),
        Style::Unsaid => (!by_name, Vec::new()),
    };

    if !files_style {
        // an engine that pulls a name
        let mut wrong = false;
        for (key, present) in [
            ("repo", run.repo.is_some()),
            ("rev", run.rev.is_some()),
            ("files", !run.files.is_empty()),
            ("companions", !run.companions.is_empty()),
            ("mode", run.mode.is_some()),
            ("place", run.place.is_some()),
        ] {
            if present {
                error(
                    at.clone().key(key),
                    format!(
                        "`{}` pulls a model by name, so `{key}` has no meaning here",
                        engine.id
                    ),
                    Some("write `ref = \"name:tag\"` and, if known, `size`".to_string()),
                );
                wrong = true;
            }
        }
        let Some(reference) = run.reference.as_deref() else {
            error(
                at.clone().key("ref"),
                format!("`{}` pulls a model by name: write `ref`", engine.id),
                Some("ref = \"qwen3.5:4b\"".to_string()),
            );
            return None;
        };
        if !valid_reference(reference) {
            error(
                at.clone().key("ref"),
                format!("`{reference}` is not a name `{}` can pull", engine.id),
                Some("a name, then `:` and a tag: `qwen3.5:4b`".to_string()),
            );
            return None;
        }
        if run.size == Some(0) {
            error(
                at.clone().key("size"),
                "a size of 0 bytes is not one".to_string(),
                None,
            );
            return None;
        }
        if wrong {
            return None;
        }
        return Some(Run::Reference(ReferenceRun {
            reference: reference.to_string(),
            size: run.size,
        }));
    }

    // an engine whose models are files
    let mut wrong = false;
    for (key, present) in [("ref", by_name), ("size", run.size.is_some())] {
        if present {
            error(
                at.clone().key(key),
                format!(
                    "`{}` takes files, so `{key}` has no meaning here",
                    engine.id
                ),
                Some("write `repo`, `rev` and `files`".to_string()),
            );
            wrong = true;
        }
    }
    if !by_files {
        error(
            at.clone(),
            format!("`{}` takes files: write `repo` and `files`", engine.id),
            None,
        );
        return None;
    }
    let Some(repo) = run.repo.as_deref() else {
        error(
            at.clone().key("repo"),
            "the files are in a repository: write `repo = \"owner/name\"`".to_string(),
            None,
        );
        return None;
    };
    if !valid_repo_id(repo) || !repo.contains('/') {
        error(
            at.clone().key("repo"),
            format!("`{repo}` is not a repository"),
            Some("`owner/name`, of letters, digits and . - _".to_string()),
        );
        wrong = true;
    }
    let rev = run.rev.as_deref().map(str::trim).unwrap_or("main");
    if !valid_revision(rev) {
        error(
            at.clone().key("rev"),
            format!("`{rev}` is not a commit or a branch"),
            Some("letters, digits and . _ -".to_string()),
        );
        wrong = true;
    }
    if run.files.is_empty() {
        error(
            at.clone().key("files"),
            "a model with no weights to fetch".to_string(),
            Some(
                "files = [ { file = \"model-Q4_K_M.gguf\", size = 123, sha256 = \"...\" } ]"
                    .to_string(),
            ),
        );
        return None;
    }

    let mut files = Vec::new();
    let mut names = BTreeSet::new();
    let mut best = 0;
    for (i, file) in run.files.iter().enumerate() {
        let here = at.clone().key("files").index(i);
        match compile_file(file, &here, &formats, &mut error) {
            Some(compiled) => {
                if !names.insert(compiled.path.clone()) {
                    error(
                        here.clone().key("file"),
                        format!("`{}` is listed twice", compiled.path),
                        None,
                    );
                    wrong = true;
                }
                if compiled.best {
                    best += 1;
                }
                files.push(compiled);
            }
            None => wrong = true,
        }
    }
    if best > 1 {
        error(
            at.clone().key("files"),
            "only one of the weights is the best".to_string(),
            Some("`best = true` on one of them".to_string()),
        );
        wrong = true;
    }

    // what goes with them, and the bundle of the engine that fetches it so
    let kinds: Vec<&str> = engine.components.iter().map(|c| c.kind.as_str()).collect();
    let mut companions: Vec<(String, CatalogFile)> = Vec::new();
    for (kind, file) in &run.companions {
        let here = at.clone().key("companions").key(kind);
        if !kinds.contains(&kind.as_str()) {
            error(
                here.clone(),
                format!("`{}` has no component called `{kind}`", engine.id),
                Some(if kinds.is_empty() {
                    "the engine document needs a [components.<kind>] for it".to_string()
                } else {
                    format!("the components it has: {}", kinds.join(", "))
                }),
            );
            wrong = true;
            continue;
        }
        match compile_file(file, &here, &formats, &mut error) {
            Some(compiled) => companions.push((kind.clone(), compiled)),
            None => wrong = true,
        }
    }
    let mut bundle = None;
    if !companions.is_empty() && !wrong {
        let wanted: BTreeSet<&str> = companions.iter().map(|(k, _)| k.as_str()).collect();
        let bundles = engine
            .models
            .as_ref()
            .map(|m| m.bundles.as_slice())
            .unwrap_or(&[]);
        match bundles.iter().find(|b| {
            b.parts
                .iter()
                .map(|p| p.kind.as_str())
                .collect::<BTreeSet<_>>()
                == wanted
        }) {
            Some(found) => {
                // the files go in the order of the bundle's parts
                companions.sort_by_key(|(kind, _)| {
                    found
                        .parts
                        .iter()
                        .position(|p| &p.kind == kind)
                        .unwrap_or(usize::MAX)
                });
                bundle = Some(found.id.clone());
            }
            None => {
                error(
                    at.clone().key("companions"),
                    format!(
                        "`{}` has no bundle that pairs weights with exactly {}",
                        engine.id,
                        wanted.iter().copied().collect::<Vec<_>>().join(" and ")
                    ),
                    Some(if bundles.is_empty() {
                        "the engine document needs a [[models.bundle]] for it".to_string()
                    } else {
                        format!(
                            "its bundles: {}",
                            bundles
                                .iter()
                                .map(|b| {
                                    format!(
                                        "{} ({})",
                                        b.id,
                                        b.parts
                                            .iter()
                                            .map(|p| p.kind.as_str())
                                            .collect::<Vec<_>>()
                                            .join(" + ")
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }),
                );
                wrong = true;
            }
        }
    }

    // the folder its weights are kept in
    let place = match run.place.as_deref().map(str::trim) {
        None => None,
        Some(folder) if crate::registry::engine::is_folder_name(folder) => Some(folder.to_string()),
        Some(folder) => {
            error(
                at.clone().key("place"),
                format!("`{folder}` is not the name of a folder"),
                Some("lower case letters, digits and _ or -, such as `checkpoints`".to_string()),
            );
            wrong = true;
            None
        }
    };

    // how the engine is started for it
    let modes = engine
        .models
        .as_ref()
        .map(|m| m.modes.as_slice())
        .unwrap_or(&[]);
    let mode = match run.mode.as_deref().map(str::trim) {
        None => None,
        Some(id) if modes.iter().any(|m| m.id == id) => Some(id.to_string()),
        Some(id) => {
            error(
                at.clone().key("mode"),
                format!("`{}` has no mode called `{id}`", engine.id),
                Some(if modes.is_empty() {
                    "the engine document defines no [[models.mode]]".to_string()
                } else {
                    format!(
                        "its modes: {}",
                        modes
                            .iter()
                            .map(|m| m.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }),
            );
            wrong = true;
            None
        }
    };

    if wrong {
        return None;
    }
    Some(Run::Files(FilesRun {
        repo: repo.to_string(),
        rev: rev.to_string(),
        mode,
        files,
        companions,
        bundle,
        place,
    }))
}

fn compile_file(
    file: &FileDoc,
    at: &DocPath,
    formats: &[String],
    error: &mut impl FnMut(DocPath, String, Option<String>),
) -> Option<CatalogFile> {
    let mut ok = true;
    let path = file.file.trim();
    if !valid_file_path(path) {
        error(
            at.clone().key("file"),
            format!("`{path}` is not a path inside the repository"),
            Some("relative, with no `..`, such as `model-Q4_K_M.gguf`".to_string()),
        );
        ok = false;
    } else if !formats.is_empty()
        && !formats.iter().any(|f| {
            path.to_lowercase()
                .ends_with(&format!(".{}", f.to_lowercase()))
        })
    {
        error(
            at.clone().key("file"),
            format!("`{path}` is not a file of a format the engine takes"),
            Some(format!("the formats: {}", formats.join(", "))),
        );
        ok = false;
    }
    if file.size == Some(0) {
        error(
            at.clone().key("size"),
            "a size of 0 bytes is not one".to_string(),
            None,
        );
        ok = false;
    }
    let sha256 = match file.sha256.as_deref().map(str::trim) {
        Some(h) if valid_sha256(h) => Some(h.to_lowercase()),
        Some(h) => {
            error(
                at.clone().key("sha256"),
                format!("`{h}` is not a SHA-256"),
                Some("64 hexadecimal digits".to_string()),
            );
            ok = false;
            None
        }
        None => None,
    };
    // a name to keep it under is a plain file name of the same kind, and nothing that leaves the folder
    if let Some(name) = file.store_as.as_deref() {
        let plain = crate::lookup::present::safe_file_name(name) == name;
        let ending = |n: &str| n.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if !plain || ending(name) != ending(path) {
            error(
                at.clone().key("store_as"),
                format!("`{name}` is not a file name to keep `{path}` under"),
                Some("a plain file name with the same ending".to_string()),
            );
            ok = false;
        }
    }
    if file.label.as_deref().is_some_and(|l| l.trim().is_empty()) {
        error(
            at.clone().key("label"),
            "an empty label says nothing".to_string(),
            None,
        );
        ok = false;
    }
    ok.then(|| CatalogFile {
        path: path.to_string(),
        size: file.size,
        sha256,
        label: file.label.as_ref().map(|l| l.trim().to_string()),
        best: file.best,
        store_as: file.store_as.clone(),
    })
}
