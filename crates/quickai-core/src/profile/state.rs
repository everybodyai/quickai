//! The working state and a configuration: capturing one from the other, applying one to the other,
//! and the difference between them (docs/configurations.md, sections 4 to 6).
//!
//! The working state is what the application holds and a run uses: `settings.json`, in the shape
//! the form model and the planner read (`<engine>.<field>` to a value). A configuration is the part
//! of it that describes a run and travels. Both directions are pure functions of the engines'
//! documents: which settings are in, which are never, and which are asked about is the `share` of
//! each field, so no engine and no setting is named here.

use std::collections::BTreeMap;

use crate::doc::engine::{Field, FieldType, Share};
use crate::plan::TRAFFIC_PROXY_KEY;
use crate::registry::catalog::{CatalogFile, CatalogModel, Run};
use crate::registry::engine::{CompiledDefault, CompiledField};
use crate::registry::{Engine, Registry};
use crate::value::Value;

use super::notes::Note;
use super::screen;
use super::{
    family, fit, label_of, split_key, value_key, Configuration, ModelPick, ModelSource, Runtime,
};

/// The id of the hidden field that holds the model an engine was last given. By convention, as the
/// application has always read it.
pub const MODEL_FIELD: &str = "last_model";

/// What a run is made of, as the application holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkingState {
    /// The engine in use.
    pub engine: String,
    /// The settings of the engine and of what goes with it, by `<engine>.<field>`. A value that is not
    /// set is not here. Settings no configuration carries (folders, programs) are here too: they ride
    /// along unchanged when a configuration is applied.
    pub values: BTreeMap<String, Value>,
    /// The label of the installed build the engine's program is, when it is one.
    pub build: Option<String>,
    /// Whether the live-traffic proxy sits in front of the engine.
    pub traffic_proxy: bool,
}

/// What the application knows about the model that is selected, besides its name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelFacts {
    pub size: Option<u64>,
    pub sha256: Option<String>,
    /// What people call it, when that is not the name.
    pub display: Option<String>,
    /// Where it was fetched from (`huggingface:owner/repo`), when the application remembers.
    pub source: Option<String>,
    /// Where in that repository the file is, when it is not at the top (`VAE/Wan2.2_VAE.safetensors`).
    pub path: Option<String>,
    /// The branch, tag or commit it was fetched at, when it is not `main`.
    pub revision: Option<String>,
}

/// Something of this computer that a configuration does not take with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeftOut {
    /// A setting that belongs to this computer or to a person (a folder, a program, an account): it
    /// stays here. The key is the setting's.
    Private { key: String },
    /// Lines or words that look like secrets, taken out of a text (export).
    Secrets { key: String, count: usize },
    /// A model whose source is not known: the file says its name and size only.
    ModelSource { name: String },
}

/// A configuration made from the working state, and what was set aside.
#[derive(Clone, Debug)]
pub struct Captured {
    /// With no id and no name: whoever saves it gives them.
    pub configuration: Configuration,
    pub left_out: Vec<LeftOut>,
}

impl Captured {
    pub fn named(mut self, id: &str, name: &str) -> Captured {
        self.configuration.id = id.to_string();
        self.configuration.name = name.to_string();
        self
    }
}

/// Whether two values are the same setting: numbers by their value, the rest as they are.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            a.as_f64() == b.as_f64()
        }
        _ => a == b,
    }
}

