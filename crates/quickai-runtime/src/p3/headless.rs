//! Group O of the test plan: a configuration run without a window (work package 4.18,
//! docs/headless.md).
//!
//! Everything runs as the program runs it: `provision::up` on a configuration file, in a data
//! directory of the test's own, against a fake GitHub that lists releases, a file server that serves their
//! archives and the files of a model, and a stand-in `llama-server` (tests/fixtures/launchers/standin.py)
//! that really listens on the port it is given. The documents are the shipped ones, patched the way a person
//! patches them (a file in the user's layer) to point at the fakes and to look for the program nowhere but
//! where it was installed, so that what a developer happens to have built on this machine is never run.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::config::ConfigDirs;
use crate::gpu::GpuVendor;
use crate::provision::{self, Event, FailureKind, Options, Outcome, Request};
use crate::testkit::archives::tar_zst;
use crate::testkit::fake_github::FakeGithub;
use crate::testkit::fake_ollama::FakeOllama;
use crate::testkit::files::FileServer;
use crate::testkit::headless_world::{
    asset_name, model_bytes, World, FILE, PROJECTOR, REPO, REV, STANDIN,
};
use crate::testkit::http::try_get;
use crate::testkit::{free_port, scratch, wait_until};

/// `provision::up` on a thread of its own, for a run that goes on until it is told to stop.
pub(super) struct Run {
    events: Arc<Mutex<Vec<Event>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Outcome>>,
}

