//! The registry: every loaded document, checked and ready to use (docs/architecture.md, section 4).
//!
//! `Registry::build` takes what the loader merged, reads each document into its typed form, checks
//! it (expressions, templates, cross references, required primitives) and keeps what passes. It
//! never fails: a document that does not pass is left out and a diagnostic says why. When the
//! invalid document is the merge of several layers, the highest layer is dropped and the rest is
//! tried again, so a mistake in a user's patch leaves the embedded document in force
//! (docs/architecture.md, section 5.3).

pub mod context;
pub mod engine;
#[cfg(test)]
pub(crate) mod fixtures;

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{Loaded, MergedDoc};
use crate::diag::{has_errors, Diagnostic, Severity};
use crate::doc::engine::EngineDoc;
use crate::doc::{read_body, Kind, PrimitiveRef};
use crate::docpath::DocPath;

pub use engine::Engine;

/// The primitives (and versions) the running application provides. The application declares them;
/// the core only compares (docs/implementation-plan.md, C5).
#[derive(Clone, Debug, Default)]
pub struct KnownPrimitives(BTreeSet<PrimitiveRef>);

impl KnownPrimitives {
    /// `KnownPrimitives::new(&["spawn@1", "locate.setting@1"])`. A malformed entry is a bug in the
    /// application, not in a document, so it panics.
    pub fn new(items: &[&str]) -> Self {
        KnownPrimitives(
            items
                .iter()
                .map(|i| PrimitiveRef::parse(i).unwrap_or_else(|e| panic!("bad primitive id: {e}")))
                .collect(),
        )
    }

    pub fn supports(&self, p: &PrimitiveRef) -> bool {
        self.0.contains(p)
    }

