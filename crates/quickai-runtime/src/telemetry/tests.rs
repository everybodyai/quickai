//! Telemetry against a fake of the API (group RP of docs/reports-and-community.md, section 16): nothing before an
//! answer, each level's events only, the waits, every answer of the server, crashes, unclean exits and deletion.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use quickai_core::telemetry::{schema, Batch};

use super::*;
use crate::testkit::fake_telemetry::{FakeApi, Reply};

const T0: u64 = 1_793_351_564;

struct World {
    _scratch: crate::testkit::Scratch,
    dir: PathBuf,
    api: FakeApi,
    time: Arc<AtomicU64>,
}

impl World {
    fn new(name: &str) -> World {
        let scratch = crate::testkit::scratch(&format!("telemetry-{name}"));
        let dir = scratch.to_path_buf();
        World {
            _scratch: scratch,
            dir,
            api: FakeApi::start(),
            time: Arc::new(AtomicU64::new(T0)),
        }
    }

    fn start(&self, chosen: Option<Level>, caps: Vec<Cap>) -> Telemetry {
        let time = self.time.clone();
        Telemetry::start(Setup {
            dir: self.dir.clone(),
            chosen,
            caps,
            endpoint: self.api.url.clone(),
            identity: Identity::detect(32 * 1024 * 1024 * 1024, &[crate::gpu::GpuVendor::Amd]),
            clock: Arc::new(move || time.load(Ordering::Relaxed)),
        })
    }

    fn pass(&self, seconds: u64) {
        self.time.fetch_add(seconds, Ordering::Relaxed);
    }

    fn batches(&self, path: &str) -> Vec<Batch> {
        self.api
            .received()
            .into_iter()
            .filter(|r| r.path == path)
            .map(|r| serde_json::from_str(&r.body).expect("a batch"))
            .collect()
    }
}

fn some_events(t: &Telemetry) {
    t.record(t.event("session.unclean_exit"));
    t.record(t.event("error.stall").with("seconds", "5-10"));
    t.record(
        t.event("error.download")
            .with("what", "model")
            .with("source", "huggingface")
            .with("reason", "network"),
    );
}

#[test]
fn nothing_is_sent_before_the_person_answers_or_at_off() {
    let w = World::new("unanswered");
    for chosen in [None, Some(Level::Off)] {
        let t = w.start(chosen, vec![]);
        some_events(&t);
        assert!(t.queued().is_empty(), "nothing is queued at {chosen:?}");
        w.pass(BATCH_EVERY + FIRST_SEND_AFTER);
        assert_eq!(t.send_due(true, SEND_TIMEOUT), None);
        t.end_session();
    }
    assert!(w.api.received().is_empty(), "no request at all");
    assert_eq!(
        std::fs::read_to_string(w.dir.join("queue.jsonl")).unwrap_or_default(),
        ""
    );
    assert!(!w.dir.join("install-id").exists(), "no install id is made");
}

#[test]
fn each_level_queues_its_own_events_and_those_below_only() {
    let w = World::new("levels");
    let names = |t: &Telemetry| t.queued().into_iter().map(|e| e.event).collect::<Vec<_>>();
    let t = w.start(Some(Level::Crash), vec![]);
    some_events(&t);
    assert!(names(&t).is_empty());
    t.set_chosen(Level::Error);
    some_events(&t);
    assert_eq!(
        names(&t),
        vec!["session.unclean_exit", "error.stall", "error.download"]
    );
    // a lower level drops what it no longer allows
    t.set_chosen(Level::Crash);
    assert!(names(&t).is_empty());
    some_events(&t);
    t.set_chosen(Level::Off);
    assert!(names(&t).is_empty());
}

