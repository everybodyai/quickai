//! What an engine that keeps its own models can be asked about them (docs/architecture.md, sections 8
//! and 9; docs/phase3-test-plan.md, rows H15 and H17): the list of what it has, kept in a cache
//! so that something can be shown when it is not running, and pulling a model by name as a job
//! with progress.
//!
//! The calls are the engine document's (`[models] list` and `pull`); this module reads their
//! replies. It is built on `http_call`, which is where the rules about where a call may go live.
//!
//! The model box and the list in the window use it from the engine selector (work package 3.7); until
//! then only the tests call it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use quickai_core::decode::Framing as DocFraming;
use quickai_core::models::{same_model, ListCall, ModelCall, PullCall};
use quickai_core::plan::Endpoint;
use quickai_core::stream::{ProgressMap, PullTracker, PullUpdate};
use regex::Regex;
use serde_json::{json, Value};

use super::http_call::{call, call_cancellable, stream, Call, CallError, Framing};
use crate::jobs::{Job, JobEvent};

fn framing_of(doc: DocFraming) -> Framing {
    match doc {
        DocFraming::Json => Framing::Json,
        DocFraming::Ndjson => Framing::Ndjson,
        DocFraming::Sse => Framing::Sse,
    }
}

/// One model an engine has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRow {
    /// The name the engine knows it by, which is what a request names (`qwen3:4b`).
    pub id: String,
    pub display: String,
    pub size_bytes: Option<u64>,
    pub digest: Option<String>,
    /// What it can do, as the engine says (`completion`, `vision`, `embedding`).
    pub capabilities: Vec<String>,
}

/// The list the engine gave, or the one remembered from the last time it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub rows: Vec<ModelRow>,
    /// What was wrong with parts of the answer (a row with no name), for the log.
    pub warnings: Vec<String>,
    /// The engine did not answer and this is what it said the last time.
    pub stale: bool,
}

/// Where the list is and where its parts are, as JSON pointers.
#[derive(Clone, Debug)]
pub struct ListSpec {
    pub method: String,
    pub path: String,
    /// The array of models in the reply (`/models`).
    pub items: String,
    pub id: String,
    pub display: Option<String>,
    pub size_bytes: Option<String>,
    pub digest: Option<String>,
    pub capabilities: Option<String>,
    /// Where the last answer is kept.
    pub cache: Option<PathBuf>,
    /// The names the engine's document says are its own bookkeeping, each matched against a whole name.
    pub hide: Vec<Regex>,
    pub timeout: Duration,
}

impl ListSpec {
    /// The call the engine's document describes, its templates filled in.
    pub fn from_call(list: &ListCall) -> ListSpec {
        ListSpec {
            method: list.call.method.clone(),
            path: list.call.path.clone(),
            items: list.items.clone(),
            id: list.id.clone(),
            display: list.display.clone(),
            size_bytes: list.size_bytes.clone(),
            digest: list.digest.clone(),
            capabilities: list.capabilities.clone(),
            cache: list.cache.as_ref().map(PathBuf::from),
            // the registry already refused a pattern that does not compile, so none is dropped here
            hide: list
                .hide
                .iter()
                .filter_map(|pattern| Regex::new(&format!("^(?:{pattern})$")).ok())
                .collect(),
            timeout: list.call.timeout,
        }
    }

    /// Ollama's `GET /api/tags`.
    pub fn ollama(cache: Option<PathBuf>) -> ListSpec {
        ListSpec {
            method: "GET".into(),
            path: "/api/tags".into(),
            items: "/models".into(),
            id: "/name".into(),
            display: Some("/name".into()),
            size_bytes: Some("/size".into()),
            digest: Some("/digest".into()),
            capabilities: Some("/capabilities".into()),
            cache,
            hide: Vec::new(),
            timeout: Duration::from_secs(5),
        }
    }
}

/// Asks the engine for its models. An answer is remembered; when the engine does not answer, what
/// was remembered is given back and marked as stale, and only when there is nothing the failure
/// is returned.
pub fn list_models(endpoint: &Endpoint, spec: &ListSpec) -> Result<Listing, CallError> {
    let asked = call(
        endpoint,
        &Call {
            method: spec.method.clone(),
            path: spec.path.clone(),
            body: None,
            framing: Framing::Json,
            timeout: spec.timeout,
        },
    );
    match asked {
        Ok(reply) => {
            let (rows, warnings) = read_rows(&reply.body, spec);
            if let Some(cache) = &spec.cache {
                // the list is a convenience: failing to keep it is not a failure to list
                let _ = write_cache(cache, &rows);
            }
            Ok(Listing {
                rows,
                warnings,
                stale: false,
            })
        }
        Err(error) => match spec.cache.as_deref().and_then(read_cache) {
            Some(rows) => Ok(Listing {
                rows,
                warnings: vec![format!(
                    "The engine did not answer ({error}); this is what it listed last time."
                )],
                stale: true,
            }),
            None => Err(error),
        },
    }
}

