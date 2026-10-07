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
use crate::decode::Framing;
use crate::diag::{Diagnostic, Severity};
use crate::doc::duration::parse_duration;
use crate::doc::engine::{
    Acquire, ApiInfo, Assets, CallSpec, DiagnosticRule, EngineDoc, EnvEntry, FieldDefault,
    FieldType, InputRule, Install, LocateStrategy, ModelBundle, ModelLookup, ModelMode, Models,
    OnTimeout, PortRole, Probe, Step, StopPolicy, StopSpec,
};
use crate::docpath::DocPath;
use crate::expr::{self, funcs, Expr, Template, Type};
use crate::lookup::bundle::{Bundle, Part};
use crate::lookup::present::Settings as ReadSettings;
use crate::lookup::{huggingface, ollama_library};
use crate::value::Value;

use super::context::{schema, Site};
use super::KnownPrimitives;

/// How long to wait for an engine to answer, and how often to ask, when a document does not say.
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(120);
pub const DEFAULT_READY_INTERVAL: Duration = Duration::from_millis(500);

/// How long a graceful stop waits for the program to leave before it is killed, and the shorter
/// wait while the application itself is closing.
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);
pub const DEFAULT_EXIT_GRACE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct Engine {
    pub id: String,
    pub doc: EngineDoc,
    /// The merged document and the files it came from, for diagnostics and provenance.
    pub merged: MergedDoc,
    pub locate: Vec<LocateStrategy>,
    pub spawn: Option<CompiledSpawn>,
    pub ready: Option<CompiledReady>,
    /// How the engine is brought up and taken down besides `spawn` and `ready`.
    pub launch: CompiledLaunch,
    /// The parsed templates and conditions of `[acquire]`; its plain data is in `doc.acquire`.
    pub acquire: Option<CompiledAcquire>,
    /// In document order, one per `[[field]]`.
    pub fields: Vec<CompiledField>,
    /// The calls and rules of `[models]`, for an engine that keeps its own models.
    pub models: Option<CompiledModels>,
    /// What the API panel says, its templates parsed and checked.
    pub api_info: Option<CompiledApiInfo>,
    /// The advice of `[[diagnostic]]`, its patterns compiled.
    pub diagnostics: Vec<CompiledDiagnostic>,
    /// How each kind of companion file is attached, by kind.
    pub components: Vec<CompiledComponent>,
}

/// Where a component's arguments stand when its document does not say: after the fields of a lower
/// `order`, before the person's own extra arguments (90).
pub const DEFAULT_COMPONENT_ORDER: i32 = 80;

/// `[components.<kind>]`: the arguments a companion file adds, templates over `${component.path}`.
#[derive(Clone, Debug)]
pub struct CompiledComponent {
    pub kind: String,
    pub order: i32,
    pub arg: Vec<Template>,
}

/// `[[models.mode]]`: a way the engine is started for a kind of model, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mode {
    pub id: String,
    /// What the model picker says of a model that runs so.
    pub label: String,
    pub help: String,
    /// The words added to the command line.
    pub arg: Vec<String>,
}

/// `[api_info]` with its templates parsed.
#[derive(Clone, Debug)]
pub struct CompiledApiInfo {
    pub base_url: Template,
    pub extra_urls: Vec<(String, Template)>,
    pub api_key: Option<(Template, Option<String>)>,
    pub model: Option<(Template, Option<String>)>,
    /// The id of the API whose curl example is shown.
    pub example: Option<String>,
}

/// One piece of advice: it applies when every condition it names holds.
#[derive(Clone, Debug)]
pub struct CompiledDiagnostic {
    pub log_matches: Option<Regex>,
    /// `starting` or `running`: the run ended in that state.
    pub exited_during: Option<String>,
    pub hint: Template,
    pub action: Option<String>,
}

/// What `[models]` holds that is a call, an expression or a template, parsed and checked at load.
#[derive(Clone, Debug)]
pub struct CompiledModels {
    pub needs_server: bool,
    pub list: Option<CompiledList>,
    pub pull: Option<CompiledPull>,
    /// The call that removes a model: `${ref}` is its name.
    pub remove: Option<CompiledStep>,
    pub input_rules: Vec<CompiledInputRule>,
    /// The ways the window can look models up online, in the order the document gives them.
    pub lookups: Vec<CompiledLookup>,
    /// What is fetched together with a model's weights, in the order the document gives them.
    pub bundles: Vec<Bundle>,
    /// The ways the engine is started for a kind of model, in the order the document gives them.
    pub modes: Vec<Mode>,
}

/// One way of looking models up online (`[[models.lookup]]`), checked.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledLookup {
    pub id: String,
    pub kind: LookupKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LookupKind {
    /// `use = "ollama_library"` (`lookup.ollama_library@1`): a search of the library at `site`,
    /// which has no trailing slash.
    OllamaLibrary { site: String, read: ReadSettings },
    /// `use = "huggingface_gguf"` (`lookup.huggingface_gguf@1`): GGUF repositories of the hub at
    /// `site`: how many are looked into for the plain list, for words and for each of their words.
    HuggingFace {
        site: String,
        popular: usize,
        keyword: usize,
        similar: usize,
        read: ReadSettings,
    },
}

impl LookupKind {
    pub fn site(&self) -> &str {
        match self {
            LookupKind::OllamaLibrary { site, .. } | LookupKind::HuggingFace { site, .. } => site,
        }
    }

    /// How what it found is read into entries.
    pub fn read(&self) -> &ReadSettings {
        match self {
            LookupKind::OllamaLibrary { read, .. } | LookupKind::HuggingFace { read, .. } => read,
        }
    }
}

/// The look-ups this version has, by the name `use` gives them.
pub const LOOKUP_KINDS: &[&str] = &["ollama_library", "huggingface_gguf"];

#[derive(Clone, Debug)]
pub struct CompiledList {
    pub call: CompiledStep,
    pub items: String,
    pub id: String,
    pub display: Option<String>,
    pub size_bytes: Option<String>,
    pub digest: Option<String>,
    pub capabilities: Option<String>,
    /// Where the last reply is kept.
    pub cache: Option<Template>,
}

#[derive(Clone, Debug)]
pub struct CompiledPull {
    /// The call, `${ref}` the name to pull; `call.stream` says how its reply is read.
    pub call: CompiledStep,
    pub progress: Option<crate::doc::engine::PullProgressSpec>,
}

/// One rule of the model box.
#[derive(Clone, Debug)]
pub struct CompiledInputRule {
    /// `None`: it matches whatever is left.
    pub when: Option<Expr>,
    pub action: CompiledInputAction,
}

#[derive(Clone, Debug)]
pub enum CompiledInputAction {
    /// Pull this name: a template over `input`.
    Pull(Template),
    Reject(String),
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
    /// What an install leaves out: the condition (always, when `None`) and the glob.
    pub skip_rules: Vec<(Option<Expr>, String)>,
    /// Each overlay with its rules: the condition and the file name it selects.
    pub overlay_rules: Vec<CompiledOverlay>,
}

/// An overlay of `[acquire]`, its conditions parsed.
#[derive(Clone, Debug)]
pub struct CompiledOverlay {
    pub id: String,
    pub label: String,
    pub rules: Vec<(Expr, String)>,
}

#[derive(Clone, Debug)]
pub struct CompiledSpawn {
    pub program: Template,
    pub args: Vec<Template>,
    pub env: Vec<CompiledEnv>,
    pub group: bool,
    pub stop: CompiledStop,
}

/// How to stop what was started, with its waits resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledStop {
    pub policy: StopPolicy,
    pub grace: Duration,
    pub exit_grace: Duration,
}

