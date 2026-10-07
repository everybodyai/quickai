//! Groups S and K of the test plan: a session that attaches to an Ollama that is already running or
//! starts one, loads a model, serves, unloads and stops, and leaves no process behind (work
//! packages 3.4 and 3.5).
//!
//! Everything runs through a document, as the application does: a test engine document
//! (`DOC`, the sections of `docs/examples/engines/ollama.toml` with timings in tens of
//! milliseconds), a plan made from it by the planner, and `Session::start`. The "daemon" is a
//! shell script that records how it was started and keeps a child of its own, as `ollama serve`
//! keeps its runner; its HTTP side is the fake of `testkit`, bound only after the script has
//! started, since an Ollama that answers before it exists would be attached to, not started.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::plan::{ModelInfo, RunPlan, Selection};
use quickai_core::registry::Registry;

use crate::events::ServerState;
use crate::launch::{self, MachineFacts};
use crate::runtime::session::Session;
use crate::testkit::fake_ollama::{FakeOllama, Identity};
use crate::testkit::procs::{gone_within, kill_quietly, pid_alive, read_pid, write_script};
use crate::testkit::{free_port, scratch, wait_until};

/// The sections of the Ollama example, with test-sized timings. `strategy_field` is how the
/// `mode` field chooses among the strategies (plan decision C18).
const DOC: &str = r#"schema = 1
kind = "engine"
id = "ollama_test"
requires = { primitives = ["spawn@1", "probe.http@1", "attach@1", "http_call@1"] }

[engine]
name = "Ollama (test)"
server_name = "Ollama"

[provides]
apis = ["openai.chat"]
default_port = 11434

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["attach_if_running", "spawn"]
strategy_field = "mode"

[launch.attach]
# an engine that is already running is on the port the settings give, not on the internal one a gateway would use
endpoint = "127.0.0.1:${public_port}"
identity = { path = "/api/version", pointer = "/version" }
liveness = { path = "/api/version", every = "100ms", failures = 2 }

[launch.spawn]
program = "${runtime.binary}"
args = ["serve"]
env = [
  { name = "OLLAMA_HOST", value = "127.0.0.1:${listen_port}", locked = true },
  { name = "OLLAMA_MODELS", value = "${field.models_dir}", when = "not field.use_default_store" },
  { name = "OLLAMA_KEEP_ALIVE", value = "-1" },
  { name = "OLLAMA_MAX_LOADED_MODELS", value = "1" },
  { name = "OLLAMA_NO_CLOUD", value = "1", locked = true },
]
group = true
stop = { policy = "graceful", grace = "GRACE", exit_grace = "EXIT_GRACE" }

[launch.ready]
probe = { use = "http", path = "/api/version", expect = 200 }
timeout = "READY_TIMEOUT"
interval = "25ms"

[[launch.warm]]
when = "model"
call = { method = "POST", path = "/api/generate", body = { model = "${model.id}", keep_alive = -1 }, timeout = "WARM_TIMEOUT" }

[[launch.unload]]
when = "model"
call = { method = "POST", path = "/api/generate", body = { model = "${model.id}", keep_alive = 0 }, timeout = "UNLOAD_TIMEOUT", best_effort = true }

[[field]]
id = "mode"
type = "choice"
options = ["auto", "attach", "spawn"]
default = "auto"

[[field]]
id = "port"
type = "port"
default = 11434
bind.port = "public"

[[field]]
id = "models_dir"
type = "dir"
default = "/quickai-test/models"

[[field]]
id = "use_default_store"
type = "bool"
default = true

[[field]]
id = "extra_env"
type = "kv_lines"
default = ""
bind.env_lines = { protect = ["OLLAMA_HOST", "OLLAMA_MODELS", "OLLAMA_NO_CLOUD"] }

[[field]]
id = "bin_override"
type = "runtime_path"
"#;

/// What the hygiene tests (group K) need and nothing more: a program started in its own process
/// group, ready when it answers, stopped with a grace period. It does not need attaching or
/// loading a model, so work package 3.5 can land without 3.4.
const DOC_KERNEL: &str = r#"schema = 1
kind = "engine"
id = "ollama_test"
requires = { primitives = ["spawn@1", "probe.http@1"] }

[engine]
name = "Ollama (test)"

[provides]
apis = ["openai.chat"]
default_port = 11434

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["serve"]
env = [
  { name = "OLLAMA_HOST", value = "127.0.0.1:${listen_port}", locked = true },
  { name = "OLLAMA_API_KEY", value = "hunter2" },
]
group = true
stop = { policy = "graceful", grace = "GRACE", exit_grace = "EXIT_GRACE" }

[launch.ready]
probe = { use = "http", path = "/api/version", expect = 200 }
timeout = "READY_TIMEOUT"
interval = "25ms"

[[field]]
id = "port"
type = "port"
default = 11434
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"
"#;

/// A stand-in for `ollama serve` that records how it was started, has a runner child, and stops on
/// SIGTERM (after 0.4 s if a file called `slow-term` is beside it, to tell a stop the run waited for
/// from one the kernel did after the run's thread had gone) unless a file called `ignore-term` is beside it (then neither it nor its child does:
/// the trap is set before the child starts, because an ignored signal is inherited and a handler
/// is not, and a group-wide SIGTERM would otherwise end the child and, with it, the loop).
const SCRIPT: &str = r#"d=$(dirname "$0")
printf '%s\n' "$@" > "$d/args.record"
env > "$d/env.record"
if [ -f "$d/ignore-term" ]; then
    trap '' TERM
else
    trap 'echo term > "$d/term.record"; [ -f "$d/slow-term" ] && sleep 0.4; kill $runner 2>/dev/null; exit 0' TERM
fi
sleep 300 &
runner=$!
echo $$ > "$d/server.pid"
echo $runner > "$d/runner.pid"
: > "$d/started"
while kill -0 $runner 2>/dev/null; do wait $runner; done
"#;

