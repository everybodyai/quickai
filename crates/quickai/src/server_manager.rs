//! Owns a single `llama-server` child process: start it, watch it come up
//! via `/health` polling, stream its log, and stop it. One manager = one
//! model running at a time, matching the GUI's single start/stop control.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::proxy::{self, TrafficEvent};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServerState {
    Idle,
    Starting,
    Running,
    Stopping,
    Crashed,
}

impl ServerState {
    pub fn label(&self) -> &'static str {
        match self {
            ServerState::Idle => "Stopped",
            ServerState::Starting => "Starting",
            ServerState::Running => "Running",
            ServerState::Stopping => "Stopping",
            ServerState::Crashed => "Crashed",
        }
    }
}

pub enum Event {
    Log(String),
    StateChanged(ServerState),
    Traffic(TrafficEvent),
}

const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);
const WAITER_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub struct ServerManager {
    child: Arc<Mutex<Option<Child>>>,
    intentional_stop: Arc<AtomicBool>,
    evt_tx: Sender<Event>,
    pub evt_rx: Receiver<Event>,
    state: ServerState,
    port: Option<u16>,
    model_name: Option<String>,
    /// Only `Some` while "Show live conversation traffic" was on for the
    /// current run — see `proxy.rs`.
    proxy: Option<proxy::ProxyHandle>,
}

impl ServerManager {
    pub fn new() -> Self {
        let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
        Self {
            child: Arc::new(Mutex::new(None)),
            intentional_stop: Arc::new(AtomicBool::new(false)),
            evt_tx,
            evt_rx,
            state: ServerState::Idle,
            port: None,
            model_name: None,
            proxy: None,
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
                Event::StateChanged(s) => {
                    self.state = s;
                    if matches!(s, ServerState::Idle | ServerState::Crashed) {
                        self.port = None;
                        self.model_name = None;
                        // Free the public port so a later Start (proxy on
                        // or off, on any port) can bind cleanly.
                        if let Some(proxy) = self.proxy.take() {
                            proxy.stop();
                        }
                    }
                }
                Event::Traffic(t) => traffic(t),
            }
        }
        changed
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        binary_path: &Path,
        model_path: &Path,
        model_name: &str,
        port: u16,
        ctx_size: u32,
        n_gpu_layers: u32,
        extra_args: &str,
        enable_proxy: bool,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        if self.is_active() {
            return;
        }
        let wake = Arc::new(wake);
        self.port = Some(port);
        self.model_name = Some(model_name.to_string());
        self.intentional_stop.store(false, Ordering::SeqCst);

        // With the proxy on, llama-server itself binds an internal-only
        // port that only the proxy talks to; the proxy binds `port` (what
        // the GUI/clients actually connect to). With it off, llama-server
        // binds `port` directly, exactly as before this feature existed.
        let llama_port = if enable_proxy {
            match ephemeral_port() {
                Ok(p) => p,
                Err(e) => {
                    send(
                        &self.evt_tx,
                        Event::Log(format!(
                            "Could not allocate an internal port for the live-traffic proxy: {e}"
                        )),
                        &wake,
                    );
                    send(
                        &self.evt_tx,
                        Event::StateChanged(ServerState::Crashed),
                        &wake,
                    );
                    return;
                }
            }
        } else {
            port
        };
        if enable_proxy {
            match proxy::spawn(port, llama_port, self.evt_tx.clone(), wake.clone()) {
                Ok(handle) => self.proxy = Some(handle),
                Err(e) => {
                    send(
                        &self.evt_tx,
                        Event::Log(format!("Could not start the live-traffic proxy on port {port}: {e}. Is something else already using that port?")),
                        &wake,
                    );
                    send(
                        &self.evt_tx,
                        Event::StateChanged(ServerState::Crashed),
                        &wake,
                    );
                    return;
                }
            }
        }
        self.state = ServerState::Starting;