impl Default for CompiledStop {
    fn default() -> Self {
        CompiledStop {
            policy: StopPolicy::Kill,
            grace: Duration::ZERO,
            exit_grace: Duration::ZERO,
        }
    }
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
    pub on_timeout: OnTimeout,
}

/// The ways to bring an engine up, as the document lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchStrategy {
    /// Use an engine that already answers on its port.
    AttachIfRunning,
    Spawn,
}

/// `[launch]` beyond `spawn` and `ready`.
#[derive(Clone, Debug, Default)]
pub struct CompiledLaunch {
    pub strategies: Vec<LaunchStrategy>,
    /// The field that picks among the strategies (`mode`).
    pub strategy_field: Option<String>,
    pub attach: Option<CompiledAttach>,
    pub warm: Vec<CompiledStep>,
    pub unload: Vec<CompiledStep>,
}

#[derive(Clone, Debug)]
pub struct CompiledAttach {
    /// `127.0.0.1:${public_port}`.
    pub endpoint: Template,
    pub identity_path: String,
    pub identity_pointer: String,
    pub liveness: Option<CompiledLiveness>,
}

/// How often an attached engine is asked whether it is still there, and how many misses in a row
/// mean it is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledLiveness {
    pub every: Duration,
    pub failures: u32,
}

/// One call of the warm or unload phase.
#[derive(Clone, Debug)]
pub struct CompiledStep {
    pub when: Option<Expr>,
    pub method: String,
    pub path: Template,
    pub body: Option<CompiledBody>,
    pub timeout: Duration,
    pub best_effort: bool,
    /// How the reply is read: a warm-up or an unload is `Json`; a pull may stream.
    pub stream: Framing,
}

/// A JSON body whose texts are templates: filled in with the context when a plan is made.
#[derive(Clone, Debug)]
pub enum CompiledBody {
    Literal(serde_json::Value),
    Template(Template),
    List(Vec<CompiledBody>),
    Map(Vec<(String, CompiledBody)>),
}

impl CompiledBody {
    /// The body with every hole filled in from `context`.
    pub fn render(&self, context: &Value) -> Result<serde_json::Value, String> {
        Ok(match self {
            CompiledBody::Literal(v) => v.clone(),
            CompiledBody::Template(t) => t.render(context).map_err(|e| e.message)?.to_json(),
            CompiledBody::List(items) => serde_json::Value::Array(
                items
                    .iter()
                    .map(|i| i.render(context))
                    .collect::<Result<_, _>>()?,
            ),
            CompiledBody::Map(entries) => serde_json::Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), v.render(context)?)))
                    .collect::<Result<_, String>>()?,
            ),
        })
    }
}

/// How long a warm-up call may take when the document does not say (a big model on a slow disk),
/// and an unload.
pub const DEFAULT_WARM_TIMEOUT: Duration = Duration::from_secs(600);
pub const DEFAULT_UNLOAD_TIMEOUT: Duration = Duration::from_secs(5);
/// How often an attached engine is asked whether it is still there, and how many misses in a row
/// mean it is gone (decision C20).
pub const DEFAULT_LIVENESS_EVERY: Duration = Duration::from_secs(5);
pub const DEFAULT_LIVENESS_FAILURES: u32 = 3;

#[derive(Clone, Debug)]
pub struct CompiledField {
    pub id: String,
    pub ty: FieldType,
    pub default: Option<CompiledDefault>,
    pub arg: Option<Vec<Template>>,
    pub arg_split: bool,
    /// Variables of the engine this field decides (`bind.env`).
    pub env: Vec<CompiledEnv>,
    /// For a field of the person's own lines of environment (`bind.env_lines`): the names those
    /// lines may not set, besides the ones the document locks.
    pub env_lines: Option<Vec<String>>,
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
    /// A text with `${...}` holes over the engine's context (`${data}/ollama/models`).
    Template(Template),
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
pub fn compile(
    merged: &MergedDoc,
    doc: EngineDoc,
    known: &KnownPrimitives,
) -> (Option<Engine>, Vec<Diagnostic>) {
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

    let fields = compile_fields(&mut c, &doc, known);
    let locate = check_locate(&mut c, &doc);
    let (spawn, ready) = compile_launch(&mut c, &doc);
    let launch = compile_launch_extras(&mut c, &doc);
    let acquire = doc
        .acquire
        .as_ref()
        .map(|a| compile_acquire(&mut c, &doc, a));
    let components = compile_components(&mut c, &doc);
    let models = doc
        .models
        .as_ref()
        .map(|m| compile_models(&mut c, &doc, m, &components));
    let api_info = doc
        .api_info
        .as_ref()
        .and_then(|a| compile_api_info(&mut c, &doc, a));
    let diagnostics = compile_diagnostics(&mut c, &doc, &doc.diagnostics);

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
            launch,
            acquire,
            fields,
            models,
            api_info,
            diagnostics,
            components,
        }),
        out,
    )
}

/// One entry of an `env` list (`[[launch.spawn.env]]` or a field's `bind.env`): a name, and what
/// becomes of it. `schema` is what its templates and its `when` may name.
fn compile_env_entry(
    c: &mut Checker,
    at: &DocPath,
    entry: &EnvEntry,
    schema: &Type,
) -> CompiledEnv {
    if entry.name.is_empty() || entry.name.contains('=') {
        c.error(
            at,
            format!("`{}` is not a valid environment variable name", entry.name),
        );
    }
    match (&entry.value, &entry.prepend) {
        (Some(_), Some(_)) => c.error(
            at,
            "an environment entry has `value` or `prepend`, not both",
        ),
        (None, None) => c.error(at, "an environment entry needs `value` or `prepend`"),
        _ => {}
    }
    let value = entry
        .value
        .as_deref()
        .and_then(|t| c.template(at, t, schema));
    let prepend = entry
        .prepend
        .as_deref()
        .and_then(|t| c.template(at, t, schema));
    let when = entry
        .when
        .as_deref()
        .and_then(|t| c.expr(at, t, schema, true));
    CompiledEnv {
        name: entry.name.clone(),
        value,
        prepend,
        sep: entry.sep.clone().unwrap_or_else(|| ":".to_string()),
        when,
        locked: entry.locked,
    }
}

fn compile_fields(c: &mut Checker, doc: &EngineDoc, known: &KnownPrimitives) -> Vec<CompiledField> {
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

        check_widget(c, &at, field, known);

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
            Some(FieldDefault::Literal(toml::Value::String(text))) if text.contains("${") => {
                // a text with holes is a template over the engine's context
                let path = at.clone().key("default");
                if !matches!(
                    field.ty,
                    FieldType::Text
                        | FieldType::Dir
                        | FieldType::File
                        | FieldType::Url
                        | FieldType::KvLines
                ) {
                    c.error(
                        &path,
                        format!(
                            "a default with `${{...}}` in it is a text, but the field is {:?}",
                            field.ty
                        ),
                    );
                }
                c.template(&path, text, &engine_schema)
                    .map(CompiledDefault::Template)
            }
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

        let env: Vec<CompiledEnv> = field
            .bind
            .env
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let env_at = at.clone().key("bind").key("env").index(i);
                if entry.locked {
                    c.error_hint(
                        &env_at.clone().key("locked"),
                        "`locked` is for the document's own variables, not for one a field decides",
                        "what a field sets is the person's to change; to keep a name out of their extra environment, list it in `bind.env_lines.protect`",
                    );
                }
                compile_env_entry(c, &env_at, entry, &value_schema)
            })
            .collect();

        let env_lines = field.bind.env_lines.as_ref().map(|lines| {
            let lines_at = at.clone().key("bind").key("env_lines");
            if field.ty != FieldType::KvLines {
                c.error(
                    &lines_at,
                    "`bind.env_lines` needs a field of type `kv_lines`",
                );
            }
            for name in &lines.protect {
                if !crate::kvlines::is_name(name) {
                    c.error(
                        &lines_at.clone().key("protect"),
                        format!("`{name}` is not an environment variable name"),
                    );
                }
            }
            lines.protect.clone()
        });

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
            env,
            env_lines,
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

