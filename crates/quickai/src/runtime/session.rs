//! Executes a run plan: starts the engine, puts the live-traffic proxy in front of it when the plan
//! asks, watches it come up, streams its log and stops it (docs/architecture.md, section 11.1).
//! One session is one run, matching the GUI's single start/stop control.
//!
//! What to run, with which arguments and environment, on which ports and how to tell it is ready
//! all come from the `RunPlan` the planner built from the engine's document; this file only
//! carries it out. A run lives on a thread of its own (`supervisor.rs`); the session is the
//! interface's side of it: it hands the thread its plan, tells it to stop, and drains what the
//! thread reports once per frame.

use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use quickai_core::plan::{RunPlan, Start};

use super::process::{send, Wake};
use super::supervisor::{self, Command, Run};
use crate::events::{Event, ServerState, TrafficEvent};

pub struct Session {
    evt_tx: Sender<Event>,
    pub evt_rx: Receiver<Event>,
    state: ServerState,
    port: Option<u16>,
    model_name: Option<String>,
    /// The model a load into the running engine is under way for.
    loading: Option<String>,
    /// The run is using an engine that was already running.
    attached: bool,
    /// The thread of the current (or last) run.
    run: Option<RunHandle>,
}

struct RunHandle {
    commands: Sender<Command>,
    thread: JoinHandle<()>,
}

impl Session {
    pub fn new() -> Self {
        let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
        Self {
            evt_tx,
            evt_rx,
            state: ServerState::Idle,
            port: None,
            model_name: None,
            loading: None,
            attached: false,
            run: None,
        }
    }

    pub fn state(&self) -> ServerState {
        self.state
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn model_name(&self) -> Option<&str> {
        self.model_name.as_deref()
    }

    /// The model that is being loaded into the running engine, until the engine says how it went.
    pub fn loading(&self) -> Option<&str> {
        self.loading.as_deref()
    }

    /// Whether the engine in use is one that was already running when the run began.
    pub fn attached(&self) -> bool {
        self.attached
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            ServerState::Starting | ServerState::Running | ServerState::Stopping
        )
    }

    /// Drains any pending events, updating cached state. Call once per
    /// frame; returns true if anything changed (so callers can decide
    /// whether a repaint is worth it, though egui's wake already covers it).
    pub fn poll(
        &mut self,
        log: &mut impl FnMut(String),
        traffic: &mut impl FnMut(TrafficEvent),
    ) -> bool {
        let mut changed = false;
        while let Ok(event) = self.evt_rx.try_recv() {
            changed = true;
            match event {
                Event::Log(line) => log(line),
                Event::Model(name) => {
                    self.model_name = name;
                    self.loading = None;
                }
                Event::Attached(attached) => self.attached = attached,
                Event::StateChanged(s) => {
                    self.state = s;
                    if matches!(s, ServerState::Idle | ServerState::Crashed) {
                        self.port = None;
                        self.model_name = None;
                        self.loading = None;
                        self.attached = false;
                        // the run's thread is ending (it sent this last, after freeing the
                        // public port); nothing is left to tell it
                        self.run = None;
                    }
                }
                Event::Traffic(t) => traffic(t),
            }
        }
        changed
    }

    /// Carries out `plan`. Nothing happens while a run is already active.
    pub fn start(
        &mut self,
        plan: &RunPlan,
        model_name: &str,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        if self.is_active() {
            return;
        }
        let wake: Wake = Arc::new(wake);
        self.port = plan.public_port;
        // a plan with no model runs the engine with nothing loaded (management mode)
        let model = plan
            .model
            .is_some()
            .then(|| model_name.to_string())
            .filter(|name| !name.is_empty());
        self.model_name = model.clone();

        let Some(unit) = plan.units.first() else {
            self.crash("The run plan has nothing to start.".into(), &wake);
            return;
        };
        if !unit
            .start
            .iter()
            .any(|s| matches!(s, Start::Attach(_) | Start::Spawn(_)))
        {
            self.crash("The run plan has no way to start the server.".into(), &wake);
            return;
        }

        self.state = ServerState::Starting;
        let (commands, receiver) = crossbeam_channel::unbounded();
        let run = Run {
            unit: unit.clone(),
            gateway: plan.gateway,
            decode: plan.decode.clone(),
            model,
            events: self.evt_tx.clone(),
            wake: wake.clone(),
            commands: receiver,
        };
        match std::thread::Builder::new()
            .name("quickai-run".into())
            .spawn(move || supervisor::run(run))
        {
            Ok(thread) => self.run = Some(RunHandle { commands, thread }),
            Err(e) => self.crash(format!("Could not start a thread for the run: {e}"), &wake),
        }
    }

