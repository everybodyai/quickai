//! Telemetry in the window (WP4.42, docs/reports-and-community.md): the notice of the first start, Settings → Privacy,
//! the telemetry log, the crash report offered at Off, Help → Report a problem, and the events the window records:
//! an engine's exit, a failed download, a stall, and what a crash report says of the moment.
//!
//! The level is the person's, held under the caps of the build, the package and the environment; at Off, and until
//! the notice is answered, `Telemetry::record` keeps nothing and nothing is sent (the runtime's `telemetry` module).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use quickai_core::telemetry::clean::Cleaner;
use quickai_core::telemetry::{measure, schema, Level};
use quickai_runtime::telemetry::{self as rt, CapBy, Setup, Telemetry};

use super::settings_window::{group, page_head, row_line, SettingsPage};
use super::{theme, App};
use crate::events::ServerState;

/// What the window keeps about telemetry.
#[derive(Default)]
pub(super) struct TelemetryUi {
    /// The notice of the first start is shown.
    pub notice: bool,
    /// A crash report offered at Off, and whether its whole text is shown.
    pub crash_offer: Option<PathBuf>,
    pub crash_shown: bool,
    /// The telemetry log window is open, and on which part.
    pub log_open: bool,
    pub log_part: LogPart,
    /// Help → Report a problem.
    pub problem: Option<Problem>,
    /// A request to EverybodyAI's server running on a thread, and its answer to come.
    pub job: Option<(Job, Receiver<Result<String, String>>)>,
    /// What the last of them said: whether it went well, and the sentence.
    pub message: Option<(bool, String)>,
    /// The delete button asks once more.
    pub confirm_delete: bool,
    seen_state: Option<ServerState>,
    run: Option<RunWatch>,
    /// How the engine ended, as the log said last.
    last_exit: Option<String>,
}