    pub fn iter(&self) -> impl Iterator<Item = &PrimitiveRef> {
        self.0.iter()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    engines: BTreeMap<String, Engine>,
    /// Documents of kinds this version does not interpret yet, kept so nothing is lost.
    other: BTreeMap<(Kind, String), MergedDoc>,
    /// Everything that went wrong while loading and building, in order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Registry {
    pub fn build(loaded: Loaded, known: &KnownPrimitives) -> Registry {
        let mut registry = Registry {
            diagnostics: loaded.diagnostics,
            ..Default::default()
        };
        for ((kind, id), merged) in loaded.docs {
            match kind {
                Kind::Engine => {
                    if let Some(engine) = build_engine(merged, known, &mut registry.diagnostics) {
                        registry.engines.insert(id, engine);
                    }
                }
                other => {
                    registry.diagnostics.push(merged.diagnostic(
                        Severity::Info,
                        &DocPath::default(),
                        format!(
                            "{} documents are not used by this version of QuickAI yet; `{id}` is kept but has no effect",
                            other.name()
                        ),
                    ));
                    registry.other.insert((other, id), merged);
                }
            }
        }
        registry
    }

    pub fn engine(&self, id: &str) -> Option<&Engine> {
        self.engines.get(id)
    }

    pub fn engines(&self) -> impl Iterator<Item = &Engine> {
        self.engines.values()
    }

    pub fn document(&self, kind: Kind, id: &str) -> Option<&MergedDoc> {
        self.other.get(&(kind, id.to_string()))
    }

    pub fn has_errors(&self) -> bool {
        has_errors(&self.diagnostics)
    }
}

/// Checks one engine document, falling back to lower layers when the merge of all of them is
/// invalid.
fn build_engine(
    mut merged: MergedDoc,
    known: &KnownPrimitives,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Engine> {
    let mut reported: BTreeSet<String> = diagnostics.iter().map(|d| d.to_string()).collect();
    let mut push = |diagnostics: &mut Vec<Diagnostic>, list: Vec<Diagnostic>| {
        for d in list {
            if reported.insert(d.to_string()) {
                diagnostics.push(d);
            }
        }
    };
    loop {
        let (engine, mut problems) = try_build(&merged, known);
        match engine {
            Some(engine) => {
                push(diagnostics, problems);
                return Some(engine);
            }
            None if merged.contributors.len() > 1 => {
                let mut contributors = merged.contributors.clone();
                let dropped = contributors.pop().expect("more than one contributor");
                if let Some(first) = problems.iter_mut().find(|d| d.is_error()) {
                    let note = format!(
                        "{} was skipped; the document below it stays in force",
                        dropped.file
                    );
                    first.hint = Some(match first.hint.take() {
                        Some(h) => format!("{h} ({note})"),
                        None => note,
                    });
                }
                push(diagnostics, problems);
                merged = MergedDoc::from_contributors(contributors);
            }
            None => {
                push(diagnostics, problems);
                return None;
            }
        }
    }
}

fn try_build(merged: &MergedDoc, known: &KnownPrimitives) -> (Option<Engine>, Vec<Diagnostic>) {
    let mut problems = Vec::new();

    for p in &merged.envelope.requires_primitives {
        if !known.supports(p) {
            problems.push(
                merged
                    .diagnostic(
                        Severity::Error,
                        &DocPath::default().key("requires").key("primitives"),
                        format!(
                            "the document needs `{p}`, which this version of QuickAI does not provide"
                        ),
                    )
                    .with_hint("update QuickAI, or remove the document"),
            );
        }
    }
    if has_errors(&problems) {
        return (None, problems);
    }

    let typed = match read_body::<EngineDoc>(&merged.body) {
        Ok(t) => t,
        Err(e) => {
            problems.push(merged.diagnostic(Severity::Error, &e.path, e.message));
            return (None, problems);
        }
    };
    for path in &typed.unknown {
        problems.push(merged.diagnostic(
            Severity::Warning,
            path,
            format!("`{path}` is not used by this version of QuickAI and is ignored"),
        ));
    }

    let (engine, more) = engine::compile(merged, typed.value);
    problems.extend(more);
    (engine, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load, Layer, LoadOptions, MemorySource};
    use crate::doc::engine::FieldType;
    use engine::CompiledDefault;

    use super::fixtures::GOOD;

    use super::fixtures::known;

    fn registry(embedded: &str, user: Option<&str>) -> Registry {
        let e = MemorySource::new(&[("embedded:engines/llamacpp.toml", embedded)]);
        let u = MemorySource::new(&[("user:engines/llamacpp.toml", user.unwrap_or(""))]);
        let mut layers: Vec<(Layer, &dyn crate::config::LayerSource)> = vec![(Layer::Embedded, &e)];
        if user.is_some() {
            layers.push((Layer::User, &u));
        }
        Registry::build(load(&layers, &LoadOptions::default()), &known())
    }

    fn texts(r: &Registry, severity: Severity) -> Vec<String> {
        r.diagnostics
            .iter()
            .filter(|d| d.severity == severity)
            .map(|d| d.to_string())
            .collect()
    }

    /// `GOOD` with one thing changed.
    fn with(old: &str, new: &str) -> String {
        assert!(GOOD.contains(old), "the fixture has no `{old}`");
        GOOD.replace(old, new)
    }

    #[test]
    fn a_good_document_builds_without_complaint() {
        let r = registry(GOOD, None);
        assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
        let e = r.engine("llamacpp").expect("the engine is registered");
        assert_eq!(e.fields.len(), 5);
        assert_eq!(e.fields[0].ty, FieldType::Port);
        assert!(e.fields[0].public_port);
        assert!(matches!(
            e.fields[1].default,
            Some(CompiledDefault::Computed(_))
        ));
        assert!(matches!(
            e.fields[0].default,
            Some(CompiledDefault::Literal(_))
        ));
        assert_eq!(e.fields[1].order, 20);
        assert_eq!(e.fields[2].derive.len(), 1);
        let spawn = e.spawn.as_ref().unwrap();
        assert_eq!(spawn.args.len(), 6);
        assert_eq!(spawn.env[0].sep, ":");
        let ready = e.ready.as_ref().unwrap();
        assert_eq!((ready.path.as_str(), ready.expect_status), ("/health", 200));
        assert_eq!(ready.timeout.as_secs(), 120);
        assert_eq!(ready.interval.as_millis(), 500);
        assert!(!r.has_errors());
    }

    #[test]
    fn a_typo_in_a_template_is_an_error_with_file_line_and_a_suggestion() {
        let r = registry(
            &with(
                "\"${value}\"]\nui = { order = 20 }",
                "\"${valu}\"]\nui = { order = 20 }",
            ),
            None,
        );
        assert!(r.engine("llamacpp").is_none());
        let errors = texts(&r, Severity::Error);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with("error: embedded:engines/llamacpp.toml:"),
            "{}",
            errors[0]
        );
        assert!(
            errors[0].contains("field[ctx_size].bind.arg"),
            "{}",
            errors[0]
        );
        assert!(errors[0].contains("unknown name `valu`"), "{}", errors[0]);
        assert!(errors[0].contains("did you mean `value`?"), "{}", errors[0]);
    }

    #[test]
    fn expressions_are_checked_where_they_are_written() {
        // an unknown name in a derive expression
        let r = registry(
            &with("gpu_offload = \"value > 0\"", "gpu_offload = \"valu > 0\""),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[n_gpu_layers].derive.gpu_offload")
                    && e.contains("unknown name `valu`")),
            "{errors:?}"
        );

        // a condition that is not one
        let r = registry(
            &with(
                "[[field]]\nid = \"extra_args\"\ntype = \"text\"",
                "[[field]]\nid = \"extra_args\"\nvisible_when = \"os\"\ntype = \"text\"",
            ),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors.iter().any(|e| e.contains("must be a condition")),
            "{errors:?}"
        );

        // a function that does not exist, and a wrong argument count
        let r = registry(
            &with("fn = \"recommend_ctx_size\"", "fn = \"recomend_ctx_size\""),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("unknown function `recomend_ctx_size`")
                    && e.contains("did you mean `recommend_ctx_size`?")),
            "{errors:?}"
        );
        let r = registry(
            &with(
                "args = [\"model.size_bytes\", \"sys.ram_bytes\"]",
                "args = [\"model.size_bytes\"]",
            ),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("takes 2 arguments, found 1")),
            "{errors:?}"
        );
    }

    /// `GOOD` with its `extra_args` field replaced by `field` (the text of one `[[field]]`).
    fn with_field(field: &str) -> String {
        with(
            "[[field]]\nid = \"extra_args\"\ntype = \"text\"\ndefault = \"\"\nlegacy_key = \"extra_args\"\nbind.arg_split = true\nui = { order = 90 }\n",
            &format!("{field}\n"),
        )
    }

    #[test]
    fn the_form_attributes_compile_when_they_are_right() {
        let r = registry(
            &with_field(
                r#"[[field]]
id = "extra_args"
type = "text"
default = ""
pattern = '[a-z ]*'
enabled_when = "not session.running"
required_when = "field.port > 9000"
validate = [ { check = "len(value) < 20", message = "At most 19 characters." } ]
persist = "profile"
applies = "live"
ui = { order = 90, hint = "e.g. --threads 8" }

[[field]]
id = "mode"
type = "choice"
options = ["auto", "attach"]
default = "auto"
"#,
            ),
            None,
        );
        assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
        let e = r.engine("llamacpp").expect("the engine is registered");
        let f = e.fields.iter().find(|f| f.id == "extra_args").unwrap();
        assert!(f.enabled_when.is_some() && f.required_when.is_some());
        assert_eq!(f.validate.len(), 1);
        assert_eq!(f.validate[0].message, "At most 19 characters.");
        let re = f.pattern.as_ref().expect("the pattern compiled");
        assert!(!re.is_match("--abc") && re.is_match("abc def"));
        assert_eq!(
            e.doc.fields[3].persist,
            crate::doc::engine::Persist::Profile
        );
        assert_eq!(e.doc.fields[3].applies, crate::doc::engine::Applies::Live);
        assert_eq!(e.doc.fields[3].ui.hint.as_deref(), Some("e.g. --threads 8"));
    }

    #[test]
    fn the_form_attributes_are_checked_where_they_are_written() {
        // a typo in a condition, and an expression that is not one
        let r = registry(
            &with_field(
                "[[field]]\nid = \"extra_args\"\ntype = \"text\"\nenabled_when = \"not session.runing\"\nrequired_when = \"os\"\n",
            ),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[extra_args].enabled_when")
                    && e.contains("`session` has no `runing`")
                    && e.contains("did you mean `session.running`?")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[extra_args].required_when")
                    && e.contains("must be a condition")),
            "{errors:?}"
        );

        // a validation rule: a typo in the check, no message at all, an empty message
        let r = registry(
            &with_field(
                "[[field]]\nid = \"extra_args\"\ntype = \"text\"\nvalidate = [ { check = \"len(valu) > 0\", message = \"x\" }, { check = \"true\", message = \" \" } ]\n",
            ),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[extra_args].validate[0].check")
                    && e.contains("unknown name `valu`")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[extra_args].validate[1].message")
                    && e.contains("needs a message")),
            "{errors:?}"
        );
        let r = registry(
            &with_field(
                "[[field]]\nid = \"extra_args\"\ntype = \"text\"\nvalidate = [ { check = \"true\" } ]\n",
            ),
            None,
        );
        assert!(
            texts(&r, Severity::Error)
                .iter()
                .any(|e| e.contains("validate") && e.contains("message")),
            "a rule with no `message` key is refused and names it: {:?}",
            r.diagnostics
        );

        // a pattern that is not one, and one on a field that cannot use it
        let r = registry(
            &with_field("[[field]]\nid = \"extra_args\"\ntype = \"text\"\npattern = \"[a-\"\n"),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[extra_args].pattern")
                    && e.contains("is not a valid pattern")),
            "{errors:?}"
        );
        let r = registry(
            &with(
                "range = [256, 131072]\n",
                "range = [256, 131072]\npattern = \"[0-9]+\"\n",
            ),
            None,
        );
        assert!(
            texts(&r, Severity::Warning)
                .iter()
                .any(|w| w.contains("field[ctx_size].pattern") && w.contains("text fields")),
            "{:?}",
            r.diagnostics
        );
        assert!(
            r.engine("llamacpp").is_some(),
            "a warning does not stop the engine"
        );

        // choices: no options, a default that is not one of them, options on a text field
        let r = registry(
            &with_field("[[field]]\nid = \"mode\"\ntype = \"choice\"\ndefault = \"auto\"\n"),
            None,
        );
        assert!(
            texts(&r, Severity::Error)
                .iter()
                .any(|e| e.contains("field[mode].type") && e.contains("has no options")),
            "{:?}",
            r.diagnostics
        );
        let r = registry(
            &with_field(
                "[[field]]\nid = \"mode\"\ntype = \"choice\"\noptions = [\"a\", \"b\"]\ndefault = \"c\"\n",
            ),
            None,
        );
        assert!(
            texts(&r, Severity::Error)
                .iter()
                .any(|e| e.contains("field[mode].default")
                    && e.contains("`c` is not one of the options (a, b)")),
            "{:?}",
            r.diagnostics
        );
        let r = registry(
            &with_field("[[field]]\nid = \"extra_args\"\ntype = \"text\"\noptions = [\"a\"]\n"),
            None,
        );
        assert!(
            texts(&r, Severity::Warning)
                .iter()
                .any(|w| w.contains("field[extra_args].options") && w.contains("`choice`")),
            "{:?}",
            r.diagnostics
        );

        // an unknown value of `applies` or `persist` is refused where it stands
        let r = registry(
            &with_field(
                "[[field]]\nid = \"extra_args\"\ntype = \"text\"\napplies = \"sometimes\"\n",
            ),
            None,
        );
        assert!(
            texts(&r, Severity::Error)
                .iter()
                .any(|e| e.contains("sometimes") && e.contains("applies")),
            "{:?}",
            r.diagnostics
        );
    }

    /// llama.cpp's `[acquire]` section as the embedded document writes it.
    const ACQUIRE: &str = r#"
