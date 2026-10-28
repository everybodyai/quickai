//! WP4.33, a configuration run on another computer by QuickAI itself (docs/remote-runs.md), end to end on this
//! machine: the other computer is a folder of it, reached through the stand-in for `ssh` and `scp`
//! (`testkit::fake_ssh`), and what runs there is this build of `quickai-run`, copied there as the window copies it,
//! against the world of the runtime's tests (a fake GitHub, a file server, a stand-in `llama-server`). What a real
//! computer adds (a login, another system) is left to the tests run with one.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use quickai_runtime::remote::job::{self, Job, Launching, Update};
use quickai_runtime::remote::{probe, run, RemoteSpec, Way};
use quickai_runtime::testkit::fake_ssh;
use quickai_runtime::testkit::headless_world::{World, FILE};
use quickai_runtime::testkit::http::try_get;
use quickai_runtime::testkit::wait_until;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The package's `remote/` folder for these tests: this build of `quickai-run` as the program for Linux, named in
/// `QUICKAI_REMOTE_PROGRAMS` once for the whole run (every test names the same).
fn programs() {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("quickai-remote-programs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(
            env!("CARGO_BIN_EXE_quickai-run"),
            dir.join("quickai-run-linux-x86_64"),
        )
        .unwrap();
        dir
    });
    std::env::set_var("QUICKAI_REMOTE_PROGRAMS", dir);
}

/// The other computer of `world`: the stand-ins in a folder of its own, a home beside them, and the variables that
/// send its `quickai-run` to the world's fakes. `None` when there is no python3 to run the stand-ins.
struct Computer {
    ssh: PathBuf,
    home: PathBuf,
    spec: RemoteSpec,
}

impl Computer {
    fn new(world: &World, name: &str) -> Option<Computer> {
        programs();
        let ssh = fake_ssh::install(&world.scratch.join(name))?;
        let home = fake_ssh::remote_home(&ssh);
        let path = std::env::var("PATH").unwrap_or_default();
        let (config, xdg) = (
            world.config.display().to_string(),
            home.display().to_string(),
        );
        fake_ssh::set_remote_env(
            &ssh,
            &[
                ("QUICKAI_CONFIG_DIR", &config),
                ("PATH", &path),
                ("XDG_CONFIG_HOME", &format!("{xdg}/.config")),
                ("XDG_DATA_HOME", &format!("{xdg}/.local/share")),
                ("XDG_CACHE_HOME", &format!("{xdg}/.cache")),
            ],
        );
        let mut spec = RemoteSpec::new("Studio", "me@studio.local");
        spec.way = Way::SetUp;
        Some(Computer { ssh, home, spec })
    }

    fn job(&self) -> Job {
        Job {
            ssh: self.ssh.clone(),
            scp: fake_ssh::scp_of(&self.ssh),
            spec: self.spec.clone(),
            version: VERSION.into(),
        }
    }

    /// What it is, as the window asks it.
    fn probe(&self) -> probe::Computer {
        probe::probe(&self.ssh, &self.spec, VERSION).expect("it answers")
    }

    /// What runs there.
    fn status(&self) -> quickai_runtime::run_file::Status {
        run::status(&self.ssh, &self.spec, &self.probe(), VERSION).expect("it says")
    }
}

impl Drop for Computer {
    /// A run a failing test left there is stopped, so that no program of a test is left running.
    fn drop(&mut self) {
        if let Ok(computer) = probe::probe(&self.ssh, &self.spec, VERSION) {
            if computer.headless.is_some() {
                let _ = run::stop(&self.ssh, &self.spec, &computer, VERSION);
            }
        }
    }
}

/// What a job says until it says one of `last` (or fails, or two minutes pass): everything it said, in order.
fn until(updates: &crossbeam_channel::Receiver<Update>, last: &[&str]) -> Vec<Update> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut said = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let update = updates
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("the job said nothing more; it said: {:?}", names(&said)));
        let name = name(&update);
        if let Update::Failed(why) = &update {
            if !last.contains(&"Failed") {
                panic!("the job failed: {why}; it said: {:?}", names(&said));
            }
        }
        said.push(update);
        if last.contains(&name) {
            return said;
        }
    }
}

