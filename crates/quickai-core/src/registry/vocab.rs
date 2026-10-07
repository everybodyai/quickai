//! The vocabulary of a registry: the APIs engines serve and how the proxy reads each one (work
//! package 3.6). Vocabulary documents are checked here, and an engine that names an API no
//! vocabulary defines is a load diagnostic (docs/phase3-test-plan.md, D08).

use std::collections::{BTreeMap, BTreeSet};

use crate::config::MergedDoc;
use crate::decode::{ApiDecoder, ChatShape, Framing, PartsShape};
use crate::diag::{Diagnostic, Severity};
use crate::doc::vocab::{ApiDef, DecodeSpec, VocabDoc};
use crate::docpath::DocPath;
use crate::expr::types::suggest;
use crate::expr::{Expr, Template, Type};

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

/// One curl example: shown when `when` holds (always, without one), `curl` a template over the
/// `api` of the engine (`${api.base_url}`, `${api.model}`).
#[derive(Clone, Debug)]
pub struct CompiledExample {
    pub label: String,
    pub when: Option<Expr>,
    pub curl: Template,
}

#[derive(Clone, Debug, Default)]
pub struct Vocabulary {
    apis: BTreeMap<String, Api>,
    /// Which API each path belongs to, for telling two definitions of one path apart.
    paths: BTreeMap<String, String>,
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

    /// The decoders of the APIs named, in that order, for those that have one.
    pub fn decoders_for<'a>(&self, ids: impl IntoIterator<Item = &'a String>) -> Vec<ApiDecoder> {
        ids.into_iter()
            .filter_map(|id| self.apis.get(id).and_then(|a| a.decoder.clone()))
            .collect()
    }

    /// Takes in the APIs of one document. A document that defines an API or a path that another
    /// one already has is refused as a whole, with the diagnostics that say which.
    pub(super) fn add(
        &mut self,
        merged: &MergedDoc,
        apis: Vec<Api>,
        paths: &[(String, String)],
    ) -> Result<(), Vec<Diagnostic>> {
        let mut problems = Vec::new();
        for api in &apis {
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
        for (api, path) in paths {
            if let Some(other) = self.paths.get(path) {
                problems.push(merged.diagnostic(
                    Severity::Error,
                    &DocPath::default().key("api").id(api).key("paths"),
                    format!("the path `{path}` already belongs to the API `{other}`"),
                ));
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        for (api, path) in paths {
            self.paths.insert(path.clone(), api.clone());
        }
        for api in apis {
            self.apis.insert(api.id.clone(), api);
        }
        self.present = true;
        Ok(())
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
pub(super) fn compile(
    merged: &MergedDoc,
    doc: &VocabDoc,
) -> (Vec<Api>, Vec<(String, String)>, Vec<Diagnostic>) {
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

    if problems.iter().any(Diagnostic::is_error) {
        return (Vec::new(), Vec::new(), problems);
    }
    (apis, paths, problems)
}

/// The curl examples of an API: the template must only name `api.base_url` and `api.model`, and the
/// condition only what an engine's context has.
fn compile_examples(
    merged: &MergedDoc,
    problems: &mut Vec<Diagnostic>,
    at: &DocPath,
    api: &ApiDef,
) -> Vec<CompiledExample> {
    let curl_schema = Type::object([(
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
        let curl = match Template::parse(&example.curl) {
            Ok(t) => {
                let found = t.check(&curl_schema);
                for e in &found {
                    error(at.clone().key("curl"), e.message.clone());
                }
                found.is_empty().then_some(t)
            }
            Err(e) => {
                error(at.clone().key("curl"), e.describe(&example.curl));
                None
            }
        };
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
