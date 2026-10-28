//! The page of a computer QuickAI sets up, when it is the one chosen in the status bar (WP4.33, option A of the
//! samples in `mockups/remote-runs/`, the user: "I pick A"): the configuration's name in the head with what its run
//! is doing, the computer under it, and the buttons of what can be done with it (play; while it runs the chat page
//! in a browser, Connect an app and Stop). While it gets ready its steps are listed, each with how far it has gone;
//! once it runs, the addresses here. The chat and the log are under it, as on the page of a model, and the chat
//! talks to that run.

use eframe::egui;

use crate::events::ServerState;
use crate::run_file::RunStep;

use super::icons::{self, Icon};
use super::remote_run::SetupRun;
use super::theme::{self, Role};
use super::App;

/// The side of the buttons of the head, as on the page of a model.
const BUTTON: f32 = 30.0;

/// What the head says the run is doing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// Nothing runs there, and nothing is being done.
    Idle,
    /// It is being found, copied, started, prepared, or stopped.
    Working,
    /// It runs there, and its ports are here.
    Running,
    /// It could not be done, or the connection closed.
    Failed,
}

impl App {
    /// The page of the computer QuickAI sets up that is chosen: the head, then the chat and the log.
    pub(super) fn show_remote_page(&mut self, ui: &mut egui::Ui) {
        let room = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("remote_page_head")
            .max_height((room * 0.6).max(140.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 16,
                        top: 14,
                        bottom: 8,
                    })
                    .show(ui, |ui| self.show_remote_head(ui));
            });
        self.show_tabs(ui);
    }

    /// The head: the name, what the run is doing, the buttons, the computer, and the steps or the addresses.
    fn show_remote_head(&mut self, ui: &mut egui::Ui) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let p = theme::palette();
        let run = self.setup_run();
        let shown = match run {
            Some(r) if r.running() => Shown::Running,
            Some(r) if r.working() => Shown::Working,
            Some(r) if r.failed.is_some() => Shown::Failed,
            _ => Shown::Idle,
        };
        let configuration = run
            .map(|r| r.configuration.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| self.selected_configuration_name());
        let doing = run.map(|r| r.doing.clone()).unwrap_or_default();
        let computer_line = run
            .and_then(|r| r.there.as_ref())
            .and_then(|t| t.computer.clone())
            .map_or_else(
                || format!("on {} · {}", spec.name, spec.host),
                |c| format!("on {} · {c}", spec.name),
            );
        let page = run.and_then(|r| r.endpoint("page")).map(|e| e.url.clone());
        let mut pressed = Pressed::default();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match shown {
                    Shown::Running => {
                        pressed.stop = stop_button(ui, &format!("Stop the run on {}.", spec.name));
                        pressed.connect = super::api_info::connect_button(
                            ui,
                            egui::Vec2::splat(BUTTON),
                            "Connect an app: the address here to give another app, with an example.",
                        );
                        if page.is_some() {
                            pressed.page = icons::framed_icon_button(
                                ui,
                                Role::Normal,
                                true,
                                Icon::Globe,
                                egui::Vec2::splat(BUTTON),
                            )
                            .on_hover_text("Open its chat page in your browser.")
                            .clicked();
                        }
                    }
                    Shown::Working => {
                        pressed.stop = stop_button(
                            ui,
                            &format!("Stop: what is being done on {} stops there.", spec.name),
                        );
                    }
                    Shown::Idle | Shown::Failed => {
                        let hint = format!(
                            "Start: run \"{configuration}\" on {}. QuickAI copies itself there when it needs to, \
                             installs what is missing, and brings its ports here (Ctrl+Enter).",
                            spec.name
                        );
                        pressed.start = icons::framed_icon_button(
                            ui,
                            Role::Primary,
                            true,
                            Icon::Play,
                            egui::Vec2::splat(BUTTON),
                        )
                        .on_hover_text(hint)
                        .clicked();
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&configuration)
                                .size(20.0)
                                .family(theme::semibold(ui))
                                .color(p.text),
                        )
                        .truncate(),
                    );
                    match shown {
                        Shown::Running => {
                            state_words(ui, p.ok, &format!("Running on {}", spec.name))
                        }
                        Shown::Working if doing.starts_with("Stopping") => {
                            state_words(ui, p.warn, "Stopping…")
                        }
                        Shown::Working => {
                            ui.add_space(6.0);
                            ui.spinner();
                            ui.label(egui::RichText::new("Getting ready").color(p.warn));
                        }
                        Shown::Failed => state_words(ui, p.bad, "Stopped"),
                        Shown::Idle => {}
                    }
                });
            });
        });
        ui.label(theme::muted(computer_line));
        ui.add_space(8.0);
        match shown {
            Shown::Running => {
                if let Some(run) = run {
                    show_addresses(ui, run);
                }
                ui.label(theme::muted(format!(
                    "It runs on {} until you stop it, also when QuickAI is closed.",
                    spec.name
                )));
            }
            Shown::Working => {
                if let Some(run) = run {
                    show_steps(ui, run, &spec.name);
                }
                if !doing.starts_with("Stopping") && !doing.starts_with("Looking") {
                    ui.label(theme::muted(
                        "You can close QuickAI meanwhile; it carries on there.",
                    ));
                }
            }
            Shown::Failed => {
                let why = run.and_then(|r| r.failed.clone()).unwrap_or_default();
                ui.label(egui::RichText::new(why).color(p.bad));
                ui.label(theme::muted(
                    "Press the play button to try again. The Computers window tests the connection step by step.",
                ));
            }
            Shown::Idle => {
                ui.label(theme::muted(format!(
                    "The play button runs \"{configuration}\" on {}: QuickAI copies itself there when it needs to, \
                     installs what is missing, and brings its ports here. It runs there until you stop it.",
                    spec.name
                )));
            }
        }
        if pressed.start {
            self.start_pressed_at = Some(std::time::Instant::now());
            self.start_setup_run(ui.ctx(), false);
        }
        if pressed.stop {
            // not the second click of a double-click on the play button, which Stop takes the place of
            let just_started = self
                .start_pressed_at
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(800));
            if !just_started {
                self.stop_setup_run();
            }
        }
        if pressed.connect {
            self.show_api_info = !self.show_api_info;
        }
        if let Some(url) = page.filter(|_| pressed.page) {
            let _ = webbrowser::open(&url);
        }
    }
}