struct World {
    dir: PathBuf,
    script: PathBuf,
    port: u16,
    registry: Registry,
    /// False in the child process of K05, which borrows its parent's directory.
    owner: bool,
    /// Built from `DOC_KERNEL`: no mode field, nothing to attach to, no model to load.
    kernel: bool,
}

struct Timings {
    grace: &'static str,
    exit_grace: &'static str,
    ready: &'static str,
    warm: &'static str,
    unload: &'static str,
}

const FAST: Timings = Timings {
    grace: "300ms",
    exit_grace: "300ms",
    ready: "3s",
    warm: "2s",
    unload: "1s",
};

impl World {
    fn new() -> World {
        Self::with(&FAST)
    }

    fn registry(t: &Timings, kernel: bool, kill_only: bool) -> Registry {
        let base = if kernel { DOC_KERNEL } else { DOC };
        let base = if kill_only {
            // what this build can do today: one process, killed at once
            base.replace(
                r#"stop = { policy = "graceful", grace = "GRACE", exit_grace = "EXIT_GRACE" }"#,
                r#"stop = { policy = "kill" }"#,
            )
        } else {
            base.to_string()
        };
        let doc = base
            .replace("EXIT_GRACE", t.exit_grace)
            .replace("GRACE", t.grace)
            .replace("READY_TIMEOUT", t.ready)
            .replace("WARM_TIMEOUT", t.warm)
            .replace("UNLOAD_TIMEOUT", t.unload);
        let source = MemorySource::new(&[("embedded:engines/ollama_test.toml", doc.as_str())]);
        let registry = Registry::build(
            load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
            &crate::prims::known(),
        );
        assert!(
            registry.diagnostics.is_empty() && registry.engine("ollama_test").is_some(),
            "the test engine document was not accepted by this build:\n{:#?}",
            registry.diagnostics
        );
        registry
    }

    fn with(t: &Timings) -> World {
        Self::build(t, false)
    }

    /// For the hygiene tests: see `DOC_KERNEL`.
    fn kernel() -> World {
        Self::build(&FAST, true)
    }

    /// The hygiene document with the stop policy this build already has, to prove the harness
    /// itself (the script, the fake, the polling) before the new behaviour is asked of it.
    fn kernel_today() -> World {
        Self::build_with(&FAST, true, true)
    }

    fn build(t: &Timings, kernel: bool) -> World {
        Self::build_with(t, kernel, false)
    }

    fn build_with(t: &Timings, kernel: bool, kill_only: bool) -> World {
        // the document first: a test that panics here has made no directory to leave behind
        let registry = Self::registry(t, kernel, kill_only);
        // the world removes its directory itself, after it has killed what is still running in it
        let dir = scratch("session").into_path();
        let script = write_script(&dir, "ollama", SCRIPT);
        World {
            dir,
            script,
            port: free_port(),
            registry,
            owner: true,
            kernel,
        }
    }

    /// The directory and port of a parent process (K05's child).
    fn adopt(dir: PathBuf, port: u16) -> World {
        World {
            script: dir.join("ollama"),
            dir,
            port,
            registry: Self::registry(&FAST, true, false),
            owner: false,
            kernel: true,
        }
    }

    /// The directory and port of a parent process, with the full document (S23's child).
    fn adopt_full(dir: PathBuf, port: u16) -> World {
        World {
            script: dir.join("ollama"),
            dir,
            port,
            registry: Self::registry(&FAST, false, false),
            owner: false,
            kernel: false,
        }
    }

    fn selection(&self, mode: &str, model: Option<&str>) -> Selection {
        let mut s = Selection::new("ollama_test").set("ollama_test.port", self.port);
        if !self.kernel {
            s = s.set("ollama_test.mode", mode);
        }
        if let Some(name) = model {
            s = s.with_model(model_info(name));
        }
        s
    }

    fn plan_for(&self, selection: &Selection, internal_port: Option<u16>) -> RunPlan {
        launch::plan_selection(
            &self.registry,
            selection,
            &self.script,
            MachineFacts::default(),
            internal_port,
        )
        .unwrap_or_else(|e| panic!("the planner refused the selection: {e:#?}"))
    }

