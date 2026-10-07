//! The planner: documents plus a selection plus facts about the machine become a run plan
//! (docs/architecture.md, section 6.6).
//!
//! `plan` is a pure function. It reads nothing from the machine: everything it needs about the
//! machine comes in as `HostFacts`, everything the user chose as a `Selection`. The same inputs
//! always give the same plan, so plans can be snapshot-tested and shown to the user.

pub mod args;
pub mod env;
pub mod explain;
pub(crate) mod resolve;
pub mod types;

use std::collections::BTreeMap;

use crate::diag::{Diagnostic, Severity};
use crate::docpath::DocPath;
use crate::registry::engine::{
    CompiledAttach, CompiledEnv, CompiledField, CompiledSpawn, CompiledStep, LaunchStrategy,
};
use crate::registry::{Engine, Registry};
use crate::value::Value;

pub use resolve::Origin;
use resolve::{port_value, public_port, resolve_fields, Resolved};

pub use types::{
    apply_env, is_secret_name, AttachPlan, CallPlan, Endpoint, EnvMode, EnvOp, Gateway,
    IdentityPlan, LivenessPlan, ReadyPlan, RunPlan, SpawnPlan, Start, StopPlan, Traced, Unit,
};

/// The model a run is about.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    /// How the interface names it.
    pub display: String,
    /// The weights file (the `main` component).
    pub path: String,
    pub size_bytes: u64,
}

/// What the user chose: an engine, optionally a model, and values for fields. A saved selection is
/// a profile.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub engine: String,
    pub model: Option<ModelInfo>,
    /// Field values by `<engine>.<field>` (`llamacpp.port`), and app-level ones such as
    /// `app.traffic_proxy`.
    pub values: BTreeMap<String, Value>,
}

impl Selection {
    pub fn new(engine: &str) -> Self {
        Selection {
            engine: engine.to_string(),
            ..Default::default()
        }
    }

    pub fn with_model(mut self, model: ModelInfo) -> Self {
        self.model = Some(model);
        self
    }

    pub fn set(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.values.insert(key.to_string(), value.into());
        self
    }
}

/// What the machine and the application know, handed to the planner so it never looks.
#[derive(Clone, Debug, Default)]
pub struct HostFacts {
    pub os: String,
    pub arch: String,
    pub ram_bytes: u64,
    pub cpus: u32,
    pub gpu_known: bool,
    pub gpu_vendors: Vec<String>,
    /// The engine's executable, when one was found.
    pub binary: Option<String>,
    pub runtime_build: Option<i64>,
    pub runtime_version: Option<String>,
    pub runtime_flags: Vec<String>,
    pub data_dir: String,
    pub config_dir: String,
    pub exe_dir: String,
    pub home: String,
    /// A free port for the engine to listen on when a proxy sits in front of it. The application
    /// picks one before planning, only when it is needed.
    pub internal_port: Option<u16>,
    /// What a form may ask about the engine's state (`enabled_when = "not session.running"`). A
    /// plan is made to start an engine, so it leaves both false.
    pub session_running: bool,
    pub session_crashed: bool,
    /// The running engine is one QuickAI did not start (it attached to it): its environment belongs
    /// to whoever started it, so what only a spawn can set cannot be changed from here.
    pub session_attached: bool,
}

/// The app-level value that puts the live-traffic proxy in front of the engine.
pub const TRAFFIC_PROXY_KEY: &str = "app.traffic_proxy";

