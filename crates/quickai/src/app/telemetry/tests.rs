//! The window's telemetry (WP4.42; group RP of docs/reports-and-community.md, section 16): the notice, its buttons and
//! when it is not shown, the Privacy page, the crash window at Off, the events the window records, and Report a problem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use quickai_core::telemetry::{schema, Batch, Level};
use quickai_runtime::config::{build_registry, ConfigDirs};
use quickai_runtime::settings::Settings;
use quickai_runtime::telemetry::{self as rt, Cap, CapBy, Setup, Telemetry};
use quickai_runtime::testkit::fake_telemetry::FakeApi;

use super::*;
use crate::app::App;

const GB: u64 = 1024 * 1024 * 1024;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "quickai-telemetry-ui-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A window on `settings`, with telemetry started in its data folder as `App::new` starts it, against `api`.
fn app_with(dir: &Path, settings: Settings, caps: Vec<Cap>, api: &str) -> App {
    let chosen = settings.telemetry_level();
    let mut app = App::build_in(
        settings,
        build_registry(&ConfigDirs::default()),
        16 * GB,
        Vec::new(),
        Some(dir.join("settings.json")),
        Some(dir.join("data").display().to_string()),
    );
    let time = Arc::new(AtomicU64::new(1_793_351_564));
    app.telemetry = Telemetry::start(Setup {
        dir: dir.join("data").join("telemetry"),
        chosen,
        caps,
        endpoint: api.to_string(),
        identity: rt::Identity::detect_with(16 * GB, &[]),
        clock: Arc::new(move || time.load(Ordering::Relaxed)),
    });
    app.after_telemetry_start(chosen);
    app
}

fn frame_with(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(980.0, 680.0),
        )),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        app.show_window(ui);
        app.telemetry_frame();
        app.show_settings_window(ui.ctx());
        app.show_telemetry_log(ui.ctx());
        app.show_crash_offer(ui.ctx());
        app.show_report_problem(ui.ctx());
    });
    output.textures_delta.clear();
    output
}

/// Every text drawn, with where (a few frames, so that what is laid out once is in place).
fn texts(app: &mut App, ctx: &egui::Context) -> BTreeMap<String, egui::Rect> {
    fn walk(shape: &egui::epaint::Shape, into: &mut BTreeMap<String, egui::Rect>) {
        match shape {
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
            egui::epaint::Shape::Text(t) => {
                into.insert(
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                );
            }
            _ => {}
        }
    }
    let mut output = frame_with(app, ctx, Vec::new());
    for _ in 0..3 {
        output = frame_with(app, ctx, Vec::new());
    }
    let mut into = BTreeMap::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut into);
    }
    into
}

fn click(app: &mut App, ctx: &egui::Context, text: &str) {
    let found = texts(app, ctx);
    let at = found
        .get(text)
        .unwrap_or_else(|| panic!("{text:?} is not drawn: {:?}", found.keys()))
        .center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [
        vec![egui::Event::PointerMoved(at)],
        vec![button(true)],
        vec![button(false)],
        Vec::new(),
    ] {
        frame_with(app, ctx, events);
    }
}

fn saved(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
}

fn names(app: &App) -> Vec<String> {
    app.telemetry
        .queued()
        .into_iter()
        .map(|e| e.event)
        .collect()
}

