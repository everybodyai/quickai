//! What is said of the models of an engine whose models are named references the daemon lists (Ollama): the words for
//! what a model can do, and the pull under way. The box that pulls one is *Add a model*'s (`add_model.rs`).

use eframe::egui;

use super::theme;
use super::widgets;
use super::App;

/// What a word the engine uses for what a model can do says to a person who has not met it.
pub(super) fn capability_help(word: &str) -> &'static str {
    match word {
        "vision" => "Can look at pictures.",
        "tools" => "Can call tools: functions an app offers it.",
        "thinking" => "Reasons step by step before it answers.",
        "embedding" => "Turns text into numbers for searching: it does not chat.",
        "insert" => "Can fill in the middle of a text, as code completion does.",
        "audio" => "Can listen to audio.",
        _ => "",
    }
}

impl App {
    /// The pull that waits for the engine to start, the pull under way, each with its Cancel, and why an input was
    /// refused.
    pub(super) fn show_pull_status(&mut self, ui: &mut egui::Ui) {
        let server = self.server_name();
        if let Some(queued) = &self.named.queued {
            ui.add_space(6.0);
            let line = queued.waiting_line(&server);
            let mut cancel = false;
            ui.horizontal(|ui| {
                ui.colored_label(theme::palette().warn, line);
                cancel = ui
                    .small_button("Cancel")
                    .on_hover_text(format!(
                        "Do not pull it. {server} goes on starting: Stop stops it."
                    ))
                    .clicked();
            });
            if cancel {
                self.named.queued = None;
            }
        }
        if let Some((job, handle)) = &self.named.pull {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.colored_label(theme::palette().warn, job.progress_line());
                if ui.small_button("Cancel").clicked() {
                    handle.cancel();
                }
            });
            widgets::progress_bar(ui, job.fraction());
        }
        if let Some(notice) = &self.named.notice {
            ui.add_space(6.0);
            ui.colored_label(theme::palette().bad, notice);
        }
    }
}

/// The X at the end of the row of the box, which closes the results under it (the workbench's).
pub(super) fn close_button(ui: &mut egui::Ui) -> egui::Response {
    theme::button_sized(ui, theme::Role::Quiet, true, "X", egui::vec2(24.0, 22.0))
        .on_hover_text("Close lookup results")
}