/// `ui.widget` names a widget the interface has (when the application said which), a `button` says
/// what a click does, and nothing else has an `action`.
fn check_widget(
    c: &mut Checker,
    at: &DocPath,
    field: &crate::doc::engine::Field,
    known: &KnownPrimitives,
) {
    let ui = at.clone().key("ui");
    if let (Some(name), Some(widgets)) = (&field.ui.widget, known.widget_names()) {
        if !widgets.contains(name) {
            let hint = match crate::expr::types::suggest(name, widgets.iter().map(String::as_str)) {
                Some(near) => format!("did you mean `{near}`?"),
                None => format!(
                    "the widgets are {}",
                    widgets.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            };
            c.error_hint(
                &ui.clone().key("widget"),
                format!("`{name}` is not a widget this version of QuickAI has"),
                hint,
            );
        }
    }
    match (field.ui.widget.as_deref(), field.ui.action.as_deref()) {
        (Some("button"), None) => c.error_hint(
            &ui.clone().key("widget"),
            format!("the button `{}` does not say what a click does", field.id),
            "add `action`: ui = { widget = \"button\", action = \"open:settings\" }",
        ),
        (Some("button"), Some(action)) if !is_action(action) => c.error_hint(
            &ui.clone().key("action"),
            format!("`{action}` is not an action id"),
            "an action is a lower case word, then optionally `:` and what it applies to: open:settings",
        ),
        (Some("button"), Some(_)) | (_, None) => {}
        (_, Some(_)) => c.error_hint(
            &ui.clone().key("action"),
            "`action` belongs to a button",
            "set `widget = \"button\"`, or take `action` out",
        ),
    }
}

/// An action id: `verb` or `verb:what`, the verb lower case letters and `_`, the rest without
/// spaces.
fn is_action(text: &str) -> bool {
    let (verb, rest) = match text.split_once(':') {
        Some((verb, rest)) => (verb, Some(rest)),
        None => (text, None),
    };
    !verb.is_empty()
        && verb.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')
        && rest.is_none_or(|r| !r.is_empty() && !r.contains(char::is_whitespace))
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
        api,
        ..
    }) = &acquire.feed
    {
        let feed_at = at.clone().key("feed");
        if let Some(api) = api {
            if !is_http_root(api) {
                c.error_hint(
                    &feed_at.clone().key("api"),
                    format!("`{api}` is not an http or https address"),
                    "write the root of the API, such as https://api.github.com",
                );
            }
        }
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

    if let Some(tested) = &acquire.tested {
        if tested.tag.trim().is_empty() {
            c.error(
                &at.clone().key("tested").key("tag"),
                "the tested release needs a tag",
            );
        }
    }
    for (name, pin) in &acquire.pins {
        let pin_at = at.clone().key("pins").key(name);
        if name.trim().is_empty() {
            c.error(&pin_at, "a pin needs the name of the file it is for");
        }
        let digest = pin.trim().strip_prefix("sha256:").unwrap_or(pin.trim());
        if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            c.error_hint(
                &pin_at,
                format!("`{pin}` is not a sha256 digest"),
                "write it as sha256: and 64 hexadecimal digits",
            );
        }
    }
    let mut overlay_rules = Vec::new();
    let mut overlay_ids: Vec<&str> = Vec::new();
    for (i, overlay) in acquire.overlays.iter().enumerate() {
        let overlay_at = at.clone().key("overlays").index(i);
        if overlay.id.is_empty()
            || !overlay
                .id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            c.error_hint(
                &overlay_at.clone().key("id"),
                format!("`{}` is not a valid overlay id", overlay.id),
                "use letters, digits and `_`",
            );
        }
        if overlay_ids.contains(&overlay.id.as_str()) {
            c.error(
                &overlay_at.clone().key("id"),
                format!("the overlay id `{}` is used twice", overlay.id),
            );
        }
        overlay_ids.push(&overlay.id);
        if overlay.rules.is_empty() {
            c.error(
                &overlay_at.clone().key("rules"),
                "an overlay with no rules is offered on no platform",
            );
        }
        let mut rules = Vec::new();
        for (j, rule) in overlay.rules.iter().enumerate() {
            let rule_at = overlay_at.clone().key("rules").index(j);
            if rule.name.trim().is_empty() {
                c.error(
                    &rule_at.clone().key("name"),
                    "a rule needs the file name it selects",
                );
            }
            if let Some(when) = c.expr(&rule_at.key("when"), &rule.when, &engine_schema, true) {
                rules.push((when, rule.name.clone()));
            }
        }
        overlay_rules.push(CompiledOverlay {
            id: overlay.id.clone(),
            label: overlay.label.clone().unwrap_or_else(|| overlay.id.clone()),
            rules,
        });
    }

    let mut skip_rules = Vec::new();
    if let Some(Install::Archive {
        binary,
        smoke,
        skip,
        ..
    }) = &acquire.install
    {
        let install_at = at.clone().key("install");
        if let Some(binary) = binary {
            let binary_at = install_at.clone().key("binary");
            if binary.name.trim().is_empty() {
                c.error(
                    &binary_at.clone().key("name"),
                    "the executable needs a name",
                );
            }
            if binary.search_depth > 16 {
                c.error(
                    &binary_at.key("search_depth"),
                    format!(
                        "looking {} levels down is too deep (16 at most)",
                        binary.search_depth
                    ),
                );
            }
        }
        if let Some(smoke) = smoke {
            if smoke.first().map(String::as_str) != Some("${binary}") {
                c.error_hint(
                    &install_at.clone().key("smoke"),
                    "a smoke test runs the executable that was just unpacked, so it starts with it",
                    "write it as smoke = [\"${binary}\", \"--version\"]",
                );
            }
        }
        for (i, rule) in skip.iter().enumerate() {
            let rule_at = install_at.clone().key("skip").index(i);
            let glob = rule.glob.trim();
            if glob.is_empty() || glob.starts_with('/') || glob.split('/').any(|part| part == "..")
            {
                c.error_hint(
                    &rule_at.clone().key("glob"),
                    format!("`{}` is not a path inside a build", rule.glob),
                    "write it relative to the build's directory, such as lib/cuda_*",
                );
            }
            let when = match &rule.when {
                Some(text) => c.expr(&rule_at.key("when"), text, &engine_schema, true),
                None => None,
            };
            if rule.when.is_none() || when.is_some() {
                skip_rules.push((when, glob.to_string()));
            }
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
        skip_rules,
        overlay_rules,
    }
}

