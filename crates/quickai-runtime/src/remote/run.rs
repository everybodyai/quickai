//! A configuration run on another computer by QuickAI's own headless program (`remote.run` and `remote.tunnel`,
//! WP4.33.7, docs/remote-runs.md sections 3 and 3.1): the configuration and its environment file copied there, the
//! run started in the background (`quickai-headless up --detach`), asked after (`status --json`) and stopped (`down`); and
//! once it is ready, each port it listens on brought here, to the same port unless the person gave another or it is
//! taken here. That is all the window does with it: what runs is the configuration's.

use std::path::Path;
use std::time::Duration;

use quickai_core::lookup::present::Download;
use serde::{Deserialize, Serialize};

use crate::facts::ComputerFacts;
use crate::provision::EndpointInfo;
use crate::run_file::{RunFile, Status};

use super::deploy;
use super::probe::{Computer, System};
use super::ssh::{self, sh_quote};
use super::{port_here, slug, RemoteSpec, Tunnel};

/// How long `status` may take: the connection and a small command.
const STATUS_TIMEOUT: Duration = Duration::from_secs(40);

/// How long `up --detach` may take: it returns once the run has made its plan, a minute at most, after the login.
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// How long `down` may take: it waits for the run to stop, two minutes at most, after the login.
const STOP_TIMEOUT: Duration = Duration::from_secs(180);

/// The exit code of `quickai-headless` when another run goes on.
const BUSY: i32 = 7;

/// What a run is started with: the configuration (a file here, as the window exports it), the name it is kept
/// under there, its environment file if there is one, and flags of `quickai-headless up` (`--accept-review`,
/// `--install-system-packages`).
pub struct Launch<'a> {
    pub configuration: &'a Path,
    pub name: &'a str,
    pub environment: Option<&'a Path>,
    pub flags: Vec<String>,
}

/// How a start went, when the connection did.
#[derive(Debug, PartialEq)]
pub enum Started {
    /// The run is in the background, past its plan.
    Started,
    /// Another run goes on there, and this is it: nothing was started (docs/remote-runs.md section 3.2).
    Busy(Option<Box<RunFile>>),
}

/// What runs on `spec`, as its headless program of `version` says (it must be there: `deploy::ensure_program`).
pub fn status(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
) -> Result<Status, String> {
    let command = format!("{} status --json", computer.program_command(version));
    let said = ssh::run(ssh, spec, &command, STATUS_TIMEOUT)?;
    if !said.ok() {
        return Err(failed(spec, "say what runs", &said));
    }
    serde_json::from_str(said.stdout.trim()).map_err(|e| {
        format!(
            "What {} said of its run could not be read: {e}",
            spec.host.trim()
        )
    })
}

/// The model files `spec` keeps (`quickai-headless models --json`), with the folders of another disk the person accepted
/// to keep them in for one configuration or another (`--models-dir`).
pub fn models(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
) -> Result<Vec<crate::models::KeptModel>, String> {
    let mut command = format!("{} models --json", computer.program_command(version));
    command.push_str(&models_dirs(spec, computer));
    let said = ssh::run(ssh, spec, &command, STATUS_TIMEOUT)?;
    if !said.ok() {
        return Err(failed(spec, "list its models", &said));
    }
    serde_json::from_str(said.stdout.trim()).map_err(|e| {
        format!(
            "What {} said of its models could not be read: {e}",
            spec.host.trim()
        )
    })
}

/// ` --models-dir <folder>` for each folder of another disk the person accepted to keep models in on `spec`, for one
/// configuration or another, once each.
fn models_dirs(spec: &RemoteSpec, computer: &Computer) -> String {
    let mut folders: Vec<&String> = Vec::new();
    for flags in spec.accepted.values() {
        let mut words = flags.iter();
        while let Some(word) = words.next() {
            if word == "--models-dir" {
                if let Some(folder) = words.next().filter(|f| !folders.contains(f)) {
                    folders.push(folder);
                }
            }
        }
    }
    folders
        .into_iter()
        .map(|folder| format!(" --models-dir {}", word(computer, folder)))
        .collect()
}

/// What `spec` has for models (`quickai-headless computer --json`): its memory, the makers of its cards, and the devices
/// of the llama.cpp installed there.
pub fn facts(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
) -> Result<ComputerFacts, String> {
    let command = format!("{} computer --json", computer.program_command(version));
    let said = ssh::run(ssh, spec, &command, STATUS_TIMEOUT)?;
    if !said.ok() {
        return Err(failed(spec, "say what it has", &said));
    }
    serde_json::from_str(said.stdout.trim()).map_err(|e| {
        format!(
            "What {} said of itself could not be read: {e}",
            spec.host.trim()
        )
    })
}