[acquire]
root = "${data}/llama-bin"
feed = { use = "github_release", repo = "ggml-org/llama.cpp", per_page = 10, show = 5, prereleases = true }
assets = { by = "keywords", ext = [".zip", ".tar.gz"], exclude = ["cudart"], os = { windows = ["win"], linux = ["ubuntu", "linux"] }, arch = { x86_64 = ["x64"] }, label = "${tag}-${stem}" }
install = { use = "archive", binary = { name = "llama-server", search_depth = 4 }, chmod = true }
backends = [ { match = "cuda", label = "CUDA", needs_gpu = "nvidia" }, { match = "cpu", label = "CPU" } ]
"#;

    /// `GOOD` with an `[acquire]` section, one thing about it changed.
    fn with_acquire(old: &str, new: &str) -> String {
        assert!(ACQUIRE.contains(old), "the section has no `{old}`");
        format!("{GOOD}{}", ACQUIRE.replace(old, new))
    }

    fn errors_of(text: &str) -> Vec<String> {
        texts(&registry(text, None), Severity::Error)
    }

    fn warnings_of(text: &str) -> Vec<String> {
        texts(&registry(text, None), Severity::Warning)
    }

    #[test]
    fn a_good_acquire_section_compiles_without_complaint() {
        let r = registry(&format!("{GOOD}{ACQUIRE}"), None);
        assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
        let engine = r.engine("llamacpp").unwrap();
        let acquire = engine.doc.acquire.as_ref().expect("the section is kept");
        assert_eq!(acquire.backends.len(), 2);
        assert!(engine.acquire.as_ref().unwrap().root.is_some());
    }

    #[test]
    fn the_acquire_section_is_checked_where_it_is_written() {
        // a feed that is not a repository, and counts out of range
        let e = errors_of(&with_acquire(
            "repo = \"ggml-org/llama.cpp\"",
            "repo = \"llama.cpp\"",
        ));
        assert!(
            e.iter()
                .any(|m| m.contains("acquire.feed.repo") && m.contains("is not a repository")),
            "{e:?}"
        );
        let e = errors_of(&with_acquire("per_page = 10", "per_page = 0"));
        assert!(
            e.iter().any(|m| m.contains("acquire.feed.per_page")),
            "{e:?}"
        );
        let e = errors_of(&with_acquire("show = 5", "show = 0"));
        assert!(e.iter().any(|m| m.contains("acquire.feed.show")), "{e:?}");

        // a feed with nothing to pick from, an install with nowhere to go
        let e = errors_of(&format!(
            "{GOOD}[acquire]\nroot = \"${{data}}/x\"\nfeed = {{ use = \"github_release\", repo = \"a/b\" }}\n"
        ));
        assert!(
            e.iter().any(|m| m.contains("a feed needs `assets`")),
            "{e:?}"
        );
        let e = errors_of(&with_acquire("root = \"${data}/llama-bin\"\n", ""));
        assert!(
            e.iter().any(|m| m.contains("installing needs `root`")),
            "{e:?}"
        );
        let e = errors_of(&with_acquire("${data}/llama-bin", "${dta}/llama-bin"));
        assert!(
            e.iter()
                .any(|m| m.contains("acquire.root") && m.contains("unknown name `dta`")),
            "{e:?}"
        );

        // file endings
        let e = errors_of(&with_acquire(
            "ext = [\".zip\", \".tar.gz\"]",
            "ext = [\"zip\"]",
        ));
        assert!(
            e.iter()
                .any(|m| m.contains("acquire.assets.ext") && m.contains("write `.zip`")),
            "{e:?}"
        );
        let e = errors_of(&with_acquire("ext = [\".zip\", \".tar.gz\"]", "ext = []"));
        assert!(
            e.iter().any(|m| m.contains("`ext` lists the file endings")),
            "{e:?}"
        );

        // an OS or architecture it does not know is a warning, not a failure
        let w = warnings_of(&with_acquire(
            "linux = [\"ubuntu\", \"linux\"]",
            "plan9 = [\"p9\"]",
        ));
        assert!(
            w.iter()
                .any(|m| m.contains("acquire.assets.os.plan9") && m.contains("never used")),
            "{w:?}"
        );

        // the label may use tag, stem and asset, and nothing else
        let e = errors_of(&with_acquire("${tag}-${stem}", "${tagg}-${stem}"));
        assert!(
            e.iter()
                .any(|m| m.contains("acquire.assets.label") && m.contains("unknown name `tagg`")),
            "{e:?}"
        );

        // the executable
        let e = errors_of(&with_acquire("name = \"llama-server\"", "name = \" \""));
        assert!(
            e.iter().any(|m| m.contains("acquire.install.binary.name")),
            "{e:?}"
        );
        let e = errors_of(&with_acquire("search_depth = 4", "search_depth = 99"));
        assert!(
            e.iter()
                .any(|m| m.contains("acquire.install.binary.search_depth")),
            "{e:?}"
        );

        // backends
        let e = errors_of(&with_acquire("match = \"cuda\"", "match = \"\""));
        assert!(
            e.iter().any(|m| m.contains("acquire.backends[0].match")),
            "{e:?}"
        );
        let w = warnings_of(&with_acquire(
            "needs_gpu = \"nvidia\"",
            "needs_gpu = \"nvidlia\"",
        ));
        assert!(
            w.iter()
                .any(|m| m.contains("acquire.backends[0].needs_gpu") && m.contains("nvidlia")),
            "{w:?}"
        );
    }

    #[test]
    fn name_rules_are_conditions_checked_at_load() {
        let by_names = |rule: &str| {
            format!(
                "{GOOD}[acquire]\nroot = \"${{data}}/x\"\nfeed = {{ use = \"github_release\", repo = \"a/b\" }}\nassets = {{ by = \"names\", rules = [ {rule} ] }}\n"
            )
        };
        let r = registry(
            &by_names("{ when = \"os == 'linux'\", name = \"x.tar.zst\" }"),
            None,
        );
        assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
        let e = errors_of(&by_names("{ when = \"oss == 'linux'\", name = \"x.zip\" }"));
        assert!(
            e.iter()
                .any(|m| m.contains("acquire.assets.rules[0].when")
                    && m.contains("unknown name `oss`")),
            "{e:?}"
        );
        let e = errors_of(&by_names("{ when = \"os\", name = \"x.zip\" }"));
        assert!(e.iter().any(|m| m.contains("must be a condition")), "{e:?}");
        let e = errors_of(&by_names("{ when = \"os == 'linux'\", name = \"\" }"));
        assert!(
            e.iter().any(|m| m.contains("the file name it selects")),
            "{e:?}"
        );
        let e = errors_of(&format!(
            "{GOOD}[acquire]\nroot = \"${{data}}/x\"\nassets = {{ by = \"names\", rules = [] }}\n"
        ));
        assert!(e.iter().any(|m| m.contains("`rules` is empty")), "{e:?}");
    }

    #[test]
    fn a_broken_template_syntax_is_reported() {
        let r = registry(&with("\"${listen_port}\"]", "\"${listen_port\"]"), None);
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("launch.spawn.args") && e.contains("never closed")),
            "{errors:?}"
        );
    }

    #[test]
    fn cross_references_and_duplicates_are_errors() {
        let r = registry(
            &with("field = \"bin_override\"", "field = \"binary\""),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("names the field `binary`")),
            "{errors:?}"
        );

        let r = registry(&with("id = \"ctx_size\"", "id = \"port\""), None);
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("the field id `port` is used twice")),
            "{errors:?}"
        );

        let r = registry(
            &with("legacy_key = \"ctx_size\"", "legacy_key = \"port\""),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("`legacy_key` `port` is used by `port` and by `ctx_size`")),
            "{errors:?}"
        );

        let r = registry(&with("[[field]]\nid = \"extra_args\"", "[[field]]\nid = \"other_port\"\ntype = \"port\"\nbind.port = \"public\"\n\n[[field]]\nid = \"extra_args\""), None);
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("only one field can be the public port")),
            "{errors:?}"
        );
    }

    #[test]
    fn defaults_must_fit_their_field() {
        let r = registry(&with("default = 999", "default = \"lots\""), None);
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[n_gpu_layers].default") && e.contains("does not fit")),
            "{errors:?}"
        );
        let r = registry(&with("default = 8080", "default = 70000"), None);
        assert!(texts(&r, Severity::Error)
            .iter()
            .any(|e| e.contains("not a valid port")));
        let r = registry(&with("default = 999", "default = 5000"), None);
        assert!(texts(&r, Severity::Error)
            .iter()
            .any(|e| e.contains("outside the range")));
    }

    #[test]
    fn launch_problems_are_reported() {
        let r = registry(
            &with("strategies = [\"spawn\"]", "strategies = [\"telepathy\"]"),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("cannot launch an engine by `telepathy`")
                    && e.contains("the strategies it knows are: spawn")),
            "{errors:?}"
        );

        let r = registry(&with("timeout = \"120s\"", "timeout = \"120\""), None);
        assert!(texts(&r, Severity::Error)
            .iter()
            .any(|e| e.contains("launch.ready.timeout") && e.contains("needs a unit")));

        let r = registry(&with("path = \"/health\"", "path = \"health\""), None);
        assert!(texts(&r, Severity::Error)
            .iter()
            .any(|e| e.contains("must start with `/`")));

        let r = registry(&with("env = [ { name = \"LD_LIBRARY_PATH\", prepend = \"${runtime.dir}\", sep = \":\" } ]", "env = [ { name = \"LD_LIBRARY_PATH\" } ]"), None);
        assert!(texts(&r, Severity::Error)
            .iter()
            .any(|e| e.contains("needs `value` or `prepend`")));
    }

    #[test]
    fn a_missing_primitive_skips_the_document_and_names_it() {
        let r = registry(
            &with(
                "[\"spawn@1\", \"locate.setting@1\"]",
                "[\"spawn@1\", \"catalog.hf_gguf@1\", \"spawn@2\"]",
            ),
            None,
        );
        assert!(r.engine("llamacpp").is_none());
        let errors = texts(&r, Severity::Error);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors
            .iter()
            .any(|e| e.contains("`catalog.hf_gguf@1`") && e.contains("requires.primitives")));
        assert!(errors.iter().any(|e| e.contains("`spawn@2`")));
    }

    #[test]
    fn keys_this_version_does_not_know_are_warnings() {
        // `models` is a section of the design that no phase has built yet
        let r = registry(&format!("{GOOD}\n[models]\nneeds_server = true\n"), None);
        assert!(r.engine("llamacpp").is_some());
        let warnings = texts(&r, Severity::Warning);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("`models` is not used by this version of QuickAI"),
            "{}",
            warnings[0]
        );
        assert!(!r.has_errors());
    }

    #[test]
    fn a_bad_user_patch_falls_back_to_the_embedded_document() {
        let user = "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\n[[field]]\nid = \"ctx_size\"\nbind.arg = [\"--ctx-size\", \"${valu}\"]\n";
        let r = registry(GOOD, Some(user));
        let e = r
            .engine("llamacpp")
            .expect("the embedded document stays in force");
        assert_eq!(e.merged.contributors.len(), 1);
        assert_eq!(
            e.merged.contributors[0].file,
            "embedded:engines/llamacpp.toml"
        );
        let errors = texts(&r, Severity::Error);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with("error: user:engines/llamacpp.toml:7:"),
            "{}",
            errors[0]
        );
        assert!(
            errors[0].contains(
                "user:engines/llamacpp.toml was skipped; the document below it stays in force"
            ),
            "{}",
            errors[0]
        );
    }

    #[test]
    fn a_good_user_patch_takes_effect() {
        let user = "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\n[[field]]\nid = \"n_gpu_layers\"\ndefault = 20\n";
        let r = registry(GOOD, Some(user));
        assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
        let e = r.engine("llamacpp").unwrap();
        assert_eq!(e.merged.contributors.len(), 2);
        assert!(
            matches!(&e.fields[2].default, Some(CompiledDefault::Literal(v)) if v.as_i64() == Some(20))
        );
    }

    #[test]
    fn the_embedded_document_itself_being_broken_leaves_no_engine() {
        let r = registry(&with("\"${valu", "\"${valu"), None);
        // GOOD is fine; break it for real
        assert!(r.engine("llamacpp").is_some());
        let r = registry(
            &with(
                "type = \"port\"\ndefault = 8080",
                "type = \"port\"\ndefault = 0",
            ),
            None,
        );
        assert!(r.engine("llamacpp").is_none());
        assert!(r.has_errors());
    }

    #[test]
    fn documents_of_other_kinds_are_kept_with_a_note() {
        let e = MemorySource::new(&[(
            "embedded:catalog/starter.toml",
            "schema = 1\nkind = \"catalog\"\nid = \"starter\"\n[[model]]\nid = \"m\"\n",
        )]);
        let r = Registry::build(
            load(&[(Layer::Embedded, &e)], &LoadOptions::default()),
            &known(),
        );
        assert!(r.document(Kind::Catalog, "starter").is_some());
        assert!(texts(&r, Severity::Info)[0]
            .contains("catalog documents are not used by this version of QuickAI yet"));
        assert!(r.engine("starter").is_none());
    }

    #[test]
    fn a_wrongly_shaped_value_names_its_place() {
        let r = registry(
            &with("type = \"int\"\nrange", "type = \"quaternion\"\nrange"),
            None,
        );
        let errors = texts(&r, Severity::Error);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("field[ctx_size].type") && e.contains("unknown variant")),
            "{errors:?}"
        );
    }
}
