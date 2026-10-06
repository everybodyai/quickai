//! What is wrong with a field's value, in a sentence for the user.
//!
//! The checks run in a fixed order and the first problem is the one reported: the value is of the
//! wrong type, a required value is missing, the number is out of range, the text does not match
//! its pattern, the choice is not an option, a validation rule fails. A field with no value is
//! never checked beyond "required": there is nothing to be wrong.

use crate::diag::{Diagnostic, Severity};
use crate::doc::engine::{Field, FieldType};
use crate::docpath::DocPath;
use crate::expr::eval;
use crate::registry::engine::CompiledField;
use crate::registry::Engine;
use crate::value::Value;

#[allow(clippy::too_many_arguments)]
pub(super) fn problem(
    engine: &Engine,
    compiled: &CompiledField,
    doc: &Field,
    invalid: Option<&str>,
    value: &Value,
    required: bool,
    context: &Value,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    if let Some(message) = invalid {
        return Some(sentence(message));
    }
    let unset = match value {
        Value::Null => true,
        Value::Str(s) => s.trim().is_empty(),
        _ => false,
    };
    if unset {
        return required.then(|| "A value is required.".to_string());
    }

    if let (Some((lo, hi)), Some(n)) = (doc.range, value.as_f64()) {
        if matches!(doc.ty, FieldType::Int | FieldType::Float | FieldType::Port)
            && (n < lo || n > hi)
        {
            return Some(format!(
                "Must be between {} and {}.",
                number(lo),
                number(hi)
            ));
        }
    }
    if let (Some(pattern), Some(text)) = (&compiled.pattern, value.as_str()) {
        if !pattern.is_match(text) {
            return Some(format!(
                "Must match the pattern `{}`.",
                doc.pattern.as_deref().unwrap_or_default()
            ));
        }
    }
    if doc.ty == FieldType::Choice {
        let chosen = value.as_str().unwrap_or_default();
        if !doc.options.iter().any(|o| o == chosen) {
            return Some(format!("Must be one of: {}.", doc.options.join(", ")));
        }
    }
    for (index, rule) in compiled.validate.iter().enumerate() {
        match eval(&rule.check, context) {
            Ok(v) if v.truthy() => {}
            Ok(_) => return Some(rule.message.clone()),
            Err(e) => diagnostics.push(
                engine.merged.diagnostic(
                    Severity::Warning,
                    &DocPath::default()
                        .key("field")
                        .id(&doc.id)
                        .key("validate")
                        .index(index)
                        .key("check"),
                    format!(
                        "the rule could not be evaluated, so it is skipped: {}",
                        e.message
                    ),
                ),
            ),
        }
    }
    None
}

/// `expected a whole number, found string` as `Expected a whole number, found string.`
fn sentence(message: &str) -> String {
    let mut chars = message.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// 256 as `256`, 0.5 as `0.5`.
fn number(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{n:.0}")
    } else {
        n.to_string()
    }
}
