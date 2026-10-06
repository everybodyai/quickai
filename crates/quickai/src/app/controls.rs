//! The numeric controls row (panel `controls`), drawn from the fields of the engine's `primary`
//! section: for llama.cpp the port, the context size and the GPU layers.

use eframe::egui;
use quickai_core::form::{Edit, FormField};

use super::{widgets, App};

impl App {
    /// The row of labelled controls. It is one group, disabled as a whole while the server runs
    /// (every field of the section says "not while the session runs"), exactly as the window has
    /// always drawn it; the labels carry the fields' help as hover text.
    pub(super) fn show_controls(&mut self, ui: &mut egui::Ui) {
        let Some(form) = self.form() else { return };
        let Some(section) = form.section("primary") else {
            return;
        };
        let fields: Vec<&FormField> = section.fields.iter().filter(|f| f.visible).collect();
        let enabled = fields.iter().all(|f| f.enabled);
        let mut edits = Vec::new();
        ui.group(|ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    for (i, field) in fields.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(12.0);
                        }
                        let label = ui.label(format!("{}:", field.label));
                        if let Some(help) = &field.help {
                            label.on_hover_text(help);
                        }
                        if let Some(value) = widgets::show(ui, field) {
                            edits.push(Edit {
                                key: field.key.clone(),
                                value,
                            });
                        }
                    }
                });
            });
        });
        self.values.apply(edits);
    }
}
