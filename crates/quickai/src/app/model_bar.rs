//! The model picker group of the main window (panel `model_bar`).

use eframe::egui;

use super::theme::{self, Tone};
use super::App;
use crate::hardware;

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

    /// The chip on a line of its own, for the picker whose row had no room for it.
    pub(super) fn show_speed_chip_below(&self, ui: &mut egui::Ui) {
        let (text, tone, hover) = self.speed_chip_parts();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            theme::chip(ui, &text, tone).on_hover_text(hover);
        });
    }

    /// The model picker, its Refresh button, the model's note and the recommendation line.
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
        if !chip_fits {
            self.show_speed_chip_below(ui);
        }
        if let Some(m) = self.models.get(self.selected) {
            if !m.note.is_empty() {
                ui.add_space(2.0);
                ui.label(theme::muted(&m.note));
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
                )
                .on_hover_text(
                    "A starting point, not a guarantee: based on total RAM and \
                     whether GPU offload is enabled, favoring a smaller/faster \
                     model on CPU-only setups since a big model stays slow \
                     token-by-token no matter how much RAM is free. This can't \
                     see actual VRAM, so a model with GPU offload on can still \
                     run slower than expected if it doesn't fully fit on the GPU.",
                );
            }
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
