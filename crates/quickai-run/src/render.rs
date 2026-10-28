//! What the program says: sentences for a person, or one JSON object a line for a script.
//!
//! The provisioner's events are typed; this is the only place they become words. A line of progress is
//! printed when it means something (a tenth of a download, a change of what is being done), not on every
//! poll, because what is printed here ends up in a service manager's journal and in a person's terminal over
//! a slow connection.

use std::collections::BTreeMap;
use std::io::Write;

use quickai_runtime::provision::{EndpointInfo, Event};

/// Bytes as a person says them.
fn bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// The sentences of a run.
pub struct Human {
    quiet: bool,
    /// The tenth of a download last printed, by step.
    tenth: BTreeMap<usize, u64>,
    /// What a step last said it was doing.
    detail: BTreeMap<usize, String>,
}

impl Human {
    pub fn new(quiet: bool) -> Human {
        Human {
            quiet,
            tenth: BTreeMap::new(),
            detail: BTreeMap::new(),
        }
    }

    /// The lines an event is, in the order they are printed, as `(is it for the error stream, text)`.
    pub fn lines(&mut self, event: &Event) -> Vec<(bool, String)> {
        let say = |text: String| vec![(false, text)];
        let note = |text: String| {
            if self.quiet {
                Vec::new()
            } else {
                vec![(false, text)]
            }
        };
        match event {
            Event::Say { text } => note(text.clone()),
            Event::Computer {
                system,
                memory_bytes,
                gpus,
                gpus_from,
            } => note(format!(
                "This computer: {system}, {} of memory, {}",
                bytes(*memory_bytes),
                if gpus.is_empty() {
                    "no graphics card found".to_string()
                } else {
                    format!(
                        "{} ({})",
                        gpus.join(", "),
                        if gpus_from == "named" {
                            "as you said"
                        } else {
                            "found"
                        }
                    )
                }
            )),
            Event::Review { level, text } => note(format!("  {level:<6} {text}")),
            Event::Environment { items } => {
                let mut lines = vec![(false, "What you said about this computer:".to_string())];
                for item in items {
                    lines.push((
                        false,
                        format!(
                            "  {} = {}  ({})",
                            item.key,
                            item.value.as_deref().unwrap_or("(hidden)"),
                            item.from
                        ),
                    ));
                }
                if self.quiet {
                    Vec::new()
                } else {
                    lines
                }
            }
            Event::Prerequisites { items } => {
                let mut lines = vec![(false, "What this computer has to have:".to_string())];
                let mut any_missing = false;
                // one command brings all the packages: it is said under the first thing it brings, and the others point to it
                let mut said_command: Option<&str> = None;
                for item in items {
                    let word = match item.state.as_str() {
                        "met" => "ok",
                        "missing" => "MISSING",
                        "unchecked" => "unknown",
                        _ => "later",
                    };
                    any_missing |= item.state == "missing";
                    lines.push((
                        false,
                        format!("  {word:<7} {}: {}", item.label, item.detail),
                    ));
                    if item.state == "missing" {
                        if let Some(why) = &item.why {
                            lines.push((false, format!("          {why}")));
                        }
                        if let Some(hint) = &item.hint {
                            lines.push((false, format!("          {hint}")));
                        }
                        if let Some(fix) = &item.fix {
                            if said_command == Some(fix.as_str()) {
                                lines.push((
                                    false,
                                    "          bring it with the same command".to_string(),
                                ));
                            } else {
                                lines.push((false, format!("          bring it with: {fix}")));
                                said_command = Some(fix.as_str());
                            }
                        }
                    }
                }
                // a quiet run says what is wrong and not what is right
                if self.quiet && !any_missing {
                    Vec::new()
                } else {
                    lines
                }
            }
            Event::SelfTest {
                id: _,
                label,
                state,
                output,
            } => match state.as_str() {
                "running" => Vec::new(),
                "passed" => note(format!("  ok      the environment passed: {label}")),
                _ => {
                    // the reason is in the message of the failure; the lines are for a script
                    let _ = output;
                    Vec::new()
                }
            },
            Event::Plan { steps } => {
                let mut lines = vec![(false, "To do:".to_string())];
                if steps.is_empty() {
                    lines.push((false, "  nothing".to_string()));
                }
                for (n, step) in steps.iter().enumerate() {
                    lines.push((false, format!("  {}. {}", n + 1, step.text)));
                    for file in &step.files {
                        if file.how == "link" {
                            lines.push((
                                false,
                                format!(
                                    "       {}  ->  {}  (a link to {})",
                                    file.name, file.to, file.from
                                ),
                            ));
                        } else {
                            lines.push((false, format!("       {}  ->  {}", file.name, file.to)));
                            lines.push((false, format!("         from {}", file.from)));
                        }
                    }
                }
                lines
            }
            Event::Step { index, of, text } => note(format!("[{index}/{of}] {text}")),
            Event::Progress {
                index,
                done,
                total,
                detail,
            } => {
                if self.quiet {
                    return Vec::new();
                }
                let mut lines = Vec::new();
                if let Some(detail) = detail {
                    if self.detail.get(index) != Some(detail) {
                        self.detail.insert(*index, detail.clone());
                        lines.push((false, format!("      {detail}")));
                    }
                }
                if let Some(total) = total.filter(|t| *t > 0) {
                    let tenth = done.saturating_mul(10) / total;
                    if self.tenth.get(index).is_none_or(|last| tenth > *last) && tenth > 0 {
                        self.tenth.insert(*index, tenth);
                        lines.push((
                            false,
                            format!(
                                "      {}% ({} of {})",
                                tenth * 10,
                                bytes(*done),
                                bytes(total)
                            ),
                        ));
                    }
                }
                lines
            }
            Event::StepDone { index } => {
                self.tenth.remove(index);
                self.detail.remove(index);
                Vec::new()
            }
            Event::State { program, state } => note(format!("{program}: {state}")),
            Event::Log { program, line } => note(format!("[{program}] {line}")),
            Event::Ready { endpoints, model } => {
                let mut lines = vec![(false, "Ready.".to_string())];
                for EndpointInfo { role, program, url } in endpoints {
                    lines.push((false, format!("  {role:<5} {program}  {url}")));
                }
                if let Some(model) = model {
                    lines.push((false, format!("  model {model}")));
                }
                lines.push((
                    false,
                    "It listens on this computer only. Stop it with Ctrl-C, or SIGTERM from a service manager.".into(),
                ));
                lines
            }
            Event::Stopping => say("Stopping...".to_string()),
            Event::Ports { items } => items
                .iter()
                .filter(|p| p.taken)
                .map(|p| {
                    let what = p
                        .answers_as
                        .as_deref()
                        .map(|a| format!(": {a} answers there"))
                        .unwrap_or_default();
                    (
                        false,
                        format!(
                            "Port {} ({}) is taken by another program{what}.",
                            p.port, p.program
                        ),
                    )
                })
                .collect(),
            Event::Finished {
                outcome, message, ..
            } => match (outcome.as_str(), message) {
                ("failed", Some(message)) => vec![(true, format!("quickai-run: {message}"))],
                ("failed", None) => vec![(true, "quickai-run: it failed".to_string())],
                ("planned", _) => say("Nothing was changed.".to_string()),
                ("prepared", _) => {
                    say("Everything it needs is in place; nothing was started.".to_string())
                }
                ("stopped", _) => say("Stopped.".to_string()),
                ("interrupted", _) => vec![(true, "quickai-run: interrupted".to_string())],
                _ => Vec::new(),
            },
        }
    }

