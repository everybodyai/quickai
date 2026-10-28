//! The steps of a run on another computer, each on a thread of its own, saying what they do as they go (WP4.33.7,
//! docs/remote-runs.md sections 3 to 3.2): what the window asks for (start, find again, stop) and what it is told.
//!
//! - **start**: what the computer is, the headless program copied there when it is not, what runs there (another
//!   run is said, and nothing is started unless the person said to override it), the configuration copied and
//!   started in the background, followed until it is ready, and its ports brought here;
//! - **find**: a run that goes on there (started earlier, from this window or another) is found, and its ports are
//!   brought here again;
//! - **stop**: what runs there is stopped.
//!
//! The tunnel that comes back belongs to whoever asked; the run there does not, and goes on until it is stopped.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::provision::EndpointInfo;
use crate::run_file::{RunFile, Status};

use super::probe::Computer;
use super::run::{self, Launch, Started};
use super::{deploy, open_forwards, probe, slug, RemoteSpec, Tunnel, OPEN_TIMEOUT};

/// How often a run that is being prepared there is asked after.
const FOLLOW_EVERY: Duration = Duration::from_secs(2);

/// What a job says, in the order it happens.
pub enum Update {
    /// What is being done now, in a few words (`Copying QuickAI to Studio`).
    Doing(String),
    /// What the run there says of itself, while it is being prepared.
    Status(Box<Status>),
    /// Another run goes on there, and nothing was started: the window asks whether to override it (section 3.2).
    Busy(Option<Box<RunFile>>),
    /// It runs there, and each thing it started is reached here at `endpoints`, through `tunnel`.
    Running {
        tunnel: Tunnel,
        endpoints: Vec<EndpointInfo>,
        run: Box<RunFile>,
    },
    /// Nothing runs there (what `find` found).
    Nothing,
    /// What ran there has stopped (what `stop` did).
    Stopped,
    /// It could not be done, and why, in a sentence.
    Failed(String),
}

/// Who and where: the programs that connect (ssh and scp, or a test's stand-ins), the computer, and the version of
/// the headless program to run there (this release's).
#[derive(Clone)]
pub struct Job {
    pub ssh: PathBuf,
    pub scp: PathBuf,
    pub spec: RemoteSpec,
    pub version: String,
}

/// What a start runs: the configuration's name and text (the window's export of what it holds), flags of
/// `quickai-run up`, and whether what runs there now is to be stopped first (the person said Override).
#[derive(Clone)]
pub struct Launching {
    pub name: String,
    pub text: String,
    pub flags: Vec<String>,
    pub override_first: bool,
}