impl App {
    /// What the status bar says of the run on the computer QuickAI set up that is shown: its state as a server's, and
    /// in words. `None` when this computer, or one the window attaches to, is shown.
    pub(super) fn remote_run_status(&self) -> Option<(ServerState, String)> {
        let spec = self.setup_in_use()?;
        let run = self.setup_run();
        let configuration = run
            .map(|r| r.configuration.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| self.selected_configuration_name());
        let said = match run {
            Some(r) if r.running() => {
                let api = r
                    .endpoint("api")
                    .map(|e| format!(" · {}", e.url))
                    .unwrap_or_default();
                (
                    ServerState::Running,
                    format!("Running · \"{configuration}\" on {}{api}", spec.name),
                )
            }
            Some(r) if r.working() && r.doing.starts_with("Stopping") => (
                ServerState::Stopping,
                format!("Stopping \"{configuration}\" on {}", spec.name),
            ),
            Some(r) if r.working() => (
                ServerState::Starting,
                format!("Getting ready · \"{configuration}\" on {}", spec.name),
            ),
            Some(r) if r.failed.is_some() => (
                ServerState::Crashed,
                format!("Stopped · \"{configuration}\" on {}", spec.name),
            ),
            _ => (ServerState::Idle, format!("Nothing runs on {}", spec.name)),
        };
        Some(said)
    }
}

/// Pressed in the head this frame.
#[derive(Default)]
struct Pressed {
    start: bool,
    stop: bool,
    connect: bool,
    page: bool,
}

/// The stop button, `hover` said when the pointer rests on it.
fn stop_button(ui: &mut egui::Ui, hover: &str) -> bool {
    icons::framed_icon_button(
        ui,
        Role::Danger,
        true,
        Icon::Stop,
        egui::Vec2::splat(BUTTON),
    )
    .on_hover_text(hover)
    .clicked()
}

/// What the run is doing, after its name: a dot and words in the colour of the state.
fn state_words(ui: &mut egui::Ui, color: egui::Color32, words: &str) {
    ui.add_space(6.0);
    theme::dot(ui, 8.0, 4.0, color);
    ui.label(egui::RichText::new(words).color(color));
}

