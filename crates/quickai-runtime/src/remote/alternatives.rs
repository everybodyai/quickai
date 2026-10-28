//! What a configuration lacks on another computer, and the other way for each, to be accepted or rejected before
//! anything is installed there (WP4.33.6, docs/remote-runs.md section 6; the user: "It will provide alternatives that
//! could be accepted or rejected by the user"). `quickai-run check` says what the run would do there and what stops
//! it; what it says is turned into the questions the window asks:
//!
//! - packages the computer lacks (a compiler, the headers of Python): installed with its own package manager
//!   (`--install-system-packages`, C138);
//! - a model file nobody says where to get, that this computer has: copied from here, and named with
//!   `--model-source`;
//! - a folder of the models with no room for them: another folder there that has room (`--models-dir`);
//! - no build of the engine's program that could be told for its cards: the build for the processor (`--gpu none`).
//!
//! Anything else that stops it is said as it is, with no other way.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::provision::Lack;

use super::probe::{Computer, System};
use super::ssh;
use super::RemoteSpec;

/// How long `check` may take there: it may ask GitHub for the list of builds.
const CHECK_TIMEOUT: Duration = Duration::from_secs(240);

/// How long a small command there may take.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// What a configuration lacks there, and the other way the person is asked to accept or reject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// What it is, the same from one run to the next (`room`, `model:<file>`, `build:<program>`, `prerequisites`).
    pub id: String,
    /// What is lacking, in a sentence.
    pub problem: String,
    /// The other way, in a sentence.
    pub alternative: String,
    /// What happens when the other way is rejected.
    pub then: String,
    pub remedy: Remedy,
}

/// How an accepted other way is done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Remedy {
    /// Words added to `quickai-run up`.
    Flags(Vec<String>),
    /// The model file `file` of this computer, at `local`, copied there and named with `--model-source`.
    CopyModel { local: PathBuf, file: String },
}

/// What a check of the configuration there found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// It can run as it is.
    Fine,
    /// It lacks these, each with another way.
    Issues(Vec<Issue>),
    /// It cannot run there, and there is no other way to propose: why.
    Blocked(String),
}