/// Whether `stored` is what the field holds when nothing was chosen. A value a first run stores
/// (`first_run`, for the settings that have always been in the file) is that; else the document's
/// literal default. A computed default is not frozen: a value is then the default only when a first
/// run holds it.
fn is_default(compiled: &CompiledField, first_run: Option<&Value>, stored: &Value) -> bool {
    if let Some(initial) = first_run {
        return same(initial, stored);
    }
    match &compiled.default {
        Some(CompiledDefault::Literal(default)) => same(default, stored),
        _ => false,
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The entry of the recommended list that has a file of this name (and this size, when both are
/// known) among its weights for `engine`.
fn catalog_entry<'a>(
    registry: &'a Registry,
    engine: &'a str,
    name: &str,
    size: Option<u64>,
) -> Option<(&'a CatalogModel, &'a CatalogFile)> {
    registry
        .catalog()
        .for_engine(engine)
        .find_map(|model| match model.runs.get(engine)? {
            Run::Files(run) => run
                .files
                .iter()
                .find(|file| {
                    basename(&file.path) == name
                        && match (file.size, size) {
                            (Some(a), Some(b)) => a == b,
                            _ => true,
                        }
                })
                .map(|file| (model, file)),
            Run::Reference(_) => None,
        })
}

/// The model of the working state as who it is: a name the engine takes, an entry of the recommended
/// list, or a file of the models folder.
fn model_pick(
    registry: &Registry,
    head: &Engine,
    name: &str,
    facts: Option<&ModelFacts>,
) -> (ModelPick, bool) {
    let size = facts.and_then(|f| f.size);
    let display = facts.and_then(|f| f.display.clone()).filter(|d| d != name);
    let refs = head
        .doc
        .provides
        .accepts
        .as_ref()
        .map(|a| a.refs.as_slice())
        .unwrap_or(&[]);
    if let Some(kind) = refs.first() {
        return (
            ModelPick {
                role: "main".into(),
                source: ModelSource::Named {
                    kind: kind.clone(),
                    name: name.to_string(),
                },
                name: display,
                size,
                sha256: facts.and_then(|f| f.sha256.clone()),
                from: None,
                path: None,
                revision: None,
                url: None,
                into: None,
            },
            true,
        );
    }
    if let Some((model, file)) = catalog_entry(registry, &head.id, name, size) {
        return (
            ModelPick {
                role: "main".into(),
                source: ModelSource::Catalog {
                    id: model.id.clone(),
                    file: Some(basename(&file.path).to_string()),
                },
                name: display.or_else(|| Some(model.name.clone())),
                size: file.size.or(size),
                sha256: file
                    .sha256
                    .clone()
                    .or_else(|| facts.and_then(|f| f.sha256.clone())),
                from: None,
                path: None,
                revision: None,
                url: None,
                into: None,
            },
            true,
        );
    }
    // a file that is no entry of the list: it can be got from where it was fetched from, if that is known
    let from = facts
        .and_then(|f| f.source.clone())
        .filter(|s| super::valid_from(s));
    let known = from.is_some();
    // where in the repository a file is only means something with the repository
    let path = facts
        .and_then(|f| f.path.clone())
        .filter(|p| from.is_some() && super::valid_repo_path(p));
    let revision = facts
        .and_then(|f| f.revision.clone())
        .filter(|r| from.is_some() && super::valid_revision(r));
    (
        ModelPick {
            role: "main".into(),
            source: ModelSource::File {
                name: name.to_string(),
            },
            name: display,
            size,
            sha256: facts.and_then(|f| f.sha256.clone()),
            from,
            path,
            revision,
            url: None,
            into: None,
        },
        known,
    )
}

/// The part of the working state that describes a run and travels. `first_run` holds, by
/// `<engine>.<field>`, what a first run stores for the settings that have always been in the
/// settings file, which count as the default; `model` is what is known of the selected model.
pub fn capture(
    registry: &Registry,
    state: &WorkingState,
    first_run: &BTreeMap<String, Value>,
    model: Option<&ModelFacts>,
) -> Captured {
    let engines = registry.engine_map();
    let members = family(engines, &state.engine);
    let mut values = BTreeMap::new();
    let mut left_out = Vec::new();
    for engine in &members {
        for (compiled, field) in engine.fields.iter().zip(&engine.doc.fields) {
            let key = value_key(&engine.id, &field.id);
            let Some(stored) = state.values.get(&key).filter(|v| !v.is_null()) else {
                continue;
            };
            match field.share() {
                None => {}
                Some(Share::Never) => {
                    // the program is a build, which has a section of its own, when it is one
                    let a_build = field.ty == FieldType::RuntimePath && state.build.is_some();
                    let empty = stored.as_str().is_some_and(|s| s.trim().is_empty());
                    if !a_build && !empty && !is_default(compiled, first_run.get(&key), stored) {
                        left_out.push(LeftOut::Private { key });
                    }
                }
                Some(_) => {
                    if is_default(compiled, first_run.get(&key), stored) {
                        continue;
                    }
                    values.insert(key, stored.clone());
                }
            }
        }
    }

    let mut models = Vec::new();
    if let Some(head) = members.first() {
        let name = state
            .values
            .get(&value_key(&head.id, MODEL_FIELD))
            .and_then(Value::as_str)
            .filter(|n| !n.trim().is_empty());
        if let Some(name) = name {
            let (pick, known_source) = model_pick(registry, head, name, model);
            if !known_source {
                left_out.push(LeftOut::ModelSource {
                    name: pick.display().to_string(),
                });
            }
            models.push(pick);
        }
    }

    Captured {
        configuration: Configuration {
            engine: state.engine.clone(),
            runtime: Runtime {
                build: state.build.clone(),
                version: None,
                companions: BTreeMap::new(),
            },
            models,
            values,
            traffic_proxy: state.traffic_proxy.then_some(true),
            ..Configuration::default()
        },
        left_out,
    }
}

impl Configuration {
    /// Whether two configurations describe the same run: the engine, the build, the model, every
    /// setting and the proxy. The name, the note and what a model is called do not matter.
    /// (A model is the same when it is the same file or name, however it is said: see `changes`.)
    pub fn same_run(&self, other: &Configuration) -> bool {
        self.engine == other.engine
            && self.runtime.build == other.runtime.build
            && self.traffic_proxy.unwrap_or(false) == other.traffic_proxy.unwrap_or(false)
            && self.models.first().map(|m| (&m.role, m.source.label()))
                == other.models.first().map(|m| (&m.role, m.source.label()))
            && self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .all(|(k, v)| other.values.get(k).is_some_and(|o| same(v, o)))
    }
}

impl Configuration {
    /// Whether what the window holds (`self`) is still what a saved configuration says. As `same_run`,
    /// except that a configuration that names no model does not mind which one is selected: the
    /// application chooses one when nothing was chosen, and its choice is not a change of the run. (The
    /// Default is such a configuration.)
    pub fn matches_run(&self, saved: &Configuration) -> bool {
        if saved.models.is_empty() {
            let without = Configuration {
                models: Vec::new(),
                ..self.clone()
            };
            without.same_run(saved)
        } else {
            self.same_run(saved)
        }
    }
}

/// What choosing a configuration does to the working state, before anything is stored: the settings
/// it sets, the ones it returns to their defaults, the model and the build it wants, and what was
/// left out on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub engine: String,
    /// The settings the configuration gives, by `<engine>.<field>`.
    pub set: BTreeMap<String, Value>,
    /// The settings of the engine and what goes with it that a configuration can carry and this one does
    /// not give: they return to their defaults.
    pub cleared: Vec<String>,
    /// The model it wants. The application finds out whether it is here.
    pub model: Option<ModelPick>,
    /// The build it wants, by its label.
    pub runtime: Runtime,
    pub traffic_proxy: bool,
    /// Refused lines and values that did not fit.
    pub notes: Vec<Note>,
}

/// Makes ready the choice of `configuration`: nothing is stored here. `Err` is the note that stops it:
/// the engine is not one this version has.
pub fn apply(registry: &Registry, configuration: &Configuration) -> Result<Applied, Note> {
    let engines = registry.engine_map();
    let members = family(engines, &configuration.engine);
    if members.is_empty() {
        return Err(Note::new(
            "unknown_engine",
            &[("engine", configuration.engine.clone())],
        ));
    }
    let deny = registry.vocabulary().deny_env();
    let mut set = BTreeMap::new();
    let mut cleared = Vec::new();
    let mut notes = Vec::new();
    for engine in &members {
        for field in &engine.doc.fields {
            if !matches!(field.share(), Some(Share::Yes | Share::Review)) {
                continue;
            }
            let key = value_key(&engine.id, &field.id);
            let Some(wanted) = configuration.values.get(&key) else {
                cleared.push(key);
                continue;
            };
            match fit(field, wanted) {
                Ok(mut value) => {
                    if let (Some(lines), Value::Str(text)) = (&field.bind.env_lines, &value) {
                        let seen = screen::env_lines(engine, &lines.protect, text, deny);
                        for name in seen.refused {
                            notes.push(
                                Note::new(
                                    "refused_line",
                                    &[("name", name), ("field", label_of(field))],
                                )
                                .about(key.clone()),
                            );
                        }
                        value = Value::Str(seen.text);
                    }
                    set.insert(key, value);
                }
                Err(why) => {
                    notes.push(
                        Note::new(
                            "bad_value",
                            &[
                                ("field", label_of(field)),
                                ("value", super::shown(wanted)),
                                ("why", why),
                            ],
                        )
                        .about(key.clone()),
                    );
                    cleared.push(key);
                }
            }
        }
    }
    Ok(Applied {
        engine: configuration.engine.clone(),
        set,
        cleared,
        model: configuration.models.first().cloned(),
        runtime: configuration.runtime.clone(),
        traffic_proxy: configuration.traffic_proxy.unwrap_or(false),
        notes,
    })
}

impl WorkingState {
    /// The state after `applied`: the engine, the settings it gives (and its family's other
    /// settings back at their defaults), the model the application found (`model`: its name for
    /// `last_model`, or none), the build it found (`build`: a label, or none to leave the program
    /// as it is) and the proxy. What a configuration does not carry stays: folders, programs,
    /// the values of other engines.
    pub fn after(
        &self,
        registry: &Registry,
        applied: &Applied,
        model: Option<&str>,
        build: Option<&str>,
    ) -> WorkingState {
        let mut next = self.clone();
        next.engine = applied.engine.clone();
        for key in &applied.cleared {
            next.values.remove(key);
        }
        for (key, value) in &applied.set {
            next.values.insert(key.clone(), value.clone());
        }
        if let Some(head) = family(registry.engine_map(), &applied.engine).first() {
            let key = value_key(&head.id, MODEL_FIELD);
            match model {
                Some(name) => {
                    next.values.insert(key, Value::Str(name.to_string()));
                }
                None => {
                    next.values.remove(&key);
                }
            }
        }
        if let Some(label) = build {
            next.build = Some(label.to_string());
        }
        next.traffic_proxy = applied.traffic_proxy;
        next
    }
}

/// One thing that differs between the working state and a configuration.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Engine {
        from: String,
        to: String,
    },
    Model {
        from: Option<String>,
        to: Option<String>,
    },
    Build {
        from: Option<String>,
        to: Option<String>,
    },
    Proxy {
        from: bool,
        to: bool,
    },
    /// A setting, as the values in force: the value chosen, else the default the document or a first
    /// run gives; `None` is a default that is worked out (a context size that follows the model).
    Setting {
        key: String,
        label: String,
        from: Option<Value>,
        to: Option<Value>,
    },
}

