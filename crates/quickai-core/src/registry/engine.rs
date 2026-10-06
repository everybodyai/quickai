//! Checking an engine document and compiling it for the planner.
//!
//! Everything a document says that is an expression or a template is parsed and type-checked here,
//! when the document is loaded, against the context it will see (`context::schema`). Cross
//! references (a locate strategy naming a field, a field id used twice) are checked too. What
//! passes is turned into `Engine`, which holds the parsed forms so the planner never parses.

use std::collections::BTreeMap;
use std::time::Duration;

use regex::Regex;

use crate::config::MergedDoc;
use crate::diag::{Diagnostic, Severity};
use crate::doc::duration::parse_duration;
use crate::doc::engine::{
    Acquire, Assets, EngineDoc, FieldDefault, FieldType, Install, LocateStrategy, PortRole, Probe,
    StopPolicy,
};
use crate::docpath::DocPath;
use crate::expr::{self, funcs, Expr, Template, Type};
use crate::value::Value;

use super::context::{schema, Site};

/// How long to wait for an engine to answer, and how often to ask, when a document does not say.
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(120);
pub const DEFAULT_READY_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct Engine {
    pub id: String,
    pub doc: EngineDoc,
    /// The merged document and the files it came from, for diagnostics and provenance.
    pub merged: MergedDoc,
    pub locate: Vec<LocateStrategy>,
    pub spawn: Option<CompiledSpawn>,
    pub ready: Option<CompiledReady>,
    /// The parsed templates and conditions of `[acquire]`; its plain data is in `doc.acquire`.
    pub acquire: Option<CompiledAcquire>,
    /// In document order, one per `[[field]]`.
    pub fields: Vec<CompiledField>,
}

/// What `[acquire]` holds that is an expression or a template, parsed and checked at load.
#[derive(Clone, Debug)]
pub struct CompiledAcquire {
    /// Where installed builds live.
    pub root: Option<Template>,
    /// Names the install directory of a build: over `tag`, `stem` and `asset`.
    pub label: Template,
    /// For `by = "names"`: each rule's condition with the file name it selects.
    pub name_rules: Vec<(Expr, String)>,
}

#[derive(Clone, Debug)]
pub struct CompiledSpawn {
    pub program: Template,
    pub args: Vec<Template>,
    pub env: Vec<CompiledEnv>,
    pub group: bool,
    pub stop: StopPolicy,
}

#[derive(Clone, Debug)]
pub struct CompiledEnv {
    pub name: String,
    pub value: Option<Template>,
    pub prepend: Option<Template>,
    pub sep: String,
    pub when: Option<Expr>,
    pub locked: bool,
}

#[derive(Clone, Debug)]
pub struct CompiledReady {
    pub path: String,
    pub expect_status: u16,
    pub timeout: Duration,
    pub interval: Duration,
}

#[derive(Clone, Debug)]
pub struct CompiledField {
    pub id: String,
    pub ty: FieldType,
    pub default: Option<CompiledDefault>,
    pub arg: Option<Vec<Template>>,
    pub arg_split: bool,
    /// This field is the port clients connect to.
    pub public_port: bool,
    /// Where its bindings go among the arguments.
    pub order: i32,
    pub derive: Vec<(String, Expr)>,
    pub visible_when: Option<Expr>,
    pub enabled_when: Option<Expr>,
    pub required_when: Option<Expr>,
    pub validate: Vec<CompiledRule>,
    pub pattern: Option<Regex>,
}

/// A cross-field check, parsed: when `check` does not hold, `message` is the field's error.
#[derive(Clone, Debug)]
pub struct CompiledRule {
    pub check: Expr,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum CompiledDefault {
    Literal(Value),
    /// A call such as `recommend_ctx_size(model.size_bytes, sys.ram_bytes)`.
    Computed(Expr),
}

/// Collects diagnostics for one document, each located in its files.
struct Checker<'a> {
    merged: &'a MergedDoc,
    out: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    fn add(
        &mut self,
        severity: Severity,
        path: &DocPath,
        message: impl Into<String>,
        hint: Option<String>,
    ) {
        let mut d = self.merged.diagnostic(severity, path, message);
        if let Some(hint) = hint {
            d = d.with_hint(hint);
        }
        self.out.push(d);
    }

    fn error(&mut self, path: &DocPath, message: impl Into<String>) {
        self.add(Severity::Error, path, message, None);
    }