    /// Reports that a run could not even be planned or prepared, the way a failure to start is
    /// reported: a log line, then `Crashed`. `port` and `model_name` are what the run was for, so
    /// the interface can say so until it settles.
    pub fn report_start_failure(
        &mut self,
        port: u16,
        model_name: &str,
        message: String,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        if self.is_active() {
            return;
        }
        self.port = Some(port);
        self.model_name = Some(model_name.to_string());
        let wake: Wake = Arc::new(wake);
        self.crash(message, &wake);
    }

    fn crash(&mut self, message: String, wake: &Wake) {
        send(&self.evt_tx, Event::Log(message), wake);
        send(
            &self.evt_tx,
            Event::StateChanged(ServerState::Crashed),
            wake,
        );
    }

    /// Asks the run to end. It does not wait: the thread reports `Idle` when it is over.
    pub fn stop(&mut self) {
        if !self.is_active() {
            return;
        }
        self.state = ServerState::Stopping;
        if let Some(run) = &self.run {
            let _ = run.commands.send(Command::Stop { closing: false });
        }
    }

    /// Loads a model into an engine that is running without one (management mode, plan decision
    /// C22): runs the plan's warm step. The model's name is reported when it is loaded; a failure is
    /// logged and leaves the engine running with nothing loaded. The run's own wake-up is used.
    /// Start calls it for an engine that is running with no model and has one chosen; a second
    /// call while the first is under way does nothing.
    pub fn load(
        &mut self,
        plan: &RunPlan,
        model_name: &str,
        _wake: impl Fn() + Send + Sync + 'static,
    ) {
        if self.state != ServerState::Running || self.loading.is_some() {
            return;
        }
        let Some(unit) = plan.units.first() else {
            return;
        };
        if let Some(run) = &self.run {
            self.loading = Some(model_name.to_string());
            let _ = run.commands.send(Command::Load {
                warm: unit.warm.clone(),
                unload: unit.unload.clone(),
                name: model_name.to_string(),
            });
        }
    }

    /// Unloads the model but keeps the engine running.
    #[allow(dead_code)]
    pub fn unload(&mut self) {
        if self.state != ServerState::Running {
            return;
        }
        if let Some(run) = &self.run {
            let _ = run.commands.send(Command::Unload);
        }
    }