impl Run {
    pub(super) fn start(request: Request) -> Run {
        let events: Arc<Mutex<Vec<Event>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (sink, flag) = (events.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            provision::up(&request, &mut |e| sink.lock().unwrap().push(e), &flag)
        });
        Run {
            events,
            stop,
            handle: Some(handle),
        }
    }

    pub(super) fn wait_for(&self, what: &str, found: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(event) = self.events.lock().unwrap().iter().find(|e| found(e)) {
                return event.clone();
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; so far: {:#?}",
                self.events.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub(super) fn ready(&self) -> Event {
        self.wait_for("the program to be ready", |e| {
            matches!(e, Event::Ready { .. })
        })
    }

    /// Asks it to stop and waits for it.
    pub(super) fn stop(mut self) -> (Outcome, Vec<Event>) {
        self.stop.store(true, Ordering::SeqCst);
        self.join()
    }

    /// Waits for it to end by itself for as long as `seconds`: a run that does not is stopped, and the test
    /// fails (a test of something that must end must not wait for ever when it does not).
    pub(super) fn ends_within(&mut self, seconds: u64) -> (Outcome, Vec<Event>) {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while !self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            if Instant::now() > deadline {
                self.stop.store(true, Ordering::SeqCst);
                let (_, events) = self.join();
                panic!("it did not end by itself within {seconds} s: {events:#?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.join()
    }

    /// Waits for it to end.
    pub(super) fn join(&mut self) -> (Outcome, Vec<Event>) {
        let outcome = self
            .handle
            .take()
            .unwrap()
            .join()
            .expect("up did not panic");
        let events = self.events.lock().unwrap().clone();
        (outcome, events)
    }
}

fn plan_of(events: &[Event]) -> Vec<(String, String, Option<u64>)> {
    events
        .iter()
        .find_map(|e| match e {
            Event::Plan { steps } => Some(
                steps
                    .iter()
                    .map(|s| (s.kind.clone(), s.text.clone(), s.bytes))
                    .collect(),
            ),
            _ => None,
        })
        .expect("a plan was made")
}

fn kinds(events: &[Event]) -> Vec<String> {
    plan_of(events).into_iter().map(|(k, _, _)| k).collect()
}

/// O10: a plan says what would be done, in order, with what each download weighs, and changes nothing: no
/// file is written, no program starts, the files themselves are not asked for. Only the list of builds is.
#[test]
fn p3_o10_a_plan_says_what_would_be_done_and_changes_nothing() {
    let world = World::new("o10");
    let (outcome, events) = world.up(
        &world.usual(),
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    assert_eq!(kinds(&events), ["install", "fetch", "start"]);
    let plan = plan_of(&events);
    assert!(
        plan[0].1.starts_with("install llama.cpp b100-") && plan[0].1.contains("ubuntu"),
        "the newest release's build: {}",
        plan[0].1
    );
    assert!(
        plan[0].2.is_some_and(|n| n > 100),
        "the archive's size: {plan:?}"
    );
    assert!(
        plan[1].1.contains(FILE)
            && plan[1].1.contains("1 more file")
            && plan[1].1.contains("huggingface:"),
        "{}",
        plan[1].1
    );
    let both = (model_bytes(FILE).len() + model_bytes(PROJECTOR).len()) as u64;
    assert_eq!(
        plan[1].2,
        Some(both),
        "the weights and the projector the list pairs with them"
    );
    assert!(
        plan[2].1.contains(&format!("127.0.0.1:{}", world.port)),
        "{}",
        plan[2].1
    );

    assert!(!world.data.exists(), "nothing was written");
    assert_eq!(world.files.requests(), 0, "no file was asked for");
    assert_eq!(world.github.requests().len(), 1, "the list of builds, once");
    assert!(
        try_get(&world.url("/health")).is_err(),
        "no program was started"
    );
    assert!(matches!(
        events.last(),
        Some(Event::Finished { outcome, .. }) if outcome == "planned"
    ));
}

/// O11: the whole thing: the build is installed, the files are fetched, the engine is started on its model
/// with the projector, the endpoints are said, and a stop leaves nothing running.
#[test]
fn p3_o11_a_configuration_becomes_a_running_engine_and_a_stop_leaves_nothing() {
    let world = World::new("o11");
    let run = Run::start(world.request(&world.usual(), world.options()));
    let ready = run.ready();
    let Event::Ready { endpoints, model } = ready else {
        unreachable!()
    };
    assert_eq!(model.as_deref(), Some(FILE));
    // llama-server has an API and a page of its own, at the same address
    let roles: Vec<&str> = endpoints.iter().map(|e| e.role.as_str()).collect();
    assert_eq!(roles, ["api", "page"], "{endpoints:?}");
    for endpoint in &endpoints {
        assert_eq!(endpoint.url, format!("http://127.0.0.1:{}", world.port));
        assert_eq!(endpoint.program, "llama.cpp");
    }

    // it answers
    assert_eq!(try_get(&world.url("/health")).unwrap().status, 200);
    let models = try_get(&world.url("/v1/models")).unwrap().json();
    assert_eq!(models["data"][0]["id"], FILE);

    // it was started on the file that was fetched, with the projector the list pairs with it, on the port
    let models_dir = world.data.join("models");
    assert_eq!(
        std::fs::read(models_dir.join(FILE)).unwrap(),
        model_bytes(FILE)
    );
    let argv = world.argv();
    let after = |flag: &str| {
        let at = argv
            .iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("{flag} in {argv:?}"));
        argv[at + 1].clone()
    };
    assert_eq!(
        after("--model"),
        models_dir.join(FILE).display().to_string()
    );
    assert_eq!(after("--port"), world.port.to_string());
    assert!(
        after("--mmproj").ends_with(&format!("acme--Alpha-4B-GGUF--{PROJECTOR}")),
        "{argv:?}"
    );

    // what it said reaches the person, as events, in the order it happened
    let (outcome, events) = run.stop();
    assert_eq!(outcome, Outcome::Stopped, "{events:#?}");
    let at = |wanted: &dyn Fn(&Event) -> bool| events.iter().position(wanted).expect("the event");
    let plan = at(&|e| matches!(e, Event::Plan { .. }));
    let first_step = at(&|e| matches!(e, Event::Step { index: 1, .. }));
    let second_step = at(&|e| matches!(e, Event::Step { index: 2, .. }));
    let ready = at(&|e| matches!(e, Event::Ready { .. }));
    let stopping = at(&|e| matches!(e, Event::Stopping));
    let finished = at(&|e| matches!(e, Event::Finished { .. }));
    assert!(plan < first_step && first_step < second_step && second_step < ready);
    assert!(ready < stopping && stopping < finished);
    assert_eq!(finished, events.len() - 1, "the end is the last thing said");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Log { line, .. } if line.contains("standin: listening"))),
        "what the engine wrote is passed on"
    );

    // nothing is left running
    wait_until(Duration::from_secs(10), "the engine to be gone", || {
        try_get(&world.url("/health")).is_err()
    });
}

/// O12: the second run finds what the first left and does nothing but start: no list of builds is asked
/// for, no file is fetched, and the same build is the one that runs.
#[test]
fn p3_o12_a_second_run_reuses_what_the_first_left() {
    let world = World::new("o12");
    let (outcome, _) = world.up(
        &world.usual(),
        Options {
            prepare_only: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Prepared);
    assert!(world.data.join("models").join(FILE).is_file());
    let (github, files) = (world.github.requests().len(), world.files.requests());

    let (outcome, events) = world.up(
        &world.usual(),
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Planned);
    assert_eq!(kinds(&events), ["start"], "{:#?}", plan_of(&events));
    assert_eq!(world.github.requests().len(), github, "no list of builds");
    assert_eq!(world.files.requests(), files, "no file");

    // and it starts, on the build the first run installed
    let run = Run::start(world.request(&world.usual(), world.options()));
    run.ready();
    assert!(world.argv().len() > 3, "it was started with its arguments");
    let (outcome, _) = run.stop();
    assert_eq!(outcome, Outcome::Stopped);
    assert_eq!(
        world.github.requests().len(),
        github,
        "still no list of builds"
    );
}

/// O13: the build is chosen by what the computer has: with no card found the build that names no backend,
/// with a card of AMD's the Vulkan one, and a person who names the cards is believed over the machine.
#[test]
fn p3_o13_the_cards_decide_the_build_when_nobody_named_one() {
    let world = World::new("o13");
    let plan_for = |gpus: Vec<GpuVendor>| {
        let (outcome, events) = world.up(
            &world.usual(),
            Options {
                dry_run: true,
                gpus: Some(gpus),
                ..Options::default()
            },
        );
        assert_eq!(outcome, Outcome::Planned, "{events:#?}");
        plan_of(&events)[0].1.clone()
    };
    let none = plan_for(vec![]);
    let amd = plan_for(vec![GpuVendor::Amd]);
    assert!(!none.contains("vulkan"), "{none}");
    assert!(amd.contains("vulkan"), "{amd}");
    // the machine's cards are said, and whether a person named them
    let (_, events) = world.up(
        &world.usual(),
        Options {
            dry_run: true,
            gpus: Some(vec![GpuVendor::Nvidia]),
            ..Options::default()
        },
    );
    assert!(events.iter().any(|e| matches!(e,
        Event::Computer { gpus, gpus_from, .. } if gpus == &["NVIDIA"] && gpus_from == "named")));
}

/// O14: a build the configuration names is the one installed, even when it is not the newest; one the list
/// does not have is a sentence that lists those it has, before anything is fetched.
#[test]
fn p3_o14_a_build_the_file_names_is_the_one_installed() {
    let world = World::new("o14");
    let old = format!(
        "b99-{}",
        asset_name("b99", None).trim_end_matches(".tar.gz")
    );
    let file = world.file(
        "named",
        &format!(
            "[runtime]\nbuild = \"{old}\"\n\n[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    );
    let (outcome, events) = world.up(
        &file,
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    assert!(
        plan_of(&events)[0].1.contains(" b99-"),
        "{:?}",
        plan_of(&events)[0]
    );

    let wrong = world.file(
        "wrong",
        "[runtime]\nbuild = \"b1-nothing\"\n\n[[model]]\ncatalog = \"alpha-4b\"\nfile = \"Alpha-4B-Q4_K_M.gguf\"\n",
    );
    let (outcome, _) = world.up(
        &wrong,
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    let Outcome::Failed(failure) = outcome else {
        panic!("a build the list does not have")
    };
    assert_eq!(failure.kind, FailureKind::Needs);
    assert!(
        failure.message.contains("b1-nothing") && failure.message.contains("b100-"),
        "{}",
        failure.message
    );
    assert!(!world.data.exists());
}

/// O15: what asks in a file from outside is not taken unless the person says so, and nothing is fetched
/// meanwhile; with the word, it goes on.
#[test]
fn p3_o15_a_file_that_asks_is_not_used_until_the_person_says_yes() {
    let world = World::new("o15");
    let asking = world.file(
        "asking",
        &format!(
            "[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nport = {}\nextra_args = \"--flash-attn\"\n",
            world.port
        ),
    );
    let (outcome, events) = world.up(
        &asking,
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    let Outcome::Failed(failure) = outcome else {
        panic!("it should have asked: {events:#?}")
    };
    assert_eq!(failure.kind, FailureKind::Asks);
    assert!(
        failure.message.contains("--accept-review"),
        "{}",
        failure.message
    );
    assert_eq!(
        world.github.requests().len(),
        0,
        "nothing was asked of anyone"
    );
    assert_eq!(outcome_code(&Outcome::Failed(failure)), 3);

    let (outcome, _) = world.up(
        &asking,
        Options {
            dry_run: true,
            accept_review: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Planned);
}

fn outcome_code(outcome: &Outcome) -> i32 {
    outcome.exit_code()
}

/// O16: an engine that takes models by name is asked what it has: a model it lacks is pulled with progress
/// and then loaded, one it has is not pulled, and a daemon that was already running is attached to and left
/// running.
#[test]
fn p3_o16_a_model_an_engine_pulls_is_pulled_when_it_lacks_it_and_loaded() {
    let root = scratch("o16");
    let port = free_port();
    let daemon = FakeOllama::start_on(Some(port)).expect("the daemon's port");
    let files = FileServer::start();
    let name = if std::env::consts::ARCH == "aarch64" {
        "ollama-linux-arm64.tar.zst"
    } else {
        "ollama-linux-amd64.tar.zst"
    };
    files.add(
        &format!("/{name}"),
        tar_zst(&crate::testkit::archives::ollama_linux_tree()),
    );
    let github = FakeGithub::with_releases(json!([{
        "tag_name": "v0.99.0", "draft": false, "prerelease": false,
        "assets": [{ "name": name, "browser_download_url": files.url(&format!("/{name}")), "size": 1000 }]
    }]));
    let config = root.join("config");
    std::fs::create_dir_all(config.join("engines")).unwrap();
    std::fs::write(
        config.join("engines/ollama.toml"),
        format!(
            "schema = 1\nkind = \"engine\"\nid = \"ollama\"\n\n[locate]\nstrategies = [ {{ use = \"setting\", field = \"bin_override\" }}, {{ use = \"installed\", root = \"${{data}}/ollama\", pick = \"newest\", binary = {{ linux = \"bin/ollama\", macos = \"bin/ollama\", windows = \"ollama.exe\" }} }} ]\n\n[acquire.feed]\napi = \"{}\"\n",
            github.base()
        ),
    )
    .unwrap();
    let file = root.join("pulls.toml");
    std::fs::write(
        &file,
        format!(
            "schema = 1\nkind = \"profile\"\nid = \"pulls\"\nname = \"Pulls\"\nengine = \"ollama\"\n\n[[model]]\nollama = \"tiny:1b\"\n\n[values.ollama]\nport = {port}\n"
        ),
    )
    .unwrap();
    let request = Request {
        dirs: ConfigDirs {
            user: Some(config.clone()),
            data: Some(root.join("data")),
            ..ConfigDirs::default()
        },
        target: file.display().to_string(),
        options: Options {
            gpus: Some(Vec::new()),
            ..Options::default()
        },
    };
    let run = Run::start(request);
    let Event::Ready { model, .. } = run.ready() else {
        unreachable!()
    };
    assert_eq!(
        model.as_deref(),
        Some("tiny:1b"),
        "the model that was loaded"
    );
    assert_eq!(
        daemon.requests_to("/api/pull").len(),
        1,
        "it lacked the model, so it was pulled"
    );
    assert!(
        daemon.loaded().is_some_and(
            |m| crate::testkit::fake_ollama::with_tag("tiny:1b") == m || m == "tiny:1b"
        ),
        "{:?}",
        daemon.loaded()
    );
    let (outcome, events) = run.stop();
    assert_eq!(outcome, Outcome::Stopped, "{events:#?}");
    assert!(
        events.iter().any(|e| matches!(e, Event::Progress { .. })
            || matches!(e, Event::Step { text, .. } if text.starts_with("pull "))),
        "the pull was reported"
    );
    // a daemon it only attached to is left as it was found
    assert!(try_get(&daemon.base()).is_ok(), "the daemon still answers");
    assert!(
        root.join("data/ollama").is_dir(),
        "the program was installed from the feed besides"
    );
}

/// O17: a program that crashes after it was started ends the run with the last of what it wrote, and the
/// code that says so.
#[test]
fn p3_o17_an_engine_that_stops_by_itself_ends_the_run_with_what_it_last_said() {
    let world = World::with("o17", r#"{ name = "STANDIN_CRASH_AFTER", value = "2" }"#);
    let run = Run::start(world.request(&world.usual(), world.options()));
    run.ready();
    let mut run = run;
    let (outcome, events) = run.ends_within(30);
    let Outcome::Failed(failure) = &outcome else {
        panic!("it should have ended by itself: {outcome:?}\n{events:#?}")
    };
    assert_eq!(failure.kind, FailureKind::Crashed);
    assert!(
        failure.message.contains("standin: listening"),
        "the last it wrote is in the sentence: {}",
        failure.message
    );
    assert_eq!(outcome.exit_code(), 4);
    assert!(matches!(events.last(), Some(Event::Finished { outcome, .. }) if outcome == "failed"));
}

/// O18: a person who stops it while a model is being fetched stops it at once: nothing is started, and the
/// end says it was interrupted.
#[test]
fn p3_o18_stopping_it_while_a_file_is_fetched_ends_it_at_once() {
    let world = World::new("o18");
    // the model comes out at 32 KB a second: ten seconds for what is 40 KB, and it is stopped in one
    world.files.add_throttled(
        &format!("/{REPO}/resolve/{REV}/{FILE}"),
        model_bytes(FILE),
        4,
    );
    let run = Run::start(world.request(&world.usual(), world.options()));
    run.wait_for(
        "the fetch to begin",
        |e| matches!(e, Event::Step { text, .. } if text.starts_with("fetch ")),
    );
    let began = Instant::now();
    let (outcome, events) = run.stop();
    assert_eq!(outcome, Outcome::Interrupted, "{events:#?}");
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
    assert!(
        !events.iter().any(|e| matches!(e, Event::Ready { .. })),
        "nothing was started"
    );
    assert!(try_get(&world.url("/health")).is_err());
    assert_eq!(outcome.exit_code(), 130);
}

/// O19: when what a configuration needs would not fit, that is said before the first byte is fetched.
#[test]
fn p3_o19_no_room_is_said_before_anything_is_fetched() {
    let world = World::new("o19");
    world.write_documents("", 1);
    let (outcome, events) = world.up(&world.usual(), world.options());
    let Outcome::Failed(failure) = outcome else {
        panic!("there is no room for that: {events:#?}")
    };
    assert_eq!(failure.kind, FailureKind::Needs);
    assert!(failure.message.contains("free"), "{}", failure.message);
    assert_eq!(world.files.requests(), 0, "not even the build was fetched");
}

/// O20: a file of a model that is not here and that nothing says where to get is a sentence that says
/// what to do; one that says where it comes from is fetched from there, when the person has said yes to what
/// a file from outside names.
#[test]
fn p3_o20_a_file_with_no_source_is_a_sentence_and_one_with_a_hint_is_fetched() {
    let world = World::new("o20");
    let nowhere = world.file(
        "nowhere",
        &format!(
            "[[model]]\nfile = \"Hinted.gguf\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    );
    let (outcome, _) = world.up(
        &nowhere,
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    let Outcome::Failed(failure) = outcome else {
        panic!("nothing says where to get it")
    };
    assert_eq!(failure.kind, FailureKind::Needs);
    assert!(
        failure.message.contains("Hinted.gguf") && failure.message.contains("does not say where"),
        "{}",
        failure.message
    );

    let hinted = world.file(
        "hinted",
        &format!(
            "[[model]]\nfile = \"Hinted.gguf\"\nfrom = \"huggingface:{REPO}\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    );
    // a file from outside that names a repository is read by the person first: nothing is fetched without the yes
    let (outcome, _) = world.up(
        &hinted,
        Options {
            prepare_only: true,
            ..world.options()
        },
    );
    let Outcome::Failed(failure) = outcome else {
        panic!("a repository named by a file from outside needs the person's yes")
    };
    assert_eq!(failure.kind, FailureKind::Asks);
    assert!(
        failure.message.contains(&format!("huggingface:{REPO}"))
            && failure.message.contains("--accept-review"),
        "{}",
        failure.message
    );
    assert_eq!(world.files.requests(), 0, "nothing was fetched");
    assert!(!world.data.exists());
    let (outcome, events) = world.up(
        &hinted,
        Options {
            prepare_only: true,
            accept_review: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        std::fs::read(world.data.join("models/Hinted.gguf")).unwrap(),
        model_bytes("Hinted.gguf")
    );
    assert!(world
        .files
        .paths_asked()
        .contains(&format!("/{REPO}/resolve/main/Hinted.gguf")));
}

/// O21: every event is a line of JSON a script can read, and the end is always the last one.
#[test]
fn p3_o21_every_event_is_json_and_the_end_is_the_last() {
    let world = World::new("o21");
    let (_, events) = world.up(
        &world.usual(),
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    for event in &events {
        let line = serde_json::to_string(event).unwrap();
        let back: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(back["event"].is_string(), "{line}");
        assert!(!line.contains('\n'));
    }
    assert!(matches!(events.last(), Some(Event::Finished { .. })));
    let (outcome, events) = world.up(Path::new("/no/such/file.toml"), world.options());
    let Outcome::Failed(failure) = outcome else {
        panic!("a file that is not there")
    };
    assert_eq!(failure.kind, FailureKind::Unreadable);
    assert!(
        matches!(events.last(), Some(Event::Finished { .. })),
        "even a failure ends with the end"
    );
}

/// O22: a configuration that is the person's own, kept by id, is run by its id.
#[test]
fn p3_o22_a_configuration_kept_by_id_is_run_by_id() {
    let world = World::new("o22");
    std::fs::create_dir_all(world.config.join("profiles")).unwrap();
    std::fs::copy(world.usual(), world.config.join("profiles/mine.toml")).unwrap();
    // the file's id is its name: write it as the store does
    let text = std::fs::read_to_string(world.config.join("profiles/mine.toml"))
        .unwrap()
        .replace("id = \"usual\"", "id = \"mine\"");
    std::fs::write(world.config.join("profiles/mine.toml"), text).unwrap();
    let (outcome, events) = world.up(
        Path::new("mine"),
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    assert_eq!(kinds(&events), ["install", "fetch", "start"]);
}

/// A program that stands in for an engine, in a folder of its own with its own copy of the stand-in
/// (so the arguments it was given are written beside it, and the two programs of a pair do not meet).
pub(super) fn program(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("standin.py"), STANDIN).unwrap();
    let run = dir.join("run");
    std::fs::write(
        &run,
        "#!/bin/sh\nexec python3 \"$(dirname \"$0\")/standin.py\" \"$@\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    run
}

/// The document of one of a pair of engines; the second goes with the first.
pub(super) fn pair_document(
    id: &str,
    program: &Path,
    port: u16,
    companion_of: Option<&str>,
) -> String {
    let (of, web, enabled, args) = match companion_of {
        Some(of) => (
            format!("companion_of = \"{of}\"\n"),
            "web_ui = { path = \"/\", label = \"Open the side page\" }\n",
            "\n[[field]]\nid = \"enabled\"\ntype = \"bool\"\ndefault = false\n",
            "[\"--port\", \"${listen_port}\", \"--parent\", \"${parent.url}\"]",
        ),
        None => (String::new(), "", "", "[\"--port\", \"${listen_port}\"]"),
    };
    format!(
        r#"schema = 1
kind = "engine"
id = "{id}"
requires = {{ primitives = ["locate.known_paths@1", "spawn@1", "probe.http@1"] }}

[engine]
name = "{id}"
server_name = "{id}"
{of}
[provides]
apis = ["openai.chat"]
{web}default_port = {port}
needs_model = false

[locate]
strategies = [ {{ use = "known_paths", linux = ["{program}"], macos = ["{program}"] }} ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${{runtime.binary}}"
args = {args}
group = false
stop = {{ policy = "kill" }}

[launch.ready]
probe = {{ use = "http", path = "/health", expect = 200 }}
timeout = "20s"
interval = "100ms"

[[field]]
id = "port"
type = "port"
default = {port}
bind.port = "public"
{enabled}"#,
        program = program.display(),
    )
}

/// O23: a program that goes with an engine is started after the engine runs, told where the engine is,
/// reported with its own address, and stopped before the engine is. A configuration that does not turn it
/// on does not start it.
#[test]
fn p3_o23_the_program_that_goes_with_an_engine_starts_after_it_and_stops_before_it() {
    let root = scratch("o23");
    let config = root.join("config");
    std::fs::create_dir_all(config.join("engines")).unwrap();
    let (main_port, side_port) = (free_port(), free_port());
    let (main_run, side_run) = (program(&root, "main"), program(&root, "side"));
    std::fs::write(
        config.join("engines/pairmain.toml"),
        pair_document("pairmain", &main_run, main_port, None),
    )
    .unwrap();
    std::fs::write(
        config.join("engines/pairside.toml"),
        pair_document("pairside", &side_run, side_port, Some("pairmain")),
    )
    .unwrap();
    let dirs = ConfigDirs {
        user: Some(config.clone()),
        data: Some(root.join("data")),
        ..ConfigDirs::default()
    };
    // these are documents of the person's own that start programs QuickAI did not install: the person says
    // yes to each by its hash, as the window's question is answered
    let registry = crate::config::build_registry(&dirs);
    assert!(
        registry
            .diagnostics
            .iter()
            .all(|d| d.severity != quickai_core::diag::Severity::Error),
        "{:#?}",
        registry.diagnostics
    );
    let confirmed: Vec<String> = ["pairmain", "pairside"]
        .iter()
        .map(|id| quickai_core::guard::document_hash(registry.engine(id).expect("the engine")))
        .collect();
    let file = |name: &str, side: bool| {
        let path = root.join(name);
        std::fs::write(
            &path,
            format!(
                "schema = 1\nkind = \"profile\"\nid = \"{name}\"\nname = \"{name}\"\nengine = \"pairmain\"\n\n[values.pairmain]\nport = {main_port}\n\n[values.pairside]\nenabled = {side}\nport = {side_port}\n"
            ),
        )
        .unwrap();
        path
    };
    let request = |target: &Path| Request {
        dirs: dirs.clone(),
        target: target.display().to_string(),
        options: Options {
            gpus: Some(Vec::new()),
            confirmed: confirmed.clone(),
            ..Options::default()
        },
    };

    // without it: the engine alone
    let run = Run::start(request(&file("alone", false)));
    let Event::Ready { endpoints, .. } = run.ready() else {
        unreachable!()
    };
    assert_eq!(endpoints.len(), 1, "{endpoints:?}");
    assert!(try_get(&format!("http://127.0.0.1:{side_port}/health")).is_err());
    let (outcome, _) = run.stop();
    assert_eq!(outcome, Outcome::Stopped);

    // with it
    let run = Run::start(request(&file("paired", true)));
    let Event::Ready { endpoints, .. } = run.ready() else {
        unreachable!()
    };
    let pages: Vec<&str> = endpoints
        .iter()
        .filter(|e| e.role == "page")
        .map(|e| e.url.as_str())
        .collect();
    assert_eq!(pages, [format!("http://127.0.0.1:{side_port}")]);
    assert_eq!(
        try_get(&format!("http://127.0.0.1:{main_port}/health"))
            .unwrap()
            .status,
        200
    );
    assert_eq!(
        try_get(&format!("http://127.0.0.1:{side_port}/health"))
            .unwrap()
            .status,
        200
    );
    // it was told where the engine is
    let side_argv = std::fs::read_to_string(root.join("side/argv.txt")).unwrap();
    assert!(
        side_argv.contains(&format!("http://127.0.0.1:{main_port}")),
        "{side_argv}"
    );
    let (outcome, events) = run.stop();
    assert_eq!(outcome, Outcome::Stopped, "{events:#?}");
    // started after the engine, stopped before it
    let state_at = |program: &str, state: &str| {
        events
            .iter()
            .position(|e| matches!(e, Event::State { program: p, state: s } if p == program && s == state))
            .unwrap_or_else(|| panic!("{program} {state} in {events:#?}"))
    };
    assert!(state_at("pairmain", "running") < state_at("pairside", "running"));
    assert!(state_at("pairside", "stopped") < state_at("pairmain", "stopped"));
    wait_until(Duration::from_secs(10), "both to be gone", || {
        try_get(&format!("http://127.0.0.1:{main_port}/health")).is_err()
            && try_get(&format!("http://127.0.0.1:{side_port}/health")).is_err()
    });

    // without the person's yes, a document of their own that starts a program QuickAI did not find asks
    let unconfirmed = Request {
        dirs: dirs.clone(),
        target: file("alone", false).display().to_string(),
        options: Options {
            gpus: Some(Vec::new()),
            ..Options::default()
        },
    };
    let mut run = Run::start(unconfirmed);
    let first = run.wait_for("the question or the start", |e| {
        matches!(e, Event::Finished { .. } | Event::Ready { .. })
    });
    if matches!(first, Event::Ready { .. }) {
        let _ = run.stop();
        panic!("a program QuickAI did not install was started on the word of a document, without the person's yes");
    }
    let (outcome, events) = run.join();
    let Outcome::Failed(failure) = outcome else {
        panic!("it should have asked: {events:#?}")
    };
    assert_eq!(failure.kind, FailureKind::Asks);
    assert!(
        failure.message.contains("--allow-program"),
        "{}",
        failure.message
    );
    // and the person's yes is what the sentence says to type: the first twelve digits of the hash it gives
    let typed: String = failure
        .message
        .split("--allow-program ")
        .nth(1)
        .expect("the sentence says what to type")
        .chars()
        .take(12)
        .collect();
    assert!(typed.chars().all(|c| c.is_ascii_hexdigit()), "{typed}");
    let run = Run::start(Request {
        dirs: dirs.clone(),
        target: file("alone", false).display().to_string(),
        options: Options {
            gpus: Some(Vec::new()),
            confirmed: vec![typed],
            ..Options::default()
        },
    });
    run.ready();
    let (outcome, _) = run.stop();
    assert_eq!(outcome, Outcome::Stopped);
    wait_until(Duration::from_secs(10), "the engine to be gone", || {
        try_get(&format!("http://127.0.0.1:{main_port}/health")).is_err()
    });
}

/// O24: a build the file names that is older than the newest few the list shows is looked for in as many
/// releases as the host gives, and found there.
#[test]
fn p3_o24_a_build_older_than_the_newest_few_is_found_in_the_wider_list() {
    let world = World::with_tags(
        "o24",
        &["b107", "b106", "b105", "b104", "b103", "b102", "b101"],
    );
    let old = format!(
        "b101-{}",
        asset_name("b101", None).trim_end_matches(".tar.gz")
    );
    let file = world.file(
        "older",
        &format!(
            "[runtime]\nbuild = \"{old}\"\n\n[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    );
    let (outcome, events) = world.up(
        &file,
        Options {
            dry_run: true,
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    assert!(
        plan_of(&events)[0].1.contains(" b101-"),
        "{:?}",
        plan_of(&events)[0]
    );
    assert_eq!(
        world.github.requests().len(),
        2,
        "the newest few, and then as many as the host gives"
    );
    assert!(
        world.github.requests()[1].0.contains("per_page=100"),
        "{:?}",
        world.github.requests()
    );
}

/// O25: what is said on the command line wins over the file: the build, the port and the folder the models
/// are kept in; the engine is started on that port, with the model from that folder.
#[test]
fn p3_o25_what_is_said_on_the_command_line_wins_over_the_file() {
    let world = World::new("o25");
    let named = format!(
        "b99-{}",
        asset_name("b99", None).trim_end_matches(".tar.gz")
    );
    let newest = format!(
        "b100-{}",
        asset_name("b100", None).trim_end_matches(".tar.gz")
    );
    let file = world.file(
        "said",
        &format!(
            "[runtime]\nbuild = \"{named}\"\n\n[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    );
    let other_port = free_port();
    let big = world.scratch.join("big-disk");
    let (outcome, events) = world.up(
        &file,
        Options {
            dry_run: true,
            build: Some(newest.clone()),
            port: Some(other_port),
            ..world.options()
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    let plan = plan_of(&events);
    assert!(plan[0].1.contains(&format!(" {newest} ")), "{}", plan[0].1);
    assert!(
        plan[2].1.contains(&format!("127.0.0.1:{other_port}")),
        "{}",
        plan[2].1
    );

    // and it is the folder that is used, to fetch into and to start from
    let run = Run::start(world.request(
        &file,
        Options {
            port: Some(other_port),
            models_dir: Some(big.clone()),
            ..world.options()
        },
    ));
    run.ready();
    assert_eq!(std::fs::read(big.join(FILE)).unwrap(), model_bytes(FILE));
    assert!(
        !world.data.join("models").exists(),
        "not the engine's own folder"
    );
    let argv = world.argv();
    let at = argv.iter().position(|a| a == "--model").unwrap();
    assert_eq!(argv[at + 1], big.join(FILE).display().to_string());
    let at = argv.iter().position(|a| a == "--port").unwrap();
    assert_eq!(argv[at + 1], other_port.to_string());
    let (outcome, _) = run.stop();
    assert_eq!(outcome, Outcome::Stopped);
}