    fn error_hint(&mut self, path: &DocPath, message: impl Into<String>, hint: impl Into<String>) {
        self.add(Severity::Error, path, message, Some(hint.into()));
    }

    fn warn(&mut self, path: &DocPath, message: impl Into<String>) {
        self.add(Severity::Warning, path, message, None);
    }

    /// Parses and checks a template; `None` (with diagnostics) when it is not valid.
    fn template(&mut self, path: &DocPath, text: &str, schema: &Type) -> Option<Template> {
        match Template::parse(text) {
            Ok(template) => {
                let problems = template.check(schema);
                let ok = problems.is_empty();
                for p in problems {
                    self.add(Severity::Error, path, p.message, p.hint);
                }
                ok.then_some(template)
            }
            Err(e) => {
                self.error(path, format!("`{text}`: {}", e.describe(text)));
                None
            }
        }
    }

    /// Parses and checks an expression; with `condition`, it must be usable as one.
    fn expr(&mut self, path: &DocPath, text: &str, schema: &Type, condition: bool) -> Option<Expr> {
        match expr::parse(text) {
            Ok(e) => {
                let (ty, problems) = expr::check(&e, schema);
                let mut ok = problems.is_empty();
                for p in problems {
                    self.add(
                        Severity::Error,
                        path,
                        format!("in `{text}`: {}", p.message),
                        p.hint,
                    );
                }
                if ok && condition && !ty.is_condition() {
                    self.error_hint(
                        path,
                        format!("`{text}` must be a condition, but it is {}", ty.name()),
                        "use a comparison, such as `x == 'a'` or `x > 0`",
                    );
                    ok = false;
                }
                ok.then_some(e)
            }
            Err(e) => {
                self.error(path, format!("`{text}`: {}", e.describe(text)));
                None
            }
        }
    }

    fn duration(&mut self, path: &DocPath, text: &str) -> Option<Duration> {
        match parse_duration(text) {
            Ok(d) => Some(d),
            Err(message) => {
                self.error(path, message);
                None
            }
        }
    }

    fn failed(&self) -> bool {
        self.out.iter().any(Diagnostic::is_error)
    }
}

/// Checks and compiles one engine document. Warnings and errors are appended to the returned list;
/// the engine is `None` when there is any error.
pub fn compile(merged: &MergedDoc, doc: EngineDoc) -> (Option<Engine>, Vec<Diagnostic>) {
    let mut c = Checker {
        merged,
        out: Vec::new(),
    };
    let root = DocPath::default();

    if doc.engine.name.trim().is_empty() {
        c.error(
            &root.clone().key("engine").key("name"),
            "the engine needs a name",
        );
    }
    if let Some(web) = &doc.provides.web_ui {
        if !web.path.starts_with('/') {
            c.error(
                &root.clone().key("provides").key("web_ui").key("path"),
                format!("the web UI path `{}` must start with `/`", web.path),
            );
        }
    }

    let fields = compile_fields(&mut c, &doc);
    let locate = check_locate(&mut c, &doc);
    let (spawn, ready) = compile_launch(&mut c, &doc);
    let acquire = doc
        .acquire
        .as_ref()
        .map(|a| compile_acquire(&mut c, &doc, a));

    let failed = c.failed();
    let out = c.out;
    if failed {
        return (None, out);
    }
    (
        Some(Engine {
            id: merged.envelope.id.clone(),
            doc,
            merged: merged.clone(),
            locate,
            spawn,
            ready,
            acquire,
            fields,
        }),
        out,
    )
}