    /// Stops the run and waits until it is over, for use on app exit. The program is given the plan's
    /// short exit grace, so closing the window is not held up by a daemon that will not leave.
    pub fn stop_blocking(&mut self) {
        if let Some(run) = self.run.take() {
            let _ = run.commands.send(Command::Stop { closing: true });
            let _ = run.thread.join();
        }
        if self.is_active() {
            self.state = ServerState::Stopping;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::launch::{self, RunSettings};
    use crate::models::ModelEntry;
    use quickai_core::plan::{
        apply_env, Endpoint, EnvMode, EnvOp, Gateway, ReadyPlan, SpawnPlan, Traced, Unit,
    };
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn traced(value: &str) -> Traced<String> {
        Traced {
            value: value.to_string(),
            origin: "test".to_string(),
        }
    }

    // ---- launch parity (docs/implementation-plan.md, WP0.3, WP0.4 and WP1.7) ---------------------------
    //
    // `tests/golden/launch_matrix.json` was recorded from the application before the kernel existed:
    // the command line and environment `ServerManager::start` launched llama-server with, over a
    // matrix of selections, and proven against real spawns. The planner and the session must still
    // produce exactly that, from the embedded engine document.

    fn golden_rows() -> Vec<serde_json::Value> {
        serde_json::from_str(&crate::golden::read_golden("launch_matrix.json")).unwrap()
    }

    fn model_for(input: &serde_json::Value) -> ModelEntry {
        let path = PathBuf::from(input["model"].as_str().unwrap());
        ModelEntry {
            filename: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
            display: "test model".to_string(),
            note: String::new(),
            bundled: false,
            size_bytes: 0,
            companions: Vec::new(),
            mode: None,
        }
    }

    fn settings_for(input: &serde_json::Value) -> RunSettings {
        RunSettings {
            port: input["port"].as_u64().unwrap() as u16,
            ctx_size: input["ctx_size"].as_u64().unwrap() as u32,
            n_gpu_layers: input["n_gpu_layers"].as_u64().unwrap() as u32,
            extra_args: input["extra_args"].as_str().unwrap().to_string(),
            bin_override: None,
            traffic_proxy: input["proxy"].as_bool().unwrap(),
        }
    }

    fn embedded_registry() -> quickai_core::registry::Registry {
        build_registry(&ConfigDirs::default())
    }

    #[test]
    fn the_planner_reproduces_the_golden_launch_matrix() {
        let registry = embedded_registry();
        let rows = golden_rows();
        assert!(rows.len() >= 13, "the matrix lost rows");
        for row in &rows {
            let name = row["name"].as_str().unwrap();
            let input = &row["input"];
            let settings = settings_for(input);
            let internal = settings.traffic_proxy.then_some(41234u16);
            let plan = launch::plan_run(
                &registry,
                &model_for(input),
                &settings,
                Path::new(input["binary"].as_str().unwrap()),
                launch::MachineFacts::default(),
                internal,
            )
            .unwrap_or_else(|e| panic!("row {name}: {e:?}"));
            let Start::Spawn(spawn) = &plan.units[0].start[0] else {
                panic!("a spawn plan")
            };

            assert_eq!(
                spawn.program.value,
                row["program"].as_str().unwrap(),
                "row {name}: program"
            );
            let args: Vec<String> = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_string())
                .collect();
            assert_eq!(spawn.arg_values(), args, "row {name}: arguments");

            let inherited = input["inherited_ld_library_path"]
                .as_str()
                .unwrap()
                .to_string();
            let env = apply_env(&spawn.env, |n| {
                (n == "LD_LIBRARY_PATH" && !inherited.is_empty()).then(|| inherited.clone())
            });
            let want: Vec<(String, String)> = row["env"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    (
                        p[0].as_str().unwrap().to_string(),
                        p[1].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            assert_eq!(env, want, "row {name}: environment");

            assert_eq!(
                spawn.command_line(),
                row["log_line"].as_str().unwrap(),
                "row {name}: log line"
            );
            assert_eq!(
                plan.units[0].endpoint.port,
                Some(row["server_port"].as_u64().unwrap() as u16),
                "row {name}: the port the engine binds"
            );
            assert_eq!(
                plan.public_port,
                Some(input["port"].as_u64().unwrap() as u16),
                "row {name}: public port"
            );
            assert_eq!(
                plan.gateway.is_some(),
                input["proxy"].as_bool().unwrap(),
                "row {name}: proxy"
            );
        }
    }

    /// The matrix again, but through the real thing: the plan goes to `Session::start`, which spawns a
    /// script that writes down what it was started with.
    #[cfg(unix)]
    #[test]
    fn the_session_spawns_exactly_what_the_golden_matrix_says() {
        use std::os::unix::fs::PermissionsExt;

        // A stand-in `llama-server`: records its argument vector (NUL-separated, so no argument can
        // confuse the format) and the loader path it was given, then idles so the session sees a
        // live child until it is stopped.
        const RECORDING_SERVER: &str = "#!/bin/sh\n\
            d=$(dirname \"$0\")\n\
            printf '%s\\0' \"$@\" > \"$d/args.record\"\n\
            printf '%s' \"$LD_LIBRARY_PATH\" > \"$d/ld.record\"\n\
            : > \"$d/done.record\"\n\
            exec sleep 60\n";

        let registry = embedded_registry();
        let root =
            std::env::temp_dir().join(format!("quickai-session-proof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let root_str = root.display().to_string();
        let ambient = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();

        for row in golden_rows() {
            let name = row["name"].as_str().unwrap().to_string();
            let input = row["input"].clone();
            let rel_binary = input["binary"]
                .as_str()
                .unwrap()
                .trim_start_matches("/ROOT/")
                .to_string();
            let rel_model = input["model"]
                .as_str()
                .unwrap()
                .trim_start_matches("/ROOT/")
                .to_string();
            let binary = root.join(&rel_binary);
            std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
            std::fs::write(&binary, RECORDING_SERVER).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            let dir = binary.parent().unwrap().to_path_buf();
            // Rows share a directory; a record left by the previous row would satisfy the wait
            // below and be read as this row's.
            for record in ["args.record", "ld.record", "done.record"] {
                let _ = std::fs::remove_file(dir.join(record));
            }

            let mut settings = settings_for(&input);
            let internal = settings.traffic_proxy.then(|| {
                // the proxy really binds the public port, so it needs a free one
                crate::runtime::process::ephemeral_port().unwrap()
            });
            if settings.traffic_proxy {
                settings.port = crate::runtime::process::ephemeral_port().unwrap();
            }
            let mut model = model_for(&input);
            model.path = root.join(&rel_model);
            let plan = launch::plan_run(
                &registry,
                &model,
                &settings,
                &binary,
                launch::MachineFacts::default(),
                internal,
            )
            .unwrap_or_else(|e| panic!("row {name}: {e:?}"));

            let mut session = Session::new();
            session.start(&plan, "proof", || {});
            let done = dir.join("done.record");
            let deadline = Instant::now() + Duration::from_secs(20);
            while !done.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(done.exists(), "row {name}: the fake server never ran");
            session.stop_blocking();

            let recorded = std::fs::read(dir.join("args.record")).unwrap();
            let mut args: Vec<String> = recorded
                .split(|b| *b == 0)
                .map(|a| String::from_utf8(a.to_vec()).unwrap())
                .collect();
            assert_eq!(args.pop().as_deref(), Some(""), "row {name}: record format");
            for arg in &mut args {
                *arg = arg.replace(&root_str, "/ROOT");
            }
            let mut expected: Vec<String> = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_string())
                .collect();
            if settings.traffic_proxy {
                // the engine must bind the internal port, never the public one the proxy holds
                let i = args.iter().position(|a| a == "--port").unwrap();
                assert_eq!(args[i + 1], internal.unwrap().to_string(), "row {name}");
                assert_ne!(args[i + 1], settings.port.to_string(), "row {name}");
                for list in [&mut args, &mut expected] {
                    let i = list.iter().position(|a| a == "--port").unwrap();
                    list[i + 1] = "<PORT>".to_string();
                }
            }
            assert_eq!(
                args, expected,
                "row {name}: argv differs from the golden file"
            );

            // The loader path: today's rule, stated independently of the planner.
            let ld = std::fs::read_to_string(dir.join("ld.record")).unwrap();
            let dir_str = dir.display().to_string();
            let want_ld = if ambient.is_empty() {
                dir_str
            } else {
                format!("{dir_str}:{ambient}")
            };
            assert_eq!(ld, want_ld, "row {name}: LD_LIBRARY_PATH");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- the session on its own: hand-built plans -------------------------------------------------------

    fn sleep_plan(
        host_port: Option<u16>,
        ready_path: Option<&str>,
        gateway: Option<Gateway>,
    ) -> RunPlan {
        RunPlan {
            engine: "test".into(),
            model: Some("m".into()),
            public_port: gateway.map(|g| g.public_port).or(host_port),
            gateway,
            decode: Vec::new(),
            units: vec![Unit {
                id: "test".into(),
                engine: "test".into(),
                server_name: "test".into(),
                needs_model: true,
                warm: vec![],
                unload: vec![],
                endpoint: Endpoint {
                    host: "127.0.0.1".into(),
                    port: host_port,
                },
                apis: vec![],
                start: vec![Start::Spawn(SpawnPlan {
                    program: traced("sleep"),
                    args: vec![traced("30")],
                    env: vec![EnvOp {
                        name: "QUICKAI_TEST".into(),
                        mode: EnvMode::Set { value: "1".into() },
                        origin: "test".into(),
                    }],
                    group: false,
                    scrub_loader_env: false,
                    cwd: None,
                    mkdir: Vec::new(),
                    write: Vec::new(),
                    identity: None,
                })],
                ready: ready_path.map(|path| ReadyPlan {
                    path: path.to_string(),
                    expect_status: 200,
                    timeout_ms: 10_000,
                    interval_ms: 25,
                    on_timeout: quickai_core::doc::engine::OnTimeout::Fail,
                }),
                stop: quickai_core::plan::StopPlan::default(),
            }],
        }
    }

    /// An in-process stand-in for an engine's HTTP side: `/health` answers 200.
    #[cfg(unix)]
    fn health_server() -> (Arc<tiny_http::Server>, u16) {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let worker = server.clone();
        std::thread::spawn(move || {
            while let Ok(request) = worker.recv() {
                let status = if request.url() == "/health" { 200 } else { 404 };
                let _ = request.respond(tiny_http::Response::empty(status));
            }
        });
        (server, port)
    }

    fn wait_for(session: &mut Session, want: ServerState, secs: u64) -> Vec<String> {
        let mut log = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            session.poll(&mut |line| log.push(line), &mut |_| {});
            if session.state() == want {
                return log;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the session never reached {want:?} (it is {:?}); log: {log:?}",
            session.state()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_run_starts_becomes_ready_and_stops() {
        let (server, port) = health_server();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counted = wakes.clone();
        let mut session = Session::new();
        session.start(
            &sleep_plan(Some(port), Some("/health"), None),
            "the model",
            move || {
                counted.fetch_add(1, Ordering::Relaxed);
            },
        );
        assert_eq!(session.state(), ServerState::Starting);
        let log = wait_for(&mut session, ServerState::Running, 15);
        assert!(log.iter().any(|l| l.starts_with("$ sleep 30")), "{log:?}");
        assert!(
            wakes.load(Ordering::Relaxed) > 0,
            "the wake callback never fired"
        );
        assert_eq!(session.port(), Some(port));
        assert_eq!(session.model_name(), Some("the model"));
        assert!(session.is_active());

        session.stop();
        assert_eq!(session.state(), ServerState::Stopping);
        let log = wait_for(&mut session, ServerState::Idle, 15);
        assert!(
            log.iter().any(|l| l.starts_with("Server stopped")),
            "{log:?}"
        );
        assert_eq!((session.port(), session.model_name()), (None, None));
        assert!(!session.is_active());
        server.unblock();
    }

    #[cfg(unix)]
    #[test]
    fn a_plan_with_no_ready_step_is_running_at_once() {
        let mut session = Session::new();
        session.start(&sleep_plan(None, None, None), "m", || {});
        wait_for(&mut session, ServerState::Running, 10);
        session.stop_blocking();
    }

    #[cfg(unix)]
    #[test]
    fn an_exit_nobody_asked_for_is_a_crash() {
        let mut plan = sleep_plan(Some(1), Some("/health"), None);
        let Start::Spawn(spawn) = &mut plan.units[0].start[0] else {
            panic!("a spawn plan")
        };
        spawn.program = traced("sh");
        spawn.args = vec![traced("-c"), traced("echo boom; exit 3")];
        let mut session = Session::new();
        session.start(&plan, "m", || {});
        let log = wait_for(&mut session, ServerState::Crashed, 15);
        assert!(log.iter().any(|l| l == "boom"), "{log:?}");
        assert!(
            log.iter().any(|l| l.contains("exited unexpectedly")),
            "{log:?}"
        );
        assert_eq!(session.port(), None, "a crash clears what the run was for");
    }

    #[test]
    fn a_program_that_cannot_start_is_a_crash() {
        let mut plan = sleep_plan(Some(1), None, None);
        let Start::Spawn(spawn) = &mut plan.units[0].start[0] else {
            panic!("a spawn plan")
        };
        spawn.program = traced("/nonexistent/quickai-test-program");
        let mut session = Session::new();
        session.start(&plan, "m", || {});
        let log = wait_for(&mut session, ServerState::Crashed, 10);
        assert!(
            log.iter()
                .any(|l| l.starts_with("Failed to launch server process")),
            "{log:?}"
        );
    }

    #[test]
    fn a_plan_with_nothing_to_start_is_reported() {
        let mut plan = sleep_plan(Some(1), None, None);
        plan.units[0].start.clear();
        let mut session = Session::new();
        session.start(&plan, "m", || {});
        let log = wait_for(&mut session, ServerState::Crashed, 5);
        assert!(log.iter().any(|l| l.contains("no way to start")), "{log:?}");
        let mut empty = sleep_plan(Some(1), None, None);
        empty.units.clear();
        let mut session = Session::new();
        session.start(&empty, "m", || {});
        let log = wait_for(&mut session, ServerState::Crashed, 5);
        assert!(
            log.iter().any(|l| l.contains("nothing to start")),
            "{log:?}"
        );
    }

    #[test]
    fn a_failure_before_the_start_is_reported_like_a_failed_start() {
        let mut session = Session::new();
        session.report_start_failure(8080, "m", "No internal port.".into(), || {});
        assert_eq!(session.port(), Some(8080));
        let log = wait_for(&mut session, ServerState::Crashed, 5);
        assert_eq!(log, vec!["No internal port."]);
        assert_eq!(session.port(), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_gateway_binds_the_public_port_and_stop_frees_it() {
        let (server, upstream) = health_server();
        // A port found by binding 0 can still be held when the proxy binds it (a child that another
        // test has just forked keeps the listener until it execs), and a start that cannot bind its
        // port crashes before anything is spawned: ask for another port.
        let (mut session, public) = (0..5)
            .find_map(|_| {
                let public = crate::runtime::process::ephemeral_port().unwrap();
                let gateway = Gateway {
                    public_port: public,
                    upstream_port: upstream,
                };
                let mut session = Session::new();
                session.start(
                    &sleep_plan(Some(upstream), Some("/health"), Some(gateway)),
                    "m",
                    || {},
                );
                // the run binds the port on its own thread: wait for it to settle either way
                let deadline = Instant::now() + Duration::from_secs(15);
                while !matches!(session.state(), ServerState::Running | ServerState::Crashed)
                    && Instant::now() < deadline
                {
                    session.poll(&mut |_| {}, &mut |_| {});
                    std::thread::sleep(Duration::from_millis(10));
                }
                (session.state() == ServerState::Running).then_some((session, public))
            })
            .expect("five free ports in a row were all taken");
        // reachable on the public port, which is the proxy
        let resp = ureq::get(format!("http://127.0.0.1:{public}/health")).call();
        assert!(resp.is_ok_and(|r| r.status() == 200));

        session.stop();
        wait_for(&mut session, ServerState::Idle, 15);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut freed = false;
        while Instant::now() < deadline {
            if std::net::TcpListener::bind(("127.0.0.1", public)).is_ok() {
                freed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(freed, "the public port was still bound after the run ended");
        server.unblock();
    }

    #[test]
    fn a_public_port_that_is_taken_is_reported_before_anything_starts() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let public = taken.local_addr().unwrap().port();
        let gateway = Gateway {
            public_port: public,
            upstream_port: 1,
        };
        let mut session = Session::new();
        session.start(
            &sleep_plan(Some(1), Some("/health"), Some(gateway)),
            "m",
            || {},
        );
        let log = wait_for(&mut session, ServerState::Crashed, 5);
        assert!(
            log.iter().any(|l| l.contains(&format!(
                "Could not start the live-traffic proxy on port {public}"
            ))),
            "{log:?}"
        );
        assert!(
            !log.iter().any(|l| l.starts_with("$ ")),
            "the server must not have been started: {log:?}"
        );
    }

    // ---- real-model tests, ignored by default ---------------------------------------------------------------

    /// Both real-model integration tests below load the actual Bonsai-8B
    /// model with full GPU offload; running them concurrently (the default
    /// with `cargo test`) would make two llama-server processes fight over
    /// the same GPU/VRAM. This forces them to run one at a time regardless
    /// of test-harness parallelism.
    static MODEL_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn real_run(
        port: u16,
        proxy: bool,
    ) -> Option<(quickai_core::plan::RunPlan, ModelEntry, PathBuf)> {
        let registry = embedded_registry();
        let binary = crate::launch::default_binary(&registry, "llamacpp");
        let model_path = crate::paths::bundled_models_dir().join("Bonsai-8B-Q1_0.gguf");
        assert!(binary.is_file(), "llama-server not built at {binary:?}");
        assert!(model_path.is_file(), "test model missing at {model_path:?}");
        let model = ModelEntry {
            filename: "Bonsai-8B-Q1_0.gguf".into(),
            path: model_path,
            display: "bonsai-8b-test".into(),
            note: String::new(),
            bundled: true,
            size_bytes: 0,
            companions: Vec::new(),
            mode: None,
        };
        let settings = RunSettings {
            port,
            ctx_size: 2048,
            n_gpu_layers: 999,
            extra_args: String::new(),
            bin_override: None,
            traffic_proxy: proxy,
        };
        let internal = proxy.then(|| crate::runtime::process::ephemeral_port().unwrap());
        let plan = launch::plan_run(
            &registry,
            &model,
            &settings,
            &binary,
            launch::MachineFacts::default(),
            internal,
        )
        .ok()?;
        Some((plan, model, binary))
    }

    /// Full start -> healthy -> stop -> idle cycle against the real
    /// Bonsai-8B model and the real llama-server binary (dev-tree paths),
    /// matching the manual validation done for the Python prototype.
    ///
    /// Ignored by default: a fresh clone (and CI) has neither the binary nor
    /// the model, since `llama.cpp/` and `models/` are gitignored. Run it with
    /// `cargo test -p quickai -- --ignored` once both are in place.
    #[test]
    #[ignore = "needs llama.cpp/build/bin/llama-server and models/Bonsai-8B-Q1_0.gguf"]
    fn start_running_stop_cycle() {
        let _guard = MODEL_TEST_LOCK.lock().unwrap();
        let (plan, model, _) = real_run(8199, false).expect("a plan");

        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_clone = wake_count.clone();

        let mut session = Session::new();
        session.start(&plan, &model.display, move || {
            wake_count_clone.fetch_add(1, Ordering::Relaxed);
        });

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut saw_running = false;
        while Instant::now() < deadline {
            session.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if session.state() == ServerState::Running {
                saw_running = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(saw_running, "server never reached Running within 30s");
        assert!(
            wake_count.load(Ordering::Relaxed) > 0,
            "wake callback never fired"
        );

        // Confirm it's really serving, not just self-reporting.
        let resp = ureq::get("http://127.0.0.1:8199/health").call();
        assert!(resp.is_ok_and(|r| r.status() == 200));

        session.stop();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut saw_idle = false;
        while Instant::now() < deadline {
            session.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if session.state() == ServerState::Idle {
                saw_idle = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            saw_idle,
            "server never returned to Idle after stop() within 10s"
        );
    }

    /// With the live-traffic proxy on, llama-server binds an internal port
    /// while the proxy itself binds the public one — confirms the model is
    /// still reachable on the public port (through the proxy) and, crucially,
    /// that stopping actually releases the public port rather than leaking
    /// the proxy's listener.
    ///
    /// Ignored by default, for the same reason as `start_running_stop_cycle`.
    #[test]
    #[ignore = "needs llama.cpp/build/bin/llama-server and models/Bonsai-8B-Q1_0.gguf"]
    fn start_stop_with_proxy_frees_the_public_port() {
        let _guard = MODEL_TEST_LOCK.lock().unwrap();
        const PUBLIC_PORT: u16 = 8198;
        let (plan, model, _) = real_run(PUBLIC_PORT, true).expect("a plan");

        let mut session = Session::new();
        session.start(&plan, &model.display, || {});

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut saw_running = false;
        while Instant::now() < deadline {
            session.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if session.state() == ServerState::Running {
                saw_running = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(saw_running, "server never reached Running within 30s");

        // Reachable on the *public* port, i.e. actually through the proxy.
        let resp = ureq::get(format!("http://127.0.0.1:{PUBLIC_PORT}/health")).call();
        assert!(resp.is_ok_and(|r| r.status() == 200));

        session.stop();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut saw_idle = false;
        while Instant::now() < deadline {
            session.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if session.state() == ServerState::Idle {
                saw_idle = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            saw_idle,
            "server never returned to Idle after stop() within 10s"
        );

        // The proxy's listener must be gone by now, or this bind fails.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut freed = false;
        while Instant::now() < deadline {
            if std::net::TcpListener::bind(("127.0.0.1", PUBLIC_PORT)).is_ok() {
                freed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(freed, "public port was still bound after stop() settled");
    }
}