#[test]
fn a_cap_holds_the_level_down_whatever_was_chosen() {
    let w = World::new("caps");
    let t = w.start(
        Some(Level::Error),
        vec![Cap {
            level: Level::Crash,
            by: CapBy::Package,
        }],
    );
    assert_eq!(t.level(), Level::Crash);
    assert_eq!(t.cap(), Level::Crash);
    some_events(&t);
    assert!(t.queued().is_empty());
    let dir = crate::testkit::scratch("telemetry-package");
    std::fs::write(
        dir.join("telemetry.toml"),
        "# set by the IT department\nlevel = \"off\"\n",
    )
    .unwrap();
    assert_eq!(package_cap(&dir), Some(Level::Off));
    assert_eq!(package_cap(&w.dir), None);
}

#[test]
fn the_first_batch_waits_two_minutes_then_goes_with_the_install_id_and_is_logged() {
    let w = World::new("first");
    let t = w.start(Some(Level::Error), vec![]);
    some_events(&t);
    assert_eq!(
        t.send_due(false, SEND_TIMEOUT),
        None,
        "not in the first two minutes"
    );
    w.pass(FIRST_SEND_AFTER);
    assert_eq!(t.send_due(false, SEND_TIMEOUT), Some(Answer::Sent));
    let received = w.api.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].method, "POST");
    assert_eq!(received[0].path, "/v1/events");
    assert_eq!(
        received[0].user_agent.as_deref(),
        Some(concat!("quickai/", env!("CARGO_PKG_VERSION")))
    );
    let batch: Batch = serde_json::from_str(&received[0].body).unwrap();
    assert_eq!(
        schema::check_batch(&batch),
        Ok(()),
        "what is sent passes the schema"
    );
    assert_eq!(batch.events.len(), 3);
    assert_eq!(batch.common.level, "error");
    assert_eq!(batch.common.memory_gb, 32);
    assert_eq!(
        received[0].install.as_deref(),
        Some(batch.common.install.as_str())
    );
    assert_eq!(
        t.install_id().as_deref(),
        Some(batch.common.install.as_str())
    );
    assert!(t.queued().is_empty());
    let log = t.log_text();
    assert!(log.contains("POST /v1/events  3 event(s)  taken"), "{log}");
    assert!(log.contains(&batch.id));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(w.dir.join("install-id"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "the install id is the person's to read only"
        );
    }
}

#[test]
fn after_a_batch_the_next_waits_for_a_hundred_events_or_six_hours() {
    let w = World::new("due");
    let t = w.start(Some(Level::Error), vec![]);
    w.pass(FIRST_SEND_AFTER);
    some_events(&t);
    assert_eq!(t.send_due(false, SEND_TIMEOUT), Some(Answer::Sent));
    t.record(t.event("error.stall").with("seconds", "5-10"));
    w.pass(600);
    assert_eq!(
        t.send_due(false, SEND_TIMEOUT),
        None,
        "one event, ten minutes later"
    );
    for _ in 0..BATCH_WHEN {
        t.record(t.event("error.stall").with("seconds", "5-10"));
    }
    assert_eq!(
        t.send_due(false, SEND_TIMEOUT),
        Some(Answer::Sent),
        "a hundred wait"
    );
    t.record(t.event("error.stall").with("seconds", "10-30"));
    w.pass(BATCH_EVERY);
    assert_eq!(
        t.send_due(false, SEND_TIMEOUT),
        Some(Answer::Sent),
        "six hours later"
    );
    assert_eq!(w.batches("/v1/events").len(), 3);
}

