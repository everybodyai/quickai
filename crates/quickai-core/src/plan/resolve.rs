//! Turning a selection into the value of every field, and the context expressions see.
//!
//! The planner and the form model both need "what is this field's value, and where did it come
//! from": a value the selection holds wins, else the field's default, which may be computed. Doing
//! it in one place means the interface can never show a number the command line then does not use.

use std::collections::BTreeMap;
use std::path::Path;

use crate::doc::engine::FieldType;
use crate::docpath::DocPath;
use crate::registry::engine::{CompiledDefault, CompiledField};
use crate::registry::Engine;
use crate::value::{map, Value};

use super::{HostFacts, Selection};

/// Where a field's value came from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The selection held it: the user chose it, or a saved file did.
    Selection,
    /// The document's literal default.
    Default,
    /// The document's default, computed by this function from the context.
    Computed(String),
    /// Neither: the field has no value.
    Unset,
}

impl Origin {
    /// How a plan explains it: `default`, `default: recommend_ctx_size`, nothing for a chosen value.
    pub fn note(&self) -> Option<String> {
        match self {
            Origin::Default => Some("default".to_string()),
            Origin::Computed(name) => Some(format!("default: {name}")),
            Origin::Selection | Origin::Unset => None,
        }
    }
}

/// A field's value for this run and why.
pub(crate) struct Resolved {
    pub value: Value,
    pub origin: Origin,
    /// What is wrong with the value or the default, with the place in the document it points at.
    pub problems: Vec<(DocPath, String)>,
    /// Why the value cannot be this field's (`expected a whole number, found string`), without the
    /// plan's wording around it.
    pub invalid: Option<String>,
}

/// Every field's value (the selection's, else the default) and the context they live in. A problem
/// with a value or a default is recorded on the field rather than raised: the planner turns it into
/// an error, the form into a message under the field. A `null` in the selection means "not set", so
/// the default applies.
pub(crate) fn resolve_fields(
    engine: &Engine,
    selection: &Selection,
    facts: &HostFacts,
) -> (Vec<Resolved>, Value) {
    let mut context = base_context(selection, facts);
    context.set_path(&["field"], Value::Map(BTreeMap::new()));
    let mut resolved = Vec::new();
    for field in &engine.fields {
        let key = format!("{}.{}", engine.id, field.id);
        let at = DocPath::default().key("field").id(&field.id);
        let mut problems = Vec::new();
        let mut invalid = None;
        let (value, origin) = match selection.values.get(&key).filter(|v| !v.is_null()) {
            Some(v) => {
                if let Err(message) = check_value(field.ty, v) {
                    problems.push((at, format!("the value for `{key}` is wrong: {message}")));
                    invalid = Some(message);
                }
                (v.clone(), Origin::Selection)
            }
            None => match default_value(field, &context) {
                Ok(found) => found,
                Err(message) => {
                    problems.push((at.key("default"), message));
                    (Value::Null, Origin::Unset)
                }
            },
        };
        context.set_path(&["field", field.id.as_str()], value.clone());
        resolved.push(Resolved {
            value,
            origin,
            problems,
            invalid,
        });
    }
    (resolved, context)
}

/// What a field is when nothing chose a value: its literal default, its computed default (with the
/// function that produced it), or nothing.
pub(crate) fn default_value(
    field: &CompiledField,
    context: &Value,
) -> Result<(Value, Origin), String> {
    match &field.default {
        None => Ok((Value::Null, Origin::Unset)),
        Some(CompiledDefault::Literal(v)) => Ok((v.clone(), Origin::Default)),
        Some(CompiledDefault::Template(template)) => template
            .render(context)
            .map(|v| (v, Origin::Default))
            .map_err(|e| format!("the default could not be filled in: {}", e.message)),
        Some(CompiledDefault::Computed(expr)) => crate::expr::eval(expr, context)
            .map(|v| (v, Origin::Computed(call_name(expr))))
            .map_err(|e| format!("the default could not be computed: {}", e.message)),
    }
}

/// The port clients connect to: the field marked `bind.port = "public"`, else the engine's default.
pub(crate) fn public_port(engine: &Engine, resolved: &[Resolved]) -> Option<u16> {
    engine
        .fields
        .iter()
        .zip(resolved)
        .find(|(f, _)| f.public_port)
        .and_then(|(_, r)| r.value.as_i64())
        .and_then(|p| u16::try_from(p).ok())
        .or(engine.doc.provides.default_port)
}

