//! Problem reports in the window (C202): when QuickAI crashed (shown at the next start), when an engine stopped by
//! itself, and when the window stopped answering, a window shows a report of what happened, for the person to read and
//! post with one click: on QuickAI's site, whose page the report fills in from the link, or as a GitHub issue, filled
//! in the same way. Help → Report a problem opens the same window. QuickAI sends nothing by itself: the page opens in
//! the browser, and nothing leaves until the person presses its button. `QUICKAI_REPORTS=off` keeps the window from
//! opening by itself (tests that read the window set it, so that it is never over what they read); Help still opens it.
//!
//! A report names no one (`quickai_core::report`): the build and the computer in ranges, what happened, and the last
//! lines of the engine's log, cleaned of the home folder, the user's and the computer's names and anything like an
//! address or a secret.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use quickai_core::report::clean::Cleaner;
use quickai_core::report::{log_tail, measure, Environment, Kind, Report};
use quickai_runtime::report::{self as rt, environment, CrashContext};

use super::{theme, App};
use crate::events::ServerState;

/// Where a report is posted on QuickAI's site: the page fills its form from what follows `#`, which the browser never
/// sends to the server.
pub(super) const SITE_REPORT_URL: &str = "https://quickai.everybodyai.dev/report";

/// GitHub's form of a bug report, whose fields a link can fill in (`.github/ISSUE_TEMPLATE/bug_report.yml`).
const BUG_REPORT_TEMPLATE: &str = "bug_report.yml";

/// The longest address opened; a longer report goes by the clipboard, and the page says to paste it.
const LONGEST_ADDRESS: usize = 6000;

/// The most reports waiting to be shown; more are dropped (a window that keeps stalling says it once).
const MOST_WAITING: usize = 3;

/// The title of the window.
pub(super) const REPORT_TITLE: &str = "Report a problem";

/// What the window keeps about problem reports.
#[derive(Default)]
pub(super) struct ReportsUi {
    /// The reports waiting to be shown; the first is shown now.
    pub waiting: VecDeque<Shown>,
    /// The build and the computer, found out at the start.
    environment: Option<Environment>,
    /// The window does not open by itself (`QUICKAI_REPORTS=off`: tests, a kiosk); Help still opens it.
    quiet: bool,
    /// What the panic hook reads of the window.
    context: Option<Arc<Mutex<CrashContext>>>,
    /// What the last button did, said under the buttons.
    said: Option<String>,
    seen_state: Option<ServerState>,
    run: Option<RunWatch>,
    /// How the engine ended, as the log said last.
    last_exit: Option<String>,
}

/// A report in the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Shown {
    pub kind: Kind,
    pub text: String,
    /// The file of a crash's report, deleted when the window closes.
    pub crash_file: Option<PathBuf>,
}

/// A run being watched, for the report of an engine that stops by itself.
struct RunWatch {
    started: u64,
    engine: String,
    model: String,
    build: String,
    ready: bool,
}

