//! The page of a computer QuickAI sets up, when it is the one chosen in the sidebar (WP4.33, C228): the chat, which
//! talks to the run there, as the page of a model here is. The bar over it names the model and says what the run is
//! doing, with Start and Stop, and the panel at its right says how it runs and what that computer has (C228). While the
//! run gets ready its steps are over the chat, each with how far it has gone; why it could not be done is said there
//! too.

use eframe::egui;

use crate::run_file::RunStep;

use super::icons::{self, Icon};
use super::remote_run::SetupRun;
use super::theme;
use super::App;

impl App {
    /// The page of the computer QuickAI sets up that is chosen: the steps of its run while it gets ready, or why it
    /// stopped, then the chat and the log.
    pub(super) fn show_remote_page(&mut self, ui: &mut egui::Ui) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let p = theme::palette();
        let run = self.setup_run();
        let working = run.is_some_and(|r| r.working() && !r.running());
        let failed = run.and_then(|r| r.failed.clone()).filter(|_| !working);
        let doing = run.map(|r| r.doing.clone()).unwrap_or_default();
        if working || failed.is_some() {
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
                        .show(ui, |ui| {
                            if let Some(why) = &failed {
                                ui.label(egui::RichText::new(why).color(p.bad));
                                ui.label(theme::muted(
                                    "Press Start to try again. The Computers window tests the connection step by step.",
                                ));
                            } else if let Some(run) = run {
                                show_steps(ui, run, &spec.name);
                                if !doing.starts_with("Stopping") && !doing.starts_with("Looking") {
                                    ui.label(theme::muted(
                                        "You can close QuickAI meanwhile; it carries on there.",
                                    ));
                                }
                            }
                        });
                });
        }
        self.show_page_body(ui);
    }
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