#[test]
fn rp_w01_the_notice_asks_once_and_a_button_chooses_the_level() {
    let dir = scratch("notice");
    let api = FakeApi::start();
    let mut app = app_with(&dir, Settings::default(), vec![], &api.url);
    let ctx = egui::Context::default();
    assert!(app.tel.notice);
    let found = texts(&mut app, &ctx);
    for text in [
        "Help make QuickAI better?",
        "All",
        "Errors and crashes",
        "Crashes only",
        "Off",
        "What is sent",
    ] {
        assert!(found.contains_key(text), "{text}: {:?}", found.keys());
    }
    // under the menu bar, over nothing: the notice is above the page
    assert!(found["Help make QuickAI better?"].top() < 80.0);
    assert!(
        app.telemetry.queued().is_empty(),
        "nothing is kept before an answer"
    );
    click(&mut app, &ctx, "Crashes only");
    assert!(!app.tel.notice);
    assert_eq!(app.telemetry.level(), Level::Crash);
    let values = &saved(&dir)["values"];
    assert_eq!(values["app.telemetry_level"], "crash");
    assert_eq!(values["app.telemetry_asked"], schema::CONSENT_VERSION);
    // the next start asks nothing
    let settings = Settings::load_from(&dir.join("settings.json"));
    let again = app_with(&dir, settings, vec![], &api.url);
    assert!(!again.tel.notice);
    assert_eq!(again.telemetry.level(), Level::Crash);
    assert!(api.received().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rp_w02_turned_off_for_good_nothing_is_asked_and_a_lower_cap_offers_only_what_it_allows() {
    let dir = scratch("caps");
    let api = FakeApi::start();
    let off = app_with(
        &dir,
        Settings::default(),
        vec![Cap {
            level: Level::Off,
            by: CapBy::Environment,
        }],
        &api.url,
    );
    assert!(!off.tel.notice, "the environment said off");
    let mut capped = app_with(
        &dir,
        Settings::default(),
        vec![Cap {
            level: Level::Error,
            by: CapBy::Package,
        }],
        &api.url,
    );
    let ctx = egui::Context::default();
    let found = texts(&mut capped, &ctx);
    assert!(found.contains_key("Errors and crashes"));
    assert!(
        !found.contains_key("All"),
        "a level above the cap is not offered: {:?}",
        found.keys()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rp_w03_a_yes_given_to_less_is_asked_again_and_an_off_is_not() {
    let dir = scratch("again");
    let api = FakeApi::start();
    // a yes given before what is collected grew
    let file = dir.join("older.json");
    std::fs::write(&file, r#"{"port":8080,"ctx_size":4096,"n_gpu_layers":999,"values":{"app.telemetry_level":"all","app.telemetry_asked":0}}"#)
        .unwrap();
    let older = Settings::load_from(&file);
    assert_eq!(older.telemetry_level(), Some(Level::All));
    assert!(app_with(&dir, older, vec![], &api.url).tel.notice);
    let mut off = Settings::default();
    off.set_telemetry_level(Level::Off);
    assert!(!app_with(&dir, off, vec![], &api.url).tel.notice);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rp_w04_the_privacy_page_is_in_the_settings_with_the_levels_and_the_install_id() {
    let dir = scratch("privacy");
    let api = FakeApi::start();
    let mut app = app_with(&dir, Settings::default(), vec![], &api.url);
    app.tel.notice = false;
    app.settings_page = SettingsPage::Privacy;
    app.show_settings = true;
    let ctx = egui::Context::default();
    let found = texts(&mut app, &ctx);
    for text in [
        "Privacy",
        "Telemetry",
        "Errors and crashes",
        "What EverybodyAI holds",
        "none yet: it is made when the first report is sent",
        "Show what is sent",
    ] {
        assert!(found.contains_key(text), "{text}: {:?}", found.keys());
    }
    click(&mut app, &ctx, "Show what is sent");
    assert!(app.tel.log_open);
    let found = texts(&mut app, &ctx);
    assert!(
        found.contains_key("Nothing has been sent from this computer."),
        "{:?}",
        found.keys()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rp_w05_the_window_records_the_windows_opened_a_run_and_how_its_engine_ended() {
    let dir = scratch("events");
    let api = FakeApi::start();
    let mut settings = Settings::default();
    settings.set_telemetry_level(Level::All);
    let mut app = app_with(&dir, settings, vec![], &api.url);
    assert_eq!(names(&app), vec!["session.start"]);
    app.show_about = true;
    app.telemetry_frame();
    app.telemetry_frame();
    app.note_run_state(ServerState::Starting);
    app.note_run_state(ServerState::Running);
    app.note_log_line("Server process exited unexpectedly (exit status: signal: 9 (SIGKILL)).");
    app.note_run_state(ServerState::Crashed);
    assert_eq!(
        names(&app),
        vec![
            "session.start",
            "window.open",
            "run.start",
            "run.ready",
            "error.engine_exit"
        ],
        "a window counted once, while it stays open"
    );
    let queued = app.telemetry.queued();
    assert_eq!(queued[1].fields["window"], "about");
    assert_eq!(queued[2].fields["engine"], "llamacpp");
    assert_eq!(queued[2].fields["where"], "local");
    let exit = &queued[4];
    assert_eq!(exit.fields["status"], "signal 9");
    assert_eq!(exit.fields["phase"], "serving");
    for event in &queued {
        assert_eq!(schema::check(event), Ok(()), "{event:?}");
    }
    // a feature on, counted once
    app.enable_traffic_proxy = true;
    app.telemetry_frame();
    app.telemetry_frame();
    assert_eq!(
        names(&app).iter().filter(|n| *n == "feature.use").count(),
        1
    );
    // closing says how long it was open
    app.telemetry_closing();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rp_w06_at_crashes_only_nothing_but_crashes_is_kept() {
    let dir = scratch("crash-level");
    let api = FakeApi::start();
    let mut settings = Settings::default();
    settings.set_telemetry_level(Level::Crash);
    let mut app = app_with(&dir, settings, vec![], &api.url);
    app.show_about = true;
    app.telemetry_frame();
    app.note_run_state(ServerState::Starting);
    app.note_run_state(ServerState::Crashed);
    assert!(names(&app).is_empty(), "{:?}", names(&app));
    let _ = std::fs::remove_dir_all(&dir);
}

fn crash_file(dir: &Path) -> PathBuf {
    let identity = rt::Identity::detect_with(16 * GB, &[]);
    let event = rt::crash_event(
        "index out of bounds",
        Some(("crates/quickai/src/app/shell.rs", 10, 5)),
        "main",
        "",
        &rt::CrashContext::default(),
        3,
        1_793_351_564,
        &quickai_core::telemetry::clean::Cleaner::default(),
    )
    .unwrap();
    let batch = Batch::new(
        rt::random_id(),
        identity.common("", "s", Level::Off),
        vec![event],
    );
    rt::write_report(&dir.join("data").join("telemetry").join("crashes"), &batch).unwrap()
}

fn wait_for_job(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.tel.job.is_some() {
        assert!(Instant::now() < deadline, "the request did not end");
        std::thread::sleep(Duration::from_millis(20));
        app.poll_telemetry_job();
    }
}

#[test]
fn rp_w07_at_off_a_crash_is_offered_once_and_sent_only_when_the_person_says() {
    let dir = scratch("crash-offer");
    let api = FakeApi::start();
    let first = crash_file(&dir);
    let mut app = app_with(&dir, Settings::default(), vec![], &api.url);
    app.tel.notice = false;
    assert_eq!(app.tel.crash_offer.as_deref(), Some(first.as_path()));
    let ctx = egui::Context::default();
    let found = texts(&mut app, &ctx);
    for text in [
        CRASH_TITLE,
        "index out of bounds",
        "Send this report",
        "Don't send",
    ] {
        assert!(found.contains_key(text), "{text}: {:?}", found.keys());
    }
    click(&mut app, &ctx, "Don't send");
    assert_eq!(app.tel.crash_offer, None);
    assert!(!first.exists(), "set aside");
    assert!(api.received().is_empty(), "not sent");

    let second = crash_file(&dir);
    let mut app = app_with(&dir, Settings::default(), vec![], &api.url);
    app.tel.notice = false;
    click(&mut app, &ctx, "Send this report");
    wait_for_job(&mut app);
    assert_eq!(app.tel.crash_offer, None);
    let received = api.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/v1/crash");
    let batch: Batch = serde_json::from_str(&received[0].body).unwrap();
    assert_eq!(schema::check_batch(&batch), Ok(()));
    assert!(!second.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rp_w08_report_a_problem_opens_an_issue_with_the_text_and_the_details_and_sends_us_nothing() {
    let dir = scratch("problem");
    let api = FakeApi::start();
    let mut app = app_with(&dir, Settings::default(), vec![], &api.url);
    app.tel.notice = false;
    app.open_report_problem();
    let details = &app.tel.problem.as_ref().unwrap().details;
    assert!(
        details.starts_with(&format!("QuickAI {}", env!("CARGO_PKG_VERSION"))),
        "{details}"
    );
    assert!(details.contains("16 GB"));
    let ctx = egui::Context::default();
    let found = texts(&mut app, &ctx);
    assert!(found.contains_key("Open on GitHub"), "{:?}", found.keys());
    let address = issue_address("Crash on start", "It stops & says \"no\"\nline two");
    assert_eq!(
        address,
        "https://github.com/everybodyai/quickai/issues/new?title=Crash%20on%20start&body=It%20stops%20%26%20says%20%22no%22%0Aline%20two"
    );
    assert!(api.received().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rp_w09_a_failed_download_says_why_in_a_word() {
    for (message, reason) in [
        ("The file's SHA-256 does not match the list", "checksum"),
        (
            "write failed: No space left on device (os error 28)",
            "disk_full",
        ),
        ("HTTP 404 from huggingface.co", "http_4xx"),
        ("server answered status 503", "http_5xx"),
        ("could not connect: timed out", "network"),
        ("something odd", "other"),
    ] {
        assert_eq!(download_reason(message), reason, "{message}");
    }
}

#[test]
fn rp_w10_a_window_of_a_test_records_nothing() {
    let dir = scratch("inert");
    let mut app = App::build_in(
        Settings::default(),
        build_registry(&ConfigDirs::default()),
        16 * GB,
        Vec::new(),
        Some(dir.join("settings.json")),
        Some(dir.join("data").display().to_string()),
    );
    assert!(
        !app.tel.notice,
        "the notice is the real start's, not a test's"
    );
    app.show_about = true;
    app.telemetry_frame();
    assert!(app.telemetry.queued().is_empty());
    assert!(!dir.join("data").join("telemetry").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
