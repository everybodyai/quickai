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
            format!("{} {} on this computer", tier.emoji(), tier.label()),
            tone,
            format!(
                "Estimated speed for the selected model on this machine: {}. \
                 Rough estimate (model size vs. detected RAM, and whether GPU \
                 offload is on) — not a live benchmark.",
                tier.label()
            ),
        )
    }

    /// The speed chip at the end of the picker's row, when the row has room for it after the Refresh button, and under the
    /// picker when it has not (a narrow window: it was drawn over the button). Call it in the row, last.
    pub(super) fn show_speed_chip_in_row(&self, ui: &mut egui::Ui) -> bool {
        if self.models.is_empty() {
            return true;
        }
        let (text, tone, hover) = self.speed_chip_parts();
        if ui.available_width() < theme::chip_width(ui, &text) + 8.0 {
            return false;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::chip(ui, &text, tone).on_hover_text(hover);
        });
        true
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

    /// The model picker, its Refresh button, and under them what is said of the model.
    pub(super) fn show_model_bar(&mut self, ui: &mut egui::Ui, active: bool) {
        if self.lists_models() {
            self.show_named_model_bar(ui, active);
            return;
        }
        let mut chip_fits = true;
        ui.horizontal(|ui| {
            ui.label("Model:").on_hover_text(
                "Which .gguf file llama-server will load on Start. Entries highlighted \
                 green are bundled with this app and always available; others were \
                 downloaded or dropped into a models folder by hand.",
            );
            let prev_selected = self.selected;
            egui::ComboBox::from_id_salt("model_combo")
                .width(320.0)
                .selected_text(
                    self.models
                        .get(self.selected)
                        .map(|m| m.display.as_str())
                        .unwrap_or("(no models found)"),
                )
                .show_ui(ui, |ui| {
                    for (i, m) in self.models.iter().enumerate() {
                        let label =
                            if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                                format!("{} (recommended)", m.display)
                            } else {
                                m.display.clone()
                            };
                        if m.bundled {
                            egui::Frame::NONE
                                .fill(theme::palette_of(ui.visuals().dark_mode).accent_soft)
                                .corner_radius(egui::CornerRadius::same(4))
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .show(ui, |ui| {
                                    ui.selectable_value(&mut self.selected, i, label);
                                });
                        } else {
                            ui.selectable_value(&mut self.selected, i, label);
                        }
                    }
                });
            // A fresh pick of a different model gets its own
            // context-size default — bigger models/tighter RAM get
            // a smaller suggestion. Still just a starting point:
            // the DragValue below stays freely editable afterward.
            if self.selected != prev_selected {
                self.apply_model_defaults();
            }
            ui.add_enabled_ui(!active, |ui| {
                if ui
                    .button("Refresh")
                    .on_hover_text(
                        "Re-scan the models folders for new .gguf files — e.g. one you \
                         just dropped in by hand — without restarting the app.",
                    )
                    .clicked()
                {
                    self.rescan_models();
                }
            });
            chip_fits = self.show_speed_chip_in_row(ui);
        });
        if let Some(m) = self.models.get(self.selected) {
            let mut chips = Vec::new();
            if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                chips.push(self.recommended_fact());
            }
            // how it is started (an embedding server) is a chip, and the rest of the note is cut at its first sentence
            let (note, mode) = split_mode(&m.note);
            chips.extend(mode);
            let sentence = (!note.is_empty()).then_some(note);
            self.show_model_facts(ui, chips, sentence, !chip_fits);
        } else {
            ui.colored_label(
                theme::palette().bad,
                format!(
                    "No .gguf files found in {} or {}",
                    self.bundled_models_dir.display(),
                    self.models_dir.display()
                ),
            );
        }
    }
}
