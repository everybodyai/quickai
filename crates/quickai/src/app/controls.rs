//! The fields of the engine's `primary` section, drawn from the document: for llama.cpp the port, the context size
//! and the GPU layers. They are under *Change*, beside the model's name (`run_settings.rs`).

use eframe::egui;
use quickai_core::doc::engine::FieldType;
use quickai_core::form::{Edit, FormField};

use super::theme;
use super::widgets::{self, Reply};
use super::App;

impl App {
    /// The engine's fields of its `primary` section (the port, the context size, the GPU), a row each with its label,
    /// greyed while the server runs (every field of the section says "not while the session runs"); the labels carry
    /// the fields' help as hover text. *Change* shows them under Custom, and for an engine with no presets.
    pub(super) fn show_controls(&mut self, ui: &mut egui::Ui) {
        let Some(form) = self.form() else { return };
        let Some(section) = form.section("primary") else {
            return;
        };
        // the engine on another computer is that computer's to set up: its port here is the tunnel's
        if let Some(remote) = self.remote_in_use() {
            let name = remote.name.clone();
            ui.label(theme::muted(format!(
                "{} runs on {name}: its port, its context size and its graphics card are set there. Here, QuickAI \
                 reaches it through an SSH tunnel on a port of its own.",
                self.server_name()
            )));
            return;
        }
        let fields: Vec<&FormField> = section.fields.iter().filter(|f| f.visible).collect();
        let enabled = fields.iter().all(|f| f.enabled);
        let mut edits = Vec::new();
        let mut actions = Vec::new();
        ui.add_enabled_ui(enabled, |ui| {
            egui::Grid::new("primary_fields")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for field in &fields {
                        let label = ui.label(format!("{}:", field.label));
                        if let Some(help) = &field.help {
                            label.on_hover_text(help);
                        }
                        ui.horizontal(|ui| match widgets::show(ui, field) {
                            Some(Reply::Set(value)) => edits.push(Edit {
                                key: field.key.clone(),
                                value,
                            }),
                            Some(Reply::Invoke(action)) => actions.push(action),
                            None => {}
                        });
                        ui.end_row();
                    }
                });
            // what is wrong with a choice is said under the rows (ComfyUI's Memory with the GPU off: the two flags
            // cannot be used together); a number says it beside its box
            for field in fields.iter().filter(|f| f.ty == FieldType::Choice) {
                for problem in widgets::problem_lines(field.error.as_deref()) {
                    ui.colored_label(widgets::problem_color(), problem);
                }
            }
        });
        self.values.apply(edits);
        for action in actions {
            self.invoke(&action);
        }
    }
}