/// The root of an HTTP API: `http://` or `https://` and a host, nothing else.
fn is_http_root(text: &str) -> bool {
    let rest = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"));
    rest.is_some_and(|rest| {
        !rest.is_empty() && !rest.starts_with('/') && !rest.contains(char::is_whitespace)
    })
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
    let engine_schema = schema(doc, Site::Engine);
    for (i, strategy) in doc.locate.strategies.iter().enumerate() {
        let at = DocPath::default().key("locate").key("strategies").index(i);
        match strategy {
            LocateStrategy::Setting { field } => {
                if !doc.fields.iter().any(|f| &f.id == field) {
                    c.error(
                        &at,
                        format!("the `setting` strategy names the field `{field}`, which this engine does not have"),
                    );
                }
            }
            LocateStrategy::Installed { root, binary, .. } => {
                c.template(&at.clone().key("root"), root, &engine_schema);
                if binary.is_empty() {
                    c.error_hint(
                        &at.clone().key("binary"),
                        "an installed build needs the path of its executable",
                        "write it per system: binary = { linux = \"bin/tool\", windows = \"tool.exe\" }",
                    );
                }
                for (os, relative) in binary {
                    let here = at.clone().key("binary").key(os);
                    if !KNOWN_OS.contains(&os.as_str()) {
                        c.warn(
                            &here,
                            format!("`{os}` is not a system QuickAI knows ({}); the entry is never used", KNOWN_OS.join(", ")),
                        );
                    }
                    if relative.trim().is_empty()
                        || relative.starts_with('/')
                        || relative.starts_with('\\')
                        || relative.contains(':')
                        || relative.split(['/', '\\']).any(|part| part == "..")
                    {
                        c.error_hint(
                            &here,
                            format!("`{relative}` is not a path inside a build"),
                            "write it relative to the build's directory, such as bin/tool",
                        );
                    }
                }
            }
            LocateStrategy::PathSearch { name } => {
                if name.trim().is_empty() || name.contains(['/', '\\']) {
                    c.error_hint(
                        &at.key("name"),
                        format!("`{name}` is not a program name"),
                        "write the file's name only, such as ollama: the search looks in each directory of the PATH",
                    );
                }
            }
            LocateStrategy::KnownPaths {
                linux,
                macos,
                windows,
            } => {
                if linux.is_empty() && macos.is_empty() && windows.is_empty() {
                    c.error(&at.clone(), "`known_paths` lists no place at all");
                }
                for (os, paths) in [("linux", linux), ("macos", macos), ("windows", windows)] {
                    for (j, path) in paths.iter().enumerate() {
                        if path.trim().is_empty() {
                            c.error(
                                &at.clone().key(os).index(j),
                                "a known place cannot be empty",
                            );
                        }
                    }
                }
            }
            LocateStrategy::NextToExe { .. } | LocateStrategy::DevTree { .. } => {}
        }
    }
    check_runtime(c, doc);
    doc.locate.strategies.clone()
}

/// `[runtime] version`: the command starts with the executable, the pattern compiles and has a
/// group to read, and it fills a value this build has.
fn check_runtime(c: &mut Checker, doc: &EngineDoc) {
    let Some(probe) = doc.runtime.as_ref().and_then(|r| r.version.as_ref()) else {
        return;
    };
    let at = DocPath::default().key("runtime").key("version");
    if probe.exec.first().map(String::as_str) != Some("${binary}") {
        c.error_hint(
            &at.clone().key("exec"),
            "the version is read by running the executable, so the command starts with it",
            "write it as exec = [\"${binary}\", \"--version\"]",
        );
    }
    match Regex::new(&probe.regex) {
        Ok(re) if re.captures_len() < 2 => c.error_hint(
            &at.clone().key("regex"),
            format!("`{}` has no group to read the version from", probe.regex),
            "put the version in parentheses, such as version is (\\S+)",
        ),
        Ok(_) => {}
        Err(e) => c.error(
            &at.clone().key("regex"),
            format!("`{}` is not a valid pattern: {e}", probe.regex),
        ),
    }
    if probe.name != "version" {
        c.error_hint(
            &at.key("as"),
            format!(
                "`{}` is not something this version of QuickAI reads",
                probe.name
            ),
            "the one it knows is version",
        );
    }
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
        if !["spawn", "attach_if_running"].contains(s) {
            c.error_hint(
                &launch_path.clone().key("strategies"),
                format!("this version of QuickAI cannot launch an engine by `{s}`"),
                "the strategies it knows are: attach_if_running, spawn",
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
            let env: Vec<CompiledEnv> = spawn
                .env
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    compile_env_entry(c, &at.clone().key("env").index(i), entry, &engine_schema)
                })
                .collect();
            let stop = compile_stop(c, &at.clone().key("stop"), &spawn.stop);
            match (program, stop) {
                (Some(program), Some(stop)) if args.iter().all(Option::is_some) => {
                    Some(CompiledSpawn {
                        program,
                        args: args.into_iter().flatten().collect(),
                        env,
                        group: spawn.group,
                        stop,
                    })
                }
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
            on_timeout: ready.on_timeout,
        })
    });
    (spawn, ready)
}

/// `[launch]` beyond `spawn` and `ready`: the strategies and what chooses among them, attaching to an
/// engine that is already running, and the calls that load and unload a model.
fn compile_launch_extras(c: &mut Checker, doc: &EngineDoc) -> CompiledLaunch {
    let launch_path = DocPath::default().key("launch");
    let engine_schema = schema(doc, Site::Engine);
    let mut launch = CompiledLaunch::default();

    // the strategies, in the document's order (a document that lists none has `spawn` if it has one)
    let named: Vec<&str> = if doc.launch.strategies.is_empty() {
        if doc.launch.spawn.is_some() {
            vec!["spawn"]
        } else {
            Vec::new()
        }
    } else {
        doc.launch.strategies.iter().map(String::as_str).collect()
    };
    for name in named {
        match name {
            "spawn" => launch.strategies.push(LaunchStrategy::Spawn),
            "attach_if_running" => launch.strategies.push(LaunchStrategy::AttachIfRunning),
            _ => {} // reported by `compile_launch`
        }
    }

    // attach
    let attach_at = launch_path.clone().key("attach");
    match (
        &doc.launch.attach,
        launch.strategies.contains(&LaunchStrategy::AttachIfRunning),
    ) {
        (None, true) => c.error(
            &attach_at,
            "the launch strategy `attach_if_running` needs a `[launch.attach]` table",
        ),
        (Some(attach), _) => {
            let endpoint = c.template(
                &attach_at.clone().key("endpoint"),
                &attach.endpoint,
                &engine_schema,
            );
            for (key, path) in [
                ("identity", &attach.identity.path),
                (
                    "liveness",
                    attach
                        .liveness
                        .as_ref()
                        .map_or(&attach.identity.path, |l| &l.path),
                ),
            ] {
                if !path.starts_with('/') {
                    c.error(
                        &attach_at.clone().key(key).key("path"),
                        format!("the path `{path}` must start with `/`"),
                    );
                }
            }
            if !attach.identity.pointer.starts_with('/') {
                c.error_hint(
                    &attach_at.clone().key("identity").key("pointer"),
                    format!("`{}` is not a JSON pointer", attach.identity.pointer),
                    "write it as /version: a pointer starts with a slash",
                );
            }
            let liveness = match &attach.liveness {
                None => None,
                Some(l) => {
                    let every = match &l.every {
                        Some(text) => {
                            c.duration(&attach_at.clone().key("liveness").key("every"), text)
                        }
                        None => Some(DEFAULT_LIVENESS_EVERY),
                    };
                    let failures = l.failures.unwrap_or(DEFAULT_LIVENESS_FAILURES);
                    if failures == 0 {
                        c.error(
                            &attach_at.clone().key("liveness").key("failures"),
                            "`failures` is how many misses in a row mean the engine is gone, and it must be at least 1",
                        );
                    }
                    every.map(|every| CompiledLiveness {
                        every,
                        failures: failures.max(1),
                    })
                }
            };
            if let Some(endpoint) = endpoint {
                launch.attach = Some(CompiledAttach {
                    endpoint,
                    identity_path: attach.identity.path.clone(),
                    identity_pointer: attach.identity.pointer.clone(),
                    liveness,
                });
            }
        }
        (None, false) => {}
    }

    // the field that chooses
    if let Some(field_id) = &doc.launch.strategy_field {
        let at = launch_path.clone().key("strategy_field");
        match doc.fields.iter().find(|f| &f.id == field_id) {
            None => c.error(
                &at,
                format!("`strategy_field` names the field `{field_id}`, which this engine does not have"),
            ),
            Some(field) if field.ty != FieldType::Choice => c.error(
                &at,
                format!("the field `{field_id}` is not a `choice`, so it cannot pick among the strategies"),
            ),
            Some(field) => {
                for option in &field.options {
                    let wanted = match option.as_str() {
                        "auto" => None,
                        "attach" => Some(LaunchStrategy::AttachIfRunning),
                        "spawn" => Some(LaunchStrategy::Spawn),
                        other => {
                            c.error_hint(
                                &at,
                                format!("the option `{other}` of `{field_id}` is not one of the ways to launch"),
                                "the options of a strategy field are auto, attach and spawn",
                            );
                            continue;
                        }
                    };
                    if let Some(wanted) = wanted {
                        if !launch.strategies.contains(&wanted) {
                            c.error(
                                &at,
                                format!("the option `{option}` of `{field_id}` needs the strategy {wanted:?}, which `[launch] strategies` does not list"),
                            );
                        }
                    }
                }
                launch.strategy_field = Some(field_id.clone());
            }
        }
    }

    // warm and unload
    for (phase, steps, default_timeout) in [
        ("warm", &doc.launch.warm, DEFAULT_WARM_TIMEOUT),
        ("unload", &doc.launch.unload, DEFAULT_UNLOAD_TIMEOUT),
    ] {
        for (i, step) in steps.iter().enumerate() {
            let at = launch_path.clone().key(phase).index(i);
            if let Some(compiled) = compile_step(c, &at, step, default_timeout, &engine_schema) {
                if phase == "warm" {
                    launch.warm.push(compiled);
                } else {
                    launch.unload.push(compiled);
                }
            }
        }
    }
    launch
}