impl Change {
    /// The same difference seen from the other side: what was `from` is `to`.
    pub fn flipped(self) -> Change {
        match self {
            Change::Engine { from, to } => Change::Engine { from: to, to: from },
            Change::Model { from, to } => Change::Model { from: to, to: from },
            Change::Build { from, to } => Change::Build { from: to, to: from },
            Change::Proxy { from, to } => Change::Proxy { from: to, to: from },
            Change::Setting {
                key,
                label,
                from,
                to,
            } => Change::Setting {
                key,
                label,
                from: to,
                to: from,
            },
        }
    }

    /// A phrase for a list: `Port: 8080 to 8081`, `Context size: default to 16384`.
    pub fn describe(&self) -> String {
        let shown = |v: &Option<Value>| match v {
            Some(Value::Str(text)) if text.trim().is_empty() => "none".to_string(),
            Some(v) => super::shown(v),
            None => "default".to_string(),
        };
        let name = |v: &Option<String>| v.clone().unwrap_or_else(|| "none".to_string());
        match self {
            Change::Engine { from, to } => format!("Engine: {from} to {to}"),
            Change::Model { from, to } => format!("Model: {} to {}", name(from), name(to)),
            Change::Build { from, to } => format!("Program: {} to {}", name(from), name(to)),
            Change::Proxy { from, to } => {
                let word = |on: &bool| if *on { "on" } else { "off" };
                format!("Live traffic proxy: {} to {}", word(from), word(to))
            }
            Change::Setting {
                label, from, to, ..
            } => format!("{label}: {} to {}", shown(from), shown(to)),
        }
    }
}