pub fn plan(
    registry: &Registry,
    selection: &Selection,
    facts: &HostFacts,
) -> Result<RunPlan, Vec<Diagnostic>> {
    let Some(engine) = registry.engine(&selection.engine) else {
        return Err(vec![Diagnostic::error(format!(
            "there is no engine called `{}`",
            selection.engine
        ))]);
    };
    let mut errors = Vec::new();

    // ---- ports ----
    let (resolved, mut context) = resolve_fields(engine, selection, facts);
    for r in &resolved {
        for (path, message) in &r.problems {
            errors.push(engine_error(engine, path.clone(), message.clone()));
        }
    }
    let public_port = public_port(engine, &resolved);
    let proxy_on = selection
        .values
        .get(TRAFFIC_PROXY_KEY)
        .is_some_and(Value::truthy);
    let mut gateway = None;
    let listen_port = if proxy_on {
        match (public_port, facts.internal_port) {
            (Some(public), Some(internal)) => {
                gateway = Some(Gateway {
                    public_port: public,
                    upstream_port: internal,
                });
                Some(internal)
            }
            (None, _) => {
                errors.push(Diagnostic::error(
                    "the live-traffic proxy needs a public port, and this engine has no port field",
                ));
                None
            }
            (_, None) => {
                errors.push(Diagnostic::error(
                    "the live-traffic proxy needs a free internal port, and none was provided",
                ));
                None
            }
        }
    } else {
        public_port
    };
    context.set_path(&["public_port"], port_value(public_port));
    context.set_path(&["listen_port"], port_value(listen_port));

    // ---- how to bring it up ----
    let mut start = Vec::new();
    for strategy in chosen_strategies(engine, &resolved, &mut errors) {
        match strategy {
            LaunchStrategy::AttachIfRunning => {
                if let Some(attach) = &engine.launch.attach {
                    if let Some(plan) = plan_attach(engine, attach, &context, &mut errors) {
                        start.push(Start::Attach(plan));
                    }
                }
            }
            LaunchStrategy::Spawn => {
                if let Some(spawn) = &engine.spawn {
                    if let Some(mut plan) =
                        plan_spawn(engine, spawn, &resolved, &context, selection, &mut errors)
                    {
                        // where an engine of this kind already answers, a second is not started
                        plan.identity = engine.launch.attach.as_ref().map(|a| IdentityPlan {
                            path: a.identity_path.clone(),
                            pointer: a.identity_pointer.clone(),
                        });
                        start.push(Start::Spawn(plan));
                    }
                }
            }
        }
    }
    if start.is_empty() && errors.is_empty() {
        errors.push(Diagnostic::error(
            "this engine has no way to be started with these settings",
        ));
    }

    // ---- loading and unloading the model ----
    let warm = plan_calls(engine, &engine.launch.warm, &context, &mut errors);
    let unload = plan_calls(engine, &engine.launch.unload, &context, &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    let ready = engine.ready.as_ref().map(|r| ReadyPlan {
        path: r.path.clone(),
        expect_status: r.expect_status,
        timeout_ms: r.timeout.as_millis() as u64,
        interval_ms: r.interval.as_millis() as u64,
        on_timeout: r.on_timeout,
    });
    let stop = engine
        .spawn
        .as_ref()
        .map(|s| StopPlan {
            policy: s.stop.policy,
            grace_ms: s.stop.grace.as_millis() as u64,
            exit_grace_ms: s.stop.exit_grace.as_millis() as u64,
        })
        .unwrap_or_default();
    Ok(RunPlan {
        engine: engine.id.clone(),
        model: selection.model.as_ref().map(|m| m.display.clone()),
        public_port,
        gateway,
        decode: if gateway.is_some() {
            // the APIs the document says the Live tab reads, else every one it serves
            let read = match &engine.doc.traffic {
                Some(traffic) => &traffic.apis,
                None => &engine.doc.provides.apis,
            };
            registry.decoders_for(read)
        } else {
            Vec::new()
        },
        units: vec![Unit {
            id: engine.id.clone(),
            engine: engine.id.clone(),
            endpoint: Endpoint {
                host: "127.0.0.1".to_string(),
                port: listen_port,
            },
            apis: engine.doc.provides.apis.clone(),
            server_name: engine
                .doc
                .engine
                .server_name
                .clone()
                .unwrap_or_else(|| engine.doc.engine.name.clone()),
            start,
            ready,
            warm,
            unload,
            stop,
        }],
    })
}

/// The launch strategies this selection uses: all the document lists, in its order, unless a
/// strategy field narrows them (`auto` keeps both, `attach` and `spawn` one each).
fn chosen_strategies(
    engine: &Engine,
    resolved: &[Resolved],
    errors: &mut Vec<Diagnostic>,
) -> Vec<LaunchStrategy> {
    let all = engine.launch.strategies.clone();
    let Some(field) = &engine.launch.strategy_field else {
        return all;
    };
    let value = engine
        .fields
        .iter()
        .position(|f| &f.id == field)
        .map(|i| resolved[i].value.to_string())
        .unwrap_or_default();
    let only = |wanted: LaunchStrategy, errors: &mut Vec<Diagnostic>| {
        if all.contains(&wanted) {
            vec![wanted]
        } else {
            errors.push(engine_error(
                engine,
                DocPath::default().key("launch").key("strategies"),
                format!("`{value}` was chosen, but this engine cannot be launched that way"),
            ));
            Vec::new()
        }
    };
    match value.as_str() {
        "attach" => only(LaunchStrategy::AttachIfRunning, errors),
        "spawn" => only(LaunchStrategy::Spawn, errors),
        _ => all,
    }
}

/// An endpoint written `host:port` once its template is filled in.
fn plan_attach(
    engine: &Engine,
    attach: &CompiledAttach,
    context: &Value,
    errors: &mut Vec<Diagnostic>,
) -> Option<AttachPlan> {
    let at = DocPath::default().key("launch").key("attach");
    let text = match attach.endpoint.render_string(context) {
        Ok(text) => text,
        Err(e) => {
            errors.push(engine_error(engine, at.key("endpoint"), e.message));
            return None;
        }
    };
    let Some((host, port)) = text.rsplit_once(':') else {
        errors.push(engine_error(
            engine,
            at.key("endpoint"),
            format!("`{text}` is not host:port"),
        ));
        return None;
    };
    let Ok(port) = port.parse::<u16>() else {
        errors.push(engine_error(
            engine,
            at.key("endpoint"),
            format!("`{text}` has no usable port (the engine has no port field, or it is empty)"),
        ));
        return None;
    };
    let liveness = attach
        .liveness
        .unwrap_or(crate::registry::engine::CompiledLiveness {
            every: crate::registry::engine::DEFAULT_LIVENESS_EVERY,
            failures: crate::registry::engine::DEFAULT_LIVENESS_FAILURES,
        });
    Some(AttachPlan {
        endpoint: Endpoint {
            host: host.to_string(),
            port: Some(port),
        },
        identity: IdentityPlan {
            path: attach.identity_path.clone(),
            pointer: attach.identity_pointer.clone(),
        },
        liveness: LivenessPlan {
            path: engine
                .doc
                .launch
                .attach
                .as_ref()
                .and_then(|a| a.liveness.as_ref())
                .map_or_else(|| attach.identity_path.clone(), |l| l.path.clone()),
            every_ms: liveness.every.as_millis() as u64,
            failures: liveness.failures,
        },
    })
}

/// The calls of a phase whose condition holds, with their paths and bodies filled in.
fn plan_calls(
    engine: &Engine,
    steps: &[CompiledStep],
    context: &Value,
    errors: &mut Vec<Diagnostic>,
) -> Vec<CallPlan> {
    let mut calls = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        let at = DocPath::default().key("launch").index(i);
        if let Some(when) = &step.when {
            match crate::expr::eval(when, context) {
                Ok(v) if v.truthy() => {}
                Ok(_) => continue,
                Err(e) => {
                    errors.push(engine_error(engine, at.key("when"), e.message));
                    continue;
                }
            }
        }
        let path = match step.path.render_string(context) {
            Ok(p) => p,
            Err(e) => {
                errors.push(engine_error(
                    engine,
                    at.clone().key("call").key("path"),
                    e.message,
                ));
                continue;
            }
        };
        let body = match &step.body {
            None => serde_json::Value::Null,
            Some(body) => match body.render(context) {
                Ok(v) => v,
                Err(message) => {
                    errors.push(engine_error(
                        engine,
                        at.clone().key("call").key("body"),
                        message,
                    ));
                    continue;
                }
            },
        };
        calls.push(CallPlan {
            method: step.method.clone(),
            path,
            body,
            timeout_ms: step.timeout.as_millis() as u64,
            best_effort: step.best_effort,
        });
    }
    calls
}