fn name(update: &Update) -> &'static str {
    match update {
        Update::Doing(_) => "Doing",
        Update::Status(_) => "Status",
        Update::Busy(_) => "Busy",
        Update::Occupied(_) => "Occupied",
        Update::Alternatives(_) => "Alternatives",
        Update::Remember(_) => "Remember",
        Update::Running { .. } => "Running",
        Update::Nothing => "Nothing",
        Update::Stopped => "Stopped",
        Update::Failed(_) => "Failed",
    }
}

fn names(said: &[Update]) -> Vec<String> {
    said.iter()
        .map(|u| match u {
            Update::Doing(what) => format!("Doing({what})"),
            Update::Failed(why) => format!("Failed({why})"),
            other => name(other).to_string(),
        })
        .collect()
}

/// The configuration `body` of `world` as the window would send it: its name and its text.
fn launching(world: &World, name: &str, body: &str) -> Launching {
    let file = world.file(&name.to_lowercase(), body);
    Launching {
        name: name.into(),
        text: std::fs::read_to_string(file).unwrap(),
        // the cards of this machine are not what the test is about
        flags: vec!["--gpu".into(), "none".into()],
        ..Launching::default()
    }
}

/// The usual configuration of the world: the model of its list, on its port.
fn usual(world: &World, name: &str) -> Launching {
    launching(
        world,
        name,
        &format!(
            "[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    )
}

fn start(computer: &Computer, launching: Launching) -> crossbeam_channel::Receiver<Update> {
    job::start(
        computer.job(),
        launching,
        || {},
        Arc::new(AtomicBool::new(false)),
    )
}

fn port_of(url: &str) -> u16 {
    url.rsplit(':')
        .next()
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

/// A program of another kind that listens on `port` of this machine until it is ended: QuickAI did not start it.
fn listener_on(port: u16) -> std::process::Child {
    let child = std::process::Command::new("python3")
        .args([
            "-c",
            &format!(
                "import socket,time\ns=socket.socket()\ns.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)\ns.bind(('127.0.0.1',{port}))\ns.listen(4)\ntime.sleep(600)"
            ),
        ])
        .spawn()
        .expect("python3 starts");
    wait_until(
        Duration::from_secs(10),
        "the other program to listen",
        || std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
    );
    child
}

#[test]
fn a_configuration_runs_on_the_other_computer_its_ports_come_here_it_is_found_again_and_stopped() {
    let world = World::new("remote-run");
    let Some(computer) = Computer::new(&world, "studio") else {
        eprintln!("no python3: the stand-in for ssh cannot run");
        return;
    };
    // what it is, before anything is there
    let found = computer.probe();
    assert_eq!(found.system, probe::System::Linux);
    assert_eq!(found.home, computer.home.display().to_string());
    assert_eq!(found.headless, None);

    // Start: QuickAI copied there, the configuration sent, run in the background, and its port brought here
    let said = until(&start(&computer, usual(&world, "Usual")), &["Running"]);
    let doing: Vec<String> = names(&said);
    for step in [
        "Connecting to Studio",
        "Copying QuickAI to Studio",
        "Sending \"Usual\" to Studio",
        "Starting \"Usual\" on Studio",
    ] {
        assert!(
            doing.iter().any(|d| d.contains(step)),
            "{step} in {doing:?}"
        );
    }
    let Some(Update::Running {
        tunnel,
        endpoints,
        run,
    }) = said.into_iter().last()
    else {
        unreachable!()
    };
    assert!(computer
        .home
        .join(format!(".quickai/bin/quickai-run-{VERSION}"))
        .is_file());
    assert!(computer.home.join(".quickai/remote/usual.toml").is_file());
    assert_eq!(
        computer.probe().headless.as_deref(),
        Some(format!("quickai-run {VERSION}").as_str())
    );
    assert_eq!(run.state, "ready");
    assert!(run.detached);
    assert_eq!(run.model.as_deref(), Some(FILE));
    assert!(
        run.steps.iter().all(|s| s.state == "done"),
        "{:?}",
        run.steps
    );
    assert!(run.computer.is_some(), "it says what computer it runs on");
    // the port there is taken here (the other computer is this one): a free one is used
    let here = port_of(&endpoints[0].url);
    assert_eq!(here, tunnel.local_port);
    assert_ne!(here, world.port);
    assert_eq!(
        try_get(&format!("http://127.0.0.1:{here}/health"))
            .unwrap()
            .status,
        200
    );
    assert!(
        computer.home.join(".quickai/llama-bin").is_dir(),
        "llama.cpp installed in its home"
    );
    assert!(
        computer.home.join(".quickai/models").join(FILE).is_file(),
        "the model fetched there"
    );

    // what runs there is said, and another configuration is not started over it
    let status = computer.status();
    assert_eq!(
        status.running.as_ref().map(|r| r.name()).as_deref(),
        Some("usual")
    );
    let busy = until(&start(&computer, usual(&world, "Other")), &["Busy"]);
    match busy.last() {
        Some(Update::Busy(Some(other))) => assert_eq!(other.name(), "usual"),
        other => panic!("{:?}", other.map(name)),
    }

    // the window closed (the tunnel with it): the run goes on there, and is found again
    drop(tunnel);
    wait_until(Duration::from_secs(10), "the tunnel to close", || {
        try_get(&format!("http://127.0.0.1:{here}/health")).is_err()
    });
    assert!(computer.status().running.is_some(), "the run goes on there");
    let again = until(&job::find(computer.job(), || {}), &["Running"]);
    let Some(Update::Running {
        tunnel, endpoints, ..
    }) = again.into_iter().last()
    else {
        unreachable!()
    };
    let here = port_of(&endpoints[0].url);
    assert_eq!(
        try_get(&format!("http://127.0.0.1:{here}/health"))
            .unwrap()
            .status,
        200
    );

    // Stop: it stops there, and it says how it ended
    until(&job::stop(computer.job(), || {}), &["Stopped"]);
    drop(tunnel);
    let after = computer.status();
    assert!(after.running.is_none());
    assert_eq!(after.last.map(|l| l.outcome).as_deref(), Some("stopped"));
    wait_until(
        Duration::from_secs(15),
        "the engine there to be gone",
        || try_get(&world.url("/health")).is_err(),
    );
    let nothing = until(&job::find(computer.job(), || {}), &["Nothing"]);
    assert!(matches!(nothing.last(), Some(Update::Nothing)));
}

#[test]
fn a_model_only_this_computer_has_is_offered_copied_when_accepted_and_run() {
    let world = World::new("remote-model");
    let Some(computer) = Computer::new(&world, "studio") else {
        return;
    };
    let mine = world.scratch.join("Mine.gguf");
    std::fs::write(
        &mine,
        quickai_runtime::testkit::headless_world::model_bytes("Mine.gguf"),
    )
    .unwrap();
    let size = std::fs::metadata(&mine).unwrap().len();
    let body = format!(
        "[[model]]\nfile = \"Mine.gguf\"\n\n[values.llamacpp]\nport = {}\n",
        world.port
    );
    let mut asking = launching(&world, "Mine", &body);
    asking.local_models = BTreeMap::from([("Mine.gguf".to_string(), (mine.clone(), size))]);

    // asked first: nothing is copied or started before the person answers
    let said = until(&start(&computer, asking.clone()), &["Alternatives"]);
    let Some(Update::Alternatives(issues)) = said.last() else {
        unreachable!()
    };
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].id, "model:Mine.gguf");
    assert!(
        issues[0].alternative.contains("Copy it from this computer"),
        "{}",
        issues[0].alternative
    );
    assert!(!computer
        .home
        .join(".quickai/remote/models/Mine.gguf")
        .exists());
    assert!(computer.status().running.is_none());

    // rejected: it stops with what then
    let mut rejected = asking.clone();
    rejected.answers = BTreeMap::from([("model:Mine.gguf".to_string(), false)]);
    let said = until(&start(&computer, rejected), &["Failed"]);
    let Some(Update::Failed(why)) = said.last() else {
        unreachable!()
    };
    assert!(why.contains("Then it stops here"), "{why}");

    // accepted: copied there, named, run, and what was accepted is said to be kept
    let mut accepted = asking;
    accepted.answers = BTreeMap::from([("model:Mine.gguf".to_string(), true)]);
    let said = until(&start(&computer, accepted), &["Running"]);
    let copied = computer.home.join(".quickai/remote/models/Mine.gguf");
    assert_eq!(
        std::fs::read(&copied).unwrap(),
        std::fs::read(&mine).unwrap()
    );
    let remembered: Vec<&Vec<String>> = said
        .iter()
        .filter_map(|u| match u {
            Update::Remember(words) => Some(words),
            _ => None,
        })
        .collect();
    assert_eq!(
        remembered,
        [&vec![
            "--model-source".to_string(),
            format!("Mine.gguf={}", copied.display())
        ]]
    );
    let Some(Update::Running { tunnel, run, .. }) = said.into_iter().last() else {
        unreachable!()
    };
    assert_eq!(run.model.as_deref(), Some("Mine.gguf"));
    assert_eq!(
        try_get(&format!("http://127.0.0.1:{}/health", tunnel.local_port))
            .unwrap()
            .status,
        200
    );
    until(&job::stop(computer.job(), || {}), &["Stopped"]);
}

#[test]
fn a_port_another_program_holds_there_is_said_and_overriding_stops_it_and_runs_ours() {
    let world = World::new("remote-port");
    let Some(computer) = Computer::new(&world, "studio") else {
        return;
    };
    let mut other = listener_on(world.port);

    // said, with nothing started
    let said = until(&start(&computer, usual(&world, "Usual")), &["Occupied"]);
    let Some(Update::Occupied(ports)) = said.last() else {
        unreachable!()
    };
    assert_eq!(ports.len(), 1);
    assert_eq!(ports[0].port, world.port);
    assert!(ports[0].taken);
    assert_eq!(
        ports[0].answers_as, None,
        "it does not answer as llama.cpp does"
    );
    assert!(
        other.try_wait().unwrap().is_none(),
        "nothing was stopped before the person said so"
    );

    // Override: it is stopped, and ours runs
    let mut overriding = usual(&world, "Usual");
    overriding.override_first = true;
    let said = until(&start(&computer, overriding), &["Running"]);
    assert!(
        names(&said)
            .iter()
            .any(|d| d.contains("Stopping what holds the ports")),
        "{:?}",
        names(&said)
    );
    let ended = Instant::now();
    while other.try_wait().unwrap().is_none() {
        assert!(
            ended.elapsed() < Duration::from_secs(20),
            "the other program was not stopped"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let Some(Update::Running { tunnel, .. }) = said.into_iter().last() else {
        unreachable!()
    };
    assert_eq!(
        try_get(&format!("http://127.0.0.1:{}/health", tunnel.local_port))
            .unwrap()
            .status,
        200
    );
    until(&job::stop(computer.job(), || {}), &["Stopped"]);
}

#[test]
fn a_computer_that_refuses_the_login_says_so_and_nothing_is_copied() {
    let world = World::new("remote-denied");
    let Some(mut computer) = Computer::new(&world, "denied") else {
        return;
    };
    computer.spec.host = "me@denied".into();
    let said = until(&start(&computer, usual(&world, "Usual")), &["Failed"]);
    let Some(Update::Failed(why)) = said.last() else {
        unreachable!()
    };
    assert!(why.contains("refused the login"), "{why}");
    assert!(!computer.home.join(".quickai").exists());
    let ssh: &Path = &computer.ssh;
    assert!(
        fake_ssh::calls(ssh).iter().all(|c| c[0] == "ssh"),
        "no copy was tried"
    );
}
