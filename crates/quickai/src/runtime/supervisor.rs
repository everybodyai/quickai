//! One run, on one thread: choosing between an engine that is already running and one to start,
//! the live-traffic gateway, the program, the wait for it to answer, loading the model, the watch
//! over it, unloading and the stop (docs/architecture.md, sections 11.1 and 11.2).
//!
//! The thread that starts a program is the one that stays with it until it is gone. That is not
//! tidiness: on Linux the parent-death signal that makes the program follow QuickAI fires when the
//! *thread* that started it exits, so a thread that started it and went away would end it at once
//! (`process.rs`). The interface talks to the thread through `Command`s and hears from it through
//! the `Event` channel; it never touches the child.
//!
//! Two ways of running share what comes after the engine answers:
//!
//! * **spawned**: the program is QuickAI's. It is watched through its process, unloaded and then
//!   stopped by the plan's policy, and a model that cannot be loaded ends the start with the
//!   program stopped.
//! * **attached**: the engine was already there (a person's own Ollama). Nothing of it is QuickAI's
//!   to stop: it is asked about from time to time (`liveness`), its model is unloaded when the run
//!   ends, and it is left running. No gateway is put in front of it.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use quickai_core::decode::ApiDecoder;
use quickai_core::doc::engine::{OnTimeout, StopPolicy};
use quickai_core::plan::{
    AttachPlan, CallPlan, Endpoint, Gateway, IdentityPlan, SpawnPlan, Start, StopPlan, Unit,
};

use super::probe::HttpProbe;
use super::process::{child_env, send, spawn_reader, wait_for_readers, Daemon, ReaderDone, Wake};
use crate::events::{Event, ServerState};
use crate::prims::http_call::{call, call_cancellable, Call, CallError, CancelWhen, Framing};
use crate::proxy::{self, ProxyHandle};

/// How often the thread looks up from what it is doing to see whether it has been told something
/// or the program has exited.
const TICK: Duration = Duration::from_millis(25);

/// How long the end of a run waits for the program's last words to reach the log: a grandchild
/// that kept the pipes open must not hold the end of a run up.
const OUTPUT_GRACE: Duration = Duration::from_millis(500);

/// How long a question to whatever is on the port ("who are you?") may take before something that
/// accepts a connection and says nothing is taken for something else.
const IDENTITY_TIMEOUT: Duration = Duration::from_secs(2);

/// What an unload may take while the application is closing, whatever the document allows.
const UNLOAD_WHEN_CLOSING: Duration = Duration::from_secs(2);

/// What the interface can tell a run.
pub enum Command {
    /// End the run. `closing` is set while the application is exiting: the stop then waits only
    /// the plan's short grace, so that closing the window is not held up.
    Stop { closing: bool },
    /// Load a model into the engine that is running with nothing loaded (management mode).
    Load {
        warm: Vec<CallPlan>,
        unload: Vec<CallPlan>,
        name: String,
    },
    /// Unload the model but keep the engine.
    Unload,
}

/// Everything a run needs, owned by its thread.
pub struct Run {
    pub unit: Unit,
    pub gateway: Option<Gateway>,
    /// How the gateway reads the traffic it relays (the plan's, from the vocabulary).
    pub decode: Vec<ApiDecoder>,
    /// The model the run is for, as the interface names it; `None` for management mode.
    pub model: Option<String>,
    pub events: Sender<Event>,
    pub wake: Wake,
    pub commands: Receiver<Command>,
}

/// How watching the engine ended.
enum Outcome {
    /// The program exited and nobody had asked it to.
    Exited(ExitStatus),
    /// The program could not be asked about any more.
    Lost(std::io::Error),
    /// The ready timeout passed and the plan says to give up.
    TimedOut,
    /// The model could not be loaded, or an engine that was attached to stopped answering: the
    /// sentence that says so.
    Failed(String),
    /// A stop was asked for.
    Stop { closing: bool },
}

/// What answers on the port when the engine is asked who it is.
enum Identified {
    /// The engine, and the version it says it is.
    Engine(String),
    /// Something answers, and it is not the engine.
    Other,
    /// Nothing is listening.
    Nobody,
}