/// Starts the download of `download` on `spec`, in the background (`quickai-headless get --detach`), into
/// `models_dir` there or QuickAI's models folder: what to fetch is copied there as a file, and the files are fetched
/// by that computer from where they come from.
pub fn get_start(
    ssh: &Path,
    scp: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
    download: &Download,
    models_dir: Option<&str>,
) -> Result<(), String> {
    let text = serde_json::to_string_pretty(download)
        .map_err(|e| format!("What to fetch could not be written: {e}"))?;
    // a file of this download's own: two windows may send one to two computers at once
    static GETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let file = std::env::temp_dir().join(format!(
        "quickai-get-{}-{}-{}.json",
        std::process::id(),
        GETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        spec.id
    ));
    std::fs::write(&file, text).map_err(|e| format!("Could not write {}: {e}", file.display()))?;
    let there = ".quickai/remote/get.json";
    let copied = deploy::copy_file(ssh, scp, spec, computer, &file, there);
    let _ = std::fs::remove_file(&file);
    copied?;
    let mut command = format!(
        "{} get {} --detach",
        computer.program_command(version),
        computer.shell_path(there)
    );
    if let Some(folder) = models_dir {
        command.push_str(" --models-dir ");
        command.push_str(&word(computer, folder));
    }
    let said = ssh::run(ssh, spec, &command, START_TIMEOUT)?;
    match said.code {
        Some(0) => Ok(()),
        Some(BUSY) => {
            let text = said.stderr.trim();
            let line = text
                .lines()
                .map(|l| l.trim_start_matches("quickai-headless: ").trim())
                .find(|l| !l.is_empty())
                .unwrap_or("another download goes on there");
            Err(format!("On {}, {line}", spec.name.trim()))
        }
        _ => Err(failed(spec, "start the download", &said)),
    }
}

/// The download that goes on on `spec`, or how the last one ended (`quickai-headless get --status --json`).
pub fn get_status(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
) -> Result<Status, String> {
    let command = format!("{} get --status --json", computer.program_command(version));
    let said = ssh::run(ssh, spec, &command, STATUS_TIMEOUT)?;
    if !said.ok() {
        return Err(failed(spec, "say how its download goes", &said));
    }
    serde_json::from_str(said.stdout.trim()).map_err(|e| {
        format!(
            "What {} said of its download could not be read: {e}",
            spec.host.trim()
        )
    })
}

/// Stops the download that goes on on `spec`, and waits until it has (what came so far stays, and a new download of
/// the same files goes on from there).
pub fn get_cancel(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
) -> Result<(), String> {
    let command = format!("{} get --cancel", computer.program_command(version));
    let said = ssh::run(ssh, spec, &command, STOP_TIMEOUT)?;
    if said.ok() {
        Ok(())
    } else {
        Err(failed(spec, "stop its download", &said))
    }
}

/// Deletes the model file `file` on `spec` (`quickai-headless models --delete`), with the files that went with it that no
/// other model uses; what was done, as sentences for the log. A model that came with QuickAI there, or that its run
/// uses, is not deleted: what it said is the error.
pub fn delete_model(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
    file: &str,
) -> Result<Vec<String>, String> {
    let mut command = format!(
        "{} models --delete {} --json",
        computer.program_command(version),
        word(computer, file)
    );
    command.push_str(&models_dirs(spec, computer));
    let said = ssh::run(ssh, spec, &command, STATUS_TIMEOUT)?;
    if !said.ok() {
        let text = said.stderr.trim();
        return Err(
            match text
                .lines()
                .map(|l| l.trim_start_matches("quickai-headless: ").trim())
                .find(|l| !l.is_empty())
            {
                Some(line) => format!("On {}, {line}", spec.name.trim()),
                None => failed(spec, "delete the model", &said),
            },
        );
    }
    let report: Deleted = serde_json::from_str(said.stdout.trim()).map_err(|e| {
        format!(
            "What {} said of the delete could not be read: {e}",
            spec.host.trim()
        )
    })?;
    Ok(report.sentences(file, &spec.name))
}

