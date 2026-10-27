//! The model picker group of the main window (panel `model_bar`).

use eframe::egui;

use super::theme::{self, Tone};
use super::App;
use crate::hardware;

/// One thing that is said of the picked model, as a chip: its words, the tone it is drawn in and what is said more of
/// it when the pointer rests on it (nothing when it needs no more).
pub(super) struct Fact {
    pub text: String,
    pub tone: Option<Tone>,
    pub hover: String,
}

/// What the end of a note says of how the model is started (`models::say_modes` puts the sentence there): a chip of its
/// own, since it is not a part of the note that can be cut and left for the hover. The note without it comes with it.
pub(super) fn split_mode(note: &str) -> (&str, Option<Fact>) {
    if let Some(at) = note.rfind("Starts as an ") {
        let sentence = &note[at..];
        let label = sentence
            .trim_start_matches("Starts as an ")
            .trim_end_matches('.');
        let fact = Fact {
            text: label.to_string(),
            tone: None,
            hover: sentence.to_string(),
        };
        return (note[..at].trim_end(), Some(fact));
    }
    if let Some(at) = note.rfind("It is meant to start in the mode") {
        let fact = Fact {
            text: "mode this engine lacks".to_string(),
            tone: Some(Tone::Warn),
            hover: note[at..].to_string(),
        };
        return (note[..at].trim_end(), Some(fact));
    }
    (note, None)
}

/// `note` up to and including its first full stop (all of it when it has none).
pub(super) fn first_sentence(note: &str) -> &str {
    match note.find(". ") {
        Some(end) => &note[..=end],
        None => note,
    }
}

