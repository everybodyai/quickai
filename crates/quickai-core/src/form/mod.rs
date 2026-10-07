//! The form model (docs/architecture.md, section 9.1): what the settings of an engine look like to
//! a person right now.
//!
//! `FormModel::build` takes the registry, what the user has chosen and the facts about the
//! machine, and returns the fields grouped into sections, each with its resolved value, where the
//! value came from, whether it is visible and enabled, its error and its help. It has no egui in it
//! and no state: the interface builds it every frame, draws it, and hands an edit back as a change
//! to the selection. The values come from the same resolver the planner uses, so a number shown
//! here is the number the command line gets.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::diag::{Diagnostic, Severity};
use crate::doc::engine::{Applies, FieldType, Persist};
use crate::docpath::DocPath;
use crate::expr::{eval, Expr};
use crate::plan::resolve::{default_value, port_value, public_port, resolve_fields};
use crate::plan::{HostFacts, Origin, Selection};
use crate::registry::{Engine, Registry};
use crate::value::Value;

mod validate;

#[cfg(test)]
mod tests;

/// The settings of one engine, ready to draw.
#[derive(Clone, Debug, Serialize)]
pub struct FormModel {
    pub engine: String,
    /// In display order: `primary`, `advanced`, then the others by name.
    pub sections: Vec<FormSection>,
    /// Facts the fields declare for the rest of the program (`derive`), such as `gpu_offload`.
    pub derived: BTreeMap<String, Value>,
    /// Things worth telling the user that are not about one field, such as a condition in the
    /// document that could not be evaluated. The form is built anyway.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormSection {
    pub id: String,
    /// By `ui.order`, then document order; fields without an order come last.
    pub fields: Vec<FormField>,
}

/// One field as a person sees it.
#[derive(Clone, Debug, Serialize)]
pub struct FormField {
    /// The field's id in the document (`port`).
    pub id: String,
    /// The key of its value in a selection (`llamacpp.port`).
    pub key: String,
    /// The label, or the id when the document gives none.
    pub label: String,
    pub help: Option<String>,
    /// Greyed text for an empty text box.
    pub hint: Option<String>,
    #[serde(rename = "type")]
    pub ty: FieldType,
    /// The widget the document asks for, when it asks for one.
    pub widget: Option<String>,
    /// For a `button`: what a click makes the application do (`open:settings`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// The value in force: the selection's, else the default. `null` when there is none.
    pub value: Value,
    pub origin: Origin,
    /// Whether to show it at all (`visible_when`; a `hidden` field is not shown unless a widget is named).
    pub visible: bool,
    /// Whether it can be edited now (`enabled_when`).
    pub enabled: bool,
    /// Whether it needs a value (`required_when` holds).
    pub required: bool,
    /// Why the value is not acceptable, in a sentence for the user. The value is kept and shown.
    pub error: Option<String>,
    pub range: Option<(f64, f64)>,
    pub step: Option<f64>,
    pub options: Vec<String>,
    pub applies: Applies,
    pub persist: Persist,
    pub legacy_key: Option<String>,
}

/// A change to make to a selection.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub key: String,
    pub value: Value,
}

impl FormModel {
    pub fn build(
        registry: &Registry,
        selection: &Selection,
        facts: &HostFacts,
    ) -> Result<FormModel, Vec<Diagnostic>> {
        let engine = engine_of(registry, selection)?;
        let (resolved, mut context) = resolve_fields(engine, selection, facts);
        // A form has no proxy in front of the engine: both ports are the one the field holds.
        let port = public_port(engine, &resolved);
        context.set_path(&["public_port"], port_value(port));
        context.set_path(&["listen_port"], port_value(port));

        let mut diagnostics = Vec::new();
        let mut derived = BTreeMap::new();
        // (document position, the field), grouped into sections below
        let mut entries: Vec<(String, i32, usize, FormField)> = Vec::new();

        for (index, (compiled, doc)) in engine.fields.iter().zip(&engine.doc.fields).enumerate() {
            let r = &resolved[index];
            let mut ctx = context.clone();
            ctx.set_path(&["value"], r.value.clone());
            let at = DocPath::default().key("field").id(&doc.id);
            let mut holds = |expr: &Option<Expr>,
                             text: &Option<String>,
                             name: &str,
                             otherwise: bool,
                             outcome: &str|
             -> bool {
                let Some(expr) = expr else { return otherwise };
                match eval(expr, &ctx) {
                    Ok(v) => v.truthy(),
                    Err(e) => {
                        diagnostics.push(engine.merged.diagnostic(
                            Severity::Warning,
                            &at.clone().key(name),
                            format!(
                                "`{}` could not be evaluated, so the field counts as {outcome}: {}",
                                text.as_deref().unwrap_or_default(),
                                e.message
                            ),
                        ));
                        false
                    }
                }
            };
            let visible = (doc.ty != FieldType::Hidden || doc.ui.widget.is_some())
                && holds(
                    &compiled.visible_when,
                    &doc.visible_when,
                    "visible_when",
                    true,
                    "hidden",
                );
            // what only a spawn can set belongs to the owner of an engine QuickAI attached to
            let owned_by_another = doc.applies == Applies::SpawnOnly && facts.session_attached;
            let enabled = !owned_by_another
                && holds(
                    &compiled.enabled_when,
                    &doc.enabled_when,
                    "enabled_when",
                    true,
                    "disabled",
                );
            let required = holds(
                &compiled.required_when,
                &doc.required_when,
                "required_when",
                false,
                "not required",
            );

            for (name, expr) in &compiled.derive {
                match eval(expr, &ctx) {
                    Ok(v) => {
                        derived.insert(name.clone(), v);
                    }
                    Err(e) => diagnostics.push(engine.merged.diagnostic(
                        Severity::Warning,
                        &at.clone().key("derive").key(name),
                        format!("the fact `{name}` could not be worked out: {}", e.message),
                    )),
                }
            }

            let error = validate::problem(
                engine,
                compiled,
                doc,
                r.invalid.as_deref(),
                &r.value,
                required,
                &ctx,
                &mut diagnostics,
            );
            entries.push((
                doc.ui
                    .section
                    .clone()
                    .unwrap_or_else(|| "advanced".to_string()),
                doc.ui.order.unwrap_or(i32::MAX),
                index,
                FormField {
                    id: doc.id.clone(),
                    key: format!("{}.{}", engine.id, doc.id),
                    label: doc.label.clone().unwrap_or_else(|| doc.id.clone()),
                    help: doc.help.clone(),
                    hint: doc.ui.hint.clone(),
                    ty: doc.ty,
                    widget: doc.ui.widget.clone(),
                    action: doc.ui.action.clone(),
                    value: r.value.clone(),
                    origin: r.origin.clone(),
                    visible,
                    enabled,
                    required,
                    error,
                    range: doc.range,
                    step: doc.step,
                    options: doc.options.clone(),
                    applies: doc.applies,
                    persist: doc.persist,
                    legacy_key: doc.legacy_key.clone(),
                },
            ));
        }

        entries.sort_by(|a, b| {
            (section_rank(&a.0), &a.0, a.1, a.2).cmp(&(section_rank(&b.0), &b.0, b.1, b.2))
        });
        let mut sections: Vec<FormSection> = Vec::new();
        for (section, _, _, field) in entries {
            match sections.last_mut() {
                Some(last) if last.id == section => last.fields.push(field),
                _ => sections.push(FormSection {
                    id: section,
                    fields: vec![field],
                }),
            }
        }
        Ok(FormModel {
            engine: engine.id.clone(),
            sections,
            derived,
            diagnostics,
        })
    }