fn compile_fields(c: &mut Checker, doc: &EngineDoc) -> Vec<CompiledField> {
    let mut compiled = Vec::new();
    let mut seen_ids: BTreeMap<&str, usize> = BTreeMap::new();
    let mut seen_keys: BTreeMap<&str, &str> = BTreeMap::new();
    let mut public_port: Option<&str> = None;

    for field in &doc.fields {
        let at = DocPath::default().key("field").id(&field.id);
        if field.id.is_empty()
            || !field
                .id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            c.error_hint(
                &at.clone().key("id"),
                format!("`{}` is not a valid field id", field.id),
                "use letters, digits and `_`",
            );
        }
        *seen_ids.entry(&field.id).or_insert(0) += 1;
        if seen_ids[field.id.as_str()] == 2 {
            c.error(&at, format!("the field id `{}` is used twice", field.id));
        }
        if let Some(key) = &field.legacy_key {
            if let Some(other) = seen_keys.insert(key, &field.id) {
                c.error(
                    &at.clone().key("legacy_key"),
                    format!(
                        "`legacy_key` `{key}` is used by `{other}` and by `{}`",
                        field.id
                    ),
                );
            }
        }

        if let Some((lo, hi)) = field.range {
            if lo > hi {
                c.error(
                    &at.clone().key("range"),
                    format!("the range [{lo}, {hi}] is empty"),
                );
            }
        }

        let value_schema = schema(doc, Site::Field(field.ty));
        let engine_schema = schema(doc, Site::Engine);

        let default = match &field.default {
            None => None,
            Some(FieldDefault::Literal(v)) => {
                let value = Value::from_toml(v);
                check_literal(c, &at.clone().key("default"), field.ty, field.range, &value);
                Some(CompiledDefault::Literal(value))
            }
            Some(FieldDefault::Computed { func, args }) => {
                let path = at.clone().key("default");
                if funcs::find(func).is_none() {
                    c.add(
                        Severity::Error,
                        &path,
                        format!("unknown function `{func}`"),
                        crate::expr::types::suggest(func, funcs::names())
                            .map(|s| format!("did you mean `{s}`?")),
                    );
                    None
                } else {
                    let parsed: Vec<Option<Expr>> = args
                        .iter()
                        .map(|a| c.expr(&path, a, &engine_schema, false))
                        .collect();
                    if parsed.iter().all(Option::is_some) {
                        let call = Expr::Call {
                            name: func.clone(),
                            args: parsed.into_iter().flatten().collect(),
                        };
                        let (ty, problems) = expr::check(&call, &engine_schema);
                        for p in problems {
                            c.add(Severity::Error, &path, p.message, p.hint);
                        }
                        let wanted = super::context::field_value_type(field.ty);
                        if !ty.assignable_to(&wanted) {
                            c.error(
                                &path,
                                format!(
                                    "`{func}` gives {}, but the field is {}",
                                    ty.name(),
                                    wanted.name()
                                ),
                            );
                        }
                        Some(CompiledDefault::Computed(call))
                    } else {
                        None
                    }
                }
            }
        };

        let arg = field.bind.arg.as_ref().map(|templates| {
            let path = at.clone().key("bind").key("arg");
            templates
                .iter()
                .filter_map(|t| c.template(&path, t, &value_schema))
                .collect::<Vec<_>>()
        });

        if field.bind.arg_split && !matches!(field.ty, FieldType::Text | FieldType::KvLines) {
            c.error(
                &at.clone().key("bind").key("arg_split"),
                "`arg_split` needs a text field",
            );
        }

        let is_public = field.bind.port == Some(PortRole::Public);
        if field.bind.port.is_some() {
            if !matches!(field.ty, FieldType::Port | FieldType::Int) {
                c.error(
                    &at.clone().key("bind").key("port"),
                    "`bind.port` needs a field of type `port` or `int`",
                );
            }
            if is_public {
                if let Some(other) = public_port {
                    c.error(
                        &at.clone().key("bind").key("port"),
                        format!("only one field can be the public port; `{other}` already is"),
                    );
                }
                public_port = Some(&field.id);
            }
        }

        let visible_when = field
            .visible_when
            .as_deref()
            .and_then(|text| c.expr(&at.clone().key("visible_when"), text, &value_schema, true));

        let enabled_when = field
            .enabled_when
            .as_deref()
            .and_then(|text| c.expr(&at.clone().key("enabled_when"), text, &value_schema, true));
        let required_when = field
            .required_when
            .as_deref()
            .and_then(|text| c.expr(&at.clone().key("required_when"), text, &value_schema, true));

        let mut validate = Vec::new();
        for (i, rule) in field.validate.iter().enumerate() {
            let rule_at = at.clone().key("validate").index(i);
            if rule.message.trim().is_empty() {
                c.error(
                    &rule_at.clone().key("message"),
                    "a validation rule needs a message: it is what the user reads",
                );
            }
            if let Some(check) = c.expr(&rule_at.key("check"), &rule.check, &value_schema, true) {
                validate.push(CompiledRule {
                    check,
                    message: rule.message.clone(),
                });
            }
        }

        let pattern = field.pattern.as_deref().and_then(|text| {
            let path = at.clone().key("pattern");
            if matches!(
                field.ty,
                FieldType::Int | FieldType::Float | FieldType::Bool | FieldType::Port
            ) {
                c.warn(
                    &path,
                    "`pattern` applies to text fields; it has no effect here",
                );
            }
            match Regex::new(&format!("^(?:{text})$")) {
                Ok(re) => Some(re),
                Err(e) => {
                    c.error(&path, format!("`{text}` is not a valid pattern: {e}"));
                    None
                }
            }
        });

        check_options(c, &at, field);

        let derive: Vec<(String, Expr)> = field
            .derive
            .iter()
            .filter_map(|(name, text)| {
                c.expr(
                    &at.clone().key("derive").key(name),
                    text,
                    &value_schema,
                    false,
                )
                .map(|e| (name.clone(), e))
            })
            .collect();

        for trigger in &field.recompute_on {
            if trigger != "model" {
                c.warn(
                    &at.clone().key("recompute_on"),
                    format!("`recompute_on` knows `model`; `{trigger}` has no effect"),
                );
            }
        }

        compiled.push(CompiledField {
            id: field.id.clone(),
            ty: field.ty,
            default,
            arg,
            arg_split: field.bind.arg_split,
            public_port: is_public,
            order: field.arg_order(),
            derive,
            visible_when,
            enabled_when,
            required_when,
            validate,
            pattern,
        });
    }
    compiled
}