impl App {
    /// The chosen model's name as the heading of the page, and at the right end of its row the quiet button that
    /// deletes it (after a question); why it cannot be deleted now is what the button says when it is greyed.
    pub(super) fn show_model_heading(&mut self, ui: &mut egui::Ui) {
        let Some(model) = self.models.get(self.selected) else {
            return;
        };
        let name = model.display.clone();
        // what came with the application is never the person's to delete: no button, rather than one always greyed
        let shipped = model.bundled || model.path.starts_with(&self.bundled_models_dir);
        if shipped {
            let mut details = false;
            ui.horizontal(|ui| {
                ui.heading(name.as_str());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    details = theme::button_sized(
                        ui,
                        theme::Role::Quiet,
                        true,
                        "Details…",
                        egui::vec2(0.0, 22.0),
                    )
                    .on_hover_text(
                        "What is known of this model: its file, its size, its quantization.",
                    )
                    .clicked();
                });
            });
            if details {
                self.details_of = Some(model_name_of(&self.models, self.selected));
            }
            return;
        }
        let index = self.selected;
        let deletable = self.delete_request(index);
        let mut delete = false;
        let mut details = false;
        ui.horizontal(|ui| {
            ui.heading(name.as_str());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = theme::button_sized(
                    ui,
                    theme::Role::Quiet,
                    deletable.is_ok(),
                    "Delete…",
                    egui::vec2(0.0, 22.0),
                );
                let button = match &deletable {
                    Ok(_) => button.on_hover_text(
                        "Delete this model from this computer. It asks first; right-click a model in the \
                         list for the same.",
                    ),
                    Err(why) => button.on_disabled_hover_text(why),
                };
                                delete = button.clicked();
                details = theme::button_sized(
                    ui,
                    theme::Role::Quiet,
                    true,
                    "Details…",
                    egui::vec2(0.0, 22.0),
                )
                .on_hover_text("What is known of this model: its file, its size, its quantization, what goes with it.")
                .clicked();
            });
        });
        if delete {
            self.ask_to_delete_from_window(index);
        }
        if details {
            self.details_of = self.models.get(index).map(|m| m.filename.clone());
        }
    }

    /// How fast the selected model is likely to run here: the words of the chip, its tone and what its hover text says. It is
    /// said of the model, so it is drawn with the model (it was an icon in the title row).
    fn speed_chip_parts(&self) -> (String, Option<Tone>, String) {
        let size = self
            .models
            .get(self.selected)
            .map(|m| m.size_bytes)
            .unwrap_or(0);
        let tier = hardware::speed_tier(size, self.total_ram_bytes, self.gpu_offload());
        let tone = match tier {
            hardware::SpeedTier::Fast => Some(Tone::Good),
            hardware::SpeedTier::Medium => None,
            hardware::SpeedTier::Slow => Some(Tone::Warn),
        };
        (
            format!("{} on this computer", tier.label()),
            tone,
            format!(
                "Estimated speed for the selected model on this machine: {}. \
                 Rough estimate (model size vs. detected RAM, and whether GPU \
                 offload is on) — not a live benchmark.",
                tier.label()
            ),
        )
    }

    /// The badge that says the picked model is the one this computer is recommended: what it rests on is said when the
    /// pointer is on it.
    pub(super) fn recommended_fact(&self) -> Fact {
        Fact {
            text: "★ Recommended".to_string(),
            tone: Some(Tone::Good),
            hover: format!(
                "Recommended for this machine (~{} RAM detected{}). A starting point, not a guarantee: \
                 based on total RAM and whether GPU offload is enabled, favoring a smaller/faster \
                 model on CPU-only setups since a big model stays slow token-by-token no matter how \
                 much RAM is free. This can't see actual VRAM, so a model with GPU offload on can \
                 still run slower than expected if it doesn't fully fit on the GPU.",
                hardware::format_bytes(self.total_ram_bytes),
                if self.gpu_offload() { "" } else { ", CPU only" }
            ),
        }
    }

    /// What is said of the picked model under the picker, on one row: the chips (what it can do, that it is the
    /// recommended one, that what is said of it is from the last run), then a sentence of what the application knows
    /// of it, cut at its first full stop and whole when the pointer is on it. `with_speed` puts the speed chip in
    /// front, for a picker whose row had no room for it. Nothing is drawn when there is nothing to say.
    pub(super) fn show_model_facts(
        &self,
        ui: &mut egui::Ui,
        chips: Vec<Fact>,
        sentence: Option<&str>,
        with_speed: bool,
    ) {
        // how fast a model is, is this computer's guess, and says nothing of one that runs on another
        let with_speed = with_speed && self.remote_in_use().is_none();
        if chips.is_empty() && sentence.is_none() && !with_speed {
            return;
        }
        ui.add_space(2.0);
        // a row of small print and chips is as high as they are and not as a button (the height of a wrapped row is
        // the room it is begun with, so it is set on the ui that begins it)
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().interact_size.y = 20.0;
            ui.horizontal_wrapped(|ui| {
                if with_speed {
                    let (text, tone, hover) = self.speed_chip_parts();
                    theme::chip(ui, &text, tone).on_hover_text(hover);
                }
                for fact in chips {
                    let chip = theme::chip(ui, &fact.text, fact.tone);
                    if !fact.hover.is_empty() {
                        chip.on_hover_text(fact.hover);
                    }
                }
                if let Some(sentence) = sentence {
                    let first = first_sentence(sentence);
                    let label = ui.label(theme::muted(first));
                    if first.len() < sentence.len() {
                        label.on_hover_text(sentence);
                    }
                }
            });
        });
    }

    /// The chosen model's name and, under it, what is said of it: how fast it will be, the tags, the first sentence of
    /// its note.
    pub(super) fn show_model_bar(&mut self, ui: &mut egui::Ui) {
        if self.lists_models() {
            self.show_named_model_bar(ui);
            return;
        }
        // the model is chosen in the list on the left; here is its name and what is known of it
        self.show_model_heading(ui);
        if let Some(m) = self.models.get(self.selected) {
            let mut chips = Vec::new();
            if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                chips.push(self.recommended_fact());
            }
            // how it is started (an embedding server) is a chip, and the rest of the note is cut at its first sentence
            let (note, mode) = split_mode(&m.note);
            chips.extend(mode);
            let sentence = (!note.is_empty()).then_some(note);
            self.show_model_facts(ui, chips, sentence, true);
        } else {
            // a first run, not a fault: what to do next, and where a file put there by hand is read from
            // a first run: the three steps from here to a conversation
            let folder =
                super::runtime_manager::display_root(&self.models_dir, &self.host_facts.home);
            ui.add_space(2.0);
            ui.label(egui::RichText::new("Getting started").strong());
            for step in [
                format!(
                    "1. The engine is {}: the switch at the right of the toolbar has the others.",
                    self.engine_name()
                ),
                format!(
                    "2. No models yet. Get one below: Recommended has a short list that suits this computer, and Look \
                     up online searches Hugging Face. A .gguf file you put in {folder} is listed too (Refresh)."
                ),
                "3. Press Start, then talk to the model in the Chat tab at the foot of the window.".to_string(),
            ] {
                ui.label(theme::muted(step));
            }
        }
    }
}

/// The name of the model at `index`, as `details_of` names it.
fn model_name_of(models: &[crate::models::ModelEntry], index: usize) -> String {
    models
        .get(index)
        .map(|m| m.filename.clone())
        .unwrap_or_default()
}
