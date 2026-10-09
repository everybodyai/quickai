//! What the planner produces: a run plan, plain data that can be shown, compared and executed.
//!
//! A plan is serializable, so it can be snapshot-tested, printed by `plan --explain` and run
//! headless. Every command-line argument and environment entry carries its *origin*: the part of
//! the engine document or the field that put it there.

use serde::Serialize;

use crate::decode::ApiDecoder;
use crate::doc::engine::{OnTimeout, StopPolicy};

/// A value with the reason it is what it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Traced<T> {
    pub value: T,
    pub origin: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunPlan {
    pub engine: String,
    /// What to call the loaded model in the interface; `None` for a plan with no model.
    pub model: Option<String>,
    /// The port clients connect to.
    pub public_port: Option<u16>,
    /// Present when something sits in front of the engine (the live-traffic proxy): it binds the
    /// public port and forwards to the engine's own.
    pub gateway: Option<Gateway>,
    /// How the gateway reads the traffic of the APIs the engine serves, from the vocabulary; empty
    /// without a gateway.
    pub decode: Vec<ApiDecoder>,
    /// One unit in this version; a profile with several arrives later.
    pub units: Vec<Unit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Gateway {
    pub public_port: u16,
    pub upstream_port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Unit {
    pub id: String,
    pub engine: String,
    /// Where the engine itself listens.
    pub endpoint: Endpoint,
    /// API ids the unit serves once it is up.
    pub apis: Vec<String>,
    /// What the interface calls the engine's server in a sentence (`Ollama`, `llama-server`).
    pub server_name: String,
    /// Ways to bring it up, in the order to try them. The session chooses when it starts, because
    /// whether something already answers on the port is not known to a pure planner.
    pub start: Vec<Start>,
    /// How to tell it is ready.
    pub ready: Option<ReadyPlan>,
    /// Calls that load the model once the engine answers; none when no model is selected
    /// (management mode: the engine runs with nothing loaded).
    pub warm: Vec<CallPlan>,
    /// Calls that free the model before the engine is stopped or left.
    pub unload: Vec<CallPlan>,
    pub stop: StopPlan,
}

/// One request to the engine, with its body filled in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CallPlan {
    pub method: String,
    pub path: String,
    /// `null` for a call with no body.
    pub body: serde_json::Value,
    pub timeout_ms: u64,
    /// A failure is logged and not acted on.
    pub best_effort: bool,
}

/// How a run is ended: killed at once, or asked to leave and killed when it has not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StopPlan {
    pub policy: StopPolicy,
    /// A graceful stop waits this long for the program to exit before it kills what is left.
    pub grace_ms: u64,
    /// The same wait when the application itself is closing.
    pub exit_grace_ms: u64,
}

impl Default for StopPlan {
    fn default() -> Self {
        StopPlan {
            policy: StopPolicy::Kill,
            grace_ms: 0,
            exit_grace_ms: 0,
        }
    }
}