/// How a run of calls ended.
enum CallsOutcome {
    Done,
    /// A call failed: the engine's own sentence, or ours.
    Failed(String),
    Stop {
        closing: bool,
    },
    /// The program exited while it was being asked.
    Exited(ExitStatus),
}

pub fn run(run: Run) {
    Supervisor {
        unit: run.unit,
        gateway: run.gateway,
        decode: run.decode,
        model: run.model,
        events: run.events,
        wake: run.wake,
        commands: run.commands,
        proxy: None,
        queued: VecDeque::new(),
        unload_with: None,
    }
    .execute();
}

struct Supervisor {
    unit: Unit,
    gateway: Option<Gateway>,
    decode: Vec<ApiDecoder>,
    model: Option<String>,
    events: Sender<Event>,
    wake: Wake,
    commands: Receiver<Command>,
    proxy: Option<ProxyHandle>,
    /// Commands that arrived while the thread was busy with a call, in order.
    queued: VecDeque<Command>,
    /// What frees the model this run loaded, `None` when nothing is loaded.
    unload_with: Option<Vec<CallPlan>>,
}

fn place(endpoint: &Endpoint) -> String {
    format!(
        "{}:{}",
        endpoint.host,
        endpoint.port.map_or_else(String::new, |p| p.to_string())
    )
}

impl Supervisor {
    fn emit(&self, event: Event) {
        send(&self.events, event, &self.wake);
    }

    fn log(&self, line: impl Into<String>) {
        self.emit(Event::Log(line.into()));
    }

    fn server(&self) -> String {
        self.unit.server_name.clone()
    }

    /// The last thing a run does: free the public port, then say how it ended.
    fn finish(&mut self, state: ServerState) {
        if let Some(proxy) = self.proxy.take() {
            proxy.stop();
        }
        self.emit(Event::StateChanged(state));
    }