    pub fn field(&self, id: &str) -> Option<&FormField> {
        self.sections
            .iter()
            .flat_map(|s| &s.fields)
            .find(|f| f.id == id)
    }

    pub fn section(&self, id: &str) -> Option<&FormSection> {
        self.sections.iter().find(|s| s.id == id)
    }

    pub fn derived(&self, name: &str) -> Option<&Value> {
        self.derived.get(name)
    }
}

/// The edits that re-derive the defaults of the fields that name `trigger` in `recompute_on`
/// (`model`: another model was picked). A value the user typed is replaced, as it is today.
pub fn recompute(
    registry: &Registry,
    selection: &Selection,
    facts: &HostFacts,
    trigger: &str,
) -> Result<Vec<Edit>, Vec<Diagnostic>> {
    let engine = engine_of(registry, selection)?;
    let (_, context) = resolve_fields(engine, selection, facts);
    let mut edits = Vec::new();
    let mut errors = Vec::new();
    for (compiled, doc) in engine.fields.iter().zip(&engine.doc.fields) {
        if !doc.recompute_on.iter().any(|t| t == trigger) {
            continue;
        }
        match default_value(compiled, &context) {
            Ok((value, _)) if !value.is_null() => edits.push(Edit {
                key: format!("{}.{}", engine.id, doc.id),
                value,
            }),
            Ok(_) => {}
            Err(message) => errors.push(engine.merged.diagnostic(
                Severity::Error,
                &DocPath::default().key("field").id(&doc.id).key("default"),
                message,
            )),
        }
    }
    if errors.is_empty() {
        Ok(edits)
    } else {
        Err(errors)
    }
}

/// Sets a field's value after checking it: the type, the range, the pattern, the options and the
/// field's validation rules. On an error the selection is left as it was and the message says why.
/// A `null` clears the value, so the default applies again.
pub fn set(
    registry: &Registry,
    selection: &mut Selection,
    facts: &HostFacts,
    field: &str,
    value: Value,
) -> Result<(), String> {
    let engine = registry
        .engine(&selection.engine)
        .ok_or_else(|| format!("there is no engine called `{}`", selection.engine))?;
    if !engine.fields.iter().any(|f| f.id == field) {
        return Err(format!(
            "there is no field `{field}` in the engine `{}`",
            engine.id
        ));
    }
    let key = format!("{}.{field}", engine.id);
    if value.is_null() {
        selection.values.remove(&key);
        return Ok(());
    }
    let mut tentative = selection.clone();
    tentative.values.insert(key, value);
    let form = FormModel::build(registry, &tentative, facts)
        .map_err(|errors| errors[0].message.clone())?;
    if let Some(error) = form.field(field).and_then(|f| f.error.clone()) {
        return Err(error);
    }
    *selection = tentative;
    Ok(())
}

fn engine_of<'a>(
    registry: &'a Registry,
    selection: &Selection,
) -> Result<&'a Engine, Vec<Diagnostic>> {
    registry.engine(&selection.engine).ok_or_else(|| {
        vec![Diagnostic::error(format!(
            "there is no engine called `{}`",
            selection.engine
        ))]
    })
}

/// `primary` first, `advanced` second, every other section after them by name.
fn section_rank(id: &str) -> u8 {
    match id {
        "primary" => 0,
        "advanced" => 1,
        _ => 2,
    }
}