pub(crate) fn port_value(port: Option<u16>) -> Value {
    port.map(Value::from).unwrap_or(Value::Null)
}

/// Whether `value` can be the value of a field of type `ty`. `null` always can (the field is not set).
pub(crate) fn check_value(ty: FieldType, value: &Value) -> Result<(), String> {
    let found = value.type_name();
    match (ty, value) {
        (_, Value::Null) => Ok(()),
        (FieldType::Port, v) => match v.as_i64() {
            Some(p) if (1..=65535).contains(&p) => Ok(()),
            Some(p) => Err(format!("{p} is not a port (1 to 65535)")),
            None => Err(format!("a port is a whole number, found {found}")),
        },
        (FieldType::Int, v) if v.as_i64().is_some() => Ok(()),
        (FieldType::Int, _) => Err(format!("expected a whole number, found {found}")),
        (FieldType::Float, v) if v.as_f64().is_some() => Ok(()),
        (FieldType::Float, _) => Err(format!("expected a number, found {found}")),
        (FieldType::Bool, Value::Bool(_)) => Ok(()),
        (FieldType::Bool, _) => Err(format!("expected true or false, found {found}")),
        (FieldType::Hidden, _) => Ok(()),
        (_, Value::Str(_)) => Ok(()),
        (_, _) => Err(format!("expected text, found {found}")),
    }
}

fn call_name(expr: &crate::expr::Expr) -> String {
    match expr {
        crate::expr::Expr::Call { name, .. } => name.clone(),
        _ => "expression".to_string(),
    }
}

/// The part of the context that does not depend on the engine's fields.
pub(crate) fn base_context(selection: &Selection, facts: &HostFacts) -> Value {
    let binary = facts.binary.clone().unwrap_or_default();
    let dir = Path::new(&binary)
        .parent()
        .map(|p| p.display().to_string())
        .filter(|d| !d.is_empty());
    let model = match &selection.model {
        Some(m) => map([
            ("id", m.id.as_str().into()),
            ("ref", Value::Null),
            ("display", m.display.as_str().into()),
            ("size_bytes", m.size_bytes.into()),
            (
                "modalities",
                map([
                    ("input", vec!["text"].into()),
                    ("output", vec!["text"].into()),
                ]),
            ),
            ("main", map([("path", m.path.as_str().into())])),
            ("components", Value::Null),
        ]),
        None => Value::Null,
    };
    map([
        ("os", facts.os.as_str().into()),
        ("arch", facts.arch.as_str().into()),
        (
            "sys",
            map([
                ("ram_bytes", facts.ram_bytes.into()),
                ("cpus", facts.cpus.into()),
            ]),
        ),
        (
            "gpu",
            map([
                ("known", facts.gpu_known.into()),
                ("vendors", facts.gpu_vendors.clone().into()),
                ("targets", facts.gpu_targets.clone().into()),
            ]),
        ),
        (
            "runtime",
            map([
                (
                    "binary",
                    facts
                        .binary
                        .as_deref()
                        .map(Value::from)
                        .unwrap_or(Value::Null),
                ),
                ("dir", dir.map(Value::from).unwrap_or(Value::Null)),
                (
                    "build",
                    facts.runtime_build.map(Value::from).unwrap_or(Value::Null),
                ),
                (
                    "version",
                    facts
                        .runtime_version
                        .as_deref()
                        .map(Value::from)
                        .unwrap_or(Value::Null),
                ),
                ("flags", facts.runtime_flags.clone().into()),
            ]),
        ),
        ("model", model),
        (
            "session",
            map([
                ("running", facts.session_running.into()),
                ("crashed", facts.session_crashed.into()),
                ("attached", facts.session_attached.into()),
                (
                    "model_display",
                    selection
                        .model
                        .as_ref()
                        .map(|m| Value::from(m.display.as_str()))
                        .unwrap_or(Value::Null),
                ),
                ("model_ref", Value::Null),
            ]),
        ),
        (
            "parent",
            match &facts.parent {
                Some(parent) => map([
                    ("id", parent.id.as_str().into()),
                    ("port", parent.port.into()),
                    ("url", super::parent_url(parent.port).as_str().into()),
                ]),
                None => Value::Null,
            },
        ),
        ("data", facts.data_dir.as_str().into()),
        ("config", facts.config_dir.as_str().into()),
        ("exe_dir", facts.exe_dir.as_str().into()),
        ("home", facts.home.as_str().into()),
    ])
}