impl App {
    /// Problem reports for this start: what telemetry left is deleted, the panic hook is installed, and a crash's
    /// report left by the last session waits to be shown.
    pub(super) fn start_reports(&mut self) {
        let data_dir = PathBuf::from(&self.host_facts.data_dir);
        rt::forget_telemetry(&data_dir);
        let mut stored = self.settings_now();
        if stored.forget_telemetry() {
            self.write_settings(&stored);
            self.stored = stored;
        }
        let environment = environment::detect(self.machine.ram_bytes, &self.machine.gpu_vendors);
        let dir = data_dir.join(rt::REPORTS_DIR);
        self.reports.context = Some(rt::install_panic_hook(
            dir.clone(),
            environment.clone(),
            self.report_cleaner(),
        ));
        self.reports.environment = Some(environment);
        self.reports.quiet = std::env::var("QUICKAI_REPORTS").is_ok_and(|v| v.trim() == "off");
        if self.reports.quiet {
            return;
        }
        for path in rt::pending_crashes(&dir).into_iter().take(MOST_WAITING) {
            match std::fs::read_to_string(&path) {
                Ok(text) => self.reports.waiting.push_back(Shown {
                    kind: Kind::Crash,
                    text,
                    crash_file: Some(path),
                }),
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    /// What is taken out of a report's text: this home, user, computer, and the computers in the settings.
    fn report_cleaner(&self) -> Cleaner {
        let home = directories::BaseDirs::new().map(|d| d.home_dir().display().to_string());
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok();
        let host = environment::host_name();
        let remotes: Vec<String> = self.remote.remotes.iter().map(|r| r.host.clone()).collect();
        Cleaner::new(home.as_deref(), user.as_deref(), host.as_deref(), &remotes)
    }

    /// The build and the computer (found out now when the start did not).
    fn report_environment(&self) -> Environment {
        self.reports.environment.clone().unwrap_or_else(|| {
            environment::detect(self.machine.ram_bytes, &self.machine.gpu_vendors)
        })
    }

    /// The windows that are open, as a report names them.
    fn open_windows(&self) -> Vec<&'static str> {
        let open = [
            ("add_model", self.add_model_open),
            ("recommended", self.catalog.open),
            ("details", self.details_of.is_some()),
            ("settings", self.show_settings),
            ("configurations", self.show_configurations),
            ("help", self.help.open),
            ("about", self.show_about),
            ("report", !self.reports.waiting.is_empty()),
        ];
        open.into_iter()
            .filter(|(_, on)| *on)
            .map(|(w, _)| w)
            .collect()
    }

    /// A line of the engine's log: how it ended, when it says.
    pub(super) fn note_log_line(&mut self, line: &str) {
        if line.contains("exited unexpectedly") {
            self.reports.last_exit = measure::exit_status(line).or(Some("unknown".to_string()));
        }
    }

    /// What changed since the last frame: an engine that stopped by itself and a stall that ended are reported; what
    /// the panic hook would say of this moment is kept up to date.
    pub(super) fn reports_frame(&mut self) {
        let state = self.session.state();
        if self.reports.seen_state != Some(state) {
            self.note_run_state(state);
            self.reports.seen_state = Some(state);
        }
        for stall in self.heartbeat.take_stalls() {
            let mut report = Report::new(Kind::WindowStalled, self.report_environment())
                .detail(
                    "For how long (seconds)",
                    measure::stall_bucket(stall.seconds),
                )
                .detail("Going on", running_id(stall.running));
            if measure::is_plain_name(&self.engine) {
                report = report.detail("Engine", self.engine.as_str());
            }
            report = report
                .detail("Engine state", state_id(state))
                .detail("Windows open", self.open_windows().join(", "));
            self.show_report(rt::machine::with_machine(report, &stall.machine, true));
        }
        let running = self.running_now().map(running_id);
        let windows = self.open_windows();
        if let Some(context) = &self.reports.context {
            if let Ok(mut context) = context.try_lock() {
                context.engine = Some(self.engine.clone());
                context.engine_state = Some(state_id(state));
                context.open_windows = windows;
                context.running = running;
            }
        }
    }

    /// A run's engine, model and build from its start, and whether it was serving; a run that crashed is reported.
    fn note_run_state(&mut self, state: ServerState) {
        let now = rt::now();
        if matches!(state, ServerState::Starting | ServerState::Running)
            && self.reports.run.is_none()
        {
            self.reports.last_exit = None;
            self.reports.run = Some(RunWatch {
                started: now,
                engine: self.engine.clone(),
                model: self.report_model(),
                build: self.report_build(),
                ready: false,
            });
        }
        match state {
            ServerState::Running => {
                if let Some(run) = self.reports.run.as_mut() {
                    run.ready = true;
                }
            }
            ServerState::Crashed => {
                if let Some(run) = self.reports.run.take() {
                    let log: Vec<String> = self.log_lines.iter().cloned().collect();
                    let ended_in = if run.ready { "running" } else { "starting" };
                    let diagnostic = quickai_core::advice::matching_rule(
                        &self.registry,
                        &run.engine,
                        &log,
                        Some(ended_in),
                    );
                    let mut report = Report::new(Kind::EngineStopped, self.report_environment())
                        .detail("Engine", run.engine.as_str())
                        .detail("Build", run.build.as_str())
                        .detail("Model", run.model.as_str())
                        .detail(
                            "When",
                            if run.ready {
                                "while serving"
                            } else {
                                "while loading"
                            },
                        )
                        .detail(
                            "Exit",
                            self.reports
                                .last_exit
                                .clone()
                                .unwrap_or_else(|| "unknown".into()),
                        )
                        .detail(
                            "After (seconds)",
                            now.saturating_sub(run.started).to_string(),
                        );
                    if diagnostic > 0 {
                        report = report.detail(
                            "Matches the engine's diagnostic",
                            format!("number {diagnostic}"),
                        );
                    }
                    report.log = log_tail(&log, &self.report_cleaner());
                    self.show_report(report);
                }
            }
            ServerState::Idle => self.reports.run = None,
            _ => {}
        }
    }

    /// The model chosen, as a report names it: `catalog:<id>` when it is on the recommended list, `hf:<repo>` when it
    /// was fetched from a public repository through a look-up, otherwise `other:<size>`: a file of the person's own
    /// is never named.
    fn report_model(&self) -> String {
        let Some(model) = self.models.get(self.selected) else {
            return "other".to_string();
        };
        let catalog = self.catalog_id_of(&model.filename);
        let from = crate::sources::recall(
            std::path::Path::new(&self.host_facts.data_dir),
            &model.filename,
            model.size_bytes,
        );
        let huggingface = from.as_deref().and_then(|s| s.strip_prefix("huggingface:"));
        measure::model_name(&measure::ModelFacts {
            catalog: catalog.as_deref(),
            huggingface,
            size: model.size_bytes,
        })
    }

    /// The id on the recommended list of the model kept as `file` (or pulled as that name), if it is one of the list.
    fn catalog_id_of(&self, file: &str) -> Option<String> {
        use quickai_core::registry::catalog::Run;
        self.registry
            .catalog()
            .models()
            .iter()
            .find(|m| {
                m.runs.values().any(|run| match run {
                    Run::Files(files) => files.files.iter().any(|f| {
                        f.store_as.as_deref() == Some(file)
                            || f.path.rsplit('/').next() == Some(file)
                    }),
                    Run::Reference(r) => r.reference == file,
                })
            })
            .map(|m| m.id.clone())
    }

    /// The build of the engine's program, as a report names it: its label when QuickAI installed it, `bundled` when it
    /// came in the package, `custom` for a program of the person's own.
    fn report_build(&self) -> String {
        let path = self.binary.to_string_lossy().replace('\\', "/");
        if let Some(rest) = path.split("/.quickai/").nth(1) {
            if let Some(label) = rest.split('/').nth(1).filter(|l| measure::is_plain_name(l)) {
                return label.to_string();
            }
        }
        let package = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.to_path_buf()));
        match package {
            Some(dir) if self.binary.starts_with(crate::paths::packaging_dir(&dir)) => {
                "bundled".to_string()
            }
            _ => "custom".to_string(),
        }
    }