/// A `choice` field needs its options, and a default that is one of them; other types have no use
/// for options.
fn check_options(c: &mut Checker, at: &DocPath, field: &crate::doc::engine::Field) {
    let path = at.clone().key("options");
    if field.ty != FieldType::Choice {
        if !field.options.is_empty() {
            c.warn(
                &path,
                "`options` applies to `choice` fields; it has no effect here",
            );
        }
        return;
    }
    if field.options.is_empty() {
        c.error_hint(
            &at.clone().key("type"),
            format!("the choice `{}` has no options", field.id),
            "list them: options = [\"a\", \"b\"]",
        );
        return;
    }
    if let Some(FieldDefault::Literal(toml::Value::String(default))) = &field.default {
        if !field.options.contains(default) {
            c.error(
                &at.clone().key("default"),
                format!(
                    "the default `{default}` is not one of the options ({})",
                    field.options.join(", ")
                ),
            );
        }
    }
}

/// The words a build's label template may use.
fn label_schema() -> Type {
    Type::object([
        ("tag", Type::Str),
        ("stem", Type::Str),
        ("asset", Type::Str),
    ])
}

/// The default install directory of a build: the release, then the file's name without its extension.
pub const DEFAULT_LABEL: &str = "${tag}-${stem}";

const KNOWN_OS: [&str; 3] = ["linux", "windows", "macos"];
const KNOWN_ARCH: [&str; 2] = ["x86_64", "aarch64"];
const KNOWN_GPU_VENDORS: [&str; 4] = ["nvidia", "amd", "intel", "apple"];