fn compile_step(
    c: &mut Checker,
    at: &DocPath,
    step: &Step,
    default_timeout: Duration,
    schema: &Type,
) -> Option<CompiledStep> {
    // a `when` that is a map (`model`) is true when it is there
    let when = step
        .when
        .as_deref()
        .map(|text| c.expr(&at.clone().key("when"), text, schema, true));
    let call = compile_call(
        c,
        &at.clone().key("call"),
        &step.call,
        default_timeout,
        schema,
        false,
    );
    Some(CompiledStep {
        when: when.map_or(Some(None), |w| w.map(Some))?,
        ..call?
    })
}

/// One call: its method, its path and body as templates over `schema`, how long it may take and
/// how its reply is read. `streams` says whether the reply may be a stream (the calls of
/// `[models]`) or is always one body (a warm-up, an unload).
fn compile_call(
    c: &mut Checker,
    call_at: &DocPath,
    call: &CallSpec,
    default_timeout: Duration,
    schema: &Type,
    streams: bool,
) -> Option<CompiledStep> {
    let CallSpec {
        method,
        path,
        body,
        timeout,
        best_effort,
        stream,
    } = call;
    let method = method
        .clone()
        .unwrap_or_else(|| "POST".to_string())
        .to_ascii_uppercase();
    if !["GET", "POST", "PUT", "DELETE", "PATCH"].contains(&method.as_str()) {
        c.error(
            &call_at.clone().key("method"),
            format!("`{method}` is not a method a call uses (GET, POST, PUT, DELETE or PATCH)"),
        );
    }
    if !path.starts_with('/') {
        c.error(
            &call_at.clone().key("path"),
            format!("the path `{path}` must start with `/`"),
        );
    }
    let path_template = c.template(&call_at.clone().key("path"), path, schema);
    let body = body
        .as_ref()
        .map(|b| compile_body(c, &call_at.clone().key("body"), b, schema));
    let timeout = match timeout {
        Some(text) => c.duration(&call_at.clone().key("timeout"), text),
        None => Some(default_timeout),
    };
    let framing = match (stream.as_deref(), streams) {
        (None | Some("json"), _) => Some(Framing::Json),
        (Some("ndjson"), true) => Some(Framing::Ndjson),
        (Some("sse"), true) => Some(Framing::Sse),
        (Some("ndjson" | "sse"), false) => {
            c.error_hint(
                &call_at.clone().key("stream"),
                "a warm-up or an unload is read whole",
                "`stream` is for the calls of `[models]`",
            );
            None
        }
        (Some(other), _) => {
            c.error_hint(
                &call_at.clone().key("stream"),
                format!("`{other}` is not a way to read a reply"),
                "the ways are json, ndjson and sse",
            );
            None
        }
    };
    Some(CompiledStep {
        when: None,
        method,
        path: path_template?,
        body: match body {
            Some(b) => Some(b?),
            None => None,
        },
        timeout: timeout?,
        best_effort: *best_effort,
        stream: framing?,
    })
}

fn compile_api_info(c: &mut Checker, doc: &EngineDoc, info: &ApiInfo) -> Option<CompiledApiInfo> {
    let at = DocPath::default().key("api_info");
    let engine_schema = schema(doc, Site::Engine);
    let base_url = c.template(&at.clone().key("base_url"), &info.base_url, &engine_schema);
    let extra_urls: Vec<Option<(String, Template)>> = info
        .extra_urls
        .iter()
        .enumerate()
        .map(|(i, extra)| {
            let url_at = at.clone().key("extra_urls").index(i).key("url");
            c.template(&url_at, &extra.url, &engine_schema)
                .map(|t| (extra.label.clone(), t))
        })
        .collect();
    let note = |c: &mut Checker, key: &str, note: &Option<crate::doc::engine::ApiNote>| {
        note.as_ref().map(|n| {
            c.template(&at.clone().key(key).key("value"), &n.value, &engine_schema)
                .map(|t| (t, n.note.clone()))
        })
    };
    let api_key = note(c, "api_key", &info.api_key);
    let model = note(c, "model", &info.model);
    Some(CompiledApiInfo {
        base_url: base_url?,
        extra_urls: extra_urls.into_iter().collect::<Option<Vec<_>>>()?,
        api_key: match api_key {
            Some(k) => Some(k?),
            None => None,
        },
        model: match model {
            Some(m) => Some(m?),
            None => None,
        },
        example: info.example.clone(),
    })
}

fn compile_diagnostics(
    c: &mut Checker,
    doc: &EngineDoc,
    rules: &[DiagnosticRule],
) -> Vec<CompiledDiagnostic> {
    let engine_schema = schema(doc, Site::Engine);
    let mut compiled = Vec::new();
    for (i, rule) in rules.iter().enumerate() {
        let at = DocPath::default().key("diagnostic").index(i);
        let when_at = at.clone().key("when");
        if rule.when.log_matches.is_none() && rule.when.exited_during.is_none() {
            c.error_hint(
                &when_at,
                "a diagnostic says when it applies",
                "`when = { log_matches = '...' }` or `when = { exited_during = \"starting\" }`",
            );
        }
        let log_matches = rule.when.log_matches.as_deref().map(|pattern| {
            Regex::new(pattern).map_err(|e| {
                c.error(
                    &when_at.clone().key("log_matches"),
                    format!("`{pattern}` is not a valid pattern: {e}"),
                );
            })
        });
        let exited_during = rule.when.exited_during.as_deref().map(|state| {
            if ["starting", "running"].contains(&state) {
                Ok(state.to_string())
            } else {
                c.error_hint(
                    &when_at.clone().key("exited_during"),
                    format!("`{state}` is not a state a run ends in"),
                    "the states are starting and running",
                );
                Err(())
            }
        });
        if let Some(action) = &rule.action {
            if !is_action(action) {
                c.error_hint(
                    &at.clone().key("action"),
                    format!("`{action}` is not an action id"),
                    "an action is a lower case word, then optionally `:` and what it applies to: focus_field:port",
                );
            }
        }
        let hint = c.template(&at.clone().key("hint"), &rule.hint, &engine_schema);
        if let (Some(hint), Some(log_matches), Some(exited_during)) = (
            hint,
            log_matches.map_or(Some(None), |r| r.ok().map(Some)),
            exited_during.map_or(Some(None), |r| r.ok().map(Some)),
        ) {
            compiled.push(CompiledDiagnostic {
                log_matches,
                exited_during,
                hint,
                action: rule.action.clone(),
            });
        }
    }
    compiled
}

