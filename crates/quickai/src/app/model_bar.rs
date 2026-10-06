//! The model picker group of the main window (panel `model_bar`).

use eframe::egui;

use super::App;
use crate::hardware;

impl App {
    /// The model picker, its Refresh button, the model's note and the recommendation line.
    pub(super) fn show_model_bar(&mut self, ui: &mut egui::Ui, active: bool) {
        ui.group(|ui| {
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
                            let label = if self.recommended_filename.as_deref()
                                == Some(m.filename.as_str())
                            {
                                format!("{} (recommended)", m.display)
                            } else {
                                m.display.clone()
                            };
                            if m.bundled {
                                let bg = if ui.visuals().dark_mode {
                                    egui::Color32::from_rgb(24, 54, 30)
                                } else {
                                    egui::Color32::from_rgb(214, 245, 214)
                                };
                                egui::Frame::NONE
                                    .fill(bg)
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
            });
            if let Some(m) = self.models.get(self.selected) {
                if !m.note.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(&m.note)
                            .color(egui::Color32::GRAY)
                            .small(),
                    );
                }
                if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "★ Recommended for this machine (~{} RAM detected{})",
                            hardware::format_bytes(self.total_ram_bytes),
                            if self.gpu_offload() { "" } else { ", CPU only" }
                        ))
                        .color(egui::Color32::from_rgb(46, 139, 50))
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
                    egui::Color32::from_rgb(192, 57, 43),
                    format!(
                        "No .gguf files found in {} or {}",
                        self.bundled_models_dir.display(),
                        self.models_dir.display()
                    ),
                );
            }
        });
    }
}