fn compile_acquire(c: &mut Checker, doc: &EngineDoc, acquire: &Acquire) -> CompiledAcquire {
    let at = DocPath::default().key("acquire");
    let engine_schema = schema(doc, Site::Engine);

    let root = acquire
        .root
        .as_deref()
        .and_then(|text| c.template(&at.clone().key("root"), text, &engine_schema));
    if acquire.install.is_some() && acquire.root.is_none() {
        c.error(
            &at.clone().key("install"),
            "installing needs `root`: the directory the builds are kept in",
        );
    }

    if let Some(crate::doc::engine::Feed::GithubRelease {
        repo,
        per_page,
        show,
        ..
    }) = &acquire.feed
    {
        let feed_at = at.clone().key("feed");
        let mut parts = repo.split('/');
        let well_formed = matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(owner), Some(name), None)
                if !owner.is_empty() && !name.is_empty() && !repo.contains(char::is_whitespace)
        );
        if !well_formed {
            c.error_hint(
                &feed_at.clone().key("repo"),
                format!("`{repo}` is not a repository"),
                "write it as owner/name, such as ggml-org/llama.cpp",
            );
        }
        if !(1..=100).contains(per_page) {
            c.error(
                &feed_at.clone().key("per_page"),
                format!("`per_page` is {per_page}; GitHub gives 1 to 100 releases a page"),
            );
        }
        if *show == 0 {
            c.error(&feed_at.key("show"), "`show` must be at least 1");
        }
        if acquire.assets.is_none() {
            c.error(
                &at.clone().key("assets"),
                "a feed needs `assets`: the rule that says which file of a release is a build for this machine",
            );
        }
    }

    let mut name_rules = Vec::new();
    let assets_at = at.clone().key("assets");
    let label_text = match &acquire.assets {
        Some(Assets::Keywords {
            ext,
            os,
            arch,
            label,
            ..
        }) => {
            if ext.is_empty() {
                c.error(
                    &assets_at.clone().key("ext"),
                    "`ext` lists the file endings a build can have, and it is empty",
                );
            }
            for e in ext {
                if !e.starts_with('.') {
                    c.error_hint(
                        &assets_at.clone().key("ext"),
                        format!("the ending `{e}` does not start with a dot"),
                        format!("write `.{e}`"),
                    );
                }
            }
            for (table, known, name) in [
                (os.keys(), &KNOWN_OS[..], "os"),
                (arch.keys(), &KNOWN_ARCH[..], "arch"),
            ] {
                for key in table {
                    if !known.contains(&key.as_str()) {
                        c.warn(
                            &assets_at.clone().key(name).key(key),
                            format!("`{key}` is not an {name} QuickAI knows ({}); the entry is never used", known.join(", ")),
                        );
                    }
                }
            }
            label.clone()
        }
        Some(Assets::Names { rules, label }) => {
            for (i, rule) in rules.iter().enumerate() {
                let rule_at = assets_at.clone().key("rules").index(i);
                if rule.name.trim().is_empty() {
                    c.error(
                        &rule_at.clone().key("name"),
                        "a rule needs the file name it selects",
                    );
                }
                if let Some(when) = c.expr(&rule_at.key("when"), &rule.when, &engine_schema, true) {
                    name_rules.push((when, rule.name.clone()));
                }
            }
            if rules.is_empty() {
                c.error(
                    &assets_at.clone().key("rules"),
                    "`rules` is empty, so no file is ever chosen",
                );
            }
            label.clone()
        }
        None => None,
    };
    let label = label_text.as_deref().unwrap_or(DEFAULT_LABEL);
    let label = c
        .template(&assets_at.key("label"), label, &label_schema())
        .unwrap_or_else(|| Template::parse(DEFAULT_LABEL).expect("the default label parses"));

    if let Some(Install::Archive { binary, .. }) = &acquire.install {
        let install_at = at.clone().key("install").key("binary");
        if binary.name.trim().is_empty() {
            c.error(
                &install_at.clone().key("name"),
                "the executable needs a name",
            );
        }
        if binary.search_depth > 16 {
            c.error(
                &install_at.key("search_depth"),
                format!(
                    "looking {} levels down is too deep (16 at most)",
                    binary.search_depth
                ),
            );
        }
    }

    for (i, backend) in acquire.backends.iter().enumerate() {
        let backend_at = at.clone().key("backends").index(i);
        if backend.needle.trim().is_empty() {
            c.error(
                &backend_at.clone().key("match"),
                "a backend needs a word to look for",
            );
        }
        if backend.label.trim().is_empty() {
            c.error(&backend_at.clone().key("label"), "a backend needs a label");
        }
        if let Some(vendor) = &backend.needs_gpu {
            if !KNOWN_GPU_VENDORS.contains(&vendor.as_str()) {
                c.warn(
                    &backend_at.key("needs_gpu"),
                    format!(
                        "`{vendor}` is not a GPU vendor QuickAI knows ({}); no mismatch is flagged",
                        KNOWN_GPU_VENDORS.join(", ")
                    ),
                );
            }
        }
    }

    CompiledAcquire {
        root,
        label,
        name_rules,
    }
}

fn check_literal(
    c: &mut Checker,
    path: &DocPath,
    ty: FieldType,
    range: Option<(f64, f64)>,
    value: &Value,
) {
    let fits = match ty {
        FieldType::Int | FieldType::Port => value.as_i64().is_some(),
        FieldType::Float => value.as_f64().is_some(),
        FieldType::Bool => matches!(value, Value::Bool(_)),
        FieldType::Hidden => true,
        _ => matches!(value, Value::Str(_)),
    };
    if !fits {
        c.error(
            path,
            format!(
                "the default is {}, which does not fit a field of type {ty:?}",
                value.type_name()
            ),
        );
        return;
    }
    if ty == FieldType::Port {
        if let Some(port) = value.as_i64() {
            if !(1..=65535).contains(&port) {
                c.error(path, format!("{port} is not a valid port (1 to 65535)"));
            }
        }
    }
    if let (Some((lo, hi)), Some(n)) = (range, value.as_f64()) {
        if n < lo || n > hi {
            c.error(
                path,
                format!("the default {n} is outside the range [{lo}, {hi}]"),
            );
        }
    }
}