/// Starts `launching` on the job's computer. `wake` is called after each update; `cancel` ends the following of a
/// run that is being prepared (the run there goes on).
pub fn start(
    job: Job,
    launching: Launching,
    wake: impl Fn() + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> Receiver<Update> {
    spawn(wake, move |say| {
        let computer = reach(&job, say)?;
        let here = run::status(&job.ssh, &job.spec, &computer, &job.version)?;
        if let Some(running) = here.running {
            if running.name() == slug(&launching.name) {
                // it is this one, started earlier: it is followed, not started again
                return follow(&job, &computer, say, &cancel);
            }
            if !launching.override_first {
                say(Update::Busy(Some(Box::new(running))));
                return Ok(());
            }
            say(Update::Doing(format!(
                "Stopping \"{}\" on {}",
                running.name(),
                job.spec.name
            )));
            run::stop(&job.ssh, &job.spec, &computer, &job.version)?;
        }
        say(Update::Doing(format!(
            "Starting \"{}\" on {}",
            launching.name, job.spec.name
        )));
        let file = std::env::temp_dir().join(format!(
            "quickai-remote-{}-{}.toml",
            std::process::id(),
            slug(&launching.name)
        ));
        std::fs::write(&file, &launching.text)
            .map_err(|e| format!("Could not write {}: {e}", file.display()))?;
        let started = run::start(
            &job.ssh,
            &job.scp,
            &job.spec,
            &computer,
            &job.version,
            &Launch {
                configuration: &file,
                name: &launching.name,
                environment: None,
                flags: launching.flags.clone(),
            },
        );
        let _ = std::fs::remove_file(&file);
        match started? {
            Started::Started => follow(&job, &computer, say, &cancel),
            Started::Busy(running) => {
                say(Update::Busy(running));
                Ok(())
            }
        }
    })
}

/// Finds a run that goes on on the job's computer and brings its ports here; `Nothing` when none does (or the
/// headless program of this release is not there, so nothing of it can run).
pub fn find(job: Job, wake: impl Fn() + Send + 'static) -> Receiver<Update> {
    spawn(wake, move |say| {
        let computer = probe::probe(&job.ssh, &job.spec, &job.version)?;
        if computer.headless.is_none() {
            say(Update::Nothing);
            return Ok(());
        }
        let here = run::status(&job.ssh, &job.spec, &computer, &job.version)?;
        match here.running {
            Some(_) => follow(&job, &computer, say, &AtomicBool::new(false)),
            None => {
                say(Update::Nothing);
                Ok(())
            }
        }
    })
}

/// Stops what runs on the job's computer.
pub fn stop(job: Job, wake: impl Fn() + Send + 'static) -> Receiver<Update> {
    spawn(wake, move |say| {
        say(Update::Doing(format!(
            "Stopping the run on {}",
            job.spec.name
        )));
        let computer = probe::probe(&job.ssh, &job.spec, &job.version)?;
        if computer.headless.is_some() {
            run::stop(&job.ssh, &job.spec, &computer, &job.version)?;
        }
        say(Update::Stopped);
        Ok(())
    })
}

/// What the computer is, with the headless program of this release there (copied when it is not).
fn reach(job: &Job, say: &dyn Fn(Update)) -> Result<Computer, String> {
    say(Update::Doing(format!("Connecting to {}", job.spec.name)));
    let mut computer = probe::probe(&job.ssh, &job.spec, &job.version)?;
    if computer.headless.is_none() {
        say(Update::Doing(format!(
            "Copying QuickAI to {} ({})",
            job.spec.name,
            computer.describe()
        )));
        deploy::ensure_program(&job.ssh, &job.scp, &job.spec, &computer, &job.version)?;
        computer.headless = Some(format!("quickai-run {}", job.version));
    }
    Ok(computer)
}

/// Follows the run there until it is ready (saying what it says as it goes), then brings its ports here. A run
/// that ended before it was ready says why.
fn follow(
    job: &Job,
    computer: &Computer,
    say: &dyn Fn(Update),
    cancel: &AtomicBool,
) -> Result<(), String> {
    loop {
        let status = run::status(&job.ssh, &job.spec, computer, &job.version)?;
        match &status.running {
            Some(running) if running.state == "ready" => {
                let running = running.clone();
                say(Update::Doing(format!(
                    "Bringing the ports of {} here",
                    job.spec.name
                )));
                let forwards = run::forwards(&running, &job.spec)?;
                let tunnel = open_forwards(&job.ssh, &job.spec, &forwards, OPEN_TIMEOUT)?;
                let endpoints = run::local_endpoints(&running, &tunnel);
                say(Update::Running {
                    tunnel,
                    endpoints,
                    run: Box::new(running),
                });
                return Ok(());
            }
            Some(_) => say(Update::Status(Box::new(status))),
            None => {
                let why = status
                    .last
                    .and_then(|last| last.message.or(Some(last.outcome)))
                    .unwrap_or_else(|| "it ended before it was ready".into());
                return Err(format!("The run on {} stopped: {why}", job.spec.name));
            }
        }
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        std::thread::sleep(FOLLOW_EVERY);
    }
}

/// Runs `steps` on a thread of its own: what it says goes to the receiver (and wakes the window), and a failure is
/// said last.
fn spawn(
    wake: impl Fn() + Send + 'static,
    steps: impl FnOnce(&dyn Fn(Update)) -> Result<(), String> + Send + 'static,
) -> Receiver<Update> {
    let (tx, rx): (Sender<Update>, Receiver<Update>) = crossbeam_channel::unbounded();
    std::thread::spawn(move || {
        let say = |update: Update| {
            let _ = tx.send(update);
            wake();
        };
        if let Err(why) = steps(&say) {
            say(Update::Failed(why));
        }
    });
    rx
}