/// What differs between the working state and `wanted`, the engine's settings in the order of its
/// form. The state's settings are looked at for the engine `wanted` is for, so that a switch of
/// engine lists the settings that would change there and not the old engine's.
pub fn changes(
    registry: &Registry,
    state: &WorkingState,
    first_run: &BTreeMap<String, Value>,
    model: Option<&ModelFacts>,
    wanted: &Configuration,
) -> Vec<Change> {
    let mut out = Vec::new();
    if state.engine != wanted.engine {
        out.push(Change::Engine {
            from: state.engine.clone(),
            to: wanted.engine.clone(),
        });
    }
    let here = WorkingState {
        engine: wanted.engine.clone(),
        ..state.clone()
    };
    let now = capture(registry, &here, first_run, model).configuration;
    let shown_model = |picks: &[ModelPick]| picks.first().map(|p| p.display().to_string());
    let (from, to) = (shown_model(&now.models), shown_model(&wanted.models));
    // the same model when it is the same file or name, whichever way it is said: a configuration that
    // says `file = "x.gguf"` and one that says the entry of the recommended list that has that file
    // are about one model
    if now.models.first().map(|m| m.source.label())
        != wanted.models.first().map(|m| m.source.label())
    {
        out.push(Change::Model { from, to });
    }
    if let Some(build) = &wanted.runtime.build {
        if state.build.as_ref() != Some(build) {
            out.push(Change::Build {
                from: state.build.clone(),
                to: Some(build.clone()),
            });
        }
    }
    if now.traffic_proxy.unwrap_or(false) != wanted.traffic_proxy.unwrap_or(false) {
        out.push(Change::Proxy {
            from: now.traffic_proxy.unwrap_or(false),
            to: wanted.traffic_proxy.unwrap_or(false),
        });
    }
    // the settings, in the order of the engines' forms, as the values in force: the one chosen, else
    // the one a first run holds, else the document's literal default, else "the default"
    for engine in family(registry.engine_map(), &wanted.engine) {
        for (compiled, field) in engine.fields.iter().zip(&engine.doc.fields) {
            let key = value_key(&engine.id, &field.id);
            if !matches!(field.share(), Some(Share::Yes | Share::Review)) {
                continue;
            }
            let effective = |chosen: Option<&Value>| -> Option<Value> {
                chosen
                    .filter(|v| !v.is_null())
                    .cloned()
                    .or_else(|| first_run.get(&key).cloned())
                    .or_else(|| match &compiled.default {
                        Some(CompiledDefault::Literal(d)) => Some(d.clone()),
                        _ => None,
                    })
            };
            let (from, to) = (
                effective(here.values.get(&key)),
                effective(wanted.values.get(&key)),
            );
            let differs = match (&from, &to) {
                (None, None) => false,
                (Some(x), Some(y)) => !same(x, y),
                _ => true,
            };
            if differs {
                out.push(Change::Setting {
                    key,
                    label: label_of(field),
                    from,
                    to,
                });
            }
        }
    }
    out
}

/// The settings of the family of `engine` that a configuration can carry, in the order of the
/// engines' forms, as `(key, field)`.
pub fn carried<'a>(registry: &'a Registry, engine: &str) -> Vec<(String, &'a Field)> {
    let mut out = Vec::new();
    for member in family(registry.engine_map(), engine) {
        for field in &member.doc.fields {
            if matches!(field.share(), Some(Share::Yes | Share::Review)) {
                out.push((value_key(&member.id, &field.id), field));
            }
        }
    }
    out
}

/// The engine and the field a key of a configuration names.
pub fn field_of<'a>(registry: &'a Registry, key: &str) -> Option<(&'a Engine, &'a Field)> {
    let (owner, field) = split_key(key)?;
    let engine = registry.engine_map().get(owner)?;
    let field = engine.doc.fields.iter().find(|f| f.id == field)?;
    Some((engine, field))
}

/// Whether the key is the one that puts the live-traffic proxy in front of the engine, which a
/// configuration holds as `traffic_proxy` and not as a setting.
pub fn is_proxy_key(key: &str) -> bool {
    key == TRAFFIC_PROXY_KEY
}