        let mut args: Vec<String> = vec![
            "--model".into(),
            model_path.display().to_string(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            llama_port.to_string(),
            "--ctx-size".into(),
            ctx_size.to_string(),
            "--n-gpu-layers".into(),
            n_gpu_layers.to_string(),
        ];
        args.extend(parse_extra_args(extra_args));

        send(
            &self.evt_tx,
            Event::Log(format!("$ {} {}", binary_path.display(), args.join(" "))),
            &wake,
        );

        let mut cmd = Command::new(binary_path);
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // llama-server's RUNPATH is hardcoded to this repo's build dir at
        // dev time; in a packaged build the .so deps sit right beside the
        // binary instead, so point the loader there explicitly.
        if let Some(dir) = binary_path.parent() {
            let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
            let combined = if existing.is_empty() {
                dir.display().to_string()
            } else {
                format!("{}:{}", dir.display(), existing)
            };
            cmd.env("LD_LIBRARY_PATH", combined);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                send(
                    &self.evt_tx,
                    Event::Log(format!("Failed to launch server process: {e}")),
                    &wake,
                );
                send(
                    &self.evt_tx,
                    Event::StateChanged(ServerState::Crashed),
                    &wake,
                );
                if let Some(proxy) = self.proxy.take() {
                    proxy.stop();
                }
                return;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        spawn_reader(stdout, self.evt_tx.clone(), wake.clone());
        spawn_reader(stderr, self.evt_tx.clone(), wake.clone());

        *self.child.lock().unwrap() = Some(child);

        spawn_waiter(
            self.child.clone(),
            self.intentional_stop.clone(),
            self.evt_tx.clone(),
            wake.clone(),
        );
        spawn_health_poller(
            llama_port,
            self.intentional_stop.clone(),
            self.evt_tx.clone(),
            wake,
        );
    }

    pub fn stop(&mut self) {
        if !self.is_active() {
            return;
        }
        self.intentional_stop.store(true, Ordering::SeqCst);
        self.state = ServerState::Stopping;
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
        // Stop accepting new connections right away rather than waiting for
        // the child-exit event to reach `poll()` — that still runs as a
        // safety net (e.g. if llama-server crashes on its own).
        if let Some(proxy) = self.proxy.take() {
            proxy.stop();
        }
    }

    /// Best-effort synchronous stop, for use on app exit.
    pub fn stop_blocking(&mut self) {
        self.stop();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.wait();
        }
    }
}

/// Minimal shell-like split for the user-supplied "extra arguments" field:
/// splits on whitespace, except text wrapped in matching `"..."`/`'...'` is
/// kept as one argument (so a path containing spaces can be passed through).
/// No escape-sequence support — good enough for llama-server flags/values,
/// not a general shell parser.
fn parse_extra_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;
    for c in input.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_token = true;
            }
            None if c.is_whitespace() => {
                if in_token {
                    args.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            None => {
                current.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        args.push(current);
    }
    args
}

/// Asks the OS for a free loopback port by briefly binding to port 0, then
/// releasing it so llama-server can bind it moments later. There's a small
/// theoretical race if something else grabs the exact same port in between,
/// same as any "find a free port" trick — if it happens, llama-server just
/// fails to bind and that surfaces in the log pane like any other failure.
fn ephemeral_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr().map(|a| a.port())
}

fn send(tx: &Sender<Event>, event: Event, wake: &Arc<impl Fn() + Send + Sync + 'static>) {
    let _ = tx.send(event);
    wake();
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    evt_tx: Sender<Event>,
    wake: Arc<impl Fn() + Send + Sync + 'static>,
) {
    let Some(pipe) = pipe else { return };
    std::thread::spawn(move || {
        let reader = BufReader::new(pipe);
        for line in reader.lines() {
            match line {
                Ok(line) => send(&evt_tx, Event::Log(line), &wake),
                Err(_) => break,
            }
        }
    });
}

fn spawn_waiter(
    child: Arc<Mutex<Option<Child>>>,
    intentional_stop: Arc<AtomicBool>,
    evt_tx: Sender<Event>,
    wake: Arc<impl Fn() + Send + Sync + 'static>,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(WAITER_POLL_INTERVAL);
        let mut guard = child.lock().unwrap();
        let Some(c) = guard.as_mut() else { return };
        match c.try_wait() {
            Ok(Some(status)) => {
                *guard = None;
                drop(guard);
                let was_intentional = intentional_stop.load(Ordering::SeqCst);
                if was_intentional {
                    send(
                        &evt_tx,
                        Event::Log(format!("Server stopped (exit status: {status}).")),
                        &wake,
                    );
                    send(&evt_tx, Event::StateChanged(ServerState::Idle), &wake);
                } else {
                    send(
                        &evt_tx,
                        Event::Log(format!(
                            "Server process exited unexpectedly (exit status: {status})."
                        )),
                        &wake,
                    );
                    send(&evt_tx, Event::StateChanged(ServerState::Crashed), &wake);
                }
                return;
            }
            Ok(None) => continue, // still running
            Err(_) => return,
        }
    });
}