fn engine_error(engine: &Engine, path: DocPath, message: impl Into<String>) -> Diagnostic {
    engine.merged.diagnostic(Severity::Error, &path, message)
}

/// Where a field's contribution to the plan comes from, as `--explain` says it.
fn field_origin(engine: &Engine, field: &CompiledField, resolved: &Resolved) -> String {
    match resolved.origin.note() {
        Some(note) => format!("field {}.{} ({note})", engine.id, field.id),
        None => format!("field {}.{}", engine.id, field.id),
    }
}

/// A template filled in. A hole that is all of an argument and has no value (no model chosen, no
/// port) would become an empty argument; that is a mistake to report, not to pass on.
fn render_text(
    engine: &Engine,
    template: &crate::expr::Template,
    path: DocPath,
    ctx: &Value,
    errors: &mut Vec<Diagnostic>,
) -> Option<String> {
    match template.render(ctx) {
        Ok(Value::Null) if template.lone_hole().is_some() => {
            errors.push(engine_error(
                engine,
                path,
                format!(
                    "`${{{}}}` has no value here",
                    template.lone_hole().unwrap_or_default()
                ),
            ));
            None
        }
        Ok(v) => Some(v.to_string()),
        Err(e) => {
            errors.push(engine_error(engine, path, e.message));
            None
        }
    }
}

