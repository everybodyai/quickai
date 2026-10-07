//! The calls of an engine's `[models]` section, ready to make, and the names of models
//! (docs/phase3-test-plan.md, group M): which request lists what the engine has, which pulls a model
//! by name, which removes one, with the document's templates filled in. Pure like the planner: what
//! is made of a call, and the connection it goes over, are the application's.

use std::time::Duration;

use serde_json::Value as Json;

use crate::decode::Framing;
use crate::plan::resolve::base_context;
use crate::plan::{HostFacts, Selection};
use crate::registry::engine::CompiledStep;
use crate::registry::Engine;
use crate::stream::ProgressMap;
use crate::value::{map, Value};

/// One request to the engine, with the document's templates filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelCall {
    pub method: String,
    pub path: String,
    pub body: Option<Json>,
    pub timeout: Duration,
    /// How the reply is read: one body, or a stream of events.
    pub framing: Framing,
}

/// The request that lists the models and where each part of one is in the reply.
#[derive(Clone, Debug, PartialEq)]
pub struct ListCall {
    pub call: ModelCall,
    /// The array of models in the reply, as a JSON pointer.
    pub items: String,
    pub id: String,
    pub display: Option<String>,
    pub size_bytes: Option<String>,
    pub digest: Option<String>,
    pub capabilities: Option<String>,
    /// Where the last reply is kept, so a list can be shown while the engine is not running.
    pub cache: Option<String>,
}

/// The request that pulls a model and where its progress is in the events of the reply.
#[derive(Clone, Debug, PartialEq)]
pub struct PullCall {
    pub call: ModelCall,
    pub progress: ProgressMap,
}

/// Whether the engine's calls about its models need it running (a stopped engine can only show what
/// it listed last time). `false` for an engine with no `[models]`.
pub fn needs_server(engine: &Engine) -> bool {
    engine.models.as_ref().is_some_and(|m| m.needs_server)
}

fn render(step: &CompiledStep, context: &Value) -> Result<ModelCall, String> {
    let path = step.path.render(context).map_err(|e| e.message)?;
    let body = step
        .body
        .as_ref()
        .map(|b| b.render(context))
        .transpose()
        .map_err(|e| e.to_string())?;
    Ok(ModelCall {
        method: step.method.clone(),
        path: path.to_string(),
        body,
        timeout: step.timeout,
        framing: step.stream,
    })
}

/// What the call is about, for the templates that name `${ref}`.
fn about(reference: &str) -> Value {
    map([("ref", Value::Str(reference.to_string()))])
}

/// The call that lists the engine's models. `None` when its document says nothing about it.
pub fn list_call(engine: &Engine, facts: &HostFacts) -> Option<Result<ListCall, String>> {
    let list = engine.models.as_ref()?.list.as_ref()?;
    let context = base_context(&Selection::default(), facts);
    Some((|| {
        let cache = list
            .cache
            .as_ref()
            .map(|t| t.render(&context).map(|v| v.to_string()))
            .transpose()
            .map_err(|e| e.message)?;
        Ok(ListCall {
            call: render(&list.call, &context)?,
            items: list.items.clone(),
            id: list.id.clone(),
            display: list.display.clone(),
            size_bytes: list.size_bytes.clone(),
            digest: list.digest.clone(),
            capabilities: list.capabilities.clone(),
            cache,
        })
    })())
}

/// The call that pulls `reference`. `None` when the document says nothing about pulling.
pub fn pull_call(engine: &Engine, reference: &str) -> Option<Result<PullCall, String>> {
    let pull = engine.models.as_ref()?.pull.as_ref()?;
    Some(render(&pull.call, &about(reference)).map(|call| {
        let usual = ProgressMap::ollama_pull();
        let progress = match &pull.progress {
            Some(p) => ProgressMap {
                key: p.key.clone(),
                completed: p.completed.clone(),
                total: p.total.clone(),
                status: p.status.clone(),
                error: p.error.clone().unwrap_or(usual.error),
                done_status: p.done.clone().unwrap_or(usual.done_status),
            },
            None => usual,
        };
        PullCall { call, progress }
    }))
}

/// The call that removes `reference`. `None` when the document says nothing about removing.
pub fn remove_call(engine: &Engine, reference: &str) -> Option<Result<ModelCall, String>> {
    let remove = engine.models.as_ref()?.remove.as_ref()?;
    Some(render(remove, &about(reference)))
}

/// The name a model is listed under: the namespace every library model has (`library/`) and the tag
/// every model has (`latest`) are the same whether they are written or not, so `qwen3`,
/// `library/qwen3` and `qwen3:latest` are one model. A colon before the last slash belongs to a
/// host's port (`localhost:5000/team/m`), not to a tag.
pub fn canonical_name(name: &str) -> String {
    let name = name.trim();
    let (path, tag) = match name.rsplit_once(':') {
        Some((path, tag)) if !tag.contains('/') && !tag.is_empty() => (path, tag),
        _ => (name.trim_end_matches(':'), "latest"),
    };
    let path = path.strip_prefix("library/").unwrap_or(path);
    format!("{path}:{tag}")
}

/// Whether two names are the same model.
pub fn same_model(a: &str, b: &str) -> bool {
    canonical_name(a) == canonical_name(b)
}

/// Where in `listed` the model called `name` is, whichever way either is written.
pub fn find_listed<'a>(listed: impl IntoIterator<Item = &'a str>, name: &str) -> Option<usize> {
    listed.into_iter().position(|l| same_model(l, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_model_is_the_same_with_or_without_its_namespace_and_its_default_tag() {
        for (a, b) in [
            ("qwen3", "qwen3:latest"),
            ("library/qwen3", "qwen3"),
            ("library/qwen3:latest", "qwen3:latest"),
            ("  qwen3 ", "qwen3:latest"),
            (
                "hf.co/Qwen/Qwen3-4B-GGUF",
                "hf.co/Qwen/Qwen3-4B-GGUF:latest",
            ),
            ("localhost:5000/team/m", "localhost:5000/team/m:latest"),
        ] {
            assert!(same_model(a, b), "{a} and {b}");
            assert_eq!(canonical_name(a), canonical_name(b), "{a} and {b}");
        }
    }

    #[test]
    fn a_different_tag_or_name_is_a_different_model() {
        for (a, b) in [
            ("qwen3", "qwen3:4b"),
            ("qwen3:4b", "qwen3:8b"),
            ("qwen3", "qwen2"),
            ("Qwen3", "qwen3"),
            ("hf.co/a/b:Q4_K_M", "hf.co/a/b:Q8_0"),
            ("a/qwen3", "qwen3"),
            ("localhost:5000/m", "localhost:5001/m"),
        ] {
            assert!(!same_model(a, b), "{a} and {b}");
        }
    }

    #[test]
    fn a_name_is_found_among_the_listed_ones_by_what_it_means() {
        let listed = ["bonsai-q1:latest", "qwen3:4b"];
        assert_eq!(find_listed(listed, "bonsai-q1"), Some(0));
        assert_eq!(find_listed(listed, "library/qwen3:4b"), Some(1));
        assert_eq!(find_listed(listed, "qwen3"), None);
        assert_eq!(find_listed([], "qwen3"), None);
    }
}
