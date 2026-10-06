//! What the planner produces: a run plan, plain data that can be shown, compared and executed.
//!
//! A plan is serializable, so it can be snapshot-tested, printed by `plan --explain` and run
//! headless. Every command-line argument and environment entry carries its *origin*: the part of
//! the engine document or the field that put it there.

use serde::Serialize;

use crate::doc::engine::StopPolicy;

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
    /// Ways to bring it up, in order.
    pub start: Vec<Start>,
    /// How to tell it is ready.
    pub ready: Option<ReadyPlan>,
    pub stop: StopPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Endpoint {
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "how", rename_all = "snake_case")]
pub enum Start {
    Spawn(SpawnPlan),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SpawnPlan {
    pub program: Traced<String>,
    pub args: Vec<Traced<String>>,
    pub env: Vec<EnvOp>,
    pub group: bool,
}

impl SpawnPlan {
    /// The command as it is logged first: `$ program arg arg ...`.
    pub fn command_line(&self) -> String {
        let args: Vec<&str> = self.args.iter().map(|a| a.value.as_str()).collect();
        format!("$ {} {}", self.program.value, args.join(" "))
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
        };
        assert_eq!(
            spawn.command_line(),
            "$ /opt/llama/llama-server --model /m/q.gguf"
        );
        assert_eq!(spawn.arg_values(), vec!["--model", "/m/q.gguf"]);
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
        }))
        .unwrap();
        assert_eq!(json["how"], "spawn");
        assert_eq!(json["env"][0]["mode"]["op"], "prepend");
        assert_eq!(json["env"][1]["mode"]["op"], "set");
    }
}