fn check_locate(c: &mut Checker, doc: &EngineDoc) -> Vec<LocateStrategy> {
    for (i, strategy) in doc.locate.strategies.iter().enumerate() {
        if let LocateStrategy::Setting { field } = strategy {
            if !doc.fields.iter().any(|f| &f.id == field) {
                c.error(
                    &DocPath::default()
                        .key("locate")
                        .key("strategies")
                        .index(i),
                    format!("the `setting` strategy names the field `{field}`, which this engine does not have"),
                );
            }
        }
    }
    doc.locate.strategies.clone()
}

fn compile_launch(
    c: &mut Checker,
    doc: &EngineDoc,
) -> (Option<CompiledSpawn>, Option<CompiledReady>) {
    let launch_path = DocPath::default().key("launch");
    let strategies: Vec<&str> = if doc.launch.strategies.is_empty() {
        if doc.launch.spawn.is_some() {
            vec!["spawn"]
        } else {
            Vec::new()
        }
    } else {
        doc.launch.strategies.iter().map(String::as_str).collect()
    };
    for s in &strategies {
        if *s != "spawn" {
            c.error_hint(
                &launch_path.clone().key("strategies"),
                format!("this version of QuickAI cannot launch an engine by `{s}`"),
                "the strategies it knows are: spawn",
            );
        }
    }

    let engine_schema = schema(doc, Site::Engine);
    let spawn = match (&doc.launch.spawn, strategies.contains(&"spawn")) {
        (None, true) => {
            c.error(
                &launch_path.clone().key("spawn"),
                "the launch strategy `spawn` needs a `[launch.spawn]` table",
            );
            None
        }
        (None, false) => None,
        (Some(spawn), _) => {
            let at = launch_path.clone().key("spawn");
            let program = c.template(&at.clone().key("program"), &spawn.program, &engine_schema);
            let args: Vec<Option<Template>> = spawn
                .args
                .iter()
                .map(|a| c.template(&at.clone().key("args"), a, &engine_schema))
                .collect();
            let mut env = Vec::new();
            for (i, entry) in spawn.env.iter().enumerate() {
                let env_at = at.clone().key("env").index(i);
                if entry.name.is_empty() || entry.name.contains('=') {
                    c.error(
                        &env_at,
                        format!("`{}` is not a valid environment variable name", entry.name),
                    );
                }
                match (&entry.value, &entry.prepend) {
                    (Some(_), Some(_)) => c.error(
                        &env_at,
                        "an environment entry has `value` or `prepend`, not both",
                    ),
                    (None, None) => {
                        c.error(&env_at, "an environment entry needs `value` or `prepend`")
                    }
                    _ => {}
                }
                let value = entry
                    .value
                    .as_deref()
                    .and_then(|t| c.template(&env_at, t, &engine_schema));
                let prepend = entry
                    .prepend
                    .as_deref()
                    .and_then(|t| c.template(&env_at, t, &engine_schema));
                let when = entry
                    .when
                    .as_deref()
                    .and_then(|t| c.expr(&env_at, t, &engine_schema, true));
                env.push(CompiledEnv {
                    name: entry.name.clone(),
                    value,
                    prepend,
                    sep: entry.sep.clone().unwrap_or_else(|| ":".to_string()),
                    when,
                    locked: entry.locked,
                });
            }
            match program {
                Some(program) if args.iter().all(Option::is_some) => Some(CompiledSpawn {
                    program,
                    args: args.into_iter().flatten().collect(),
                    env,
                    group: spawn.group,
                    stop: spawn.stop.policy,
                }),
                _ => None,
            }
        }
    };

    let ready = doc.launch.ready.as_ref().and_then(|ready| {
        let at = launch_path.clone().key("ready");
        let Probe::Http { path, expect } = &ready.probe;
        if !path.starts_with('/') {
            c.error(
                &at.clone().key("probe").key("path"),
                format!("the probe path `{path}` must start with `/`"),
            );
        }
        let timeout = match &ready.timeout {
            Some(t) => c.duration(&at.clone().key("timeout"), t),
            None => Some(DEFAULT_READY_TIMEOUT),
        };
        let interval = match &ready.interval {
            Some(t) => c.duration(&at.clone().key("interval"), t),
            None => Some(DEFAULT_READY_INTERVAL),
        };
        Some(CompiledReady {
            path: path.clone(),
            expect_status: expect.unwrap_or(200),
            timeout: timeout?,
            interval: interval?,
        })
    });
    (spawn, ready)
}
