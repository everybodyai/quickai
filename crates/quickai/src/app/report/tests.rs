//! Problem reports in the window (C202): what telemetry left is forgotten at the start, a crash's report waits for
//! the next start, an engine that stops by itself is reported with its log cleaned, Help opens a report, the window
//! shows it with its buttons, and the addresses fill in QuickAI's page and GitHub's form.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quickai_core::report::{Environment, Kind, Report};
use quickai_runtime::config::{build_registry, ConfigDirs};
use quickai_runtime::settings::Settings;

use super::*;
use crate::app::App;

const GB: u64 = 1024 * 1024 * 1024;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quickai-report-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A window on `settings` in `dir`, with the build and computer of a report known (and no panic hook).
fn app_with(dir: &Path, settings: Settings) -> App {
    let mut app = App::build_in(
        settings,
        build_registry(&ConfigDirs::default()),
        16 * GB,
        Vec::new(),
        Some(dir.join("settings.json")),
        Some(dir.join("data").display().to_string()),
    );
    app.reports.environment = Some(Environment {
        version: "0.2.6".into(),
        package: "tar.gz".into(),
        os: "Linux".into(),
        arch: "x86_64".into(),
        memory_gb: 16,
        ..Default::default()
    });
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
        app.reports_frame();
        app.show_report_window(ui.ctx());
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

/// The person clicks `at`: the pointer goes there, presses and lets go, a frame each.
fn click(app: &mut App, ctx: &egui::Context, at: egui::Pos2) {
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

/// What `%XX` stands for, as `decodeURIComponent` reads it.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn what_telemetry_left_is_forgotten_and_a_crash_waits_for_the_next_start() {
    let dir = scratch("start");
    let data = dir.join("data");
    std::fs::create_dir_all(data.join("telemetry")).unwrap();
    std::fs::write(data.join("telemetry").join("install-id"), "3f1c9a2e").unwrap();
    let report = Report::new(Kind::Crash, Environment::default()).detail("Message", "boom");
    let crash = quickai_runtime::report::write_report(&data.join("reports"), &report, 1).unwrap();
    std::fs::write(
        dir.join("settings.json"),
        r#"{"port": 8080, "ctx_size": 4096, "n_gpu_layers": 0,
            "values": {"app.said_hello": true, "app.telemetry_level": "error", "app.telemetry_asked": 2}}"#,
    )
    .unwrap();
    let settings = Settings::load_from(&dir.join("settings.json"));
    let mut app = app_with(&dir, settings);
    app.start_reports();
    assert!(
        !data.join("telemetry").exists(),
        "the install id and the rest are gone"
    );
    let written = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    assert!(!written.contains("telemetry"), "{written}");
    assert!(written.contains("said_hello"), "{written}");
    let waiting: Vec<&Shown> = app.reports.waiting.iter().collect();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].kind, Kind::Crash);
    assert_eq!(waiting[0].text, report.text());
    assert_eq!(waiting[0].crash_file.as_deref(), Some(crash.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_engine_that_stops_by_itself_is_reported_with_its_log_cleaned() {
    let dir = scratch("engine");
    let mut app = app_with(&dir, Settings::default());
    app.note_run_state(ServerState::Starting);
    let home = directories::BaseDirs::new()
        .map(|d| d.home_dir().display().to_string())
        .unwrap();
    app.push_log(format!(
        "llama_model_load: loading {home}/.quickai/models/my-secret-notes.gguf"
    ));
    let ended = "Server process exited unexpectedly (exit status: 1).";
    app.note_log_line(ended);
    app.push_log(ended.to_string());
    app.note_run_state(ServerState::Crashed);
    let shown = app.reports.waiting.front().expect("a report");
    assert_eq!(shown.kind, Kind::EngineStopped);
    let text = &shown.text;
    assert!(
        text.starts_with("QuickAI problem report\nWhat happened: An engine stopped by itself\n"),
        "{text}"
    );
    assert!(text.contains("\nEngine: llamacpp\n"), "{text}");
    assert!(text.contains("\nWhen: while loading\n"), "{text}");
    assert!(text.contains("\nExit: exit 1\n"), "{text}");
    assert!(text.contains("The engine's log, its last"), "{text}");
    assert!(
        !text.contains(&home) && !text.contains("my-secret-notes"),
        "{text}"
    );
    // a stop that was asked for is not reported
    app.reports.waiting.clear();
    app.note_run_state(ServerState::Starting);
    app.note_run_state(ServerState::Running);
    app.note_run_state(ServerState::Idle);
    assert!(app.reports.waiting.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_opens_a_report_of_the_build_and_the_engine_once() {
    let dir = scratch("help");
    let mut app = app_with(&dir, Settings::default());
    app.open_report_problem();
    app.open_report_problem();
    assert_eq!(app.reports.waiting.len(), 1);
    let shown = &app.reports.waiting[0];
    assert_eq!(shown.kind, Kind::FromHelp);
    assert!(shown.text.contains("What happened: Reported from Help\nQuickAI: 0.2.6, built from source, installed as tar.gz\n"), "{}", shown.text);
    assert!(
        shown.text.contains("\nEngine state: idle\n"),
        "{}",
        shown.text
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_window_shows_the_report_and_its_buttons_and_close_lets_a_crash_go() {
    let dir = scratch("window");
    let mut app = app_with(&dir, Settings::default());
    let report =
        Report::new(Kind::Crash, Environment::default()).detail("Message", "index out of bounds");
    let crash =
        quickai_runtime::report::write_report(&dir.join("data").join("reports"), &report, 1)
            .unwrap();
    app.reports.waiting.push_back(Shown {
        kind: Kind::Crash,
        text: report.text(),
        crash_file: Some(crash.clone()),
    });
    let ctx = egui::Context::default();
    let found = texts(&mut app, &ctx);
    for text in [
        "QuickAI crashed",
        "Report on QuickAI's site",
        "Report on GitHub",
        "Copy",
        "Close",
    ] {
        assert!(found.contains_key(text), "{text}: {:?}", found.keys());
    }
    assert!(
        found
            .keys()
            .any(|t| t.contains("Message: index out of bounds")),
        "{:?}",
        found.keys()
    );
    click(&mut app, &ctx, found["Close"].center());
    assert!(app.reports.waiting.is_empty());
    assert!(!crash.exists(), "a crash's report is shown once");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_site_is_given_the_report_after_the_hash_where_the_server_never_sees_it() {
    let report = "QuickAI problem report\nWhat happened: QuickAI crashed\nMessage: ü & = # ?\n";
    let (address, pasted) = site_address(report);
    assert!(!pasted);
    let (page, fragment) = address.split_once('#').unwrap();
    assert_eq!(page, SITE_REPORT_URL);
    assert!(!page.contains('?'), "nothing in the query: {address}");
    let value = fragment.strip_prefix("report=").unwrap();
    assert!(!value.contains(['&', '#', '=', '?', ' ', '\n']), "{value}");
    assert_eq!(decode(value), report);
    // too long for an address: the page asks for it from the clipboard
    let long = "x".repeat(LONGEST_ADDRESS);
    assert_eq!(
        site_address(&long),
        (format!("{SITE_REPORT_URL}#paste"), true)
    );
}

#[test]
fn github_is_given_its_bug_form_filled_in() {
    let report = Report::new(
        Kind::EngineStopped,
        Environment {
            version: "0.2.6".into(),
            package: "AppImage".into(),
            os: "Linux".into(),
            os_version: "24.04".into(),
            arch: "x86_64".into(),
            memory_gb: 32,
            graphics: vec!["NVIDIA".into()],
            ..Default::default()
        },
    )
    .detail("Engine", "llamacpp")
    .text();
    let (address, pasted) = github_address(Kind::EngineStopped, &report);
    assert!(!pasted);
    let (page, query) = address.split_once('?').unwrap();
    assert_eq!(page, super::super::shell::NEW_ISSUE_URL);
    let fields: BTreeMap<&str, String> = query
        .split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap();
            (key, decode(value))
        })
        .collect();
    assert_eq!(fields["template"], "bug_report.yml");
    assert_eq!(
        fields["title"],
        "Problem report: An engine stopped by itself"
    );
    assert_eq!(fields["version"], "0.2.6");
    assert_eq!(
        fields["system"],
        "Linux 24.04, x86_64, 32 GB of memory, graphics: NVIDIA"
    );
    assert!(fields["what"].starts_with("An engine stopped by itself."));
    assert_eq!(fields["log"], report);
    // too long: the form is filled in but the log, which the person pastes
    let long = format!("{report}{}", "x".repeat(LONGEST_ADDRESS));
    let (address, pasted) = github_address(Kind::EngineStopped, &long);
    assert!(pasted);
    assert!(!address.contains("&log="), "{address}");
    assert!(address.contains("&version=0.2.6"), "{address}");
}

#[test]
fn quiet_reports_open_nothing_by_themselves_and_help_still_does() {
    let dir = scratch("quiet");
    let mut app = app_with(&dir, Settings::default());
    app.reports.quiet = true;
    app.note_run_state(ServerState::Starting);
    app.note_run_state(ServerState::Crashed);
    assert!(app.reports.waiting.is_empty());
    app.open_report_problem();
    assert_eq!(app.reports.waiting.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