impl StopPlan {
    pub fn describe(&self) -> String {
        match self.policy {
            StopPolicy::Kill => "kill".to_string(),
            StopPolicy::Graceful => format!(
                "graceful ({} ms to leave, {} ms when closing)",
                self.grace_ms, self.exit_grace_ms
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Endpoint {
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "how", rename_all = "snake_case")]
pub enum Start {
    /// Use an engine that already answers where `endpoint` says.
    Attach(AttachPlan),
    Spawn(SpawnPlan),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AttachPlan {
    pub endpoint: Endpoint,
    pub identity: IdentityPlan,
    pub liveness: LivenessPlan,
}

/// A request that only this engine answers in a known way: the answer is JSON and holds a string at
/// `pointer` (its version).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IdentityPlan {
    pub path: String,
    pub pointer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LivenessPlan {
    pub path: String,
    pub every_ms: u64,
    /// Misses in a row after which the engine is taken to be gone.
    pub failures: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SpawnPlan {
    pub program: Traced<String>,
    pub args: Vec<Traced<String>>,
    pub env: Vec<EnvOp>,
    /// Start it in a process group of its own (Unix) or a job (Windows), so that stopping it takes
    /// its children with it.
    pub group: bool,
    /// When set, a port where this answers has an engine of this kind already running, and starting
    /// a second one there is refused.
    pub identity: Option<IdentityPlan>,
    /// Do not hand it the loader variables of QuickAI's own package (an AppImage's or a snap's
    /// library directories): it is a program that was downloaded, with libraries of its own.
    pub scrub_loader_env: bool,
    /// Folders to make, with the ones above them, before it starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mkdir: Vec<String>,
    /// Files of the application's own to put where it reads them, before it starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write: Vec<WriteStep>,
}

/// A file or folder of the application's own, and where it goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WriteStep {
    /// Its name among the application's own files.
    pub asset: String,
    /// Where it is put.
    pub to: String,
}

impl SpawnPlan {
    /// The command as it is logged first: `$ program arg arg ...`.
    pub fn command_line(&self) -> String {
        let args: Vec<&str> = self.args.iter().map(|a| a.value.as_str()).collect();
        format!("$ {} {}", self.program.value, args.join(" "))
    }

    /// The second line of the log: the variables the plan sets (not the ones it only prepends to,
    /// which a loader path always is), `env NAME=value ...`, with the value of anything that
    /// looks like a secret hidden. `None` when it sets none, so a plan like llama.cpp's logs
    /// exactly the one line it always did.
    pub fn env_line(&self) -> Option<String> {
        let shown: Vec<String> = self
            .env
            .iter()
            .filter_map(|op| match &op.mode {
                EnvMode::Set { value } if is_secret_name(&op.name) => {
                    let _ = value;
                    Some(format!("{}=***", op.name))
                }
                EnvMode::Set { value }
                    if value.is_empty() || value.contains(char::is_whitespace) =>
                {
                    Some(format!("{}=\"{}\"", op.name, value))
                }
                EnvMode::Set { value } => Some(format!("{}={}", op.name, value)),
                EnvMode::Prepend { .. } => None,
            })
            .collect();
        (!shown.is_empty()).then(|| format!("env {}", shown.join(" ")))
    }

    pub fn arg_values(&self) -> Vec<String> {
        self.args.iter().map(|a| a.value.clone()).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EnvOp {
    pub name: String,
    pub mode: EnvMode,
    pub origin: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EnvMode {
    Set {
        value: String,
    },
    /// In front of whatever the variable already holds, joined by `sep`.
    Prepend {
        value: String,
        sep: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReadyPlan {
    pub path: String,
    pub expect_status: u16,
    pub timeout_ms: u64,
    pub interval_ms: u64,
    pub on_timeout: OnTimeout,
}

/// Whether a variable's name says its value is a secret (`OLLAMA_API_KEY`, `HF_TOKEN`): such a value
/// is never written to the log.
pub fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD"]
        .iter()
        .any(|word| upper.contains(word))
}

/// The variables a plan sets, resolved against the environment the application was started with:
/// `Set` replaces, `Prepend` goes in front of the inherited value (or stands alone when there is
/// none or it is empty).
pub fn apply_env(
    ops: &[EnvOp],
    inherited: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    ops.iter()
        .map(|op| {
            let value = match &op.mode {
                EnvMode::Set { value } => value.clone(),
                EnvMode::Prepend { value, sep } => match inherited(&op.name) {
                    Some(existing) if !existing.is_empty() => format!("{value}{sep}{existing}"),
                    _ => value.clone(),
                },
            };
            (op.name.clone(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops() -> Vec<EnvOp> {
        vec![
            EnvOp {
                name: "LD_LIBRARY_PATH".into(),
                mode: EnvMode::Prepend {
                    value: "/opt/llama".into(),
                    sep: ":".into(),
                },
                origin: "engine launch.spawn.env".into(),
            },
            EnvOp {
                name: "OLLAMA_HOST".into(),
                mode: EnvMode::Set {
                    value: "127.0.0.1:11434".into(),
                },
                origin: "engine launch.spawn.env".into(),
            },
        ]
    }

    #[test]
    fn prepend_goes_in_front_of_what_is_inherited() {
        let env = apply_env(&ops(), |name| {
            (name == "LD_LIBRARY_PATH").then(|| "/usr/lib:/lib".to_string())
        });
        assert_eq!(
            env[0],
            ("LD_LIBRARY_PATH".into(), "/opt/llama:/usr/lib:/lib".into())
        );
        assert_eq!(env[1], ("OLLAMA_HOST".into(), "127.0.0.1:11434".into()));
    }

    #[test]
    fn prepend_stands_alone_when_nothing_is_inherited() {
        for inherited in [None, Some(String::new())] {
            let env = apply_env(&ops(), |_| inherited.clone());
            assert_eq!(env[0].1, "/opt/llama");
        }
    }

    #[test]
    fn the_command_line_is_what_the_log_shows() {
        let spawn = SpawnPlan {
            program: Traced {
                value: "/opt/llama/llama-server".into(),
                origin: "engine".into(),
            },
            args: vec![
                Traced {
                    value: "--model".into(),
                    origin: "x".into(),
                },
                Traced {
                    value: "/m/q.gguf".into(),
                    origin: "x".into(),
                },
            ],
            env: vec![],
            group: false,
            scrub_loader_env: false,
            mkdir: Vec::new(),
            write: Vec::new(),
            identity: None,
        };
        assert_eq!(
            spawn.command_line(),
            "$ /opt/llama/llama-server --model /m/q.gguf"
        );
        assert_eq!(spawn.arg_values(), vec!["--model", "/m/q.gguf"]);
    }

    fn spawn_with(env: Vec<EnvOp>) -> SpawnPlan {
        SpawnPlan {
            program: Traced {
                value: "/opt/ollama".into(),
                origin: "o".into(),
            },
            args: vec![],
            env,
            group: true,
            scrub_loader_env: true,
            mkdir: Vec::new(),
            write: Vec::new(),
            identity: None,
        }
    }

    fn set(name: &str, value: &str) -> EnvOp {
        EnvOp {
            name: name.into(),
            mode: EnvMode::Set {
                value: value.into(),
            },
            origin: "test".into(),
        }
    }

    #[test]
    fn the_environment_line_lists_what_the_plan_sets_and_nothing_it_prepends_to() {
        // llama.cpp's plan only prepends to the loader path: its log keeps its one line
        let prepend_only = spawn_with(vec![EnvOp {
            name: "LD_LIBRARY_PATH".into(),
            mode: EnvMode::Prepend {
                value: "/opt".into(),
                sep: ":".into(),
            },
            origin: "test".into(),
        }]);
        assert_eq!(prepend_only.env_line(), None);
        assert_eq!(spawn_with(vec![]).env_line(), None);

        let both = spawn_with(vec![
            set("OLLAMA_HOST", "127.0.0.1:11434"),
            EnvOp {
                name: "PATH".into(),
                mode: EnvMode::Prepend {
                    value: "/x".into(),
                    sep: ":".into(),
                },
                origin: "test".into(),
            },
            set("OLLAMA_KEEP_ALIVE", "-1"),
        ]);
        assert_eq!(
            both.env_line().as_deref(),
            Some("env OLLAMA_HOST=127.0.0.1:11434 OLLAMA_KEEP_ALIVE=-1")
        );
    }

    #[test]
    fn a_secret_is_never_written_to_the_log_and_odd_values_are_quoted() {
        let line = spawn_with(vec![
            set("OLLAMA_API_KEY", "hunter2"),
            set("hf_token", "hf_abc"),
            set("DB_PASSWORD", "p"),
            set("CLIENT_SECRET", "s"),
            set("OLLAMA_DEBUG", "1"),
            set("OLLAMA_ORIGINS", "http://a b"),
            set("EMPTY", ""),
        ])
        .env_line()
        .unwrap();
        assert_eq!(
            line,
            "env OLLAMA_API_KEY=*** hf_token=*** DB_PASSWORD=*** CLIENT_SECRET=*** \
             OLLAMA_DEBUG=1 OLLAMA_ORIGINS=\"http://a b\" EMPTY=\"\""
        );
        for secret in ["hunter2", "hf_abc"] {
            assert!(!line.contains(secret));
        }
        // the names that are not secrets
        for name in ["OLLAMA_KEEP_ALIVE", "OLLAMA_HOST", "CUDA_VISIBLE_DEVICES"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn a_stop_is_described_for_the_explain_output() {
        assert_eq!(StopPlan::default().describe(), "kill");
        let graceful = StopPlan {
            policy: StopPolicy::Graceful,
            grace_ms: 10_000,
            exit_grace_ms: 2_000,
        };
        assert_eq!(
            graceful.describe(),
            "graceful (10000 ms to leave, 2000 ms when closing)"
        );
        let json = serde_json::to_value(graceful).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"policy": "graceful", "grace_ms": 10000, "exit_grace_ms": 2000})
        );
    }

    #[test]
    fn a_plan_serializes_to_json_with_tagged_variants() {
        let json = serde_json::to_value(Start::Spawn(SpawnPlan {
            program: Traced {
                value: "p".into(),
                origin: "o".into(),
            },
            args: vec![],
            env: ops(),
            group: false,
            scrub_loader_env: false,
            mkdir: Vec::new(),
            write: Vec::new(),
            identity: None,
        }))
        .unwrap();
        assert_eq!(json["how"], "spawn");
        assert_eq!(json["env"][0]["mode"]["op"], "prepend");
        assert_eq!(json["env"][1]["mode"]["op"], "set");
    }
}