/// What `models --delete --json` says: the files that went, the ones kept because another model uses them, the ones
/// that could not be deleted, and why the weights could not be, if they could not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deleted {
    #[serde(default)]
    pub removed: Vec<String>,
    #[serde(default)]
    pub kept: Vec<String>,
    #[serde(default)]
    pub failed: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl Deleted {
    /// The report as the log says a delete here: what went, what else went, what stayed and why, what could not go.
    pub fn sentences(&self, file: &str, computer: &str) -> Vec<String> {
        let computer = computer.trim();
        let mut lines = vec![match &self.error {
            None => format!("Deleted model on {computer}: {file}"),
            Some(e) => format!("Could not delete {file} on {computer}: {e}"),
        }];
        let also: Vec<&str> = self
            .removed
            .iter()
            .map(String::as_str)
            .filter(|name| *name != file)
            .collect();
        if !also.is_empty() {
            lines.push(format!("Also deleted: {}.", also.join(", ")));
        }
        for kept in &self.kept {
            lines.push(format!("Kept {kept}: another model uses it."));
        }
        for failed in &self.failed {
            lines.push(format!("Could not delete {failed}"));
        }
        lines
    }
}

/// Copies what `launch` names to `spec` and starts the run there in the background, with its headless program of
/// `version`.
pub fn start(
    ssh: &Path,
    scp: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
    launch: &Launch,
) -> Result<Started, String> {
    let sent = send(ssh, scp, spec, computer, launch)?;
    start_sent(ssh, spec, computer, version, &sent, &launch.flags)
}

/// What was copied there for a run: the configuration, and its environment file, by their paths there (relative to
/// the home).
pub struct Sent {
    pub configuration: String,
    pub environment: Option<String>,
}

/// Copies the configuration of `launch` (and its environment file) to `spec`, into `~/.quickai/remote`.
pub fn send(
    ssh: &Path,
    scp: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    launch: &Launch,
) -> Result<Sent, String> {
    let name = slug(launch.name);
    let configuration = format!(".quickai/remote/{name}.toml");
    deploy::copy_file(
        ssh,
        scp,
        spec,
        computer,
        launch.configuration,
        &configuration,
    )?;
    let environment = match launch.environment {
        Some(environment) => {
            let there = format!(".quickai/remote/{name}.environment.toml");
            deploy::copy_file(ssh, scp, spec, computer, environment, &there)?;
            Some(there)
        }
        None => None,
    };
    Ok(Sent {
        configuration,
        environment,
    })
}

/// Starts the run of what was `sent` to `spec` in the background, with `flags` of `quickai-headless up`.
pub fn start_sent(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
    sent: &Sent,
    flags: &[String],
) -> Result<Started, String> {
    let mut command = format!(
        "{} up {} --detach",
        computer.program_command(version),
        computer.shell_path(&sent.configuration)
    );
    if let Some(environment) = &sent.environment {
        command.push_str(&format!(
            " --environment {}",
            computer.shell_path(environment)
        ));
    }
    for flag in flags {
        command.push(' ');
        command.push_str(&word(computer, flag));
    }
    let said = ssh::run(ssh, spec, &command, START_TIMEOUT)?;
    match said.code {
        Some(0) => Ok(Started::Started),
        Some(BUSY) => Ok(Started::Busy(
            status(ssh, spec, computer, version)
                .ok()
                .and_then(|s| s.running)
                .map(Box::new),
        )),
        _ => Err(failed(spec, "start the run", &said)),
    }
}

/// Stops the programs that listen on `ports` of `spec` (ones QuickAI did not start), with its headless program of
/// `version` (`quickai-headless free`); what could not be stopped there is said, with the command for the person.
pub fn free(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
    ports: &[u16],
) -> Result<(), String> {
    let words: Vec<String> = ports.iter().map(u16::to_string).collect();
    let command = format!(
        "{} free {} --json",
        computer.program_command(version),
        words.join(" ")
    );
    let said = ssh::run(ssh, spec, &command, STOP_TIMEOUT)?;
    if said.ok() {
        return Ok(());
    }
    let freed: Vec<serde_json::Value> =
        serde_json::from_str(said.stdout.trim()).unwrap_or_default();
    let hints: Vec<String> = freed
        .iter()
        .filter(|f| f["free"].as_bool() == Some(false))
        .map(|f| {
            format!(
                "port {} ({}): {}",
                f["port"],
                f["program"].as_str().unwrap_or("a program"),
                f["hint"].as_str().unwrap_or("it could not be stopped")
            )
        })
        .collect();
    if hints.is_empty() {
        Err(failed(spec, "free its ports", &said))
    } else {
        Err(format!("On {}, {}", spec.name.trim(), hints.join("; ")))
    }
}

/// Stops what runs on `spec`, and waits until it has.
pub fn stop(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
) -> Result<(), String> {
    let command = format!("{} down", computer.program_command(version));
    let said = ssh::run(ssh, spec, &command, STOP_TIMEOUT)?;
    if said.ok() {
        Ok(())
    } else {
        Err(failed(spec, "stop its run", &said))
    }
}