fn read_rows(body: &Value, spec: &ListSpec) -> (Vec<ModelRow>, Vec<String>) {
    let (mut rows, mut warnings): (Vec<ModelRow>, Vec<String>) = (Vec::new(), Vec::new());
    let items = body
        .pointer(&spec.items)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for (i, item) in items.iter().enumerate() {
        let at = |pointer: &str| item.pointer(pointer);
        let text = |pointer: &Option<String>| {
            pointer
                .as_deref()
                .and_then(at)
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .map(str::to_string)
        };
        let Some(id) = text(&Some(spec.id.clone())) else {
            warnings.push(format!(
                "Entry {} of the engine's model list has no name and was skipped.",
                i + 1
            ));
            continue;
        };
        // The engine's own bookkeeping is not a model to pick (the document's `hide`), and a name it
        // lists twice is one model: a request names it, so two rows could not be told apart.
        if spec.hide.iter().any(|pattern| pattern.is_match(&id)) || rows.iter().any(|r| r.id == id)
        {
            continue;
        }
        rows.push(ModelRow {
            display: text(&spec.display).unwrap_or_else(|| id.clone()),
            id,
            size_bytes: spec
                .size_bytes
                .as_deref()
                .and_then(at)
                .and_then(Value::as_u64),
            digest: text(&spec.digest),
            capabilities: spec
                .capabilities
                .as_deref()
                .and_then(at)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    (rows, warnings)
}

/// Keeps the list where it can be read back, written so that a crash in the middle leaves the
/// old file or the new one and never half of either.
fn write_cache(path: &Path, rows: &[ModelRow]) -> std::io::Result<()> {
    let value = json!({
        "version": 1,
        "rows": rows.iter().map(|r| json!({
            "id": r.id,
            "display": r.display,
            "size_bytes": r.size_bytes,
            "digest": r.digest,
            "capabilities": r.capabilities,
        })).collect::<Vec<_>>(),
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&value)?)?;
    std::fs::rename(&temporary, path)
}

/// The remembered list, or `None` when there is none or it cannot be read (a file that is not
/// ours is ignored, not trusted).
pub fn read_cache(path: &Path) -> Option<Vec<ModelRow>> {
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if value.get("version")?.as_u64()? != 1 {
        return None;
    }
    value
        .get("rows")?
        .as_array()?
        .iter()
        .map(|row| {
            Some(ModelRow {
                id: row.get("id")?.as_str()?.to_string(),
                display: row.get("display")?.as_str()?.to_string(),
                size_bytes: row.get("size_bytes").and_then(Value::as_u64),
                digest: row
                    .get("digest")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                capabilities: row
                    .get("capabilities")?
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}

/// The call that pulls a model and where its progress is in the answers.
#[derive(Clone, Debug)]
pub struct PullSpec {
    pub method: String,
    pub path: String,
    /// The body, with the name already in it, when the document says one; otherwise the name goes
    /// under `name_key`.
    pub body: Option<Value>,
    pub name_key: String,
    /// How the reply is read: a stream whose events carry the progress, or one body.
    pub framing: Framing,
    pub progress: ProgressMap,
    /// How long the stream may be silent: a registry that does not answer must not hang a pull.
    pub silence: Duration,
}

impl PullSpec {
    /// The call the engine's document describes for one name, its templates filled in.
    pub fn from_call(pull: &PullCall) -> PullSpec {
        PullSpec {
            method: pull.call.method.clone(),
            path: pull.call.path.clone(),
            body: pull.call.body.clone(),
            name_key: "model".into(),
            framing: framing_of(pull.call.framing),
            progress: pull.progress.clone(),
            silence: pull.call.timeout,
        }
    }

    /// Ollama's `POST /api/pull`.
    pub fn ollama() -> PullSpec {
        PullSpec {
            method: "POST".into(),
            path: "/api/pull".into(),
            body: None,
            name_key: "model".into(),
            framing: Framing::Ndjson,
            progress: ProgressMap::ollama_pull(),
            silence: Duration::from_secs(120),
        }
    }
}

/// Stops a pull that is running: the connection is closed, so the engine stops working on it.
#[derive(Clone, Debug)]
pub struct PullHandle {
    cancel: Arc<AtomicBool>,
}

impl PullHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Pulls `name` in the background: progress as a fraction that never goes back, the engine's own
/// words as statuses, `Done` when the engine says it has the model and `Failed` with its sentence
/// when it says it cannot, or with ours when the stream stops short or is cancelled.
pub fn spawn_pull(
    endpoint: Endpoint,
    spec: PullSpec,
    name: String,
    wake: impl Fn() + Send + Sync + 'static,
) -> (Job<()>, PullHandle) {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let label = name.clone();
    let job = Job::spawn(label, "Pulling", wake, move |send| {
        let request = Call {
            method: spec.method.clone(),
            path: spec.path.clone(),
            body: Some(
                spec.body
                    .clone()
                    .unwrap_or_else(|| json!({ spec.name_key.clone(): name })),
            ),
            framing: spec.framing,
            timeout: spec.silence,
        };
        let mut tracker = PullTracker::new(spec.progress.clone());
        let mut finished = false;
        let mut failed: Option<String> = None;
        let mut on_event = |event: Value| {
            for update in tracker.feed(&event) {
                match update {
                    PullUpdate::Status(text) => send(JobEvent::Status(text)),
                    PullUpdate::Progress(fraction) => send(JobEvent::Fraction(fraction)),
                    PullUpdate::Failed(message) => failed = Some(message),
                    PullUpdate::Finished => finished = true,
                }
            }
        };
        let outcome = if spec.framing == Framing::Json {
            // a pull that comes as one reply says all it has to say at the end
            call_cancellable(&endpoint, &request, &flag).map(|reply| on_event(reply.body))
        } else {
            stream(&endpoint, &request, &flag, &mut on_event)
        };
        match (outcome, failed) {
            // an event that says `error` ends the stream with the engine's sentence either way
            (_, Some(message)) => send(JobEvent::Failed(message)),
            (Err(CallError::Cancelled), _) => {
                send(JobEvent::Failed("The pull was cancelled.".into()))
            }
            (Err(error), _) => send(JobEvent::Failed(error.to_string())),
            (Ok(()), None) if finished => send(JobEvent::Done(())),
            (Ok(()), None) => send(JobEvent::Failed(
                "The engine stopped sending before the model was complete.".into(),
            )),
        }
    });
    (job, PullHandle { cancel })
}

/// The models to show: asked of the engine when it is running, and what it said the last time when
/// it is not, marked as remembered. A stopped engine is not asked, and not started to be asked:
/// that is for the person to do by pressing a button (`needs_server` in the document).
pub fn models_to_show(endpoint: Option<&Endpoint>, spec: &ListSpec) -> Result<Listing, CallError> {
    match endpoint {
        Some(endpoint) => list_models(endpoint, spec),
        None => Ok(Listing {
            rows: spec
                .cache
                .as_deref()
                .and_then(read_cache)
                .unwrap_or_default(),
            warnings: Vec::new(),
            stale: true,
        }),
    }
}

/// A removal that waits for the person to say yes. Dropping it is saying no: nothing was sent.
#[derive(Debug)]
pub struct PendingRemoval {
    /// The name the engine lists the model under, which is what the request names.
    pub name: String,
}

/// Starts to remove `name`, which the person must still confirm. The model that is loaded cannot be
/// removed (the engine would unload it behind the session's back): that is refused here, with a
/// sentence, and nothing is asked.
pub fn ask_to_remove(name: &str, loaded: Option<&str>) -> Result<PendingRemoval, String> {
    if loaded.is_some_and(|l| same_model(l, name)) {
        return Err(format!(
            "{name} is loaded. Stop the server first, then remove it."
        ));
    }
    Ok(PendingRemoval {
        name: name.to_string(),
    })
}

impl PendingRemoval {
    /// The person said yes: ask the engine to remove it, and give its own sentence when it will not.
    pub fn confirm(self, endpoint: &Endpoint, remove: &ModelCall) -> Result<(), String> {
        call(
            endpoint,
            &Call {
                method: remove.method.clone(),
                path: remove.path.clone(),
                body: remove.body.clone(),
                framing: Framing::Json,
                timeout: remove.timeout,
            },
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}