/// A run being watched, for `error.engine_exit`.
struct RunWatch {
    started: u64,
    engine: String,
    model: String,
    build: String,
    ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Job {
    Download,
    Delete,
    SendCrash,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LogPart {
    #[default]
    Sent,
    Waiting,
    Everything,
}

pub(super) struct Problem {
    pub title: String,
    pub text: String,
    pub details: String,
}

/// The titles of the windows of this module.
pub(super) const LOG_TITLE: &str = "Telemetry";
pub(super) const CRASH_TITLE: &str = "QuickAI closed unexpectedly";
pub(super) const PROBLEM_TITLE: &str = "Report a problem";

/// The longest a GitHub address may be before the text goes by the clipboard instead.
const LONGEST_ADDRESS: usize = 6000;

impl App {
    /// Telemetry for this start: the level from the settings under the caps of the package (`package_config`, the
    /// package's configuration folder) and the environment; the panic hook; the sender; and what to ask (the notice,
    /// a crash report at Off).
    pub(super) fn start_telemetry(&mut self, package_config: Option<&Path>) {
        let chosen = self.stored.telemetry_level();
        let telemetry = Telemetry::start(Setup {
            dir: PathBuf::from(&self.host_facts.data_dir).join("telemetry"),
            chosen,
            caps: rt::caps_from_environment(package_config),
            endpoint: rt::endpoint_from_environment(),
            identity: rt::Identity::detect_with(self.machine.ram_bytes, &self.machine.gpu_vendors),
            clock: rt::system_clock(),
        });
        telemetry.install_panic_hook(self.telemetry_cleaner());
        self.telemetry = telemetry;
        self.after_telemetry_start(chosen);
        self.telemetry.spawn_sender();
    }

    /// What follows the start of telemetry, without the sender: what to ask.
    pub(super) fn after_telemetry_start(&mut self, chosen: Option<Level>) {
        let t = self.telemetry.clone();
        let cap = t.cap();
        let answered_now = self.stored.telemetry_asked() >= u64::from(schema::CONSENT_VERSION);
        self.tel.notice = cap > Level::Off
            && match chosen {
                None => true,
                Some(Level::Off) => false,
                Some(_) => !answered_now,
            };
        if cap > Level::Off && t.level() < Level::Crash {
            self.tel.crash_offer = t.pending_crashes().into_iter().next();
        }
    }

    /// What is taken out of a crash's text: this home, user, computer, and the computers in the settings.
    fn telemetry_cleaner(&self) -> Cleaner {
        let home = directories::BaseDirs::new().map(|d| d.home_dir().display().to_string());
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok();
        let host = rt::identity::host_name();
        let remotes: Vec<String> = self.remote.remotes.iter().map(|r| r.host.clone()).collect();
        Cleaner::new(home.as_deref(), user.as_deref(), host.as_deref(), &remotes)
    }

    /// Records the level the person chose, in the settings and in telemetry.
    pub(super) fn choose_telemetry(&mut self, level: Level) {
        self.telemetry.set_chosen(level);
        let mut stored = self.settings_now();
        stored.set_telemetry_level(level);
        self.write_settings(&stored);
        self.stored = stored;
        self.tel.notice = false;
        if level >= Level::Crash {
            // a crash waiting is sent by itself now
            self.tel.crash_offer = None;
        }
    }

    /// The windows of `schema::WINDOWS` that are open, for a crash report.
    fn open_windows(&self) -> Vec<&'static str> {
        let open = [
            ("add_model", self.add_model_open),
            ("recommended", self.catalog.open),
            ("details", self.details_of.is_some()),
            ("settings", self.show_settings),
            ("configurations", self.show_configurations),
            ("help", self.help.open),
            ("about", self.show_about),
            ("telemetry_log", self.tel.log_open),
            ("report_problem", self.tel.problem.is_some()),
        ];
        open.into_iter()
            .filter(|(_, on)| *on)
            .map(|(w, _)| w)
            .collect()
    }

    /// A line of the engine's log: how it ended, when it says.
    pub(super) fn note_log_line(&mut self, line: &str) {
        if line.contains("exited unexpectedly") {
            self.tel.last_exit = measure::exit_status(line).or(Some("unknown".to_string()));
        }
    }

    /// What changed since the last frame, as events; and what a crash report would say of this moment.
    pub(super) fn telemetry_frame(&mut self) {
        self.poll_telemetry_job();
        if !self.telemetry.is_live() {
            return;
        }
        let t = self.telemetry.clone();
        let state = self.session.state();
        if self.tel.seen_state != Some(state) {
            self.note_run_state(state);
            self.tel.seen_state = Some(state);
        }
        for seconds in self.heartbeat.take_stalls() {
            t.record(
                t.event("error.stall")
                    .with("seconds", measure::stall_bucket(seconds)),
            );
        }
        if let Ok(mut context) = t.context().try_lock() {
            context.engine = Some(self.engine.clone());
            context.engine_state = Some(state_id(state));
            context.open_windows = self.open_windows();
        }
    }

    /// A run's engine, model and build from its start, and whether it was serving, for `error.engine_exit`.
    fn note_run_state(&mut self, state: ServerState) {
        let t = self.telemetry.clone();
        let now = t.now();
        if matches!(state, ServerState::Starting | ServerState::Running) && self.tel.run.is_none() {
            self.tel.last_exit = None;
            self.tel.run = Some(RunWatch {
                started: now,
                engine: self.engine.clone(),
                model: self.telemetry_model(),
                build: self.telemetry_build(),
                ready: false,
            });
        }
        match state {
            ServerState::Running => {
                if let Some(run) = self.tel.run.as_mut() {
                    run.ready = true;
                }
            }
            ServerState::Crashed => {
                if let Some(run) = self.tel.run.take() {
                    t.record(
                        t.event("error.engine_exit")
                            .with("engine", run.engine.as_str())
                            .with("build", run.build.as_str())
                            .with("model", run.model.as_str())
                            .with("phase", if run.ready { "serving" } else { "loading" })
                            .with(
                                "status",
                                self.tel
                                    .last_exit
                                    .clone()
                                    .unwrap_or_else(|| "unknown".into()),
                            )
                            .with("after_seconds", now.saturating_sub(run.started)),
                    );
                }
            }
            ServerState::Idle => self.tel.run = None,
            _ => {}
        }
    }

    /// The model chosen, as telemetry may name it: `catalog:<id>` when it is on the recommended list, `hf:<repo>`
    /// when it was fetched from a look-up, otherwise `other:<size>`.
    fn telemetry_model(&self) -> String {
        let Some(model) = self.models.get(self.selected) else {
            return "other".to_string();
        };
        let catalog = self.catalog_id_of(&model.filename);
        let from = crate::sources::recall(
            Path::new(&self.host_facts.data_dir),
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

    /// A download failed (`error.download`): the site and the reason, never the file or the address.
    pub(super) fn note_download_failed(&mut self, message: &str, place: Option<&str>) {
        let source = if place.is_some_and(|p| p.starts_with("huggingface:")) {
            "huggingface"
        } else {
            "other"
        };
        let t = &self.telemetry;
        t.record(
            t.event("error.download")
                .with("what", "model")
                .with("source", source)
                .with("reason", download_reason(message)),
        );
    }

    /// The build of the engine's program, as telemetry names it: its label when QuickAI installed it, `bundled` when
    /// it came in the package, `custom` for a program of the person's own.
    fn telemetry_build(&self) -> String {
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

    /// The window is closing: the last batch, and the session's marker taken away.
    pub(super) fn telemetry_closing(&mut self) {
        self.telemetry.end_session();
    }

    /// Starts a request to the server on a thread; its answer is read by `poll_telemetry_job`.
    fn start_telemetry_job(&mut self, job: Job, crash: Option<PathBuf>) {
        let (tx, rx) = channel();
        let t = self.telemetry.clone();
        std::thread::spawn(move || {
            let answer = match job {
                Job::Download => t.download_held(),
                Job::Delete => t.delete_held(),
                Job::SendCrash => crash.map_or(Err("No report.".to_string()), |p| {
                    t.send_crash(&p).map(|()| String::new())
                }),
            };
            let _ = tx.send(answer);
        });
        self.tel.message = None;
        self.tel.job = Some((job, rx));
    }

    fn poll_telemetry_job(&mut self) {
        let Some((job, rx)) = &self.tel.job else {
            return;
        };
        let Ok(answer) = rx.try_recv() else { return };
        let job = *job;
        self.tel.job = None;
        self.tel.message = Some(match (job, answer) {
            (Job::Download, Ok(text)) => {
                let file = PathBuf::from(&self.host_facts.data_dir)
                    .join("telemetry")
                    .join(format!("held-{}.json", measure::moment_utc(self.telemetry.now()).replace(':', "-")));
                match std::fs::write(&file, text) {
                    Ok(()) => (true, format!("Saved to {}", file.display())),
                    Err(e) => (false, format!("Could not save it: {e}")),
                }
            }
            (Job::Delete, Ok(_)) => (
                true,
                "EverybodyAI deleted what it held for this install. What is sent from now on has a new install id."
                    .to_string(),
            ),
            (Job::SendCrash, Ok(_)) => {
                self.tel.crash_offer = None;
                (true, "The report was sent. Thank you.".to_string())
            }
            (_, Err(e)) => (false, e),
        });
    }

    /// The notice of the first start, under the menu bar: the levels to choose from, and what is sent.
    pub(super) fn show_telemetry_notice(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let cap = self.telemetry.cap();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Help make QuickAI better?").strong().color(p.text));
            ui.label(
                egui::RichText::new(
                    "QuickAI can send EverybodyAI reports of crashes and errors, with a rough description of this \
                     computer. Never your chats, files or searches. Nothing is sent until you choose.",
                )
                .color(p.muted),
            );
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for level in Level::SHOWN.into_iter().filter(|l| *l <= cap) {
                if ui
                    .button(level.label())
                    .on_hover_text(level.describe())
                    .clicked()
                {
                    self.choose_telemetry(level);
                }
            }
            ui.add_space(8.0);
            if ui.link("What is sent").clicked() {
                self.tel.log_open = true;
                self.tel.log_part = LogPart::Everything;
            }
            if ui.link("Privacy policy").clicked() {
                let _ = webbrowser::open(super::shell::PRIVACY_URL);
            }
        });
    }

    /// Settings → Privacy: the level, what is sent, the install id and what EverybodyAI holds.
    pub(super) fn show_privacy_page(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        page_head(
            ui,
            "Privacy",
            Some("What QuickAI sends EverybodyAI, and what EverybodyAI holds. Nothing is sent until you choose."),
        );
        let caps = self.telemetry.caps();
        let cap = self.telemetry.cap();
        let mut chosen = self.telemetry.chosen().unwrap_or(Level::Off).min(cap);
        group(ui, |ui| {
            ui.label(egui::RichText::new("Telemetry").strong().color(p.text));
            ui.add_space(4.0);
            for level in Level::SHOWN {
                let allowed = level <= cap;
                ui.add_enabled_ui(allowed, |ui| {
                    if ui.radio_value(&mut chosen, level, level.label()).changed() {
                        self.choose_telemetry(level);
                    }
                    ui.label(egui::RichText::new(level.describe()).small().color(p.muted));
                });
                ui.add_space(2.0);
            }
            if let Some(why) = caps.iter().min_by_key(|c| c.level).map(|c| match c.by {
                CapBy::Build => "This build of QuickAI sends no telemetry.",
                CapBy::Package => "Set by whoever installed QuickAI on this computer.",
                CapBy::Environment => {
                    "Held down by QUICKAI_TELEMETRY or DO_NOT_TRACK in the environment."
                }
            }) {
                ui.label(egui::RichText::new(why).small().color(p.warn));
            }
            row_line(ui);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Show what is sent").clicked() {
                    self.tel.log_open = true;
                    self.tel.log_part = LogPart::Sent;
                }
                if ui.link("Privacy policy").clicked() {
                    let _ = webbrowser::open(super::shell::PRIVACY_URL);
                }
            });
        });
        ui.add_space(10.0);
        group(ui, |ui| {
            ui.label(
                egui::RichText::new("What EverybodyAI holds")
                    .strong()
                    .color(p.text),
            );
            ui.add_space(4.0);
            let id = self.telemetry.install_id();
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Your install id:").color(p.muted));
                match &id {
                    Some(id) => {
                        ui.label(egui::RichText::new(id).monospace().color(p.text));
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(id.clone());
                        }
                    }
                    None => {
                        ui.label(
                            egui::RichText::new(
                                "none yet: it is made when the first report is sent",
                            )
                            .color(p.text),
                        );
                    }
                }
            });
            ui.label(
                egui::RichText::new(
                    "It is random and names nobody, but it links your reports together. It is how you ask \
                     EverybodyAI for what it holds: don't post it in public.",
                )
                .small()
                .color(p.muted),
            );
            row_line(ui);
            let busy = self.tel.job.is_some();
            ui.add_enabled_ui(id.is_some() && !busy, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Download what EverybodyAI holds").clicked() {
                        self.start_telemetry_job(Job::Download, None);
                    }
                    if self.tel.confirm_delete {
                        ui.label("Delete it all from EverybodyAI's server?");
                        if ui.button("Delete").clicked() {
                            self.tel.confirm_delete = false;
                            self.start_telemetry_job(Job::Delete, None);
                        }
                        if ui.button("Cancel").clicked() {
                            self.tel.confirm_delete = false;
                        }
                    } else if ui.button("Delete what EverybodyAI holds").clicked() {
                        self.tel.confirm_delete = true;
                    }
                    if ui
                        .button("New install id")
                        .on_hover_text(
                            "What is sent from now on is not tied to what was sent before",
                        )
                        .clicked()
                    {
                        self.telemetry.new_install_id();
                        self.tel.message = Some((
                            true,
                            "The next report will have a new install id.".to_string(),
                        ));
                    }
                });
            });
            if busy {
                ui.label(egui::RichText::new("Asking EverybodyAI's server…").color(p.muted));
            }
            if let Some((ok, message)) = &self.tel.message {
                ui.colored_label(if *ok { p.ok } else { p.bad }, message);
            }
        });
    }

    /// The telemetry log: what was sent, what waits, and everything that can be sent.
    pub(super) fn show_telemetry_log(&mut self, ctx: &egui::Context) {
        if !self.tel.log_open {
            return;
        }
        let mut close = false;
        let id = egui::ViewportId::from_hash_of("quickai_telemetry_log");
        let builder = theme::own_window(LOG_TITLE, [720.0, 560.0], [420.0, 300.0]);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class != egui::ViewportClass::EmbeddedWindow
                && ui.input(|i| i.viewport().close_requested())
            {
                close = true;
            }
            theme::own_window_page(ui, class, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tel.log_part, LogPart::Sent, "Sent");
                    ui.selectable_value(&mut self.tel.log_part, LogPart::Waiting, "Waiting");
                    ui.selectable_value(
                        &mut self.tel.log_part,
                        LogPart::Everything,
                        "Everything that can be sent",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                });
                ui.separator();
                let text = match self.tel.log_part {
                    LogPart::Sent => {
                        let log = self.telemetry.log_text();
                        if log.is_empty() {
                            "Nothing has been sent from this computer.".to_string()
                        } else {
                            // the newest at the end; a long log shows its last part
                            let start = log.len().saturating_sub(200_000);
                            let start = (start..log.len())
                                .find(|i| log.is_char_boundary(*i))
                                .unwrap_or(0);
                            log[start..].to_string()
                        }
                    }
                    LogPart::Waiting => match self.telemetry.level() {
                        Level::Off => {
                            "Telemetry is off: nothing waits and nothing is sent.".to_string()
                        }
                        _ if self.telemetry.queued().is_empty() => {
                            "Nothing waits to be sent.".to_string()
                        }
                        _ => self.telemetry.preview(),
                    },
                    LogPart::Everything => schema::reference_page(),
                };
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).monospace().small()).wrap(),
                        );
                    });
            });
        });
        if close
            || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                && self.tel.log_open
                && !self.show_settings
        {
            self.tel.log_open = false;
        }
    }

    /// At Off, after a crash: the report, and whether to send it.
    pub(super) fn show_crash_offer(&mut self, ctx: &egui::Context) {
        let Some(path) = self.tel.crash_offer.clone() else {
            return;
        };
        let Some(report) = Telemetry::read_crash(&path) else {
            self.tel.crash_offer = None;
            return;
        };
        let event = report.events.first();
        let field = |key: &str| {
            event
                .and_then(|e| e.fields.get(key))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let (message, location) = (field("message"), field("location"));
        let mut dont = false;
        let mut send = false;
        let mut settings = false;
        let id = egui::ViewportId::from_hash_of("quickai_crash_offer");
        let builder = theme::own_window(CRASH_TITLE, [560.0, 420.0], [400.0, 260.0]);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class != egui::ViewportClass::EmbeddedWindow && ui.input(|i| i.viewport().close_requested()) {
                dont = true;
            }
            theme::own_window_page(ui, class, |ui| {
                let p = theme::palette();
                ui.label(egui::RichText::new(CRASH_TITLE).strong().size(16.0).color(p.text));
                ui.add_space(4.0);
                ui.label(
                    "A report can help us fix it. It holds the error, where in QuickAI it happened, the version and a \
                     rough description of this computer, never your chats or files.",
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&message).monospace().color(p.text));
                ui.label(egui::RichText::new(&location).monospace().small().color(p.muted));
                ui.add_space(4.0);
                let label = if self.tel.crash_shown { "Hide the report" } else { "View the report" };
                if ui.link(label).clicked() {
                    self.tel.crash_shown = !self.tel.crash_shown;
                }
                if self.tel.crash_shown {
                    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(text).monospace().small()).wrap());
                    });
                }
                ui.add_space(8.0);
                let busy = self.tel.job.is_some();
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Send this report")).clicked() {
                        send = true;
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Don't send")).clicked() {
                        dont = true;
                    }
                    if ui.link("Change the telemetry level").clicked() {
                        settings = true;
                    }
                });
                if busy {
                    ui.label(egui::RichText::new("Sending…").color(p.muted));
                } else if let Some((false, message)) = &self.tel.message {
                    ui.colored_label(p.bad, message);
                }
            });
        });
        if send {
            self.start_telemetry_job(Job::SendCrash, Some(path.clone()));
        }
        if dont {
            self.telemetry.keep_crash(&path);
            self.tel.crash_offer = None;
        }
        if settings {
            self.settings_page = SettingsPage::Privacy;
            self.open_settings();
        }
    }

    /// Opens Help → Report a problem, with the details of this computer and of the last crash filled in.
    pub(super) fn open_report_problem(&mut self) {
        if self.tel.problem.is_some() {
            return;
        }
        let identity = self.telemetry.identity().cloned().unwrap_or_else(|| {
            rt::Identity::detect_with(self.machine.ram_bytes, &self.machine.gpu_vendors)
        });
        let mut details = format!(
            "QuickAI {} ({}, {}, commit {})\nSystem: {} {} {}, {} GB, GPU: {}\nEngine: {} ({})",
            identity.app.version,
            identity.app.channel,
            identity.app.package,
            identity.app.commit,
            identity.os,
            identity.os_version,
            identity.arch,
            identity.memory_gb,
            if identity.gpus.is_empty() {
                "none found".to_string()
            } else {
                identity.gpus.join(", ")
            },
            self.engine,
            self.telemetry_build(),
        );
        let last_crash = self
            .telemetry
            .pending_crashes()
            .first()
            .and_then(|p| Telemetry::read_crash(p))
            .and_then(|b| {
                b.events
                    .first()
                    .and_then(|e| e.fields.get("location"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });
        if let Some(location) = last_crash {
            details.push_str(&format!("\nLast crash: {location}"));
        }
        self.tel.problem = Some(Problem {
            title: String::new(),
            text: String::new(),
            details,
        });
    }

    /// Help → Report a problem: a GitHub issue, filled in, that the person reads before posting. Nothing goes to
    /// EverybodyAI's server.
    pub(super) fn show_report_problem(&mut self, ctx: &egui::Context) {
        let Some(problem) = self.tel.problem.as_mut() else {
            return;
        };
        let mut close = false;
        let mut open = false;
        let id = egui::ViewportId::from_hash_of("quickai_report_problem");
        let builder = theme::own_window(PROBLEM_TITLE, [600.0, 520.0], [420.0, 360.0]);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class != egui::ViewportClass::EmbeddedWindow && ui.input(|i| i.viewport().close_requested()) {
                close = true;
            }
            theme::own_window_page(ui, class, |ui| {
                let p = theme::palette();
                ui.label(
                    egui::RichText::new(
                        "This opens a new issue on GitHub with what you write here, for you to read and post. Nothing is \
                         sent to EverybodyAI from here.",
                    )
                    .color(p.muted),
                );
                ui.add_space(6.0);
                ui.label("Title");
                ui.add(egui::TextEdit::singleline(&mut problem.title).desired_width(f32::INFINITY));
                ui.add_space(4.0);
                ui.label("What happened, and what you expected");
                ui.add(egui::TextEdit::multiline(&mut problem.text).desired_rows(6).desired_width(f32::INFINITY));
                ui.add_space(4.0);
                ui.label("System information (change or remove what you like)");
                ui.add(egui::TextEdit::multiline(&mut problem.details).desired_rows(4).desired_width(f32::INFINITY));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Open on GitHub").clicked() {
                        open = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        });
        if open {
            let body = format!(
                "{}\n\n### System information\n\n```\n{}\n```\n",
                problem.text.trim(),
                problem.details.trim()
            );
            let address = issue_address(problem.title.trim(), &body);
            if address.len() > LONGEST_ADDRESS {
                ctx.copy_text(body);
                let _ = webbrowser::open(&issue_address(
                    problem.title.trim(),
                    "(The text is on the clipboard: paste it here.)",
                ));
            } else {
                let _ = webbrowser::open(&address);
            }
            close = true;
        }
        if close {
            self.tel.problem = None;
        }
    }
}

/// Why a download failed, from what it said.
pub(super) fn download_reason(message: &str) -> &'static str {
    let m = message.to_ascii_lowercase();
    if m.contains("sha-256") || m.contains("sha256") || m.contains("checksum") {
        "checksum"
    } else if m.contains("no space") || m.contains("disk full") || m.contains("not enough space") {
        "disk_full"
    } else if let Some(code) = http_status(&m) {
        if code >= 500 {
            "http_5xx"
        } else {
            "http_4xx"
        }
    } else if [
        "connect",
        "timed out",
        "timeout",
        "dns",
        "resolve",
        "network",
        "reset",
        "unreachable",
    ]
    .iter()
    .any(|w| m.contains(w))
    {
        "network"
    } else {
        "other"
    }
}

/// An HTTP status the message names (`HTTP 404`, `status 503`, `status code 403`).
fn http_status(message: &str) -> Option<u16> {
    ["http ", "status code ", "status "].iter().find_map(|key| {
        let at = message.find(key)? + key.len();
        let code: String = message[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        code.parse::<u16>().ok().filter(|c| (400..600).contains(c))
    })
}

/// The state's id in `crash.panic`.
fn state_id(state: ServerState) -> &'static str {
    match state {
        ServerState::Idle => "idle",
        ServerState::Starting => "starting",
        ServerState::Running => "running",
        ServerState::Stopping => "stopping",
        ServerState::Crashed => "crashed",
    }
}

/// The address of a new GitHub issue with this title and text.
pub(super) fn issue_address(title: &str, body: &str) -> String {
    format!(
        "{}?title={}&body={}",
        super::shell::NEW_ISSUE_URL,
        percent_encode(title),
        percent_encode(body)
    )
}

/// `text` for a query: every byte but the unreserved ones written as `%XX`.
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