/// Checks the configuration `configuration` (its path there, relative to the home) on `spec` with its headless
/// program of `version` and `flags`, and says what it lacks with the other way for each. `local_models` are the
/// model files this computer has, by name, with their path and size: what can be copied there.
pub fn check(
    ssh_program: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
    configuration: &str,
    flags: &[String],
    local_models: &BTreeMap<String, (PathBuf, u64)>,
) -> Result<Found, String> {
    let mut command = format!(
        "{} check {} --json",
        computer.program_command(version),
        computer.shell_path(configuration)
    );
    for flag in flags {
        command.push(' ');
        command.push_str(&word(computer, flag));
    }
    let said = ssh::run(ssh_program, spec, &command, CHECK_TIMEOUT)?;
    let events: Vec<serde_json::Value> = said
        .lines()
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let finished = of_kind(&events, "finished").last().cloned();
    let failed = finished
        .as_ref()
        .is_some_and(|f| f["outcome"].as_str() == Some("failed"));
    let mut issues = Vec::new();

    // packages the computer lacks, with the command that brings them
    let missing: Vec<&serde_json::Value> = of_kind(&events, "prerequisites")
        .flat_map(|e| e["items"].as_array().into_iter().flatten())
        .filter(|item| item["state"].as_str() == Some("missing"))
        .collect();
    let fixes: Vec<&str> = missing.iter().filter_map(|i| i["fix"].as_str()).collect();
    if !missing.is_empty() && !fixes.is_empty() && !flags.iter().any(|f| f == INSTALL_PACKAGES) {
        let labels: Vec<&str> = missing.iter().filter_map(|i| i["label"].as_str()).collect();
        issues.push(Issue {
            id: "prerequisites".into(),
            problem: format!("{} lacks {}.", spec.name, labels.join(", ")),
            alternative: format!(
                "Install it with its package manager: {} (as the administrator, or with sudo that asks no password).",
                fixes.join(" && ")
            ),
            then: format!(
                "Then it stops here: run that command on {} yourself, and start again.",
                spec.name
            ),
            remedy: Remedy::Flags(vec![INSTALL_PACKAGES.into()]),
        });
    }

    // what stopped the plan, when another way can be proposed for it
    let lack: Option<Lack> = finished
        .as_ref()
        .and_then(|f| serde_json::from_value(f["lack"].clone()).ok());
    let message = finished
        .as_ref()
        .and_then(|f| f["message"].as_str())
        .unwrap_or("it could not be checked")
        .to_string();
    match lack {
        Some(Lack::Model { file }) => match local_models.get(&file) {
            Some((local, size)) => issues.push(Issue {
                id: format!("model:{file}"),
                problem: format!(
                    "{file} is not on {}, and the configuration does not say where to get it.",
                    spec.name
                ),
                alternative: format!(
                    "Copy it from this computer ({}).",
                    crate::hardware::format_bytes(*size)
                ),
                then: format!(
                    "Then it stops here: put {file} on {} yourself, or choose a model of the recommended list.",
                    spec.name
                ),
                remedy: Remedy::CopyModel {
                    local: local.clone(),
                    file,
                },
            }),
            None => return Ok(Found::Blocked(message)),
        },
        Some(Lack::Room { bytes, dir, free }) => {
            match roomy_folder(ssh_program, spec, computer, bytes)? {
                Some((folder, room)) => issues.push(Issue {
                    id: "room".into(),
                    problem: format!(
                        "{dir} has {} free; the models need {}.",
                        crate::hardware::format_bytes(free),
                        crate::hardware::format_bytes(bytes)
                    ),
                    alternative: format!(
                        "Keep the models in {folder} ({} free).",
                        crate::hardware::format_bytes(room)
                    ),
                    then: format!(
                        "Then it stops here: make room on {}, and start again.",
                        spec.name
                    ),
                    remedy: Remedy::Flags(vec!["--models-dir".into(), folder]),
                }),
                None => return Ok(Found::Blocked(message)),
            }
        }
        Some(Lack::Build { program }) if !flags.iter().any(|f| f == "--gpu") => {
            issues.push(Issue {
                id: format!("build:{program}"),
                problem: format!(
                    "Which build of {program} suits the graphics cards of {} could not be told.",
                    spec.name
                ),
                alternative: "Use the build for the processor: slower, and it runs on any computer.".into(),
                then: "Then it stops here: name the build in the configuration, and start again.".into(),
                remedy: Remedy::Flags(vec!["--gpu".into(), "none".into()]),
            })
        }
        _ if failed && issues.is_empty() => return Ok(Found::Blocked(message)),
        _ => {}
    }
    if issues.is_empty() {
        Ok(Found::Fine)
    } else {
        Ok(Found::Issues(issues))
    }
}

/// Adds the words `more` (what the person accepted now) to `accepted` (what was accepted for that computer before):
/// a flag with a value replaces the one it had (`--models-dir`, `--gpu`; `--model-source` by its file), and a flag
/// alone is kept once.
pub fn merge_accepted(accepted: &mut Vec<String>, more: &[String]) {
    let pairs = |words: &[String]| -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let value = words.get(i + 1).filter(|v| !v.starts_with("--")).cloned();
            let step = if value.is_some() { 2 } else { 1 };
            out.push((words[i].clone(), value));
            i += step;
        }
        out
    };
    let key = |(flag, value): &(String, Option<String>)| match (flag.as_str(), value) {
        ("--model-source", Some(v)) => format!("{flag} {}", v.split('=').next().unwrap_or(v)),
        _ => flag.clone(),
    };
    let mut kept = pairs(accepted);
    for pair in pairs(more) {
        kept.retain(|k| key(k) != key(&pair));
        kept.push(pair);
    }
    *accepted = kept
        .into_iter()
        .flat_map(|(flag, value)| std::iter::once(flag).chain(value))
        .collect();
}

/// The events of `kind` (`finished`, `prerequisites`) among `events`.
fn of_kind<'a>(
    events: &'a [serde_json::Value],
    kind: &'a str,
) -> impl Iterator<Item = &'a serde_json::Value> + 'a {
    events
        .iter()
        .filter(move |e| e["event"].as_str() == Some(kind))
}