    /// Puts `report` in the window, after those waiting.
    fn show_report(&mut self, report: Report) {
        if !self.reports.quiet && self.reports.waiting.len() < MOST_WAITING {
            self.reports.waiting.push_back(Shown {
                kind: report.kind,
                text: report.text(),
                crash_file: None,
            });
        }
    }

    /// Help → Report a problem: the window with the build, the computer and the engine in use.
    pub(super) fn open_report_problem(&mut self) {
        if self
            .reports
            .waiting
            .iter()
            .any(|s| s.kind == Kind::FromHelp)
        {
            return;
        }
        let state = self.session.state();
        let mut report = Report::new(Kind::FromHelp, self.report_environment());
        if measure::is_plain_name(&self.engine) {
            report = report.detail("Engine", self.engine.as_str());
        }
        report = report
            .detail("Build", self.report_build())
            .detail("Engine state", state_id(state));
        // the person's own words come first, on the page they post it on
        self.reports.waiting.push_front(Shown {
            kind: Kind::FromHelp,
            text: report.text(),
            crash_file: None,
        });
    }

    /// The window of the first report waiting: what happened, the report, and the ways to post it.
    pub(super) fn show_report_window(&mut self, ctx: &egui::Context) {
        let Some(shown) = self.reports.waiting.front().cloned() else {
            return;
        };
        let mut close = false;
        let mut action = None;
        let id = egui::ViewportId::from_hash_of("quickai_report_problem");
        let builder = theme::own_window(REPORT_TITLE, [640.0, 560.0], [440.0, 380.0]);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class != egui::ViewportClass::EmbeddedWindow && ui.input(|i| i.viewport().close_requested()) {
                close = true;
            }
            theme::own_window_page(ui, class, |ui| {
                let p = theme::palette();
                ui.label(egui::RichText::new(shown.kind.headline()).strong().size(16.0).color(p.text));
                ui.add_space(4.0);
                ui.label(intro(shown.kind));
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "QuickAI sends nothing by itself. The buttons open a page in your browser with this report filled \
                         in; nothing is sent until you press Send there. The report names no one: the version, the kind \
                         of computer and what happened, with your folders, names and addresses taken out. Read it first.",
                    )
                    .color(p.muted),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    let site = theme::button_sized(
                        ui,
                        theme::Role::Primary,
                        true,
                        "Report on QuickAI's site",
                        egui::Vec2::ZERO,
                    );
                    if site.clicked() {
                        action = Some(Post::Site);
                    }
                    if ui.button("Report on GitHub").clicked() {
                        action = Some(Post::GitHub);
                    }
                    if ui.button("Copy").clicked() {
                        action = Some(Post::Copy);
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                if let Some(said) = &self.reports.said {
                    ui.label(egui::RichText::new(said).color(p.muted));
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut text = shown.text.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
            });
        });
        if let Some(action) = action {
            self.reports.said = Some(match action {
                Post::Site => {
                    let (address, pasted) = site_address(&shown.text);
                    ctx.copy_text(shown.text.clone());
                    let _ = webbrowser::open(&address);
                    if pasted {
                        "The report is long: it is on your clipboard, paste it on the page."
                            .to_string()
                    } else {
                        "Opened in your browser. The report is on your clipboard too.".to_string()
                    }
                }
                Post::GitHub => {
                    let (address, pasted) = github_address(shown.kind, &shown.text);
                    ctx.copy_text(shown.text.clone());
                    let _ = webbrowser::open(&address);
                    if pasted {
                        "The report is long: it is on your clipboard, paste it under Log."
                            .to_string()
                    } else {
                        "Opened in your browser. The report is on your clipboard too.".to_string()
                    }
                }
                Post::Copy => {
                    ctx.copy_text(shown.text.clone());
                    "Copied.".to_string()
                }
            });
        }
        if close {
            if let Some(path) = &shown.crash_file {
                let _ = std::fs::remove_file(path);
            }
            self.reports.waiting.pop_front();
            self.reports.said = None;
        }
    }
}