    fn plan(&self, mode: &str, model: Option<&str>) -> RunPlan {
        self.plan_for(&self.selection(mode, model), None)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn script_ran(&self) -> bool {
        self.file("started").exists()
    }

    fn pid(&self, which: &str) -> u32 {
        read_pid(&self.file(&format!("{which}.pid")), Duration::from_secs(5))
    }

    fn env_record(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(self.file("env.record"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// The daemon's HTTP side, bound once the script has started (on the planned port or another
    /// one, as given).
    fn fake_after_start(&self, port: u16) -> FakeOllama {
        wait_until(Duration::from_secs(5), "the daemon script to start", || {
            self.script_ran()
        });
        bind_fake(port)
    }

    /// An Ollama that is already answering before anything starts.
    fn fake_running(&self) -> FakeOllama {
        bind_fake(self.port)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if !self.owner {
            return;
        }
        for which in ["server", "runner"] {
            if let Ok(text) = std::fs::read_to_string(self.file(&format!("{which}.pid"))) {
                if let Ok(pid) = text.trim().parse() {
                    kill_quietly(pid);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn model_info(name: &str) -> ModelInfo {
    ModelInfo {
        id: name.to_string(),
        display: name.to_string(),
        path: String::new(),
        size_bytes: 2_500_000_000,
    }
}

/// Binds a fake on `port`, trying again for a moment: the port was free a moment ago, and a
/// listener that has just been closed can take a few milliseconds to let go.
fn bind_fake(port: u16) -> FakeOllama {
    for _ in 0..80 {
        if let Ok(fake) = FakeOllama::start_on(Some(port)) {
            return fake;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("could not bind a fake Ollama on port {port}");
}

/// Polls the session until it is in `want`, collecting the log; panics with the log when it never
/// gets there.
fn settle(session: &mut Session, want: ServerState, seconds: u64, log: &mut Vec<String>) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        session.poll(&mut |line| log.push(line), &mut |_| {});
        if session.state() == want {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    panic!(
        "the session never reached {want:?} (it is {:?}); log:\n{}",
        session.state(),
        log.join("\n")
    );
}

fn start(session: &mut Session, plan: &RunPlan, name: &str) {
    session.start(plan, name, || {});
}

fn warm_bodies(fake: &FakeOllama) -> Vec<serde_json::Value> {
    fake.requests_to("/api/generate")
        .into_iter()
        .filter_map(|r| r.body)
        .collect()
}

// ---- attach and start ------------------------------------------------------------------------------

#[test]
fn p3_s01_an_ollama_that_already_answers_is_attached_to_and_nothing_is_started() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 2_500_000_000);
    let plan = world.plan("auto", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    settle(&mut session, ServerState::Running, 10, &mut log);

    assert!(
        !world.script_ran(),
        "a daemon was started although one answered"
    );
    assert!(
        !log.iter().any(|l| l.starts_with("$ ")),
        "a command was logged: {log:?}"
    );
    assert!(
        log.iter()
            .any(|l| l.to_lowercase().contains("attached") && l.contains("0.35.1")),
        "the log does not say which Ollama it attached to: {log:?}"
    );
    assert_eq!(session.port(), Some(world.port));
    assert_eq!(session.model_name(), Some("qwen3:4b"));
    // it asked who is there before it trusted the answer, then loaded the model: no `num_ctx`
    // (a per-request context does not stick, A7), and for ever
    assert!(!fake.requests_to("/api/version").is_empty());
    assert_eq!(
        warm_bodies(&fake),
        [serde_json::json!({"model": "qwen3:4b", "keep_alive": -1})]
    );
    session.stop_blocking();
}

#[test]
fn p3_s02_with_nothing_on_the_port_the_daemon_is_started_as_the_document_says() {
    let world = World::new();
    let plan = world.plan("auto", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    let fake = world.fake_after_start(world.port);
    fake.add_model("qwen3:4b", 2_500_000_000);
    settle(&mut session, ServerState::Running, 10, &mut log);

    assert_eq!(log[0], format!("$ {} serve", world.script.display()));
    let args = std::fs::read_to_string(world.file("args.record")).unwrap();
    assert_eq!(args.trim(), "serve");
    let env = world.env_record();
    assert_eq!(
        env.get("OLLAMA_HOST"),
        Some(&format!("127.0.0.1:{}", world.port))
    );
    assert_eq!(env.get("OLLAMA_NO_CLOUD").map(String::as_str), Some("1"));
    assert_eq!(env.get("OLLAMA_KEEP_ALIVE").map(String::as_str), Some("-1"));
    assert_eq!(
        env.get("OLLAMA_MAX_LOADED_MODELS").map(String::as_str),
        Some("1")
    );
    assert_eq!(warm_bodies(&fake).len(), 1);
    session.stop_blocking();
}

#[test]
fn p3_s03_attach_mode_with_nothing_running_fails_and_starts_nothing() {
    let world = World::new();
    let plan = world.plan("attach", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(!world.script_ran());
    assert!(
        log.iter()
            .any(|l| l.contains(&world.port.to_string()) && l.to_lowercase().contains("no ollama")),
        "the log does not say that no Ollama answers on port {}: {log:?}",
        world.port
    );
}

#[test]
fn p3_s04_spawn_mode_refuses_a_port_where_an_ollama_already_answers() {
    let world = World::new();
    let fake = world.fake_running();
    let plan = world.plan("spawn", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(
        !world.script_ran(),
        "a second daemon was started on a port in use"
    );
    assert!(
        log.iter().any(|l| l.to_lowercase().contains("already")),
        "the log does not explain: {log:?}"
    );
    assert!(
        warm_bodies(&fake).is_empty(),
        "the foreign daemon was asked to load a model"
    );
}

#[test]
fn p3_s05_a_server_on_the_port_that_is_not_ollama_is_never_attached_to() {
    for identity in [Identity::LlamaServer, Identity::WebServer] {
        let world = World::new();
        let fake = world.fake_running();
        fake.behave(|b| b.identity = identity);
        let plan = world.plan("auto", Some("qwen3:4b"));
        let (mut session, mut log) = (Session::new(), Vec::new());
        start(&mut session, &plan, "qwen3:4b");
        settle(&mut session, ServerState::Crashed, 10, &mut log);
        assert!(
            !world.script_ran(),
            "{identity:?}: a daemon was started on a port in use"
        );
        assert!(
            log.iter().any(|l| l.contains(&world.port.to_string())
                && l.to_lowercase().contains("not ollama")
                || l.to_lowercase().contains("something else")),
            "{identity:?}: the log does not say another program holds the port: {log:?}"
        );
        assert!(
            fake.requests_to("/api/generate").is_empty(),
            "{identity:?}: a model was requested from a server that is not Ollama"
        );
    }
}

#[test]
fn p3_s06_a_daemon_that_never_answers_is_given_up_on_and_stopped_with_its_runner() {
    let world = World::with(&Timings {
        ready: "600ms",
        ..FAST
    });
    let plan = world.plan("spawn", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(
        log.iter().any(|l| l.to_lowercase().contains("timed out")),
        "the log does not say the wait timed out: {log:?}"
    );
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    assert!(
        gone_within(server, Duration::from_secs(3)),
        "the daemon was left running"
    );
    assert!(
        gone_within(runner, Duration::from_secs(3)),
        "its runner was left running"
    );
}

// ---- loading the model ---------------------------------------------------------------------------

#[test]
fn p3_s07_the_session_is_not_running_until_the_model_is_loaded() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 2_500_000_000);
    fake.behave(|b| b.load_delay = Duration::from_millis(500));
    let plan = world.plan("auto", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    wait_until(Duration::from_secs(5), "the load request", || {
        !fake.requests_to("/api/generate").is_empty()
    });
    session.poll(&mut |l| log.push(l), &mut |_| {});
    assert_eq!(
        session.state(),
        ServerState::Starting,
        "running before the model had loaded"
    );
    settle(&mut session, ServerState::Running, 10, &mut log);
    session.stop_blocking();
}

#[test]
fn p3_s08_a_model_that_is_not_there_crashes_the_start_and_a_started_daemon_is_stopped() {
    // spawned: everything QuickAI started is stopped
    let world = World::new();
    std::fs::write(world.file("slow-term"), b"").unwrap();
    let plan = world.plan("spawn", Some("ghost:1b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "ghost:1b");
    let _fake = world.fake_after_start(world.port);
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(
        log.iter().any(|l| l.contains("model 'ghost:1b' not found")),
        "the engine's own words are not in the log: {log:?}"
    );
    // Crashed is said only once what was started has been stopped: the run did it itself, and did
    // not leave it to the parent-death signal that follows the end of its thread
    // (the daemon takes 0.4 s to leave once asked, so a run that only left it to the kernel would
    // say Crashed with it still there)
    assert!(
        !pid_alive(world.pid("server")),
        "the state said Crashed while the daemon was still running"
    );
    assert!(gone_within(world.pid("runner"), Duration::from_secs(3)));
    assert!(
        world.file("term.record").exists(),
        "the daemon was not asked to stop by the run"
    );

    // attached: the daemon is not ours and stays as it was
    let world = World::new();
    let fake = world.fake_running();
    let plan = world.plan("attach", Some("ghost:1b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "ghost:1b");
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(crate::testkit::http::try_get(&format!("{}/api/version", fake.base())).is_ok());
}

#[test]
fn p3_s09_a_model_that_does_not_fit_is_reported_with_the_engines_sentence() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("big:70b", 40_000_000_000);
    fake.behave(|b| {
        b.load_failure = Some((
            500,
            "model requires more system memory (41.2 GiB) than is available (5.1 GiB)".into(),
        ))
    });
    let plan = world.plan("attach", Some("big:70b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "big:70b");
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(
        log.iter()
            .any(|l| l.contains("requires more system memory (41.2 GiB)")),
        "{log:?}"
    );
}

#[test]
fn p3_s10_a_load_that_takes_too_long_is_a_crash_that_says_so() {
    let world = World::with(&Timings {
        warm: "500ms",
        ..FAST
    });
    let fake = world.fake_running();
    fake.add_model("slow:7b", 1);
    fake.behave(|b| b.load_delay = Duration::from_secs(4));
    let plan = world.plan("attach", Some("slow:7b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    let started = Instant::now();
    start(&mut session, &plan, "slow:7b");
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the timeout was not honoured"
    );
    assert!(
        log.iter()
            .any(|l| l.to_lowercase().contains("longer") || l.to_lowercase().contains("timed out")),
        "{log:?}"
    );
}

// ---- management mode -----------------------------------------------------------------------------

#[test]
fn p3_s11_with_no_model_the_daemon_runs_with_nothing_loaded() {
    let world = World::new();
    let plan = world.plan("spawn", None);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "");
    let fake = world.fake_after_start(world.port);
    settle(&mut session, ServerState::Running, 10, &mut log);
    assert!(
        fake.requests_to("/api/generate").is_empty(),
        "something was loaded"
    );
    assert_eq!(session.model_name(), None);
    assert!(
        log.iter()
            .any(|l| l.contains("Ollama ready, no model loaded")),
        "{log:?}"
    );
    // the daemon can be asked things: this is what pulling and listing need
    assert!(crate::testkit::http::get(&format!("{}/api/tags", fake.base())).status == 200);
    session.stop_blocking();
}

#[test]
fn p3_s12_a_model_is_loaded_into_a_running_session_and_a_failed_load_leaves_it_running() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 1).add_model("big:70b", 1);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &world.plan("attach", None), "");
    settle(&mut session, ServerState::Running, 10, &mut log);
    assert_eq!(session.model_name(), None);

    fake.behave(|b| b.load_delay = Duration::from_millis(300));
    session.load(&world.plan("attach", Some("qwen3:4b")), "qwen3:4b", || {});
    assert_eq!(session.loading(), Some("qwen3:4b"));
    // a second one while the first is under way is not queued behind it
    session.load(&world.plan("attach", Some("big:70b")), "big:70b", || {});
    wait_until(Duration::from_secs(5), "the model to load", || {
        session.poll(&mut |l| log.push(l), &mut |_| {});
        session.model_name() == Some("qwen3:4b")
    });
    // long enough for a load that had been queued behind the first (300 ms each) to have happened
    std::thread::sleep(Duration::from_millis(700));
    session.poll(&mut |l| log.push(l), &mut |_| {});
    assert_eq!(session.loading(), None);
    assert_eq!(session.model_name(), Some("qwen3:4b"));
    assert_eq!(fake.loaded().as_deref(), Some("qwen3:4b"));
    let asked: Vec<_> = fake
        .requests_to("/api/generate")
        .iter()
        .map(|r| r.body.clone().unwrap_or_default()["model"].clone())
        .collect();
    assert_eq!(asked, ["qwen3:4b"], "the second load was never sent");
    fake.behave(|b| b.load_delay = Duration::ZERO);

    session.unload();
    wait_until(Duration::from_secs(5), "the model to unload", || {
        session.poll(&mut |l| log.push(l), &mut |_| {});
        session.model_name().is_none()
    });
    assert_eq!(fake.loaded(), None);
    assert_eq!(
        session.state(),
        ServerState::Running,
        "unloading a model must not stop the daemon"
    );

    // a load that fails is reported and the daemon goes on running with nothing loaded
    fake.behave(|b| b.load_failure = Some((500, "out of memory".into())));
    session.load(&world.plan("attach", Some("big:70b")), "big:70b", || {});
    wait_until(Duration::from_secs(5), "the failure to be logged", || {
        session.poll(&mut |l| log.push(l), &mut |_| {});
        log.iter().any(|l| l.contains("out of memory"))
    });
    assert_eq!(session.state(), ServerState::Running);
    assert_eq!(session.model_name(), None);
    session.stop_blocking();
}

// ---- stopping --------------------------------------------------------------------------------------

#[test]
fn p3_s13_stop_unloads_the_model_first_and_then_stops_the_daemon_it_started() {
    let world = World::new();
    let plan = world.plan("spawn", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    let fake = world.fake_after_start(world.port);
    fake.add_model("qwen3:4b", 1);
    // when the unload request arrives, is the daemon still alive?
    let alive_at_unload = Arc::new(AtomicBool::new(false));
    let pid_file = world.file("server.pid");
    let seen = alive_at_unload.clone();
    fake.on_request(move |r| {
        let unloading = r.body.as_ref().is_some_and(|b| b["keep_alive"] == 0);
        if r.path == "/api/generate" && unloading {
            let pid = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|t| t.trim().parse().ok());
            seen.store(pid.is_some_and(pid_alive), Ordering::SeqCst);
        }
    });
    settle(&mut session, ServerState::Running, 10, &mut log);
    let (server, runner) = (world.pid("server"), world.pid("runner"));

    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    let unloads: Vec<_> = warm_bodies(&fake)
        .into_iter()
        .filter(|b| b["keep_alive"] == 0)
        .collect();
    assert_eq!(
        unloads,
        [serde_json::json!({"model": "qwen3:4b", "keep_alive": 0})]
    );
    assert!(
        alive_at_unload.load(Ordering::SeqCst),
        "the daemon was stopped before it was asked to unload"
    );
    assert!(
        world.file("term.record").exists(),
        "the daemon was not asked to stop with SIGTERM"
    );
    assert!(
        gone_within(server, Duration::from_secs(3)) && gone_within(runner, Duration::from_secs(3))
    );
}

#[test]
fn p3_s14_stop_on_an_attached_daemon_unloads_the_model_and_leaves_the_daemon_running() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 1);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(
        &mut session,
        &world.plan("attach", Some("qwen3:4b")),
        "qwen3:4b",
    );
    settle(&mut session, ServerState::Running, 10, &mut log);
    assert_eq!(fake.loaded().as_deref(), Some("qwen3:4b"));
    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    assert_eq!(fake.loaded(), None, "the model was left loaded");
    assert!(
        crate::testkit::http::try_get(&format!("{}/api/version", fake.base())).is_ok(),
        "the daemon was not left running"
    );
}

#[test]
fn p3_s15_an_unload_that_fails_or_hangs_does_not_hold_up_the_stop() {
    let world = World::with(&Timings {
        unload: "400ms",
        ..FAST
    });
    let plan = world.plan("spawn", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    let fake = world.fake_after_start(world.port);
    fake.add_model("qwen3:4b", 1);
    settle(&mut session, ServerState::Running, 10, &mut log);
    fake.behave(|b| b.unload_delay = Duration::from_secs(5));
    let stopping = Instant::now();
    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    assert!(
        stopping.elapsed() < Duration::from_secs(3),
        "the stop waited for the unload: {:?}",
        stopping.elapsed()
    );
    assert!(gone_within(world.pid("server"), Duration::from_secs(3)));
}

#[test]
fn p3_s16_a_daemon_that_dies_by_itself_is_a_crash_and_clears_what_the_run_was_for() {
    let world = World::new();
    let plan = world.plan("spawn", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    let fake = world.fake_after_start(world.port);
    fake.add_model("qwen3:4b", 1);
    settle(&mut session, ServerState::Running, 10, &mut log);
    kill_quietly(world.pid("server"));
    settle(&mut session, ServerState::Crashed, 10, &mut log);
    assert!(
        log.iter().any(|l| l.contains("exited unexpectedly")),
        "{log:?}"
    );
    assert_eq!((session.port(), session.model_name()), (None, None));
}

// ---- an attached daemon that goes away ------------------------------------------------------------

#[test]
fn p3_s17_an_attached_daemon_that_disappears_is_noticed_and_probing_stops_with_the_run() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 1);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(
        &mut session,
        &world.plan("attach", Some("qwen3:4b")),
        "qwen3:4b",
    );
    settle(&mut session, ServerState::Running, 10, &mut log);
    fake.go_down();
    settle(&mut session, ServerState::Crashed, 5, &mut log);
    assert!(
        log.iter()
            .any(|l| l.to_lowercase().contains("stopped answering")),
        "{log:?}"
    );

    // after a normal stop the probe does not go on asking
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 1);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(
        &mut session,
        &world.plan("attach", Some("qwen3:4b")),
        "qwen3:4b",
    );
    settle(&mut session, ServerState::Running, 10, &mut log);
    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    let asked = fake.requests_to("/api/version").len();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        fake.requests_to("/api/version").len(),
        asked,
        "the liveness probe outlived the run"
    );
}

#[test]
fn p3_s18_one_missed_probe_is_not_a_crash() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 1);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(
        &mut session,
        &world.plan("attach", Some("qwen3:4b")),
        "qwen3:4b",
    );
    settle(&mut session, ServerState::Running, 10, &mut log);
    fake.behave(|b| {
        b.fail_next.insert("/api/version".into(), (500, 1));
    });
    let until = Instant::now() + Duration::from_millis(800);
    while Instant::now() < until {
        session.poll(&mut |l| log.push(l), &mut |_| {});
        assert_eq!(
            session.state(),
            ServerState::Running,
            "a single failed probe was taken for a crash: {log:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    session.stop_blocking();
}

// ---- what the log shows ----------------------------------------------------------------------------

#[test]
fn p3_s19_the_log_starts_with_the_command_then_the_variables_set_and_hides_secrets() {
    let world = World::new();
    let selection = world.selection("spawn", Some("qwen3:4b")).set(
        "ollama_test.extra_env",
        "OLLAMA_DEBUG=1\nOLLAMA_API_KEY=hunter2\nOLLAMA_HOST=0.0.0.0:1",
    );
    let plan = world.plan_for(&selection, None);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    let fake = world.fake_after_start(world.port);
    fake.add_model("qwen3:4b", 2_500_000_000);
    settle(&mut session, ServerState::Running, 10, &mut log);
    assert_eq!(log[0], format!("$ {} serve", world.script.display()));
    let env_line = log[1].clone();
    assert!(
        env_line.starts_with("env "),
        "the second line is not the environment: {env_line}"
    );
    assert!(env_line.contains(&format!("OLLAMA_HOST=127.0.0.1:{}", world.port)));
    assert!(env_line.contains("OLLAMA_NO_CLOUD=1"));
    assert!(env_line.contains("OLLAMA_DEBUG=1"));
    assert!(
        env_line.contains("OLLAMA_API_KEY=***") && !env_line.contains("hunter2"),
        "{env_line}"
    );
    // the locked variable is the document's, whatever the user's lines said
    assert_eq!(
        world.env_record().get("OLLAMA_HOST"),
        Some(&format!("127.0.0.1:{}", world.port))
    );
    assert_eq!(
        world.env_record().get("OLLAMA_DEBUG").map(String::as_str),
        Some("1")
    );
    session.stop_blocking();
}

#[test]
fn p3_s20_live_traffic_is_not_started_in_front_of_a_daemon_that_was_already_there() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 1);
    let selection = world
        .selection("attach", Some("qwen3:4b"))
        .set(quickai_core::plan::TRAFFIC_PROXY_KEY, true);
    let internal = free_port();
    let plan = world.plan_for(&selection, Some(internal));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    settle(&mut session, ServerState::Running, 10, &mut log);
    assert_eq!(
        session.port(),
        Some(world.port),
        "the public port is the daemon's own"
    );
    assert!(
        std::net::TcpListener::bind(("127.0.0.1", internal)).is_ok(),
        "a gateway was bound on the spare port in front of a daemon that was not started here"
    );
    assert!(
        log.iter().any(|l| l.to_lowercase().contains("traffic")),
        "the log does not say why: {log:?}"
    );
    session.stop_blocking();
}

#[test]
fn p3_s21_live_traffic_in_front_of_a_started_daemon_binds_the_public_port_and_the_daemon_the_internal_one(
) {
    let world = World::new();
    let public = world.port;
    let internal = free_port();
    let selection = world
        .selection("spawn", Some("qwen3:4b"))
        .set(quickai_core::plan::TRAFFIC_PROXY_KEY, true);
    let plan = world.plan_for(&selection, Some(internal));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    let fake = world.fake_after_start(internal);
    fake.add_model("qwen3:4b", 1);
    settle(&mut session, ServerState::Running, 10, &mut log);
    assert_eq!(
        world.env_record().get("OLLAMA_HOST"),
        Some(&format!("127.0.0.1:{internal}")),
        "the daemon was told to listen on the public port the gateway holds"
    );
    // a client on the public port reaches the daemon through the gateway
    let reply = crate::testkit::http::get(&format!("http://127.0.0.1:{public}/api/version"));
    assert_eq!(reply.json()["version"], "0.35.1");
    session.stop_blocking();
}

#[test]
fn p3_s22_a_second_start_while_running_is_ignored_and_stop_then_start_works_again() {
    let world = World::new();
    let fake = world.fake_running();
    fake.add_model("qwen3:4b", 1);
    let plan = world.plan("attach", Some("qwen3:4b"));
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "qwen3:4b");
    settle(&mut session, ServerState::Running, 10, &mut log);
    let loads = warm_bodies(&fake).len();
    start(&mut session, &plan, "qwen3:4b");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        warm_bodies(&fake).len(),
        loads,
        "a start while running loaded the model again"
    );

    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    start(&mut session, &plan, "qwen3:4b");
    settle(&mut session, ServerState::Running, 10, &mut log);
    session.stop_blocking();
}

/// The child of S23: an application started with an environment of its own (`OLLAMA_HOST=0.0.0.0:80`
/// and the like, set by the parent), which starts the daemon and stops it again.
#[test]
#[ignore = "runs only as the child of p3_s23"]
fn child_main_s23() {
    if !crate::testkit::procs::is_test_child() {
        return;
    }
    let dir = PathBuf::from(std::env::var("QUICKAI_P3_DIR").unwrap());
    let port: u16 = std::env::var("QUICKAI_P3_PORT").unwrap().parse().unwrap();
    let own_store = std::env::var("QUICKAI_S23_OWN_STORE").unwrap() == "1";
    let world = World::adopt_full(dir, port);
    let selection = world
        .selection("spawn", None)
        .set("ollama_test.use_default_store", own_store)
        .set("ollama_test.models_dir", "/qa/models");
    let plan = world.plan_for(&selection, None);
    let mut session = Session::new();
    session.start(&plan, "", || {});
    wait_until(
        Duration::from_secs(10),
        "the daemon script to start",
        || world.script_ran(),
    );
    session.stop_blocking();
    println!("done");
}

/// The environment the daemon was started with, by an application whose own environment is
/// `inherited` and whose engine uses its own store or Ollama's.
fn s23_daemon_env(own_store: bool, inherited: &[(&str, &str)]) -> (u16, BTreeMap<String, String>) {
    let world = World::new();
    let port = world.port.to_string();
    let mut env: Vec<(&str, &str)> = vec![
        ("QUICKAI_P3_DIR", world.dir.to_str().unwrap()),
        ("QUICKAI_P3_PORT", &port),
        ("QUICKAI_S23_OWN_STORE", if own_store { "1" } else { "0" }),
    ];
    env.extend_from_slice(inherited);
    let mut child = crate::testkit::procs::spawn_test_child("p3::session::child_main_s23", &env);
    let status = child.wait().expect("the child test process");
    assert!(status.success(), "the child test process failed: {status}");
    assert!(world.script_ran(), "the daemon was never started");
    (world.port, world.env_record())
}

#[test]
fn p3_s23_the_environment_the_application_was_started_with_never_overrides_the_plan() {
    let inherited = [
        ("OLLAMA_HOST", "0.0.0.0:80"),
        ("OLLAMA_NO_CLOUD", "0"),
        ("OLLAMA_MODELS", "/inherited/store"),
        ("OLLAMA_DEBUG", "1"),
    ];
    // Ollama's own store switched on: the plan sets no store, so the user's own is left alone
    let (port, env) = s23_daemon_env(true, &inherited);
    assert_eq!(
        env.get("OLLAMA_HOST"),
        Some(&format!("127.0.0.1:{port}")),
        "the inherited host won over the planned one"
    );
    assert_eq!(env.get("OLLAMA_NO_CLOUD").map(String::as_str), Some("1"));
    assert_eq!(
        env.get("OLLAMA_MODELS").map(String::as_str),
        Some("/inherited/store"),
        "the plan took the user's own model store from them"
    );
    assert_eq!(
        env.get("OLLAMA_DEBUG").map(String::as_str),
        Some("1"),
        "a variable the plan says nothing about was dropped"
    );

    // the engine's own store: what the plan sets is what the daemon gets
    let (port, env) = s23_daemon_env(false, &inherited);
    assert_eq!(
        env.get("OLLAMA_MODELS").map(String::as_str),
        Some("/qa/models"),
        "an inherited store won over the engine's own"
    );
    assert_eq!(env.get("OLLAMA_HOST"), Some(&format!("127.0.0.1:{port}")));
    assert_eq!(env.get("OLLAMA_NO_CLOUD").map(String::as_str), Some("1"));
}

// ---- process hygiene (K) ---------------------------------------------------------------------------

/// A running spawned session of the hygiene document, ready to be stopped one way or another.
fn running_spawned(world: &World) -> (Session, Vec<String>, FakeOllama) {
    let plan = world.plan("spawn", None);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "");
    let fake = world.fake_after_start(world.port);
    settle(&mut session, ServerState::Running, 10, &mut log);
    (session, log, fake)
}

/// The harness itself, on what this build does today: a daemon is started with the document's
/// environment, answers when the fake binds its port, is Running, and Stop ends the session. If
/// this fails, the red tests around it fail for a reason of their own.
#[test]
fn p3_k00_the_harness_runs_a_daemon_to_running_and_stops_it() {
    let world = World::kernel_today();
    let plan = world.plan("spawn", None);
    let (mut session, mut log) = (Session::new(), Vec::new());
    start(&mut session, &plan, "");
    let _fake = world.fake_after_start(world.port);
    settle(&mut session, ServerState::Running, 10, &mut log);
    assert_eq!(log[0], format!("$ {} serve", world.script.display()));
    assert_eq!(
        world.env_record().get("OLLAMA_HOST"),
        Some(&format!("127.0.0.1:{}", world.port))
    );
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    assert!(pid_alive(server) && pid_alive(runner));
    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    assert!(gone_within(server, Duration::from_secs(3)));
    // the runner is not asserted on: today only the daemon itself is killed (that is K01)
}

#[test]
fn p3_k01_stop_ends_the_whole_process_group_not_only_the_child() {
    let world = World::kernel();
    let (mut session, mut log, _fake) = running_spawned(&world);
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    assert!(pid_alive(server) && pid_alive(runner));
    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    assert!(
        gone_within(server, Duration::from_secs(3)),
        "the daemon survived Stop"
    );
    assert!(
        gone_within(runner, Duration::from_secs(3)),
        "the daemon's runner survived Stop"
    );
}

#[test]
fn p3_k02_a_daemon_that_ignores_sigterm_is_killed_when_the_grace_has_passed() {
    let world = World::kernel();
    std::fs::write(world.file("ignore-term"), b"").unwrap();
    let (mut session, mut log, _fake) = running_spawned(&world);
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    let stopping = Instant::now();
    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    let took = stopping.elapsed();
    assert!(
        took >= Duration::from_millis(250),
        "killed before the grace period was over: {took:?}"
    );
    assert!(took < Duration::from_secs(4), "took {took:?}");
    assert!(
        gone_within(server, Duration::from_secs(3)),
        "a daemon that ignores SIGTERM survived the kill"
    );
    assert!(gone_within(runner, Duration::from_secs(3)));
}

#[test]
fn p3_k03_a_daemon_that_obeys_sigterm_is_not_made_to_wait_out_the_grace() {
    let world = World::build(
        &Timings {
            grace: "10s",
            ..FAST
        },
        true,
    );
    let (mut session, mut log, _fake) = running_spawned(&world);
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    let stopping = Instant::now();
    session.stop();
    settle(&mut session, ServerState::Idle, 12, &mut log);
    assert!(
        stopping.elapsed() < Duration::from_secs(3),
        "waited {:?} for a daemon that had exited",
        stopping.elapsed()
    );
    assert!(world.file("term.record").exists());
    assert!(
        gone_within(server, Duration::from_secs(1)) && gone_within(runner, Duration::from_secs(1))
    );
    assert!(
        log.iter().any(|l| l.starts_with("Server stopped")),
        "{log:?}"
    );
}

#[test]
fn p3_k04_closing_the_app_stops_even_a_stubborn_daemon_within_the_exit_grace() {
    let world = World::kernel();
    std::fs::write(world.file("ignore-term"), b"").unwrap();
    let (mut session, _log, _fake) = running_spawned(&world);
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    let closing = Instant::now();
    session.stop_blocking();
    assert!(
        closing.elapsed() < Duration::from_secs(3),
        "closing took {:?}",
        closing.elapsed()
    );
    assert!(
        gone_within(server, Duration::from_secs(2)) && gone_within(runner, Duration::from_secs(2))
    );
}

#[test]
fn p3_k06_the_daemon_leads_a_process_group_of_its_own() {
    let world = World::kernel();
    let (mut session, _log, _fake) = running_spawned(&world);
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    let group = crate::testkit::procs::process_group(server).unwrap();
    assert_eq!(group, server, "the daemon is not the leader of its group");
    assert_eq!(
        crate::testkit::procs::process_group(runner),
        Some(server),
        "the runner is not in the daemon's group"
    );
    assert_ne!(
        Some(group),
        crate::testkit::procs::process_group(std::process::id())
    );
    session.stop_blocking();
}

/// The hygiene document puts `OLLAMA_API_KEY=hunter2` in the environment: the log must show that
/// it was set and not what it was set to. (S19 is the same with the user's own lines, which come
/// with work package 3.12.)
#[test]
fn p3_s19a_the_log_starts_with_the_command_then_the_variables_set_and_hides_secrets() {
    let world = World::kernel();
    let (mut session, log, _fake) = running_spawned(&world);
    assert_eq!(log[0], format!("$ {} serve", world.script.display()));
    let env_line = &log[1];
    assert!(
        env_line.starts_with("env "),
        "the second line is not the environment: {env_line}"
    );
    assert!(env_line.contains(&format!("OLLAMA_HOST=127.0.0.1:{}", world.port)));
    assert!(
        env_line.contains("OLLAMA_API_KEY=***") && !env_line.contains("hunter2"),
        "{env_line}"
    );
    // and the program really did get the secret: the line hides it, the plan does not
    assert_eq!(
        world.env_record().get("OLLAMA_API_KEY").map(String::as_str),
        Some("hunter2")
    );
    session.stop_blocking();
}

/// Where there is no parent-death signal (macOS) the daemon ends only through Stop and the exit
/// path, so those must do everything on their own. This is K01, K04 and K06 without anything
/// Linux-specific: the group is read with `ps` where there is no `/proc`.
#[test]
fn p3_k10_stop_and_the_exit_path_end_the_whole_group_without_a_parent_death_signal() {
    // Stop: the daemon leads a group of its own, and Stop ends it and its runner
    let world = World::kernel();
    let (mut session, mut log, _fake) = running_spawned(&world);
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    assert_eq!(
        crate::testkit::procs::process_group(server),
        Some(server),
        "the daemon is not the leader of its group"
    );
    assert_eq!(
        crate::testkit::procs::process_group(runner),
        Some(server),
        "the runner is not in the daemon's group"
    );
    session.stop();
    settle(&mut session, ServerState::Idle, 10, &mut log);
    assert!(gone_within(server, Duration::from_secs(3)));
    assert!(gone_within(runner, Duration::from_secs(3)));

    // the exit path: a daemon that ignores SIGTERM is still gone within the exit grace
    let world = World::kernel();
    std::fs::write(world.file("ignore-term"), b"").unwrap();
    let (mut session, _log, _fake) = running_spawned(&world);
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    let closing = Instant::now();
    session.stop_blocking();
    assert!(closing.elapsed() < Duration::from_secs(3));
    assert!(gone_within(server, Duration::from_secs(2)));
    assert!(gone_within(runner, Duration::from_secs(2)));
}

/// Runs in a process of its own: starts a session on the script of `QUICKAI_P3_DIR` and waits to
/// be killed.
#[test]
#[ignore = "runs only as the child of p3_k05"]
fn child_main_session() {
    if !crate::testkit::procs::is_test_child() {
        return;
    }
    let dir = PathBuf::from(std::env::var("QUICKAI_P3_DIR").unwrap());
    let port: u16 = std::env::var("QUICKAI_P3_PORT").unwrap().parse().unwrap();
    let world = World::adopt(dir, port);
    let mut plan = world.plan("spawn", None);
    if std::env::var_os("QUICKAI_P3_NO_GROUP").is_some() {
        // a program that does not lead a group of its own, as llama-server does not
        for unit in &mut plan.units {
            for start in &mut unit.start {
                if let quickai_core::plan::Start::Spawn(spawn) = start {
                    spawn.group = false;
                }
            }
        }
    }
    let mut session = Session::new();
    session.start(&plan, "", || {});
    println!("started");
    std::thread::sleep(Duration::from_secs(60));
}

#[cfg(target_os = "linux")]
#[test]
fn p3_k05_a_daemon_follows_the_app_when_the_app_is_killed() {
    a_program_follows_the_app_when_the_app_is_killed(&[]);
}

/// K11: the same for a program with no process group of its own (llama-server, whose document says
/// `group = false`): it used to outlive an application that was killed (finding F6, TC-SRV-022).
#[cfg(target_os = "linux")]
#[test]
fn p3_k11_a_program_that_leads_no_group_follows_the_app_too() {
    a_program_follows_the_app_when_the_app_is_killed(&[("QUICKAI_P3_NO_GROUP", "1")]);
}

#[cfg(target_os = "linux")]
fn a_program_follows_the_app_when_the_app_is_killed(extra_env: &[(&str, &str)]) {
    let world = World::kernel();
    let port = world.port.to_string();
    let mut env = vec![
        ("QUICKAI_P3_DIR", world.dir.to_str().unwrap()),
        ("QUICKAI_P3_PORT", port.as_str()),
    ];
    env.extend_from_slice(extra_env);
    let mut child =
        crate::testkit::procs::spawn_test_child("p3::session::child_main_session", &env);
    wait_until(
        Duration::from_secs(10),
        "the child's daemon to start",
        || world.script_ran(),
    );
    let (server, runner) = (world.pid("server"), world.pid("runner"));
    assert!(pid_alive(server));
    // the application dies without a chance to clean up
    child.kill().unwrap();
    let _ = child.wait();
    // the daemon gets SIGTERM from the kernel, which is what makes Ollama stop its runner (A3)
    assert!(
        gone_within(server, Duration::from_secs(5)),
        "the daemon outlived the application"
    );
    assert!(
        world.file("term.record").exists(),
        "it was killed, not asked to stop: the runner would have been orphaned"
    );
    assert!(
        gone_within(runner, Duration::from_secs(5)),
        "the daemon's runner outlived the application"
    );
}
