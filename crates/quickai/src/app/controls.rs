//! The numeric controls row (panel `controls`), drawn from the fields of the engine's `primary`
//! section: for llama.cpp the port, the context size and the GPU layers.

use eframe::egui;
use quickai_core::doc::engine::FieldType;
use quickai_core::form::{Edit, FormField};

use super::theme;
use super::widgets::{self, Reply};
use super::App;

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
        let mut actions = Vec::new();
        theme::card(ui, |ui| {
            let row = ui
                .add_enabled_ui(enabled, |ui| {
                    let row = ui.horizontal(|ui| {
                        for (i, field) in fields.iter().enumerate() {
                            if i > 0 {
                                ui.add_space(12.0);
                            }
                            let label = ui.label(format!("{}:", field.label));
                            if let Some(help) = &field.help {
                                label.on_hover_text(help);
                            }
                            match widgets::show(ui, field) {
                                Some(Reply::Set(value)) => edits.push(Edit {
                                    key: field.key.clone(),
                                    value,
                                }),
                                Some(Reply::Invoke(action)) => actions.push(action),
                                None => {}
                            }
                        }
                    });
                    // a number says what is wrong with it beside its box; a choice has no room beside it in this row,
                    // so what is wrong with it (ComfyUI's Memory with the GPU off: the two flags cannot be used together)
                    // is said under the row
                    for field in fields.iter().filter(|f| f.ty == FieldType::Choice) {
                        for problem in widgets::problem_lines(field.error.as_deref()) {
                            ui.colored_label(widgets::problem_color(), problem);
                        }
                    }
                    row.response.rect
                })
                .inner;
            // why the boxes are dim, at the end of their row when there is room for it
            if !enabled {
                let palette = theme::palette();
                let font = egui::TextStyle::Small.resolve(ui.style());
                let galley = ui.painter().layout_no_wrap(
                    "Locked while the server is running.".to_string(),
                    font,
                    palette.muted,
                );
                let right = ui.max_rect().right();
                if right - galley.size().x > row.right() + 16.0 {
                    let at = egui::pos2(
                        right - galley.size().x,
                        row.center().y - galley.size().y / 2.0,
                    );
                    ui.painter().galley(at, galley, palette.muted);
                }
            }
        });
        self.values.apply(edits);
        for action in actions {
            self.invoke(&action);
        }
    }
}