fn spawn_health_poller(
    port: u16,
    intentional_stop: Arc<AtomicBool>,
    evt_tx: Sender<Event>,
    wake: Arc<impl Fn() + Send + Sync + 'static>,
) {
    std::thread::spawn(move || {
        let url = format!("http://127.0.0.1:{port}/health");
        let deadline = std::time::Instant::now() + HEALTH_TIMEOUT;
        loop {
            // A Stop while still starting means "Running" would be a stale,
            // misleading transition — bail without reporting either way.
            if intentional_stop.load(Ordering::SeqCst) {
                return;
            }
            if std::time::Instant::now() > deadline {
                send(
                    &evt_tx,
                    Event::Log("Timed out waiting for server to become healthy.".into()),
                    &wake,
                );
                return;
            }
            if let Ok(resp) = ureq::get(&url).call() {
                if resp.status() == 200 {
                    send(&evt_tx, Event::StateChanged(ServerState::Running), &wake);
                    return;
                }
            }
            std::thread::sleep(HEALTH_POLL_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Both real-model integration tests below load the actual Bonsai-8B
    /// model with full GPU offload; running them concurrently (the default
    /// with `cargo test`) would make two llama-server processes fight over
    /// the same GPU/VRAM. This forces them to run one at a time regardless
    /// of test-harness parallelism.
    static MODEL_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parse_extra_args_splits_on_whitespace() {
        assert_eq!(parse_extra_args(""), Vec::<String>::new());
        assert_eq!(
            parse_extra_args("--threads 8   --flash-attn on"),
            vec!["--threads", "8", "--flash-attn", "on"]
        );
    }

    #[test]
    fn parse_extra_args_keeps_quoted_spaces_together() {
        assert_eq!(
            parse_extra_args(r#"--lora "/path/with spaces/adapter.gguf" --threads 4"#),
            vec!["--lora", "/path/with spaces/adapter.gguf", "--threads", "4"]
        );
        assert_eq!(parse_extra_args("'single quoted'"), vec!["single quoted"]);
    }

    /// Full start -> healthy -> stop -> idle cycle against the real
    /// Bonsai-8B model and the real llama-server binary (dev-tree paths),
    /// matching the manual validation done for the Python prototype.
    #[test]
    fn start_running_stop_cycle() {
        let _guard = MODEL_TEST_LOCK.lock().unwrap();
        let binary = crate::paths::llama_server_bin();
        let model = crate::paths::bundled_models_dir().join("Bonsai-8B-Q1_0.gguf");
        assert!(binary.is_file(), "llama-server not built at {binary:?}");
        assert!(model.is_file(), "test model missing at {model:?}");

        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_clone = wake_count.clone();

        let mut mgr = ServerManager::new();
        mgr.start(
            &binary,
            &model,
            "bonsai-8b-test",
            8199,
            2048,
            999,
            "",
            false,
            move || {
                wake_count_clone.fetch_add(1, Ordering::Relaxed);
            },
        );

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut saw_running = false;
        while std::time::Instant::now() < deadline {
            mgr.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if mgr.state() == ServerState::Running {
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

        mgr.stop();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut saw_idle = false;
        while std::time::Instant::now() < deadline {
            mgr.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if mgr.state() == ServerState::Idle {
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
    #[test]
    fn start_stop_with_proxy_frees_the_public_port() {
        let _guard = MODEL_TEST_LOCK.lock().unwrap();
        let binary = crate::paths::llama_server_bin();
        let model = crate::paths::bundled_models_dir().join("Bonsai-8B-Q1_0.gguf");
        assert!(binary.is_file(), "llama-server not built at {binary:?}");
        assert!(model.is_file(), "test model missing at {model:?}");

        const PUBLIC_PORT: u16 = 8198;
        let mut mgr = ServerManager::new();
        mgr.start(
            &binary,
            &model,
            "bonsai-8b-test-proxy",
            PUBLIC_PORT,
            2048,
            999,
            "",
            true,
            || {},
        );

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut saw_running = false;
        while std::time::Instant::now() < deadline {
            mgr.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if mgr.state() == ServerState::Running {
                saw_running = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(saw_running, "server never reached Running within 30s");

        // Reachable on the *public* port, i.e. actually through the proxy.
        let resp = ureq::get(format!("http://127.0.0.1:{PUBLIC_PORT}/health")).call();
        assert!(resp.is_ok_and(|r| r.status() == 200));

        mgr.stop();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut saw_idle = false;
        while std::time::Instant::now() < deadline {
            mgr.poll(&mut |line| println!("LOG: {line}"), &mut |_| {});
            if mgr.state() == ServerState::Idle {
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
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut freed = false;
        while std::time::Instant::now() < deadline {
            if std::net::TcpListener::bind(("127.0.0.1", PUBLIC_PORT)).is_ok() {
                freed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(freed, "public port was still bound after stop() settled");
    }
}