/// One entry of an `env` list as an operation, when its condition holds. `ctx` has what the
/// entry's templates may name (for a field's entry, `value`).
fn plan_env_entry(
    engine: &Engine,
    entry: &CompiledEnv,
    path: DocPath,
    ctx: &Value,
    origin: String,
    errors: &mut Vec<Diagnostic>,
) -> Option<EnvOp> {
    if let Some(when) = &entry.when {
        match crate::expr::eval(when, ctx) {
            Ok(v) if v.truthy() => {}
            Ok(_) => return None,
            Err(e) => {
                errors.push(engine_error(engine, path, e.message));
                return None;
            }
        }
    }
    if let Some(value) = &entry.value {
        let text = render_text(engine, value, path, ctx, errors)?;
        return Some(EnvOp {
            name: entry.name.clone(),
            mode: EnvMode::Set { value: text },
            origin,
        });
    }
    let prepend = entry.prepend.as_ref()?;
    let text = render_text(engine, prepend, path, ctx, errors)?;
    (!text.is_empty()).then(|| EnvOp {
        name: entry.name.clone(),
        mode: EnvMode::Prepend {
            value: text,
            sep: entry.sep.clone(),
        },
        origin,
    })
}

fn plan_spawn(
    engine: &Engine,
    spawn: &CompiledSpawn,
    resolved: &[Resolved],
    context: &Value,
    selection: &Selection,
    errors: &mut Vec<Diagnostic>,
) -> Option<SpawnPlan> {
    let render =
        |template: &crate::expr::Template,
         path: DocPath,
         ctx: &Value,
         errors: &mut Vec<Diagnostic>| { render_text(engine, template, path, ctx, errors) };
    let spawn_path = DocPath::default().key("launch").key("spawn");

    // The program has its own message: a missing executable is the commonest way to get here.
    let program = match spawn.program.render(context) {
        Ok(value) if !value.truthy() => {
            errors.push(engine_error(
                engine,
                spawn_path.clone().key("program"),
                "there is no program to start: the engine's executable was not found",
            ));
            return None;
        }
        Ok(value) => value.to_string(),
        Err(e) => {
            errors.push(engine_error(
                engine,
                spawn_path.clone().key("program"),
                e.message,
            ));
            return None;
        }
    };

    let mut arguments: Vec<Traced<String>> = Vec::new();
    for template in &spawn.args {
        if let Some(text) = render(template, spawn_path.clone().key("args"), context, errors) {
            arguments.push(Traced {
                value: text,
                origin: "engine launch.spawn.args".to_string(),
            });
        }
    }
    if selection.model.is_none()
        && engine
            .fields
            .iter()
            .all(|f| f.public_port || f.arg.is_none() && !f.arg_split)
    {
        // nothing more to say: an engine that runs without a model is planned as it is
    }

    // Field bindings go in `order`, then document order.
    let mut bound: Vec<(usize, &CompiledField)> = engine
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| f.arg.is_some() || f.arg_split)
        .collect();
    bound.sort_by_key(|(i, f)| (f.order, *i));
    for (i, field) in bound {
        let r = &resolved[i];
        if r.value.is_null() {
            continue; // a field with no value adds no arguments
        }
        let origin = field_origin(engine, field, r);
        let mut ctx = context.clone();
        ctx.set_path(&["value"], r.value.clone());
        if let Some(templates) = &field.arg {
            for template in templates {
                let path = DocPath::default()
                    .key("field")
                    .id(&field.id)
                    .key("bind")
                    .key("arg");
                if let Some(text) = render(template, path, &ctx, errors) {
                    arguments.push(Traced {
                        value: text,
                        origin: origin.clone(),
                    });
                }
            }
        }
        if field.arg_split {
            for part in args::split_args(&r.value.to_string()) {
                arguments.push(Traced {
                    value: part,
                    origin: origin.clone(),
                });
            }
        }
    }

    let mut env = Vec::new();
    for (i, entry) in spawn.env.iter().enumerate() {
        let path = spawn_path.clone().key("env").index(i);
        let origin = "engine launch.spawn.env".to_string();
        env.extend(plan_env_entry(engine, entry, path, context, origin, errors));
    }

    // What the fields decide, in the order of their other bindings; a field with no value decides
    // nothing (an optional context size left empty sets no variable).
    let mut decided: Vec<(usize, &CompiledField)> = engine
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.env.is_empty())
        .collect();
    decided.sort_by_key(|(i, f)| (f.order, *i));
    for (i, field) in decided {
        let r = &resolved[i];
        if r.value.is_null() {
            continue;
        }
        let mut ctx = context.clone();
        ctx.set_path(&["value"], r.value.clone());
        let origin = field_origin(engine, field, r);
        for (k, entry) in field.env.iter().enumerate() {
            let path = DocPath::default()
                .key("field")
                .id(&field.id)
                .key("bind")
                .key("env")
                .index(k);
            env.extend(plan_env_entry(
                engine,
                entry,
                path,
                &ctx,
                origin.clone(),
                errors,
            ));
        }
    }

    // The person's own lines come last. A line for a variable that something earlier set (not
    // locked: those were refused) replaces it where it stood; the rest follow.
    let windows = context.lookup(&["os"]).and_then(Value::as_str) == Some("windows");
    for (i, field) in engine.fields.iter().enumerate() {
        let Some(protect) = &field.env_lines else {
            continue;
        };
        let text = resolved[i].value.to_string();
        for (name, value) in env::user_lines(engine, protect, &text, windows).entries {
            let op = EnvOp {
                name,
                mode: EnvMode::Set { value },
                origin: field_origin(engine, field, &resolved[i]),
            };
            let same = |a: &str, b: &str| {
                if windows {
                    a.eq_ignore_ascii_case(b)
                } else {
                    a == b
                }
            };
            match env
                .iter_mut()
                .find(|e| same(&e.name, &op.name) && matches!(e.mode, EnvMode::Set { .. }))
            {
                Some(earlier) => *earlier = op,
                None => env.push(op),
            }
        }
    }

    Some(SpawnPlan {
        program: Traced {
            value: program,
            origin: "engine launch.spawn.program".to_string(),
        },
        args: arguments,
        env,
        group: spawn.group,
        // A program QuickAI does not ship is a downloaded one, with libraries of its own.
        scrub_loader_env: !engine.doc.engine.bundled,
        identity: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fixtures::good_registry;

    fn facts() -> HostFacts {
        HostFacts {
            os: "linux".into(),
            arch: "x86_64".into(),
            ram_bytes: 32 * 1024 * 1024 * 1024,
            cpus: 8,
            binary: Some("/opt/quickai/llama-server".into()),
            ..Default::default()
        }
    }

    fn model() -> ModelInfo {
        ModelInfo {
            id: "bonsai-8b".into(),
            display: "Bonsai 8B".into(),
            path: "/opt/quickai/models/Bonsai-8B-Q1_0.gguf".into(),
            size_bytes: 1024 * 1024 * 1024,
        }
    }

    fn selection() -> Selection {
        Selection::new("llamacpp")
            .with_model(model())
            .set("llamacpp.port", 8080u16)
            .set("llamacpp.ctx_size", 4096u32)
            .set("llamacpp.n_gpu_layers", 999u32)
            .set("llamacpp.extra_args", "")
    }

    fn spawn_of(plan: &RunPlan) -> &SpawnPlan {
        let Start::Spawn(s) = &plan.units[0].start[0] else {
            panic!("a spawn plan")
        };
        s
    }

    #[test]
    fn the_arguments_come_out_in_todays_order() {
        let plan = plan(&good_registry(), &selection(), &facts()).unwrap();
        let spawn = spawn_of(&plan);
        assert_eq!(spawn.program.value, "/opt/quickai/llama-server");
        assert_eq!(
            spawn.arg_values(),
            vec![
                "--model",
                "/opt/quickai/models/Bonsai-8B-Q1_0.gguf",
                "--host",
                "127.0.0.1",
                "--port",
                "8080",
                "--ctx-size",
                "4096",
                "--n-gpu-layers",
                "999"
            ]
        );
        assert_eq!(plan.public_port, Some(8080));
        assert_eq!(plan.gateway, None);
        assert_eq!(plan.units[0].endpoint.port, Some(8080));
        assert_eq!(plan.model.as_deref(), Some("Bonsai 8B"));
    }

    #[test]
    fn extra_arguments_come_last_and_are_split_like_a_shell() {
        let sel = selection().set("llamacpp.extra_args", "--threads 8 --lora \"/a b/c.gguf\"");
        let plan = plan(&good_registry(), &sel, &facts()).unwrap();
        let args = spawn_of(&plan).arg_values();
        assert_eq!(&args[10..], ["--threads", "8", "--lora", "/a b/c.gguf"]);
    }

    #[test]
    fn origins_say_where_each_argument_came_from() {
        let plan = plan(&good_registry(), &selection(), &facts()).unwrap();
        let spawn = spawn_of(&plan);
        assert_eq!(spawn.args[0].origin, "engine launch.spawn.args");
        assert_eq!(spawn.args[6].value, "--ctx-size");
        assert_eq!(spawn.args[6].origin, "field llamacpp.ctx_size");
        assert_eq!(spawn.program.origin, "engine launch.spawn.program");
    }

    #[test]
    fn a_missing_value_falls_back_to_the_default_and_says_so() {
        let sel = Selection::new("llamacpp").with_model(model());
        let plan = plan(&good_registry(), &sel, &facts()).unwrap();
        let spawn = spawn_of(&plan);
        let args = spawn.arg_values();
        assert_eq!(args[5], "8080", "the port default");
        assert_eq!(
            args[7], "16384",
            "recommend_ctx_size for a 1 GB model on 32 GB"
        );
        assert_eq!(args[9], "999");
        assert_eq!(
            spawn.args[7].origin,
            "field llamacpp.ctx_size (default: recommend_ctx_size)"
        );
        assert_eq!(
            spawn.args[9].origin,
            "field llamacpp.n_gpu_layers (default)"
        );
        assert_eq!(args.len(), 10, "an empty extra_args default adds nothing");
    }

    #[test]
    fn the_loader_path_is_prepended_to_whatever_was_inherited() {
        let plan = plan(&good_registry(), &selection(), &facts()).unwrap();
        let spawn = spawn_of(&plan);
        assert_eq!(spawn.env.len(), 1);
        assert_eq!(
            spawn.env[0].mode,
            EnvMode::Prepend {
                value: "/opt/quickai".into(),
                sep: ":".into()
            }
        );
        let resolved = apply_env(&spawn.env, |_| Some("/usr/lib".to_string()));
        assert_eq!(
            resolved,
            vec![(
                "LD_LIBRARY_PATH".to_string(),
                "/opt/quickai:/usr/lib".to_string()
            )]
        );
    }

    #[test]
    fn the_proxy_puts_the_engine_on_the_internal_port() {
        let mut f = facts();
        f.internal_port = Some(41234);
        let sel = selection()
            .set("llamacpp.port", 9000u16)
            .set(TRAFFIC_PROXY_KEY, true);
        let plan = plan(&good_registry(), &sel, &f).unwrap();
        assert_eq!(plan.public_port, Some(9000));
        assert_eq!(
            plan.gateway,
            Some(Gateway {
                public_port: 9000,
                upstream_port: 41234
            })
        );
        assert_eq!(plan.units[0].endpoint.port, Some(41234));
        assert_eq!(spawn_of(&plan).arg_values()[5], "41234");
    }

    #[test]
    fn the_proxy_without_a_free_port_is_an_error() {
        let sel = selection().set(TRAFFIC_PROXY_KEY, true);
        let errors = plan(&good_registry(), &sel, &facts()).unwrap_err();
        assert!(
            errors[0].message.contains("free internal port"),
            "{}",
            errors[0]
        );
    }

    #[test]
    fn readiness_and_stop_come_from_the_document() {
        let plan = plan(&good_registry(), &selection(), &facts()).unwrap();
        let ready = plan.units[0].ready.as_ref().unwrap();
        assert_eq!(
            ready,
            &ReadyPlan {
                path: "/health".into(),
                expect_status: 200,
                timeout_ms: 120_000,
                interval_ms: 500,
                on_timeout: crate::doc::engine::OnTimeout::Fail,
            }
        );
        assert_eq!(plan.units[0].stop, StopPlan::default());
        assert_eq!(plan.units[0].engine, "llamacpp");
    }

    #[test]
    fn no_binary_is_an_error_that_points_at_the_document() {
        let mut f = facts();
        f.binary = None;
        let errors = plan(&good_registry(), &selection(), &f).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.contains("executable was not found"),
            "{}",
            errors[0]
        );
        assert_eq!(errors[0].path, "launch.spawn.program");
        assert!(errors[0].line.is_some());
    }

    #[test]
    fn an_unknown_engine_is_an_error() {
        let errors = plan(&good_registry(), &Selection::new("nope"), &facts()).unwrap_err();
        assert!(errors[0].message.contains("no engine called `nope`"));
    }

    #[test]
    fn a_value_of_the_wrong_type_is_an_error_not_a_silent_default() {
        for (key, value, needle) in [
            (
                "llamacpp.port",
                Value::from("abc"),
                "a port is a whole number, found string",
            ),
            (
                "llamacpp.port",
                Value::from(70000u32),
                "70000 is not a port",
            ),
            (
                "llamacpp.ctx_size",
                Value::from("big"),
                "expected a whole number, found string",
            ),
            (
                "llamacpp.extra_args",
                Value::from(5u32),
                "expected text, found int",
            ),
        ] {
            let sel = selection().set(key, value);
            let errors = plan(&good_registry(), &sel, &facts()).unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|e| e.message.contains(needle) && e.message.contains(key)),
                "{key}: {errors:?}"
            );
            assert!(errors[0].path.starts_with("field["), "{}", errors[0].path);
        }
        // null means "not set": the default applies
        let sel = selection().set("llamacpp.n_gpu_layers", Value::Null);
        assert!(plan(&good_registry(), &sel, &facts()).is_ok());
    }

    #[test]
    fn a_null_in_the_selection_means_not_set_so_the_default_applies() {
        let sel = selection().set("llamacpp.n_gpu_layers", Value::Null);
        let plan = plan(&good_registry(), &sel, &facts()).unwrap();
        let spawn = spawn_of(&plan);
        assert_eq!(spawn.args[8].value, "--n-gpu-layers");
        assert_eq!(spawn.args[9].value, "999");
        assert_eq!(
            spawn.args[9].origin,
            "field llamacpp.n_gpu_layers (default)"
        );
    }

    #[test]
    fn an_argument_with_no_value_is_an_error() {
        // no model: `--model ${model.main.path}` would be an empty argument
        let sel = Selection::new("llamacpp").set("llamacpp.port", 8080u16);
        let errors = plan(&good_registry(), &sel, &facts()).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("`${model.main.path}` has no value here")),
            "{errors:?}"
        );
    }

    #[test]
    fn planning_is_deterministic() {
        let a = serde_json::to_string(&plan(&good_registry(), &selection(), &facts()).unwrap())
            .unwrap();
        let b = serde_json::to_string(&plan(&good_registry(), &selection(), &facts()).unwrap())
            .unwrap();
        assert_eq!(a, b);
    }
}