/// How long the calls of `[models]` may take when the document does not say.
const DEFAULT_MODEL_CALL_TIMEOUT: Duration = Duration::from_secs(30);

fn compile_models(
    c: &mut Checker,
    doc: &EngineDoc,
    models: &Models,
    components: &[CompiledComponent],
) -> CompiledModels {
    let at = DocPath::default().key("models");
    let engine_schema = schema(doc, Site::Engine);
    // `${ref}` is the name of the model a call is about
    let ref_schema = Type::object([("ref", Type::Str)]);
    let pointer = |c: &mut Checker, path: DocPath, text: &str| {
        let ok = text.starts_with('/');
        if !ok {
            c.error_hint(
                &path,
                format!("`{text}` is not a JSON pointer"),
                "a pointer starts with a slash: /models",
            );
        }
        ok
    };

    let list = models.list.as_ref().map(|list| {
        let at = at.clone().key("list");
        let call = compile_call(
            c,
            &at.clone().key("call"),
            &list.call,
            DEFAULT_MODEL_CALL_TIMEOUT,
            &engine_schema,
            false,
        );
        let map_at = at.clone().key("map");
        let mut ok = pointer(c, at.clone().key("items"), &list.items);
        ok &= pointer(c, map_at.clone().key("id"), &list.map.id);
        for (key, value) in [
            ("display", &list.map.display),
            ("size_bytes", &list.map.size_bytes),
            ("digest", &list.map.digest),
            ("capabilities", &list.map.capabilities),
        ] {
            if let Some(p) = value {
                ok &= pointer(c, map_at.clone().key(key), p);
            }
        }
        let cache = list
            .cache
            .as_deref()
            .map(|t| c.template(&at.clone().key("cache"), t, &engine_schema));
        if !ok {
            return None;
        }
        Some(CompiledList {
            call: call?,
            items: list.items.clone(),
            id: list.map.id.clone(),
            display: list.map.display.clone(),
            size_bytes: list.map.size_bytes.clone(),
            digest: list.map.digest.clone(),
            capabilities: list.map.capabilities.clone(),
            cache: match cache {
                Some(t) => Some(t?),
                None => None,
            },
        })
    });

    let pull = models.pull.as_ref().map(|pull| {
        let at = at.clone().key("pull");
        let call = compile_call(
            c,
            &at.clone().key("call"),
            &pull.call,
            // the stream's own silence limit is what bounds a pull
            Duration::from_secs(120),
            &ref_schema,
            true,
        );
        let mut ok = true;
        if let Some(p) = &pull.progress {
            let progress_at = at.clone().key("progress");
            for (key, value) in [
                ("key", &p.key),
                ("completed", &p.completed),
                ("total", &p.total),
                ("status", &p.status),
            ] {
                ok &= pointer(c, progress_at.clone().key(key), value);
            }
            if let Some(error) = &p.error {
                ok &= pointer(c, progress_at.clone().key("error"), error);
            }
        }
        if !ok {
            return None;
        }
        Some(CompiledPull {
            call: call?,
            progress: pull.progress.clone(),
        })
    });

    let remove = models.remove.as_ref().and_then(|remove| {
        compile_call(
            c,
            &at.clone().key("remove").key("call"),
            &remove.call,
            DEFAULT_MODEL_CALL_TIMEOUT,
            &ref_schema,
            false,
        )
    });

    if let Some(capabilities) = &models.capabilities {
        let at = at.clone().key("capabilities");
        compile_call(
            c,
            &at.clone().key("call"),
            &capabilities.call,
            DEFAULT_MODEL_CALL_TIMEOUT,
            &ref_schema,
            false,
        );
        pointer(c, at.key("pointer"), &capabilities.pointer);
    }

    CompiledModels {
        needs_server: models.needs_server,
        list: list.flatten(),
        pull: pull.flatten(),
        remove,
        input_rules: compile_input_rules(c, &at, &models.input_rules),
        lookups: compile_lookups(c, &at, &models.lookup),
        bundles: compile_bundles(c, &at, &models.bundle, components),
        modes: compile_modes(c, &at, &models.mode),
    }
}

/// `[components.<kind>]`: each kind is a name, and says which arguments it adds, with templates that
/// only name the file (`${component.path}`).
fn compile_components(c: &mut Checker, doc: &EngineDoc) -> Vec<CompiledComponent> {
    let schema = Type::object([("component", Type::object([("path", Type::Str)]))]);
    let mut compiled = Vec::new();
    for (kind, component) in &doc.components {
        let at = DocPath::default().key("components").key(kind);
        if kind.is_empty()
            || !kind
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "._-".contains(ch))
        {
            c.error_hint(
                &at,
                format!("`{kind}` is not a name for a kind of component"),
                "lower case letters, digits and . _ -, such as `mmproj`",
            );
            continue;
        }
        if component.bind.arg.is_empty() {
            c.error_hint(
                &at.clone().key("bind"),
                format!("the component `{kind}` does not say how it reaches the program"),
                "bind.arg = [\"--mmproj\", \"${component.path}\"]",
            );
            continue;
        }
        let arg: Vec<Template> = component
            .bind
            .arg
            .iter()
            .enumerate()
            .filter_map(|(i, text)| {
                c.template(&at.clone().key("bind").key("arg").index(i), text, &schema)
            })
            .collect();
        if arg.len() == component.bind.arg.len() {
            compiled.push(CompiledComponent {
                kind: kind.clone(),
                order: component.order.unwrap_or(DEFAULT_COMPONENT_ORDER),
                arg,
            });
        }
    }
    compiled
}

/// `[[models.bundle]]`: each is named, has parts, and every part is a kind of component the engine
/// can attach, with patterns that say which files it is. A bundle that cannot be attached would
/// download files the program is never told about, so it is an error.
/// `[[models.mode]]`: each is a name with the words it adds to the command line.
fn compile_modes(c: &mut Checker, at: &DocPath, modes: &[ModelMode]) -> Vec<Mode> {
    let mut compiled: Vec<Mode> = Vec::new();
    for (i, mode) in modes.iter().enumerate() {
        let at = at.clone().key("mode").index(i);
        if mode.id.is_empty()
            || !mode
                .id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "._-".contains(ch))
        {
            c.error_hint(
                &at.clone().key("id"),
                format!("`{}` is not a name for a mode", mode.id),
                "lower case letters, digits and . _ -, such as `embedding`",
            );
            continue;
        }
        if compiled.iter().any(|m| m.id == mode.id) {
            c.error(
                &at.clone().key("id"),
                format!("two modes are called `{}`", mode.id),
            );
            continue;
        }
        if mode.arg.is_empty() {
            c.error_hint(
                &at.clone().key("arg"),
                format!("the mode `{}` adds nothing to the command line", mode.id),
                "arg = [\"--embeddings\"]",
            );
            continue;
        }
        if let Some(n) = mode.arg.iter().position(|a| a.trim().is_empty()) {
            c.error(
                &at.clone().key("arg").index(n),
                "an empty word is not an argument".to_string(),
            );
            continue;
        }
        compiled.push(Mode {
            id: mode.id.clone(),
            label: mode
                .label
                .clone()
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| mode.id.clone()),
            help: mode.help.clone().unwrap_or_default(),
            arg: mode.arg.clone(),
        });
    }
    compiled
}