/// What a button of the window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Post {
    Site,
    GitHub,
    Copy,
}

/// What the window says of what happened.
fn intro(kind: Kind) -> &'static str {
    match kind {
        Kind::Crash => {
            "QuickAI closed because of an error the last time it ran. A report helps us find it and fix it."
        }
        Kind::EngineStopped => {
            "The engine ended on its own. The log says why when it can; if it is not something you can change, a \
             report helps us fix it."
        }
        Kind::WindowStalled => {
            "The window did not answer for a while. If it keeps happening, a report helps us find why."
        }
        Kind::FromHelp => {
            "Tell us what went wrong: the page asks what you were doing. This report says which version and which kind \
             of computer, so that we can try it the same way."
        }
    }
}

/// The address of QuickAI's report page with `report` filled in after `#`, and whether it was too long for that (the
/// page then says to paste it).
pub(super) fn site_address(report: &str) -> (String, bool) {
    let address = format!("{SITE_REPORT_URL}#report={}", percent_encode(report));
    if address.len() > LONGEST_ADDRESS {
        (format!("{SITE_REPORT_URL}#paste"), true)
    } else {
        (address, false)
    }
}

/// The address of a new GitHub issue on the bug report's form, with its title, what happened, the version, the
/// system and the report (as its log) filled in; whether the report was too long for that (it is then left out, for
/// the person to paste under Log).
pub(super) fn github_address(kind: Kind, report: &str) -> (String, bool) {
    let line = |name: &str| {
        report
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let version = line("QuickAI: ")
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let system = [line("System: "), line("Graphics: ")]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(", graphics: ");
    let what = format!(
        "{}. (Say what you were doing when it happened.)",
        kind.headline()
    );
    let address = |log: Option<&str>| {
        let mut address = format!(
            "{}?template={BUG_REPORT_TEMPLATE}&title={}&what={}&version={}&system={}",
            super::shell::NEW_ISSUE_URL,
            percent_encode(&format!("Problem report: {}", kind.headline())),
            percent_encode(&what),
            percent_encode(&version),
            percent_encode(&system),
        );
        if let Some(log) = log {
            address.push_str(&format!("&log={}", percent_encode(log)));
        }
        address
    };
    let whole = address(Some(report));
    if whole.len() > LONGEST_ADDRESS {
        (address(None), true)
    } else {
        (whole, false)
    }
}

/// The id of what the window says is running (`an install` → `install`), or `none`.
fn running_id(words: &str) -> &'static str {
    measure::RUNNING[1..]
        .iter()
        .copied()
        .find(|id| words.contains(id))
        .unwrap_or(measure::RUNNING[0])
}

/// The state, as a report says it.
fn state_id(state: ServerState) -> &'static str {
    match state {
        ServerState::Idle => "idle",
        ServerState::Starting => "starting",
        ServerState::Running => "running",
        ServerState::Stopping => "stopping",
        ServerState::Crashed => "crashed",
    }
}

/// `text` for an address: every byte but the unreserved ones written as `%XX` (what `decodeURIComponent` reads back).
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests;
