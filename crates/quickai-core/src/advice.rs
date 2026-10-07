//! Advice for a run that went wrong, from the engine's `[[diagnostic]]` rules (docs/phase3-test-plan.md,
//! E14): each rule names what the log says or in which state the run ended, and what to tell the
//! person. Pure, so the sentences and the patterns can be tested without a window.

use crate::plan::resolve::{port_value, public_port, resolve_fields};
use crate::plan::{HostFacts, Selection};
use crate::registry::Registry;

/// One piece of advice, its template filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advice {
    pub text: String,
    /// A button under it: what a click does (`focus_field:port`).
    pub action: Option<String>,
}

/// The advice that applies to a run of the selected engine: the rules whose conditions all hold, in
/// the document's order, with each hint's template filled in from the settings as they are now.
/// `log` is what the engine printed, `ended_in` the state it was in when it ended (`starting`,
/// `running`), if it ended.
pub fn advice(
    registry: &Registry,
    selection: &Selection,
    facts: &HostFacts,
    log: &[String],
    ended_in: Option<&str>,
) -> Vec<Advice> {
    let Some(engine) = registry.engine(&selection.engine) else {
        return Vec::new();
    };
    if engine.diagnostics.is_empty() {
        return Vec::new();
    }
    let (resolved, mut context) = resolve_fields(engine, selection, facts);
    let port = port_value(public_port(engine, &resolved));
    context.set_path(&["public_port"], port.clone());
    context.set_path(&["listen_port"], port);
    let mut out: Vec<Advice> = Vec::new();
    for rule in &engine.diagnostics {
        let in_the_log = rule
            .log_matches
            .as_ref()
            .is_none_or(|pattern| log.iter().any(|line| pattern.is_match(line)));
        let at_the_end = rule
            .exited_during
            .as_deref()
            .is_none_or(|state| ended_in == Some(state));
        if !(in_the_log && at_the_end) {
            continue;
        }
        let Ok(text) = rule.hint.render(&context) else {
            continue;
        };
        let advice = Advice {
            text: text.to_string(),
            action: rule.action.clone(),
        };
        if !out.contains(&advice) {
            out.push(advice);
        }
    }
    out
}
