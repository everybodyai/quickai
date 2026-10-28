//! What the API panel tells a person who connects an app to the running engine, from the engine's
//! `[api_info]` and the curl example of the vocabulary (docs/phase3-test-plan.md, E13): pure, so
//! the same sentences can be tested without a window.

use crate::plan::resolve::{port_value, public_port, resolve_fields};
use crate::plan::{HostFacts, Selection};
use crate::registry::Registry;
use crate::value::Value;

/// The panel's content, every template filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiPanel {
    /// Where an OpenAI-compatible client connects (`http://127.0.0.1:11434/v1`).
    pub base_url: String,
    /// More addresses, by label (the engine's own API).
    pub extra_urls: Vec<(String, String)>,
    pub api_key: Option<Note>,
    pub model: Option<Note>,
    /// The curl example of the API the document names, with this engine's address and model.
    pub example: Option<Example>,
}

/// A value and a note under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub value: String,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Example {
    pub label: String,
    pub curl: String,
    /// The same request in Python and in JavaScript, when the vocabulary has them.
    pub python: Option<String>,
    pub javascript: Option<String>,
}

/// The model a client is told to name when none is selected.
pub const NO_MODEL_NAME: &str = "local-model";

/// The panel for the selected engine, `None` when its document has no `[api_info]` (or there is no
/// such engine). The model's name is what the engine lists it under, which is what a request must
/// name; a run with no model shows `local-model`.
pub fn api_panel(
    registry: &Registry,
    selection: &Selection,
    facts: &HostFacts,
) -> Option<ApiPanel> {
    let engine = registry.engine(&selection.engine)?;
    let info = engine.api_info.as_ref()?;
    let (resolved, mut context) = resolve_fields(engine, selection, facts);
    let port = port_value(public_port(engine, &resolved));
    context.set_path(&["public_port"], port.clone());
    context.set_path(&["listen_port"], port);
    let render = |t: &crate::expr::Template| t.render(&context).ok().map(|v| v.to_string());

    let base_url = render(&info.base_url)?;
    let model = info.model.as_ref().map(|(value, note)| {
        let value = render(value).filter(|v| !v.is_empty());
        Note {
            value: value.unwrap_or_else(|| NO_MODEL_NAME.to_string()),
            note: note.clone(),
        }
    });
    let api_key = info.api_key.as_ref().and_then(|(value, note)| {
        Some(Note {
            value: render(value)?,
            note: note.clone(),
        })
    });
    let example = info.example.as_deref().and_then(|api| {
        let example = registry
            .vocabulary()
            .api(api)?
            .examples
            .iter()
            .find(|e| e.when.as_ref().is_none_or(|w| holds(w, &context)))?;
        let api_context = Value::Map(
            [(
                "api".to_string(),
                Value::Map(
                    [
                        ("base_url".to_string(), Value::Str(base_url.clone())),
                        (
                            "model".to_string(),
                            Value::Str(
                                model
                                    .as_ref()
                                    .map_or(NO_MODEL_NAME.to_string(), |m| m.value.clone()),
                            ),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            )]
            .into_iter()
            .collect(),
        );
        let fill = |t: &crate::expr::Template| {
            t.render(&api_context)
                .ok()
                .map(|v| v.to_string().trim_end_matches('\n').to_string())
        };
        Some(Example {
            label: example.label.clone(),
            curl: fill(&example.curl)?,
            python: example.python.as_ref().and_then(fill),
            javascript: example.javascript.as_ref().and_then(fill),
        })
    });
    Some(ApiPanel {
        base_url,
        extra_urls: info
            .extra_urls
            .iter()
            .filter_map(|(label, url)| Some((label.clone(), render(url)?)))
            .collect(),
        api_key,
        model,
        example,
    })
}

fn holds(condition: &crate::expr::Expr, context: &Value) -> bool {
    crate::expr::eval(condition, context).is_ok_and(|v| v.truthy())
}