/// The forwards of the tunnel of `run` on `spec`: each port it listens on there, once, to the same port here, the
/// first to the one the person gave (`port_here`); a port taken here is any free one.
pub fn forwards(run: &RunFile, spec: &RemoteSpec) -> Result<Vec<(Option<u16>, u16)>, String> {
    let mut ports: Vec<u16> = Vec::new();
    for endpoint in &run.endpoints {
        if let Some(port) = url_port(&endpoint.url).filter(|p| !ports.contains(p)) {
            ports.push(port);
        }
    }
    let mut forwards = Vec::new();
    for (i, there) in ports.into_iter().enumerate() {
        let here = if i == 0 {
            port_here(spec, there)?
        } else {
            let free = std::net::TcpListener::bind(("127.0.0.1", there)).is_ok();
            free.then_some(there)
        };
        forwards.push((here, there));
    }
    Ok(forwards)
}

/// Where each thing `run` started is reached here, through `tunnel`: its address there with the port there put
/// in place of the port here.
pub fn local_endpoints(run: &RunFile, tunnel: &Tunnel) -> Vec<EndpointInfo> {
    run.endpoints
        .iter()
        .map(|endpoint| {
            let mut here = endpoint.clone();
            if let Some(there) = url_port(&endpoint.url) {
                if let Some((port, _)) = tunnel.forwards.iter().find(|(_, t)| *t == there) {
                    here.url = endpoint
                        .url
                        .replacen(&format!(":{there}"), &format!(":{port}"), 1);
                }
            }
            here
        })
        .collect()
}

/// The port of an address (`http://127.0.0.1:8080/v1` is 8080).
pub(super) fn url_port(url: &str) -> Option<u16> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next()?;
    authority.rsplit_once(':')?.1.parse().ok()
}

/// `text` as one word of the shell of `computer`.
fn word(computer: &Computer, text: &str) -> String {
    match computer.system {
        System::Windows => format!("\"{}\"", text.replace('"', "")),
        _ => sh_quote(text),
    }
}

/// A command of the headless program that did not end well, in a sentence: what it was to do, and what it said.
fn failed(spec: &RemoteSpec, what: &str, said: &ssh::Said) -> String {
    let text = if said.stderr.trim().is_empty() {
        said.stdout.trim()
    } else {
        said.stderr.trim()
    };
    let line = text
        .lines()
        .map(|l| l.trim_start_matches("quickai-headless: ").trim())
        .find(|l| !l.is_empty())
        .unwrap_or("it said nothing");
    format!("QuickAI on {} could not {what}: {line}", spec.host.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(role: &str, url: &str) -> EndpointInfo {
        EndpointInfo {
            role: role.into(),
            program: "llama-server".into(),
            url: url.into(),
        }
    }

    #[test]
    fn a_delete_there_is_said_as_one_here_is() {
        let report: Deleted = serde_json::from_str(
            r#"{"removed":["m.gguf","mmproj-m.gguf"],"kept":["shared.gguf"],"failed":[],"error":null}"#,
        )
        .unwrap();
        assert_eq!(
            report.sentences("m.gguf", "Studio"),
            [
                "Deleted model on Studio: m.gguf",
                "Also deleted: mmproj-m.gguf.",
                "Kept shared.gguf: another model uses it.",
            ]
        );
        let failed = Deleted {
            error: Some("permission denied".into()),
            ..Deleted::default()
        };
        assert_eq!(
            failed.sentences("m.gguf", "Studio"),
            ["Could not delete m.gguf on Studio: permission denied"]
        );
    }

    #[test]
    fn the_port_of_an_address_is_read() {
        assert_eq!(url_port("http://127.0.0.1:8080/v1"), Some(8080));
        assert_eq!(url_port("http://127.0.0.1:8188"), Some(8188));
        assert_eq!(url_port("127.0.0.1:11434"), Some(11434));
        assert_eq!(url_port("http://localhost/"), None);
    }

    #[test]
    fn each_port_there_is_brought_once_to_the_same_port_here_unless_it_is_taken() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = taken.local_addr().unwrap().port();
        let free = crate::runtime::process::ephemeral_port().unwrap();
        let run = RunFile {
            pid: 1,
            configuration: "vision".into(),
            version: "0.2.0".into(),
            started: 0,
            detached: true,
            state: "ready".into(),
            endpoints: vec![
                endpoint("api", &format!("http://127.0.0.1:{free}/v1")),
                endpoint("page", &format!("http://127.0.0.1:{free}/")),
                endpoint("page", &format!("http://127.0.0.1:{busy}")),
            ],
            model: None,
            steps: Vec::new(),
            computer: None,
        };
        let spec = RemoteSpec::new("Studio", "studio.local");
        assert_eq!(
            forwards(&run, &spec).unwrap(),
            [(Some(free), free), (None, busy)]
        );
    }
}