/// The steps of a run that is getting ready: QuickAI on the computer, each step of the configuration's plan with how
/// far it has gone, and the start.
fn show_steps(ui: &mut egui::Ui, run: &SetupRun, computer: &str) {
    let there = run.there.as_deref();
    let steps: &[RunStep] = there.map_or(&[], |t| t.steps.as_slice());
    let ready = there.is_some_and(|t| t.state == "ready");
    let all_done = !steps.is_empty() && steps.iter().all(|s| s.state == "done");
    theme::card(ui, |ui| {
        ui.set_max_width(620.0);
        let first = if run.reached {
            format!("QuickAI on {computer}")
        } else if run.doing.is_empty() {
            format!("Connecting to {computer}")
        } else {
            run.doing.clone()
        };
        step_row(
            ui,
            if run.reached { Mark::Done } else { Mark::Doing },
            &first,
            None,
            None,
        );
        for step in steps {
            let mark = match step.state.as_str() {
                "done" => Mark::Done,
                "doing" => Mark::Doing,
                _ => Mark::Pending,
            };
            let took = match (step.began, step.ended) {
                (Some(began), Some(ended)) => Some(format!("{} s", ended.saturating_sub(began))),
                _ => None,
            };
            let progress = (mark == Mark::Doing)
                .then_some(step.total)
                .flatten()
                .filter(|total| *total > 0)
                .map(|total| (step.done, total));
            step_row(ui, mark, &step.text, took.as_deref(), progress);
        }
        // the start, when the plan does not say it as a step of its own
        if !steps.iter().any(|s| s.kind == "start") {
            let start = if ready {
                Mark::Done
            } else if all_done {
                Mark::Doing
            } else {
                Mark::Pending
            };
            step_row(ui, start, "Start", None, None);
        }
    });
}

/// How far a step has gone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Pending,
    Doing,
    Done,
}

/// A row of the steps: a tick, a spinner or a ring, what it is, how long it took, and a bar of how far it has gone.
fn step_row(
    ui: &mut egui::Ui,
    mark: Mark,
    text: &str,
    took: Option<&str>,
    progress: Option<(u64, u64)>,
) {
    let p = theme::palette();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        match mark {
            Mark::Done => icons::paint(ui.painter(), rect, Icon::Check, p.ok),
            Mark::Doing => {
                egui::Spinner::new()
                    .size(14.0)
                    .paint_at(ui, rect.shrink(2.0));
            }
            Mark::Pending => {
                ui.painter()
                    .circle_stroke(rect.center(), 6.0, egui::Stroke::new(1.2, p.faint));
            }
        }
        let color = if mark == Mark::Pending {
            p.muted
        } else {
            p.text
        };
        let text = match progress {
            Some((done, total)) => format!(
                "{text}: {} of {}",
                super::shell::size_text(done),
                super::shell::size_text(total)
            ),
            None => text.to_string(),
        };
        ui.label(egui::RichText::new(text).color(color));
        if let Some(took) = took {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(theme::muted(took).small());
            });
        }
    });
    if let Some((done, total)) = progress {
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            ui.add(
                egui::ProgressBar::new((done as f32 / total as f32).clamp(0.0, 1.0))
                    .desired_height(6.0),
            );
        });
    }
    ui.add_space(2.0);
}

/// The addresses here of what the run started, each with its copy button.
fn show_addresses(ui: &mut egui::Ui, run: &SetupRun) {
    let p = theme::palette();
    theme::card(ui, |ui| {
        ui.set_max_width(620.0);
        ui.label(egui::RichText::new("ADDRESSES HERE").small().color(p.muted));
        for endpoint in &run.endpoints {
            let what = match endpoint.role.as_str() {
                "api" => "API",
                "page" => "Chat page",
                other => other,
            };
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_sized(
                    egui::vec2(80.0, 22.0),
                    egui::Label::new(egui::RichText::new(what).color(p.muted)),
                )
                .on_hover_text(&endpoint.program);
                let room = ui.available_width() - 36.0;
                ui.add_sized(
                    egui::vec2(room, 22.0),
                    egui::Label::new(egui::RichText::new(&endpoint.url).monospace().color(p.text))
                        .truncate(),
                );
                icons::copy_button(ui, &format!("remote {what}"), &endpoint.url);
            });
        }
    });
}
