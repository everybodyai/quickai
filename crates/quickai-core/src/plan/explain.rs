//! `plan --explain`: a run plan as text, with the origin of every line.

use super::types::{EnvMode, RunPlan, Start};

/// Renders `plan` for a person. The origins are in a column on the right, so it reads as "this
/// command, and here is where each part of it comes from".
pub fn render_explain(plan: &RunPlan) -> String {
    struct Line {
        label: String,
        text: String,
        origin: String,
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut push = |label: &str, text: String, origin: &str| {
        lines.push(Line {
            label: label.to_string(),
            text,
            origin: origin.to_string(),
        })
    };

    if let Some(g) = &plan.gateway {
        let reads: Vec<&str> = plan.decode.iter().map(|d| d.id.as_str()).collect();
        push(
            "gateway",
            format!("proxy :{} -> engine :{}", g.public_port, g.upstream_port),
            "app traffic proxy",
        );
        if !reads.is_empty() {
            push("", format!("reads {}", reads.join(", ")), "vocabulary");
        }
    }
    for unit in &plan.units {
        for start in &unit.start {
            let spawn = match start {
                Start::Attach(attach) => {
                    push(
                        "start",
                        format!(
                            "attach  {}:{} when GET {} answers with a version",
                            attach.endpoint.host,
                            attach
                                .endpoint
                                .port
                                .map_or_else(String::new, |p| p.to_string()),
                            attach.identity.path
                        ),
                        "engine launch.attach",
                    );
                    push(
                        "",
                        format!(
                            "  watched by GET {} every {} ms, gone after {} misses",
                            attach.liveness.path,
                            attach.liveness.every_ms,
                            attach.liveness.failures
                        ),
                        "engine launch.attach.liveness",
                    );
                    continue;
                }
                Start::Spawn(spawn) => spawn,
            };
            push(
                "start",
                format!("spawn   {}", spawn.program.value),
                &spawn.program.origin,
            );
            for arg in &spawn.args {
                push("", format!("  {}", arg.value), &arg.origin);
            }
            if let Some(dir) = &spawn.cwd {
                push("", format!("cwd   {dir}"), "engine launch.spawn.cwd");
            }
            for dir in &spawn.mkdir {
                push("", format!("mkdir {dir}"), "engine launch.spawn.mkdir");
            }
            for step in &spawn.write {
                push(
                    "",
                    format!("write {}   (the application's own {})", step.to, step.asset),
                    "engine launch.spawn.write",
                );
            }
            for env in &spawn.env {
                let text = match &env.mode {
                    EnvMode::Set { value } => format!("env {}={}", env.name, value),
                    EnvMode::Prepend { value, sep } => {
                        format!("env {0}={1}{2}${0}", env.name, value, sep)
                    }
                };
                push("", text, &env.origin);
            }
        }
        if let Some(ready) = &unit.ready {
            push(
                "ready",
                format!(
                    "GET {} == {}   ({} s, every {} ms)",
                    ready.path,
                    ready.expect_status,
                    ready.timeout_ms / 1000,
                    ready.interval_ms
                ),
                "",
            );
        }
        for (label, calls) in [("warm", &unit.warm), ("unload", &unit.unload)] {
            for call in calls {
                push(
                    label,
                    format!("{} {} {}", call.method, call.path, call.body),
                    &format!("engine launch.{label}"),
                );
            }
        }
        push("stop", unit.stop.describe(), "");
    }

    let width = lines
        .iter()
        .map(|l| l.text.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = format!(
        "plan: {} on {}\n",
        plan.model.as_deref().unwrap_or("(no model)"),
        plan.engine
    );
    for line in lines {
        let pad = width - line.text.chars().count();
        out.push_str(&format!(" {:<8}{}", line.label, line.text));
        if !line.origin.is_empty() {
            out.push_str(&" ".repeat(pad + 3));
            out.push_str(&line.origin);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{plan, HostFacts, ModelInfo, Selection};
    use crate::registry::fixtures::good_registry;

    #[test]
    fn explain_shows_every_argument_with_its_origin() {
        let facts = HostFacts {
            binary: Some("/opt/quickai/llama-server".into()),
            ram_bytes: 32 * 1024 * 1024 * 1024,
            ..Default::default()
        };
        let sel = Selection::new("llamacpp")
            .with_model(ModelInfo {
                id: "bonsai-8b".into(),
                display: "Bonsai 8B".into(),
                path: "/opt/quickai/models/Bonsai-8B-Q1_0.gguf".into(),
                size_bytes: 1 << 30,
            })
            .set("llamacpp.port", 8080u16)
            .set("llamacpp.ctx_size", 8192u32);
        let text = render_explain(&plan(&good_registry(), &sel, &facts).unwrap());
        let expected = "\
plan: Bonsai 8B on llamacpp
 start   spawn   /opt/quickai/llama-server                   engine launch.spawn.program
           --model                                           engine launch.spawn.args
           /opt/quickai/models/Bonsai-8B-Q1_0.gguf           engine launch.spawn.args
           --host                                            engine launch.spawn.args
           127.0.0.1                                         engine launch.spawn.args
           --port                                            engine launch.spawn.args
           8080                                              engine launch.spawn.args
           --ctx-size                                        field llamacpp.ctx_size
           8192                                              field llamacpp.ctx_size
           --n-gpu-layers                                    field llamacpp.n_gpu_layers (default)
           999                                               field llamacpp.n_gpu_layers (default)
         env LD_LIBRARY_PATH=/opt/quickai:$LD_LIBRARY_PATH   engine launch.spawn.env
 ready   GET /health == 200   (120 s, every 500 ms)
 stop    kill
";
        assert_eq!(text, expected, "\n{text}");
    }

    #[test]
    fn a_plan_without_a_model_says_so() {
        // an engine whose command line does not name a model (a daemon, say)
        let doc =
            crate::registry::fixtures::GOOD.replace("\"--model\", \"${model.main.path}\", ", "");
        let registry = crate::registry::fixtures::registry_from(&doc);
        let facts = HostFacts {
            binary: Some("/x/llama-server".into()),
            ..Default::default()
        };
        let text = render_explain(&plan(&registry, &Selection::new("llamacpp"), &facts).unwrap());
        assert!(text.starts_with("plan: (no model) on llamacpp\n"), "{text}");
    }
}