    /// Prints an event to the streams.
    pub fn show(&mut self, event: &Event, out: &mut dyn Write, err: &mut dyn Write) {
        for (to_err, text) in self.lines(event) {
            // a closed pipe is the reader's choice, not a failure of the run
            let _ = if to_err {
                writeln!(err, "{text}")
            } else {
                writeln!(out, "{text}")
            };
        }
        let _ = out.flush();
    }
}

/// One JSON object a line.
pub fn json_line(event: &Event) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_runtime::provision::StepInfo;

    fn texts(human: &mut Human, event: Event) -> Vec<String> {
        human.lines(&event).into_iter().map(|(_, t)| t).collect()
    }

    /// Y96: what the computer has is said line by line, with ok, MISSING, unknown or later before it, the words of
    /// the configuration and of the document and the command under what is missing; a quiet run says only when
    /// something is missing; a test of the environment is said when it passes, and when it fails the failure says it.
    #[test]
    fn p3_y96_what_the_computer_has_is_said_line_by_line_and_a_quiet_run_says_what_is_wrong() {
        use quickai_runtime::provision::PrerequisiteInfo;
        let item = |id: &str, state: &str, detail: &str| PrerequisiteInfo {
            id: id.into(),
            label: format!("the {id}"),
            state: state.into(),
            detail: detail.into(),
            why: None,
            fix: None,
            hint: None,
        };
        let mut missing = item("compiler", "missing", "none of gcc is on the PATH");
        missing.why = Some("The int8 weights run through Triton.".into());
        missing.hint = Some("Install gcc.".into());
        missing.fix = Some("sudo apt-get install -y build-essential".into());
        let mut also = item("headers", "missing", "Python.h is not there");
        also.fix = missing.fix.clone();
        let items = vec![
            item("driver", "met", "driver 595.58.03"),
            missing,
            also,
            item("venv", "unchecked", "no Python yet"),
            item("triton", "later", "a test of the installed environment"),
        ];
        let mut human = Human::new(false);
        assert_eq!(
            texts(
                &mut human,
                Event::Prerequisites {
                    items: items.clone()
                }
            ),
            [
                "What this computer has to have:",
                "  ok      the driver: driver 595.58.03",
                "  MISSING the compiler: none of gcc is on the PATH",
                "          The int8 weights run through Triton.",
                "          Install gcc.",
                "          bring it with: sudo apt-get install -y build-essential",
                "  MISSING the headers: Python.h is not there",
                "          bring it with the same command",
                "  unknown the venv: no Python yet",
                "  later   the triton: a test of the installed environment",
            ]
        );
        // a quiet run says it only when something is wrong
        let mut quiet = Human::new(true);
        assert_eq!(texts(&mut quiet, Event::Prerequisites { items }).len(), 10);
        assert!(texts(
            &mut quiet,
            Event::Prerequisites {
                items: vec![item("driver", "met", "driver 595.58.03")]
            }
        )
        .is_empty());
        // the test of the environment
        let ended = |state: &str| Event::SelfTest {
            id: "triton".into(),
            label: "a kernel".into(),
            state: state.into(),
            output: vec!["a line".into()],
        };
        assert!(texts(&mut human, ended("running")).is_empty());
        assert_eq!(
            texts(&mut human, ended("passed")),
            ["  ok      the environment passed: a kernel"]
        );
        assert!(
            texts(&mut human, ended("failed")).is_empty(),
            "the failure says it, with the lines, in its own sentence"
        );
        assert!(texts(&mut quiet, ended("passed")).is_empty());
        // and a script reads both as JSON
        let json: serde_json::Value = serde_json::from_str(&json_line(&ended("passed"))).unwrap();
        assert_eq!(json["event"], "self_test");
        assert_eq!(json["output"][0], "a line");
    }

    #[test]
    fn p3_o32_progress_is_printed_by_the_tenth_and_by_what_changed_not_by_the_poll() {
        let mut human = Human::new(false);
        let progress = |done: u64, detail: Option<&str>| Event::Progress {
            index: 1,
            done,
            total: Some(1000),
            detail: detail.map(str::to_string),
        };
        assert!(
            texts(&mut human, progress(10, None)).is_empty(),
            "under a tenth"
        );
        assert_eq!(
            texts(&mut human, progress(120, None)),
            ["      10% (120 B of 1000 B)"]
        );
        assert!(
            texts(&mut human, progress(150, None)).is_empty(),
            "the same tenth"
        );
        assert_eq!(
            texts(&mut human, progress(310, Some("unpacking"))),
            ["      unpacking", "      30% (310 B of 1000 B)"]
        );
        assert!(texts(&mut human, progress(320, Some("unpacking"))).is_empty());
        // a quiet run says none of it
        let mut quiet = Human::new(true);
        assert!(texts(&mut quiet, progress(900, Some("x"))).is_empty());
    }

    /// Y62: what was said about this computer is said with where, a secret as hidden; and a step of the plan that
    /// puts files in the models folder says each of them, from where to where, or that it is a link.
    #[test]
    fn p3_y62_what_was_said_about_the_computer_and_where_each_file_goes_are_said() {
        use quickai_runtime::provision::{EnvironmentItem, FileInfo};
        let mut human = Human::new(false);
        let item = |key: &str, value: Option<&str>, from: &str| EnvironmentItem {
            key: key.into(),
            value: value.map(str::to_string),
            from: from.into(),
        };
        let said = texts(
            &mut human,
            Event::Environment {
                items: vec![
                    item("values.comfyui.models_dir", Some("/mnt/big"), "--set"),
                    item("fetch.huggingface_token", None, "the variable HF_TOKEN"),
                ],
            },
        );
        assert_eq!(
            said,
            [
                "What you said about this computer:",
                "  values.comfyui.models_dir = /mnt/big  (--set)",
                "  fetch.huggingface_token = (hidden)  (the variable HF_TOKEN)"
            ]
        );
        let plan = Event::Plan {
            steps: vec![StepInfo {
                kind: "fetch".into(),
                text: "fetch a.safetensors and 1 more file (3.0 GB) from huggingface:o/r; use b.safetensors that you have".into(),
                bytes: Some(3),
                files: vec![
                    FileInfo {
                        name: "a.safetensors".into(),
                        how: "fetch".into(),
                        from: "https://h.example/o/r/resolve/main/a.safetensors".into(),
                        to: "/m/vae/a.safetensors".into(),
                        bytes: Some(3),
                        sha256: None,
                    },
                    FileInfo {
                        name: "b.safetensors".into(),
                        how: "link".into(),
                        from: "/srv/b.safetensors".into(),
                        to: "/m/b.safetensors".into(),
                        bytes: None,
                        sha256: None,
                    },
                ],
            }],
        };
        let said = texts(&mut human, plan.clone());
        assert_eq!(
            &said[2..],
            [
                "       a.safetensors  ->  /m/vae/a.safetensors",
                "         from https://h.example/o/r/resolve/main/a.safetensors",
                "       b.safetensors  ->  /m/b.safetensors  (a link to /srv/b.safetensors)"
            ]
        );
        // a quiet run says the plan with its files, and not what was said about the computer
        let mut quiet = Human::new(true);
        assert!(texts(
            &mut quiet,
            Event::Environment {
                items: vec![item("a", Some("b"), "--set")]
            }
        )
        .is_empty());
        assert_eq!(texts(&mut quiet, plan).len(), 5);
        // the JSON line of a step carries its files, and of a step with none does not say so
        let json: serde_json::Value = serde_json::from_str(&json_line(&Event::Plan {
            steps: vec![StepInfo {
                kind: "start".into(),
                text: "start".into(),
                bytes: None,
                files: Vec::new(),
            }],
        }))
        .unwrap();
        assert!(json["steps"][0].get("files").is_none(), "{json}");
    }

    #[test]
    fn p3_o33_the_plan_the_end_and_a_failure_are_said_even_when_quiet() {
        let mut quiet = Human::new(true);
        let plan = Event::Plan {
            steps: vec![StepInfo {
                kind: "install".into(),
                text: "install X".into(),
                bytes: None,
                files: Vec::new(),
            }],
        };
        assert_eq!(texts(&mut quiet, plan), ["To do:", "  1. install X"]);
        let ready = Event::Ready {
            endpoints: vec![EndpointInfo {
                role: "api".into(),
                program: "X".into(),
                url: "http://127.0.0.1:1".into(),
            }],
            model: Some("m".into()),
        };
        let said = texts(&mut quiet, ready);
        assert_eq!(said[0], "Ready.");
        assert!(said[1].contains("http://127.0.0.1:1") && said[2].contains("model m"));
        assert!(said.last().unwrap().contains("this computer only"));
        let failure = Event::Finished {
            outcome: "failed".into(),
            message: Some("it did not work".into()),
            lack: None,
        };
        let lines = quiet.lines(&failure);
        assert_eq!(lines, [(true, "quickai-run: it did not work".to_string())]);
        assert_eq!(
            texts(
                &mut quiet,
                Event::Say {
                    text: "chatter".into()
                }
            ),
            Vec::<String>::new()
        );
    }
}