fn compile_bundles(
    c: &mut Checker,
    at: &DocPath,
    bundles: &[ModelBundle],
    components: &[CompiledComponent],
) -> Vec<Bundle> {
    let kinds: Vec<&str> = components.iter().map(|k| k.kind.as_str()).collect();
    let mut compiled: Vec<Bundle> = Vec::new();
    for (i, bundle) in bundles.iter().enumerate() {
        let at = at.clone().key("bundle").index(i);
        if bundle.id.is_empty() || bundle.id.contains(char::is_whitespace) {
            c.error_hint(
                &at.clone().key("id"),
                format!("`{}` is not a name for a bundle", bundle.id),
                "a name has no spaces, such as `vision`",
            );
            continue;
        }
        if compiled.iter().any(|b| b.id == bundle.id) {
            c.error(
                &at.clone().key("id"),
                format!("two bundles are called `{}`", bundle.id),
            );
            continue;
        }
        if bundle.parts.is_empty() {
            c.error_hint(
                &at.clone().key("parts"),
                format!("the bundle `{}` has no parts", bundle.id),
                "parts = [ { kind = \"mmproj\", match = [\"mmproj*.gguf\"] } ]",
            );
            continue;
        }
        let mut parts = Vec::new();
        let mut ok = true;
        for (k, part) in bundle.parts.iter().enumerate() {
            let at = at.clone().key("parts").index(k);
            if !kinds.contains(&part.kind.as_str()) {
                c.error_hint(
                    &at.clone().key("kind"),
                    format!("this engine has no component called `{}`", part.kind),
                    if kinds.is_empty() {
                        "the engine document needs a [components.<kind>] with bind.arg for it"
                            .to_string()
                    } else {
                        format!("the components it has: {}", kinds.join(", "))
                    },
                );
                ok = false;
            }
            if part.matches.is_empty() {
                c.error_hint(
                    &at.clone().key("match"),
                    "a part says which files it is".to_string(),
                    "match = [\"mmproj*.gguf\"]",
                );
                ok = false;
            }
            for (list_key, list) in [("match", &part.matches), ("prefer", &part.prefer)] {
                for (n, entry) in list.iter().enumerate() {
                    if entry.trim().is_empty() {
                        c.error(
                            &at.clone().key(list_key).index(n),
                            format!("an empty entry in `{list_key}` matches nothing"),
                        );
                        ok = false;
                    }
                }
            }
            parts.push(Part {
                kind: part.kind.clone(),
                label: part
                    .label
                    .clone()
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| part.kind.clone()),
                matches: part.matches.iter().map(|m| m.trim().to_string()).collect(),
                prefer: part.prefer.iter().map(|m| m.trim().to_string()).collect(),
            });
        }
        if ok {
            compiled.push(Bundle {
                id: bundle.id.clone(),
                label: bundle
                    .label
                    .clone()
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| bundle.id.clone()),
                help: bundle.help.clone().unwrap_or_default(),
                parts,
            });
        }
    }
    compiled
}

/// The look-ups of `[[models.lookup]]`: each is named, is one this version has, and has the keys
/// that look-up takes. A look-up that is wrong is an error (the document is refused, as for any
/// other broken section), and a key it does not take is a warning, as everywhere in a document.
fn compile_lookups(c: &mut Checker, at: &DocPath, lookups: &[ModelLookup]) -> Vec<CompiledLookup> {
    let mut compiled: Vec<CompiledLookup> = Vec::new();
    for (i, lookup) in lookups.iter().enumerate() {
        let at = at.clone().key("lookup").index(i);
        if lookup.id.is_empty() || lookup.id.contains(char::is_whitespace) {
            c.error_hint(
                &at.clone().key("id"),
                format!("`{}` is not a name for a look-up", lookup.id),
                "a name has no spaces, such as `library`",
            );
            continue;
        }
        if compiled.iter().any(|l| l.id == lookup.id) {
            c.error(
                &at.clone().key("id"),
                format!("two look-ups are called `{}`", lookup.id),
            );
            continue;
        }
        let (default_site, keys, base): (&str, &[&str], ReadSettings) = match lookup.kind.as_str() {
            "ollama_library" => (
                ollama_library::SITE,
                &["site", "bytes_per_parameter", "show_variants", "types"],
                ReadSettings {
                    show_variants: 16,
                    ..ReadSettings::default()
                },
            ),
            "huggingface_gguf" => (
                huggingface::SITE,
                &[
                    "site",
                    "popular",
                    "keyword",
                    "similar",
                    "prefer",
                    "skip",
                    "show_variants",
                    "types",
                ],
                ReadSettings {
                    prefer: [
                        "Q4_K_M", "Q4_K_S", "Q5_K_M", "Q4_0", "Q8_0", "Q6_K", "Q3_K_M",
                    ]
                    .map(String::from)
                    .to_vec(),
                    skip: ["*-?????-of-?????*", "mtp/*", "mtp-*", "imatrix*"]
                        .map(String::from)
                        .to_vec(),
                    show_variants: 4,
                    ..ReadSettings::default()
                },
            ),
            other => {
                c.error_hint(
                    &at.clone().key("use"),
                    format!("`{other}` is not a look-up this version has"),
                    format!("the look-ups it has: {}", LOOKUP_KINDS.join(", ")),
                );
                continue;
            }
        };
        for key in lookup.settings.keys() {
            if !keys.contains(&key.as_str()) {
                c.warn(
                    &at.clone().key(key),
                    format!(
                        "`{key}` is not a key of the look-up `{}` ({}); it is ignored",
                        lookup.kind,
                        keys.join(", ")
                    ),
                );
            }
        }
        let site = match lookup.settings.get("site") {
            None => default_site.to_string(),
            Some(toml::Value::String(text)) if is_http_root(text.trim()) => {
                text.trim().trim_end_matches('/').to_string()
            }
            Some(other) => {
                c.error_hint(
                    &at.clone().key("site"),
                    format!("`{other}` is not an http or https address"),
                    format!("write the address of the site, such as {default_site}"),
                );
                continue;
            }
        };
        let Some(read) = read_settings(c, &at, lookup, base) else {
            continue;
        };
        let kind = match lookup.kind.as_str() {
            "ollama_library" => LookupKind::OllamaLibrary { site, read },
            _ => {
                let count = |c: &mut Checker, key: &str, default: usize| -> Option<usize> {
                    match lookup.settings.get(key) {
                        None => Some(default),
                        Some(toml::Value::Integer(n)) if (1..=50).contains(n) => Some(*n as usize),
                        Some(other) => {
                            c.error_hint(
                                &at.clone().key(key),
                                format!("`{other}` is not a number of repositories"),
                                "a whole number from 1 to 50",
                            );
                            None
                        }
                    }
                };
                let (Some(popular), Some(keyword), Some(similar)) = (
                    count(c, "popular", 30),
                    count(c, "keyword", 20),
                    count(c, "similar", 20),
                ) else {
                    continue;
                };
                LookupKind::HuggingFace {
                    site,
                    popular,
                    keyword,
                    similar,
                    read,
                }
            }
        };
        compiled.push(CompiledLookup {
            id: lookup.id.clone(),
            kind,
        });
    }
    compiled
}

