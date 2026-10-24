//! The model panels of an engine whose models are named references the daemon lists (Ollama):
//! the picker of what it has, and the box that pulls a model by name. llama.cpp's panels (files in a
//! folder) are `model_bar.rs` and `acquire.rs`. The results of "Look up online" are drawn by
//! `lookup_view.rs`, the same panel for both.

use eframe::egui;

use super::theme;
use super::widgets;
use super::App;
use crate::hardware;

impl App {
    /// The picker of the models the engine listed, its Refresh button and the recommendation line.
    pub(super) fn show_named_model_bar(&mut self, ui: &mut egui::Ui, active: bool) {
        let name = self.engine_name();
        let server = self.server_name();
        let mut chip_fits = true;
        ui.horizontal(|ui| {
            ui.label("Model:").on_hover_text(format!(
                "Which model {server} loads on Start. These are the models {name} has: \
                 they are listed by the running {server}, and by what it listed last time \
                 while it is stopped. Pull another with the box below."
            ));
            let prev_selected = self.selected;
            egui::ComboBox::from_id_salt("model_combo")
                .width(320.0)
                .selected_text(
                    self.models
                        .get(self.selected)
                        .map(|m| m.display.as_str())
                        .unwrap_or("(no models listed)"),
                )
                .show_ui(ui, |ui| {
                    for (i, m) in self.models.iter().enumerate() {
                        let label =
                            if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                                format!("{} (recommended)", m.display)
                            } else {
                                m.display.clone()
                            };
                        ui.selectable_value(&mut self.selected, i, label);
                    }
                });
            if self.selected != prev_selected {
                self.apply_model_defaults();
            }
            ui.add_enabled_ui(!active, |ui| {
                if ui
                    .button("Refresh")
                    .on_hover_text(format!(
                        "Ask the running {server} which models it has. Stopped, this shows \
                         what it listed the last time it ran: nothing is asked of a program \
                         that is not running."
                    ))
                    .clicked()
                {
                    self.refresh_models(ui.ctx());
                }
            });
            chip_fits = self.show_speed_chip_in_row(ui);
        });
        if !chip_fits {
            self.show_speed_chip_below(ui);
        }
        match self.models.get(self.selected) {
            Some(m) => {
                if !m.note.is_empty() {
                    ui.add_space(2.0);
                    ui.label(theme::muted(&m.note));
                }
                if self.named.stale {
                    ui.add_space(2.0);
                    ui.label(theme::muted(format!(
                        "(as {name} listed them the last time it ran)"
                    )));
                }
                if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "★ Recommended for this machine (~{} RAM detected{})",
                            hardware::format_bytes(self.total_ram_bytes),
                            if self.gpu_offload() { "" } else { ", CPU only" }
                        ))
                        .color(theme::palette().ok)
                        .small(),
                    );
                }
            }
            None => {
                ui.add_space(2.0);
                ui.label(theme::muted(format!(
                    "No models listed yet. Start {name} to see what it has, or to pull \
                     one with the box below."
                )));
            }
        }
    }

    /// The box that pulls a model by name, its look-up online, the pull's progress and the reason an
    /// input was refused.
    pub(super) fn show_named_acquire(&mut self, ui: &mut egui::Ui, active: bool) {
        let server = self.server_name();
        let engine_name = self.engine_name();
        let can_look_up = self.can_look_up();
        let pulling = self.named.pull.is_some();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!pulling, |ui| {
                ui.label("Get:").on_hover_text(format!(
                    "A model's name (qwen3:4b), or a Hugging Face reference \
                     (hf.co/user/repo:Q4_K_M), pulled by the running {server} when you \
                     press Enter or Pull. Nothing is fetched until you do."
                ));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.model_input)
                        .hint_text("model name, or hf.co/user/repo...")
                        .desired_width(260.0),
                );
                let trimmed = self.model_input.trim().to_string();
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let go = ui
                    .add_enabled(!trimmed.is_empty(), egui::Button::new("Pull"))
                    .on_hover_text(format!("Ask the running {server} to pull this model."))
                    .clicked();
                if !trimmed.is_empty() && (go || submitted) {
                    self.submit_model_input(ui.ctx());
                }
            });
            if can_look_up {
                ui.add_space(8.0);
                if ui
                    .button("Look up online")
                    .on_hover_text(
                        "Search the library of models at ollama.com for the words in the \
                         box, or list the ones it shows first when the box is empty. \
                         Nothing is pulled until you choose one.",
                    )
                    .clicked()
                {
                    let words = self.model_input.clone();
                    self.start_lookup(ui.ctx(), &words);
                }
            }
            if self.can_recommend()
                && ui
                    .button("Recommended")
                    .on_hover_text(format!(
                        "Browse the models QuickAI recommends for {engine_name}, from small ones \
                         that run on any computer to bigger ones: chat, code, pictures, \
                         audio, documents, translation and search. The list is part of the \
                         application: nothing is asked of the internet to open it."
                    ))
                    .clicked()
            {
                self.open_catalog();
            }
            // Only shown once there is something on screen to close, as llama.cpp's is.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.lookup.has_results() && close_button(ui).clicked() {
                    self.lookup.open = false;
                }
            });
        });
        if let Some((job, handle)) = &self.named.pull {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.colored_label(theme::palette().warn, job.progress_line());
                if ui.small_button("Cancel").clicked() {
                    handle.cancel();
                }
            });
            widgets::progress_bar(ui, job.fraction());
        }
        if let Some(notice) = &self.named.notice {
            ui.add_space(4.0);
            ui.colored_label(theme::palette().bad, notice);
        }
        self.show_lookup_results(ui);
        let _ = active;
    }
}

/// The X at the end of the row of the box, which closes the results under it.
pub(super) fn close_button(ui: &mut egui::Ui) -> egui::Response {
    theme::button_sized(ui, theme::Role::Quiet, true, "X", egui::vec2(24.0, 22.0))
        .on_hover_text("Close lookup results")
}
