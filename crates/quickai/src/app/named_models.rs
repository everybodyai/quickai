//! The model panels of an engine whose models are named references the daemon lists (Ollama):
//! the picker of what it has, and the box that pulls a model by name. llama.cpp's panels (files in a
//! folder) are `model_bar.rs` and `acquire.rs`. The results of "Look up online" are drawn by
//! `lookup_view.rs`, the same panel for both.

use eframe::egui;

use super::model_bar::Fact;
use super::theme;
use super::widgets;
use super::App;

/// What a word the engine uses for what a model can do says to a person who has not met it.
fn capability_help(word: &str) -> &'static str {
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
    /// The chosen model's name (it is chosen in the list on the left) and, under it, what the engine says it can do.
    pub(super) fn show_named_model_bar(&mut self, ui: &mut egui::Ui) {
        let name = self.engine_name();
        match self.models.get(self.selected) {
            Some(m) => {
                ui.heading(m.display.as_str());
                // what the engine says the model can do is a chip each; `completion` is what every model that
                // chats does, and is not worth a chip
                let mut chips: Vec<Fact> = m
                    .note
                    .split(", ")
                    .filter(|word| !word.is_empty() && *word != "completion")
                    .map(|word| Fact {
                        text: word.to_string(),
                        tone: None,
                        hover: capability_help(word).to_string(),
                    })
                    .collect();
                if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                    chips.push(self.recommended_fact());
                }
                if self.named.stale {
                    chips.push(Fact {
                        text: "from the last run".to_string(),
                        tone: None,
                        hover: format!("(as {name} listed them the last time it ran)"),
                    });
                }
                self.show_model_facts(ui, chips, None, true);
            }
            None => {
                ui.add_space(2.0);
                ui.label(theme::muted(format!(
                    "No models yet. Start {name} to list them, or pull one below."
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
        ui.horizontal_wrapped(|ui| {
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