/// How a look-up reads what it found, from the keys of its entry; `base` is what the kind says when
/// the document says nothing. `None` (with the errors) when a key is wrong.
fn read_settings(
    c: &mut Checker,
    at: &DocPath,
    lookup: &ModelLookup,
    base: ReadSettings,
) -> Option<ReadSettings> {
    let mut read = base;
    let mut ok = true;
    let words = |c: &mut Checker, key: &str| -> Option<Option<Vec<String>>> {
        match lookup.settings.get(key) {
            None => Some(None),
            Some(toml::Value::Array(items)) => {
                let mut out = Vec::new();
                for (n, item) in items.iter().enumerate() {
                    match item {
                        toml::Value::String(s) if !s.trim().is_empty() => {
                            out.push(s.trim().to_string())
                        }
                        other => {
                            c.error(
                                &at.clone().key(key).index(n),
                                format!("`{other}` is not a word: `{key}` is a list of texts"),
                            );
                            return None;
                        }
                    }
                }
                Some(Some(out))
            }
            Some(other) => {
                c.error_hint(
                    &at.clone().key(key),
                    format!("`{other}` is not a list"),
                    format!("{key} = [\"...\"]"),
                );
                None
            }
        }
    };
    for key in ["prefer", "skip", "types"] {
        if !lookup.settings.contains_key(key) {
            continue;
        }
        match words(c, key) {
            Some(Some(list)) => match key {
                "prefer" => read.prefer = list,
                "skip" => read.skip = list,
                _ => read.types = list,
            },
            _ => ok = false,
        }
    }
    match lookup.settings.get("show_variants") {
        None => {}
        Some(toml::Value::Integer(n)) if (1..=64).contains(n) => read.show_variants = *n as usize,
        Some(other) => {
            c.error_hint(
                &at.clone().key("show_variants"),
                format!("`{other}` is not a number of buttons"),
                "a whole number from 1 to 64",
            );
            ok = false;
        }
    }
    match lookup.settings.get("bytes_per_parameter") {
        None => {}
        Some(toml::Value::Float(f)) if (0.05..=4.0).contains(f) => read.bytes_per_parameter = *f,
        Some(toml::Value::Integer(n)) if (1..=4).contains(n) => {
            read.bytes_per_parameter = *n as f64
        }
        Some(other) => {
            c.error_hint(
                &at.clone().key("bytes_per_parameter"),
                format!("`{other}` is not a size of a parameter"),
                "bytes a parameter takes once quantized, from 0.05 to 4: 0.6 is about 4.8 bits",
            );
            ok = false;
        }
    }
    ok.then_some(read)
}

/// The rules of the model box. A rule with no `when` takes whatever is left, so a rule after it
/// can never be reached.
fn compile_input_rules(
    c: &mut Checker,
    at: &DocPath,
    rules: &[InputRule],
) -> Vec<CompiledInputRule> {
    let schema = Type::object([("input", Type::Str)]);
    let mut compiled = Vec::new();
    let mut catch_all: Option<usize> = None;
    for (i, rule) in rules.iter().enumerate() {
        let at = at.clone().key("input_rules").index(i);
        if let Some(first) = catch_all {
            c.warn(
                &at,
                format!(
                    "this rule is never reached: rule {} has no `when` and takes everything",
                    first + 1
                ),
            );
        }
        let when = match rule.when.as_deref() {
            Some(text) => c
                .expr(&at.clone().key("when"), text, &schema, true)
                .map(Some),
            None => {
                catch_all.get_or_insert(i);
                Some(None)
            }
        };
        let action = match (rule.action.as_str(), &rule.reference, &rule.message) {
            ("pull", Some(reference), message) => {
                if message.is_some() {
                    c.warn(
                        &at.clone().key("message"),
                        "`message` is for a `reject` rule and is not used here",
                    );
                }
                c.template(&at.clone().key("ref"), reference, &schema)
                    .map(CompiledInputAction::Pull)
            }
            ("pull", None, _) => {
                c.error_hint(
                    &at,
                    "a `pull` rule says what to pull",
                    "add `ref = \"${input}\"`",
                );
                None
            }
            ("reject", reference, Some(message)) if !message.trim().is_empty() => {
                if reference.is_some() {
                    c.warn(
                        &at.clone().key("ref"),
                        "`ref` is for a `pull` rule and is not used here",
                    );
                }
                Some(CompiledInputAction::Reject(message.clone()))
            }
            ("reject", _, _) => {
                c.error_hint(
                    &at,
                    "a `reject` rule says why: it is what the person reads",
                    "add `message = \"...\"`",
                );
                None
            }
            (other, _, _) => {
                c.error_hint(
                    &at.clone().key("do"),
                    format!("`{other}` is not something a rule does"),
                    "a rule does `pull` or `reject`",
                );
                None
            }
        };
        if let (Some(when), Some(action)) = (when, action) {
            compiled.push(CompiledInputRule { when, action });
        }
    }
    compiled
}

/// A body of a call: tables and lists are kept, numbers and booleans are literal, every text is
/// a template (and a text with no hole is the same text).
fn compile_body(
    c: &mut Checker,
    at: &DocPath,
    value: &toml::Value,
    schema: &Type,
) -> Option<CompiledBody> {
    Some(match value {
        toml::Value::String(text) => {
            if text.contains("${") {
                CompiledBody::Template(c.template(at, text, schema)?)
            } else {
                CompiledBody::Literal(serde_json::Value::String(text.clone()))
            }
        }
        toml::Value::Integer(n) => CompiledBody::Literal((*n).into()),
        toml::Value::Float(f) => CompiledBody::Literal(serde_json::json!(f)),
        toml::Value::Boolean(b) => CompiledBody::Literal((*b).into()),
        toml::Value::Datetime(d) => CompiledBody::Literal(d.to_string().into()),
        toml::Value::Array(items) => {
            let compiled: Vec<Option<CompiledBody>> = items
                .iter()
                .enumerate()
                .map(|(i, v)| compile_body(c, &at.clone().index(i), v, schema))
                .collect();
            CompiledBody::List(compiled.into_iter().collect::<Option<Vec<_>>>()?)
        }
        toml::Value::Table(table) => {
            let compiled: Vec<Option<(String, CompiledBody)>> = table
                .iter()
                .map(|(k, v)| {
                    compile_body(c, &at.clone().key(k), v, schema).map(|b| (k.clone(), b))
                })
                .collect();
            CompiledBody::Map(compiled.into_iter().collect::<Option<Vec<_>>>()?)
        }
    })
}

/// Reads `stop = { policy, grace, exit_grace }`. The waits belong to a graceful stop; given with
/// `kill` they would never be used, which is worth a warning.
fn compile_stop(c: &mut Checker, at: &DocPath, stop: &StopSpec) -> Option<CompiledStop> {
    let mut ok = true;
    let mut wait = |c: &mut Checker, key: &str, text: &Option<String>, default: Duration| match text
    {
        None => default,
        Some(text) => {
            if stop.policy == StopPolicy::Kill {
                c.warn(
                    &at.clone().key(key),
                    format!(
                        "`{key}` belongs to `policy = \"graceful\"`; a `kill` stop never waits"
                    ),
                );
            }
            match c.duration(&at.clone().key(key), text) {
                Some(d) => d,
                None => {
                    ok = false;
                    default
                }
            }
        }
    };
    let (grace, exit_grace) = match stop.policy {
        StopPolicy::Kill => (
            wait(c, "grace", &stop.grace, Duration::ZERO),
            wait(c, "exit_grace", &stop.exit_grace, Duration::ZERO),
        ),
        StopPolicy::Graceful => (
            wait(c, "grace", &stop.grace, DEFAULT_STOP_GRACE),
            wait(c, "exit_grace", &stop.exit_grace, DEFAULT_EXIT_GRACE),
        ),
    };
    ok.then_some(CompiledStop {
        policy: stop.policy,
        grace,
        exit_grace,
    })
}