#[test]
fn a_refused_batch_is_dropped_and_a_blocked_or_retired_install_stops_sending() {
    let w = World::new("answers");
    let t = w.start(Some(Level::Error), vec![]);
    w.pass(FIRST_SEND_AFTER);
    some_events(&t);
    w.api.reply(Reply::status(400));
    assert_eq!(t.send_due(true, SEND_TIMEOUT), Some(Answer::Refused));
    assert!(t.queued().is_empty(), "it would never pass");

    some_events(&t);
    w.api.reply(Reply::status(403));
    assert_eq!(t.send_due(true, SEND_TIMEOUT), Some(Answer::Blocked));
    assert_eq!(t.queued().len(), 3, "kept");
    w.pass(BLOCKED_FOR - 60);
    assert_eq!(t.send_due(true, SEND_TIMEOUT), None, "nothing for 30 days");
    w.pass(120);
    w.api.reply(Reply::status(410));
    assert_eq!(t.send_due(true, SEND_TIMEOUT), Some(Answer::Retired));
    w.pass(BATCH_EVERY * 10);
    assert_eq!(
        t.send_due(true, SEND_TIMEOUT),
        None,
        "this version is no longer collected"
    );
    assert_eq!(
        t.send_state().retired_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(w.api.received().len(), 3);
}

#[test]
fn a_failure_waits_an_hour_then_six_then_a_day_and_what_waited_three_days_goes() {
    let w = World::new("waits");
    let t = w.start(Some(Level::Error), vec![]);
    w.pass(FIRST_SEND_AFTER);
    some_events(&t);
    for wait in WAITS {
        w.api.reply(Reply::status(503));
        assert_eq!(t.send_due(true, SEND_TIMEOUT), Some(Answer::Later(None)));
        w.pass(wait - 1);
        assert_eq!(t.send_due(true, SEND_TIMEOUT), None, "waiting {wait} s");
        w.pass(1);
    }
    // the fourth and fifth failures wait a day each; after the fifth the events have waited more than three days
    for _ in 0..2 {
        w.api.reply(Reply::status(503));
        assert_eq!(t.send_due(true, SEND_TIMEOUT), Some(Answer::Later(None)));
        w.pass(WAITS[2]);
    }
    assert_eq!(t.send_due(true, SEND_TIMEOUT), None);
    assert!(
        t.queued().is_empty(),
        "what waited more than three days was dropped"
    );
}

#[test]
fn too_many_waits_what_the_server_says_and_a_success_resets_the_waits() {
    let w = World::new("retry-after");
    let t = w.start(Some(Level::Error), vec![]);
    w.pass(FIRST_SEND_AFTER);
    some_events(&t);
    w.api.reply(Reply {
        status: 429,
        retry_after: Some(90),
        body: String::new(),
    });
    assert_eq!(
        t.send_due(true, SEND_TIMEOUT),
        Some(Answer::Later(Some(90)))
    );
    w.pass(89);
    assert_eq!(t.send_due(true, SEND_TIMEOUT), None);
    w.pass(1);
    assert_eq!(t.send_due(true, SEND_TIMEOUT), Some(Answer::Sent));
    assert_eq!(t.send_state().failures, 0);
}

#[test]
fn the_queue_and_the_state_survive_a_restart() {
    let w = World::new("restart");
    let t = w.start(Some(Level::Error), vec![]);
    some_events(&t);
    drop(t);
    let t = w.start(Some(Level::Error), vec![]);
    assert_eq!(t.queued().len(), 3);
    // a level set lower while the app was closed drops what it does not allow
    let t = w.start(Some(Level::Crash), vec![]);
    assert!(t.queued().is_empty());
}

#[test]
fn closing_sends_what_waits_even_in_the_first_two_minutes_and_takes_the_marker_away() {
    let w = World::new("closing");
    let t = w.start(Some(Level::Error), vec![]);
    let marker = w.dir.join(format!("running-{}.json", std::process::id()));
    assert!(marker.exists());
    t.record(t.event("error.stall").with("seconds", "5-10"));
    t.end_session();
    assert_eq!(w.batches("/v1/events").len(), 1);
    assert!(!marker.exists());
}

#[test]
fn a_session_left_open_is_an_unclean_exit_and_one_that_crashed_is_not() {
    let w = World::new("unclean");
    let gone = u32::MAX - 7;
    let marker = |session: &str, pid: u32| {
        std::fs::write(
            w.dir.join(format!("running-{pid}.json")),
            format!(r#"{{"session":"{session}","pid":{pid},"started":1}}"#),
        )
        .unwrap();
    };
    marker("left-open", gone);
    marker("crashed", gone - 1);
    let identity = Identity::detect(8 << 30, &[]);
    let crash = Batch::new(
        "c1".into(),
        identity.common("", "crashed", Level::Off),
        vec![],
    );
    write_report(&w.dir.join("crashes"), &crash).unwrap();
    let t = w.start(Some(Level::Error), vec![]);
    assert_eq!(t.unclean_sessions(), 1);
    assert_eq!(
        t.queued().into_iter().map(|e| e.event).collect::<Vec<_>>(),
        vec!["session.unclean_exit"]
    );
    assert!(!w.dir.join(format!("running-{gone}.json")).exists());
    assert!(!w.dir.join(format!("running-{}.json", gone - 1)).exists());
    // the next start finds none
    assert_eq!(w.start(Some(Level::Error), vec![]).unclean_sessions(), 0);
}

fn a_crash(w: &World) -> PathBuf {
    let identity = Identity::detect(8 << 30, &[]);
    let event = crash_event(
        "index out of bounds",
        Some(("crates/quickai/src/app/shell.rs", 10, 5)),
        "main",
        "",
        &CrashContext::default(),
        3,
        T0,
        &Cleaner::default(),
    )
    .unwrap();
    write_report(
        &w.dir.join("crashes"),
        &Batch::new(
            random_id(),
            identity.common("", "s", Level::Off),
            vec![event],
        ),
    )
    .unwrap()
}

#[test]
fn at_off_a_crash_is_kept_and_sent_only_when_the_person_says() {
    let w = World::new("crash-off");
    let t = w.start(None, vec![]);
    let file = a_crash(&w);
    assert_eq!(t.pending_crashes(), vec![file.clone()]);
    t.send_due(true, SEND_TIMEOUT);
    assert!(w.api.received().is_empty(), "not sent by itself at Off");
    assert_eq!(t.send_crash(&file), Ok(()));
    let sent = w.batches("/v1/crash");
    assert_eq!(sent.len(), 1);
    assert_eq!(schema::check_batch(&sent[0]), Ok(()));
    assert_eq!(sent[0].common.level, "off");
    assert!(!sent[0].common.install.is_empty());
    assert!(!file.exists(), "sent, it goes");
    // one set aside is not pending any more, and is kept for a GitHub issue
    let other = a_crash(&w);
    t.keep_crash(&other);
    assert!(t.pending_crashes().is_empty());
    assert!(w
        .dir
        .join("crashes")
        .join("kept")
        .join(other.file_name().unwrap())
        .exists());
}

#[test]
fn at_crashes_only_a_crash_is_sent_by_itself() {
    let w = World::new("crash-on");
    let file = a_crash(&w);
    let t = w.start(Some(Level::Crash), vec![]);
    t.send_due(false, SEND_TIMEOUT);
    assert_eq!(w.batches("/v1/crash").len(), 1);
    assert!(!file.exists());
    assert!(t.log_text().contains("POST /v1/crash"));
}

#[test]
fn turned_off_for_good_a_crash_cannot_be_sent_even_when_asked() {
    let w = World::new("crash-capped");
    let t = w.start(
        None,
        vec![Cap {
            level: Level::Off,
            by: CapBy::Environment,
        }],
    );
    let file = a_crash(&w);
    assert!(t.send_crash(&file).is_err());
    assert!(w.api.received().is_empty());
}

#[test]
fn deleting_what_is_held_asks_the_server_then_forgets_the_install_id_and_the_queue() {
    let w = World::new("delete");
    let t = w.start(Some(Level::Error), vec![]);
    assert!(t.delete_held().is_err(), "nothing was ever sent");
    w.pass(FIRST_SEND_AFTER);
    some_events(&t);
    t.send_due(true, SEND_TIMEOUT);
    let first = t.install_id().unwrap();
    w.api.reply(Reply {
        status: 200,
        retry_after: None,
        body: r#"{"events":3}"#.into(),
    });
    assert_eq!(t.download_held().as_deref(), Ok(r#"{"events":3}"#));
    some_events(&t);
    w.api.reply(Reply {
        status: 200,
        retry_after: None,
        body: r#"{"deleted":true}"#.into(),
    });
    assert_eq!(t.delete_held().as_deref(), Ok(r#"{"deleted":true}"#));
    let received = w.api.received();
    let last = received.last().unwrap();
    assert_eq!(
        (last.method.as_str(), last.path.as_str()),
        ("DELETE", "/v1/me")
    );
    assert_eq!(last.install.as_deref(), Some(first.as_str()));
    assert_eq!(t.install_id(), None);
    assert!(t.queued().is_empty());
    t.send_due(true, SEND_TIMEOUT);
    assert_ne!(
        t.install_id(),
        Some(first),
        "what is sent later has a new id"
    );
}

#[test]
fn the_preview_is_the_next_batch_and_an_inert_telemetry_does_nothing() {
    let w = World::new("preview");
    let t = w.start(Some(Level::Error), vec![]);
    some_events(&t);
    let preview: serde_json::Value = serde_json::from_str(&t.preview()).unwrap();
    assert_eq!(preview["events"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        preview["common"]["install"],
        "(made when the first report is sent)"
    );
    let inert = Telemetry::inert();
    inert.record(inert.event("error.stall").with("seconds", "5-10"));
    assert_eq!(inert.level(), Level::Off);
    assert!(inert.queued().is_empty());
    assert_eq!(inert.send_due(true, SEND_TIMEOUT), None);
}

#[test]
fn the_queue_keeps_its_last_megabyte() {
    let w = World::new("cap");
    let t = w.start(Some(Level::Error), vec![]);
    for _ in 0..12_000 {
        t.record(t.event("error.stall").with("seconds", "10-30"));
    }
    let size = std::fs::metadata(w.dir.join("queue.jsonl")).unwrap().len() as usize;
    assert!(size <= QUEUE_MOST_BYTES, "{size} bytes");
    let kept = t.queued().len();
    assert!(kept < 12_000 && kept > 8_000, "{kept} kept");
    // what is on disk is what is held
    drop(t);
    assert_eq!(w.start(Some(Level::Error), vec![]).queued().len(), kept);
}

#[test]
fn random_ids_are_uuids_and_differ() {
    let a = random_id();
    let b = random_id();
    assert_ne!(a, b);
    assert_eq!(a.len(), 36);
    assert_eq!(a.as_bytes()[14], b'4', "version 4");
}

#[test]
fn the_panic_hook_writes_a_clean_report_and_sends_nothing() {
    let w = World::new("hook");
    let t = w.start(Some(Level::Error), vec![]);
    t.context().lock().unwrap().engine = Some("llamacpp".into());
    t.install_panic_hook(Cleaner::new(Some("/home/grace"), None, None, &[]));
    let _ = std::thread::Builder::new()
        .name("telemetry-test-panic".into())
        .spawn(|| panic!("cannot open /home/grace/notes.txt"))
        .unwrap()
        .join();
    // the default hook again, for the tests that panic on purpose
    let _ = std::panic::take_hook();
    let ours: Vec<Batch> = t
        .pending_crashes()
        .iter()
        .filter_map(|p| Telemetry::read_crash(p))
        .filter(|b| b.events[0].fields["thread"] == "telemetry-test-panic")
        .collect();
    assert_eq!(ours.len(), 1);
    let event = &ours[0].events[0];
    assert_eq!(event.fields["message"], "cannot open ~/notes.txt");
    assert_eq!(event.fields["engine"], "llamacpp");
    assert!(event.fields["location"]
        .as_str()
        .unwrap()
        .starts_with("crates/quickai-runtime/src/telemetry/tests.rs:"));
    assert!(w.api.received().is_empty(), "a panic sends nothing");
}