/// The flag that lets the run install the packages the computer lacks.
const INSTALL_PACKAGES: &str = "--install-system-packages";

/// Where on `spec` a model file copied from here is kept (relative to the home), and what `--model-source` names it
/// by there (the whole path).
pub fn copied_model(computer: &Computer, file: &str) -> (String, String) {
    let relative = format!(".quickai/remote/models/{file}");
    let whole = match computer.system {
        System::Windows => format!(
            "{}\\{}",
            computer.home.trim_end_matches('\\'),
            relative.replace('/', "\\")
        ),
        _ => format!("{}/{relative}", computer.home.trim_end_matches('/')),
    };
    (relative, whole)
}

/// A folder of `spec` with room for `bytes` that the person may write in, on another disk than the home: a folder
/// `quickai-models` at the top of it, and how much is free there. On Linux and macOS the disks `df` lists, on Windows
/// the drives.
fn roomy_folder(
    ssh_program: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    bytes: u64,
) -> Result<Option<(String, u64)>, String> {
    let need = bytes + bytes / 20;
    let command = match computer.system {
        System::Windows => r#"powershell -NoProfile -Command "Get-PSDrive -PSProvider FileSystem | ForEach-Object { '{0} {1}' -f $_.Free, $_.Root }""#.to_string(),
        _ => r#"df -Pk 2>/dev/null | awk 'NR>1 {print $4*1024, $6}' | while read free mount; do [ -w "$mount" ] && echo "$free $mount"; done"#.to_string(),
    };
    let said = ssh::run(ssh_program, spec, &command, COMMAND_TIMEOUT)?;
    let mut best: Option<(String, u64)> = None;
    for line in said.lines() {
        let Some((free, place)) = line.trim().split_once(' ') else {
            continue;
        };
        let Ok(free) = free.trim().parse::<u64>() else {
            continue;
        };
        let place = place.trim();
        let system_place = ["/proc", "/sys", "/dev", "/run", "/boot", "/snap", "/tmp"]
            .iter()
            .any(|p| place.starts_with(p));
        if free < need || system_place || place == computer.home {
            continue;
        }
        if best.as_ref().is_none_or(|(_, room)| free > *room) {
            let folder = match computer.system {
                System::Windows => format!("{}quickai-models", place),
                _ => format!("{}/quickai-models", place.trim_end_matches('/')),
            };
            best = Some((folder, free));
        }
    }
    Ok(best)
}

/// `text` as one word of the shell of `computer`.
fn word(computer: &Computer, text: &str) -> String {
    match computer.system {
        System::Windows => format!("\"{}\"", text.replace('"', "")),
        _ => ssh::sh_quote(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_accepted_before_is_replaced_by_what_is_accepted_now() {
        let words =
            |text: &str| -> Vec<String> { text.split_whitespace().map(str::to_string).collect() };
        let mut accepted =
            words("--models-dir /data/a --install-system-packages --model-source m.gguf=/x/m.gguf");
        merge_accepted(
            &mut accepted,
            &words(
                "--models-dir /data/b --install-system-packages --model-source n.gguf=/x/n.gguf",
            ),
        );
        assert_eq!(
            accepted,
            words("--model-source m.gguf=/x/m.gguf --models-dir /data/b --install-system-packages --model-source n.gguf=/x/n.gguf")
        );
    }

    #[test]
    fn a_model_copied_there_is_kept_below_the_home_and_named_by_its_whole_path() {
        let linux = Computer {
            system: System::Linux,
            arch: "x86_64".into(),
            home: "/home/me".into(),
            headless: None,
        };
        assert_eq!(
            copied_model(&linux, "m.gguf"),
            (
                ".quickai/remote/models/m.gguf".to_string(),
                "/home/me/.quickai/remote/models/m.gguf".to_string()
            )
        );
        let windows = Computer {
            system: System::Windows,
            home: r"C:\Users\me".into(),
            ..linux
        };
        assert_eq!(
            copied_model(&windows, "m.gguf").1,
            r"C:\Users\me\.quickai\remote\models\m.gguf"
        );
    }
}