    /// The next command, waiting up to `wait` for one. Commands that arrived during a call come
    /// first. `Err` when the interface is gone: the application is closing.
    fn next_command(&mut self, wait: Duration) -> Result<Option<Command>, ()> {
        if let Some(command) = self.queued.pop_front() {
            return Ok(Some(command));
        }
        match self.commands.recv_timeout(wait) {
            Ok(command) => Ok(Some(command)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        }
    }

    fn execute(mut self) {
        let attach = self.unit.start.iter().find_map(|s| match s {
            Start::Attach(a) => Some(a.clone()),
            Start::Spawn(_) => None,
        });
        let spawn = self.unit.start.iter().find_map(|s| match s {
            Start::Spawn(s) => Some(s.clone()),
            Start::Attach(_) => None,
        });

        // Who is on the port decides which way to run. Only a port that answers *like the engine* is
        // attached to; a port that answers like something else is never used and never taken as a
        // reason to start a second engine on it; and where an engine already answers, a second is
        // not started.
        match (attach, spawn) {
            (Some(attach), spawn) => match self.identify(&attach.endpoint, &attach.identity) {
                Identified::Engine(version) => self.run_attached(&attach, &version),
                Identified::Other => {
                    self.log(format!(
                        "{} is held by something that is not {}: it did not answer like one, so none was used or started. Pick another port.",
                        place(&attach.endpoint),
                        self.server()
                    ));
                    self.finish(ServerState::Crashed);
                }
                Identified::Nobody => match spawn {
                    Some(spawn) => self.run_spawned(&spawn),
                    None => {
                        self.log(format!(
                            "No {} is answering on {}, and this setting uses only one that is already running.",
                            self.server(),
                            place(&attach.endpoint)
                        ));
                        self.finish(ServerState::Crashed);
                    }
                },
            },
            (None, Some(spawn)) => {
                if let Some(identity) = &spawn.identity {
                    let endpoint = self.unit.endpoint.clone();
                    if let Identified::Engine(version) = self.identify(&endpoint, identity) {
                        self.log(format!(
                            "{} {version} is already running on {}, so a second one cannot start there. Use it, or pick another port.",
                            self.server(),
                            place(&endpoint)
                        ));
                        self.finish(ServerState::Crashed);
                        return;
                    }
                }
                self.run_spawned(&spawn);
            }
            (None, None) => {
                self.log("The run plan has no way to start the server.");
                self.finish(ServerState::Crashed);
            }
        }
    }

    /// Asks what answers on `endpoint`.
    fn identify(&self, endpoint: &Endpoint, identity: &IdentityPlan) -> Identified {
        let asked = call(
            endpoint,
            &Call {
                method: "GET".into(),
                path: identity.path.clone(),
                body: None,
                framing: Framing::Json,
                timeout: IDENTITY_TIMEOUT,
            },
        );
        match asked {
            Ok(reply) => match reply
                .body
                .pointer(&identity.pointer)
                .and_then(|v| v.as_str())
            {
                Some(version) if !version.is_empty() => Identified::Engine(version.to_string()),
                _ => Identified::Other,
            },
            Err(CallError::Unreachable(_) | CallError::Refused(_)) => Identified::Nobody,
            // something answered, in a way that is not the engine's
            Err(_) => Identified::Other,
        }
    }

    // ---- an engine that was already running ---------------------------------------------------

    fn run_attached(&mut self, attach: &AttachPlan, version: &str) {
        self.emit(Event::Attached(true));
        self.log(format!(
            "Attached to {} {version} that is already running on {}.",
            self.server(),
            place(&attach.endpoint)
        ));
        if self.gateway.is_some() {
            self.log(format!(
                "Live conversation traffic is not shown for this run: {} was already running, so QuickAI cannot sit in front of it. Start one from QuickAI to see its traffic.",
                self.server()
            ));
        }
        let outcome = match self.bring_up_model(&attach.endpoint, None) {
            Ok(()) => self.watch_attached(attach),
            Err(outcome) => outcome,
        };
        match outcome {
            Outcome::Stop { closing } => {
                self.unload_model(&attach.endpoint, closing);
                self.log(format!(
                    "Stopped using {}; it is still running.",
                    self.server()
                ));
                self.finish(ServerState::Idle);
            }
            Outcome::Failed(message) => {
                // the engine is not ours: the model that was being loaded is all that is dropped
                self.log(message);
                self.finish(ServerState::Crashed);
            }
            Outcome::Exited(_) | Outcome::Lost(_) | Outcome::TimedOut => {
                self.finish(ServerState::Crashed)
            }
        }
    }

    /// Asks the attached engine from time to time whether it is still there, while answering the
    /// interface.
    fn watch_attached(&mut self, attach: &AttachPlan) -> Outcome {
        let probe = HttpProbe::new(
            &attach.endpoint.host,
            attach.endpoint.port.unwrap_or(0),
            &attach.liveness.path,
            200,
        );
        let every = Duration::from_millis(attach.liveness.every_ms.max(1));
        let mut next = Instant::now() + every;
        let mut misses = 0u32;
        loop {
            match self.next_command(TICK) {
                Err(()) => return Outcome::Stop { closing: true },
                Ok(Some(Command::Stop { closing })) => return Outcome::Stop { closing },
                Ok(Some(command)) => {
                    if let Some(outcome) = self.serve(command, &attach.endpoint, None) {
                        return outcome;
                    }
                }
                Ok(None) => {}
            }
            if Instant::now() < next {
                continue;
            }
            next = Instant::now() + every;
            if probe.ready() {
                misses = 0;
            } else {
                misses += 1;
                if misses >= attach.liveness.failures {
                    return Outcome::Failed(format!(
                        "{} stopped answering on {} ({misses} questions in a row went unanswered).",
                        self.server(),
                        place(&attach.endpoint)
                    ));
                }
            }
        }
    }

    // ---- an engine this run starts -------------------------------------------------------------

    fn run_spawned(&mut self, spawn: &SpawnPlan) {
        // With the proxy on, the engine itself listens on an internal-only port that only the
        // proxy talks to; the proxy binds the public port (what the GUI and clients actually
        // connect to). With it off there is no gateway and the engine binds the public port
        // directly, exactly as before the proxy existed.
        if let Some(gateway) = self.gateway {
            let wake = self.wake.clone();
            match proxy::spawn(
                gateway.public_port,
                gateway.upstream_port,
                self.decode.clone(),
                self.events.clone(),
                std::sync::Arc::new(move || wake()),
            ) {
                Ok(handle) => self.proxy = Some(handle),
                Err(e) => {
                    self.log(format!(
                        "Could not start the live-traffic proxy on port {}: {e}. Is something else already using that port?",
                        gateway.public_port
                    ));
                    self.finish(ServerState::Crashed);
                    return;
                }
            }
        }
        self.start_and_watch(spawn);
    }

    fn start_and_watch(&mut self, spawn: &SpawnPlan) {
        self.log(spawn.command_line());
        if let Some(line) = spawn.env_line() {
            self.log(line);
        }

        let env = child_env(spawn, &|name| std::env::var(name).ok());
        let mut daemon = match Daemon::spawn(spawn, &env) {
            Ok(daemon) => daemon,
            Err(e) => {
                self.log(format!("Failed to launch server process: {e}"));
                self.finish(ServerState::Crashed);
                return;
            }
        };
        let (stdout, stderr) = daemon.take_output();
        let readers = [
            spawn_reader(stdout, self.events.clone(), self.wake.clone()),
            spawn_reader(stderr, self.events.clone(), self.wake.clone()),
        ];

        let outcome = self.watch_spawned(&mut daemon);
        self.end_spawned(&mut daemon, &readers, outcome);
    }

    fn end_spawned(&mut self, daemon: &mut Daemon, readers: &[ReaderDone], outcome: Outcome) {
        match outcome {
            Outcome::Stop { closing } => {
                if let Some(proxy) = self.proxy.take() {
                    // stop taking new connections now; the program may take a while to leave
                    proxy.stop();
                }
                let endpoint = self.unit.endpoint.clone();
                self.unload_model(&endpoint, closing);
                let status = self.stop_program(daemon, closing);
                wait_for_readers(readers, OUTPUT_GRACE);
                match status {
                    // `ExitStatus` writes "exit status: 0", which would read twice here
                    Some(status) if status.success() => {
                        self.log("Server stopped (exit status: 0).")
                    }
                    Some(status) => self.log(format!("Server stopped (exit status: {status}).")),
                    None => self.log("Server stopped."),
                }
                self.finish(ServerState::Idle);
            }
            Outcome::Exited(status) => {
                // what the program started and left running goes with it
                daemon.sweep();
                self.crashed(
                    readers,
                    format!("Server process exited unexpectedly (exit status: {status})."),
                );
            }
            Outcome::Lost(e) => {
                daemon.kill();
                self.crashed(readers, format!("Lost track of the server process: {e}"));
            }
            Outcome::TimedOut => {
                self.log("Giving up: stopping what was started.");
                self.stop_program(daemon, false);
                self.crashed(readers, "The server never became healthy.".to_string());
            }
            Outcome::Failed(message) => {
                // the model could not be loaded: what was started for it is stopped again
                self.log(message);
                self.stop_program(daemon, false);
                wait_for_readers(readers, OUTPUT_GRACE);
                self.finish(ServerState::Crashed);
            }
        }
    }

    fn crashed(&mut self, readers: &[ReaderDone], message: String) {
        wait_for_readers(readers, OUTPUT_GRACE);
        self.log(message);
        self.finish(ServerState::Crashed);
    }

    /// Waits for the program to answer, loads the model, then watches it, until it exits or is
    /// told to stop.
    fn watch_spawned(&mut self, daemon: &mut Daemon) -> Outcome {
        let endpoint = self.unit.endpoint.clone();
        let probe = match (&self.unit.ready, endpoint.port) {
            (Some(ready), Some(port)) => Some((
                HttpProbe::new(&endpoint.host, port, &ready.path, ready.expect_status),
                ready.clone(),
            )),
            _ => None,
        };
        // an engine with nothing to ask is as ready as it will get
        let mut ready = probe.is_none();
        let mut waiting = probe.is_some();
        let mut running = false;
        let started = Instant::now();
        let mut next_probe = started;

        loop {
            if ready && !running {
                // it answers: load the model (or say that there is none), and only then run
                if let Err(outcome) = self.bring_up_model(&endpoint, Some(daemon)) {
                    return outcome;
                }
                running = true;
            }
            match self.next_command(TICK) {
                Err(()) => return Outcome::Stop { closing: true },
                Ok(Some(Command::Stop { closing })) => return Outcome::Stop { closing },
                Ok(Some(command)) if running => {
                    if let Some(outcome) = self.serve(command, &endpoint, Some(daemon)) {
                        return outcome;
                    }
                }
                // a load before the engine is running is not something to do yet
                Ok(Some(_)) | Ok(None) => {}
            }
            match daemon.try_wait() {
                Ok(Some(status)) => return Outcome::Exited(status),
                Ok(None) => {}
                Err(e) => return Outcome::Lost(e),
            }
            let Some((probe, plan)) = probe.as_ref().filter(|_| waiting) else {
                continue;
            };
            if Instant::now() < next_probe {
                continue;
            }
            if started.elapsed() > Duration::from_millis(plan.timeout_ms) {
                self.log("Timed out waiting for server to become healthy.");
                match plan.on_timeout {
                    OnTimeout::Fail => return Outcome::TimedOut,
                    // it may still be loading a big model: stop asking, keep watching, and the
                    // state stays `Starting`, as it always has
                    OnTimeout::Wait => waiting = false,
                }
                continue;
            }
            if probe.ready() {
                waiting = false;
                ready = true;
            } else {
                next_probe = Instant::now() + Duration::from_millis(plan.interval_ms);
            }
        }
    }

    // ---- the model ------------------------------------------------------------------------------

    /// The engine answers: load the model, then run. With no model the engine runs with nothing
    /// loaded. A failure to load is the engine's own sentence, and ends the start.
    fn bring_up_model(
        &mut self,
        endpoint: &Endpoint,
        daemon: Option<&mut Daemon>,
    ) -> Result<(), Outcome> {
        if self.unit.warm.is_empty() {
            if self.model.is_none() {
                self.log(format!("{} ready, no model loaded.", self.server()));
            }
            self.emit(Event::StateChanged(ServerState::Running));
            return Ok(());
        }
        let name = self.model.clone().unwrap_or_default();
        self.log(format!("Loading {name} into {}...", self.server()));
        let warm = self.unit.warm.clone();
        match self.run_calls(endpoint, &warm, false, daemon) {
            CallsOutcome::Done => {
                self.unload_with = Some(self.unit.unload.clone());
                self.emit(Event::StateChanged(ServerState::Running));
                Ok(())
            }
            CallsOutcome::Failed(message) => Err(Outcome::Failed(message)),
            CallsOutcome::Stop { closing } => Err(Outcome::Stop { closing }),
            CallsOutcome::Exited(status) => Err(Outcome::Exited(status)),
        }
    }

    /// Makes the calls of a phase one after the other. A call that cannot be made is a failure
    /// unless it is `best_effort`; a stop that arrives meanwhile cancels the call that is under
    /// way, and a program that exits meanwhile is reported as that.
    fn run_calls(
        &mut self,
        endpoint: &Endpoint,
        calls: &[CallPlan],
        always_best_effort: bool,
        mut daemon: Option<&mut Daemon>,
    ) -> CallsOutcome {
        for planned in calls {
            let request = Call {
                method: planned.method.clone(),
                path: planned.path.clone(),
                body: (!planned.body.is_null()).then(|| planned.body.clone()),
                framing: Framing::Json,
                timeout: Duration::from_millis(planned.timeout_ms),
            };
            // while the call waits, the commands are watched: a stop cancels it, the rest queue up
            let stop: Cell<Option<bool>> = Cell::new(None);
            let queue: RefCell<Vec<Command>> = RefCell::new(Vec::new());
            let commands = &self.commands;
            let result = call_cancellable(
                endpoint,
                &request,
                &CancelWhen(|| {
                    if stop.get().is_some() {
                        return true;
                    }
                    while let Ok(command) = commands.try_recv() {
                        match command {
                            Command::Stop { closing } => {
                                stop.set(Some(closing));
                                return true;
                            }
                            other => queue.borrow_mut().push(other),
                        }
                    }
                    false
                }),
            );
            self.queued.extend(queue.into_inner());
            if let Some(closing) = stop.get() {
                return CallsOutcome::Stop { closing };
            }
            if let Err(error) = result {
                // an engine that died while it was asked is a crash, not a failed call
                if let Some(daemon) = daemon.as_deref_mut() {
                    if let Ok(Some(status)) = daemon.try_wait() {
                        return CallsOutcome::Exited(status);
                    }
                }
                let message = match &error {
                    CallError::TimedOut => format!(
                        "{} took longer than {} to answer {} {}.",
                        self.server(),
                        human(Duration::from_millis(planned.timeout_ms)),
                        planned.method,
                        planned.path
                    ),
                    other => other.to_string(),
                };
                if planned.best_effort || always_best_effort {
                    self.log(format!("{message} (not acted on)"));
                    continue;
                }
                return CallsOutcome::Failed(message);
            }
        }
        CallsOutcome::Done
    }

    /// Frees the model this run loaded, best effort: an engine that does not answer must not hold
    /// up the end of a run.
    fn unload_model(&mut self, endpoint: &Endpoint, closing: bool) {
        let Some(unload) = self.unload_with.take() else {
            return;
        };
        let calls: Vec<CallPlan> = unload
            .into_iter()
            .map(|mut c| {
                if closing {
                    c.timeout_ms = c.timeout_ms.min(UNLOAD_WHEN_CLOSING.as_millis() as u64);
                }
                c
            })
            .collect();
        // it has its own timeout, and the run is ending anyway
        let _ = self.run_calls(endpoint, &calls, true, None);
    }

    /// A command that arrives while the engine runs.
    fn serve(
        &mut self,
        command: Command,
        endpoint: &Endpoint,
        daemon: Option<&mut Daemon>,
    ) -> Option<Outcome> {
        match command {
            Command::Stop { closing } => Some(Outcome::Stop { closing }),
            Command::Unload => {
                self.unload_model(endpoint, false);
                self.emit(Event::Model(None));
                None
            }
            Command::Load { warm, unload, name } => {
                // a model already loaded makes way first
                self.unload_model(endpoint, false);
                self.log(format!("Loading {name} into {}...", self.server()));
                match self.run_calls(endpoint, &warm, false, daemon) {
                    CallsOutcome::Done => {
                        self.unload_with = Some(unload);
                        self.emit(Event::Model(Some(name)));
                        None
                    }
                    // the engine goes on running with nothing loaded; only the sentence is kept
                    CallsOutcome::Failed(message) => {
                        self.log(message);
                        self.emit(Event::Model(None));
                        None
                    }
                    CallsOutcome::Stop { closing } => Some(Outcome::Stop { closing }),
                    CallsOutcome::Exited(status) => Some(Outcome::Exited(status)),
                }
            }
        }
    }

    /// Ends the program the way the plan says and returns how it exited. A graceful stop asks it to
    /// leave and kills what is left when the grace has passed; a command to stop that arrives
    /// meanwhile, because the application is closing, shortens the wait.
    fn stop_program(&mut self, daemon: &mut Daemon, closing: bool) -> Option<ExitStatus> {
        let StopPlan {
            policy,
            grace_ms,
            exit_grace_ms,
        } = self.unit.stop;
        let status = match policy {
            StopPolicy::Kill => {
                daemon.kill();
                daemon.wait().ok()
            }
            StopPolicy::Graceful => {
                daemon.terminate();
                let exit_grace = Duration::from_millis(exit_grace_ms);
                let mut deadline = Instant::now()
                    + if closing {
                        exit_grace
                    } else {
                        Duration::from_millis(grace_ms)
                    };
                loop {
                    match daemon.try_wait() {
                        Ok(Some(status)) => break Some(status),
                        Ok(None) if Instant::now() < deadline => {}
                        Ok(None) | Err(_) => {
                            daemon.kill();
                            break daemon.wait().ok();
                        }
                    }
                    match self.commands.recv_timeout(Duration::from_millis(10)) {
                        Ok(Command::Stop { closing: true })
                        | Err(RecvTimeoutError::Disconnected) => {
                            deadline = deadline.min(Instant::now() + exit_grace);
                        }
                        Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                    }
                }
            }
        };
        // whatever the program left behind goes too
        daemon.sweep();
        status
    }
}

/// `600 ms`, `5 s`, `10 min`: a time for a sentence.
fn human(time: Duration) -> String {
    if time.as_millis() < 1000 {
        format!("{} ms", time.as_millis())
    } else if time.as_secs() < 120 {
        format!("{} s", time.as_secs())
    } else {
        format!("{} min", time.as_secs() / 60)
    }
}
