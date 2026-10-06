//! The ⚙ Settings window: the downloaded-models folder and list, live traffic, and the
//! "Advanced: llama.cpp" block (binary override, installed builds, build look-up, extra arguments).

use std::path::PathBuf;

use eframe::egui;

use quickai_core::form::{Edit, FormField};

use super::{widgets, App, PendingDelete};
use crate::hardware;
use crate::launch::ENGINE_ID;
use crate::paths;

impl App {
    pub(super) fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut close_clicked = false;
        let form = self.form();
        egui::Window::new("⚙ Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("App").strong());
                ui.add_space(4.0);

                if let Some(field) = form.as_ref().and_then(|f| f.field("models_dir")) {
                    let label = ui.label(format!("{}:", field.label));
                    if let Some(help) = &field.help {
                        label.on_hover_text(help);
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.models_dir_input)
                        .desired_width(ui.available_width()),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button("Apply")
                        .on_hover_text("Use this folder from now on and re-scan it for models.")
                        .clicked()
                    {
                        let candidate = PathBuf::from(self.models_dir_input.trim());
                        if std::fs::create_dir_all(&candidate).is_ok() {
                            self.models_dir = candidate.clone();
                            self.models_dir_override = Some(candidate);
                            self.rescan_models();
                            self.save_settings();
                            self.settings_error = None;
                        } else {
                            self.settings_error = Some(format!(
                                "Could not create/use {}",
                                self.models_dir_input.trim()
                            ));
                        }
                    }
                    if ui.button("Reset to default").clicked() {
                        let default_dir = paths::user_models_dir();
                        let _ = std::fs::create_dir_all(&default_dir);
                        self.models_dir = default_dir.clone();
                        self.models_dir_input = default_dir.display().to_string();
                        self.models_dir_override = None;
                        self.rescan_models();
                        self.save_settings();
                        self.settings_error = None;
                    }
                });

                let mut downloaded_model_indices: Vec<usize> = self
                    .models
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| !m.path.starts_with(&self.bundled_models_dir))
                    .map(|(i, _)| i)
                    .collect();
                // Most-recently-downloaded first — a model has no other
                // notion of "latest", so this uses the file's own mtime.
                downloaded_model_indices.sort_by_key(|&i| {
                    std::cmp::Reverse(
                        std::fs::metadata(&self.models[i].path)
                            .and_then(|m| m.modified())
                            .ok(),
                    )
                });
                if !downloaded_model_indices.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Downloaded models").strong())
                        .on_hover_text(
                            "Models fetched via the \"Model\" box, \"Look up online\", or \
                             dropped into the folder above by hand, newest first. Bundled \
                             models aren't listed here — they can't be deleted.",
                        );
                    let mut to_delete: Option<PendingDelete> = None;
                    egui::ScrollArea::vertical()
                        .id_salt("downloaded_models_scroll")
                        .auto_shrink([false, true])
                        .max_height(list_scroll_height(ui, DELETE_LIST_MAX_ROWS))
                        .show(ui, |ui| {
                            for &i in &downloaded_model_indices {
                                let m = &self.models[i];
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(egui::Button::new("🗑").small())
                                        .on_hover_text("Delete this model from disk")
                                        .clicked()
                                    {
                                        to_delete = Some(PendingDelete::Model {
                                            filename: m.filename.clone(),
                                            path: m.path.clone(),
                                        });
                                    }
                                    ui.label(format!(
                                        "{} ({})",
                                        m.display,
                                        hardware::format_bytes(m.size_bytes)
                                    ));
                                });
                            }
                        });
                    if let Some(pending) = to_delete {
                        self.pending_delete = Some(pending);
                    }
                }

                ui.add_space(10.0);
                let active = self.session.is_active();
                ui.add_enabled_ui(!active, |ui| {
                    if ui
                        .checkbox(
                            &mut self.enable_traffic_proxy,
                            "Show live conversation traffic",
                        )
                        .on_hover_text(
                            "Adds a small local proxy in front of llama-server so a new \"📡 \
                             Live\" button can show chat/completion requests and replies live, \
                             from any client — the built-in chat UI or an external app via the \
                             🔌 API. Off by default: clients then connect straight to \
                             llama-server, with no extra hop or overhead. Takes effect the next \
                             time you click Start.",
                        )
                        .changed()
                    {
                        self.save_settings();
                    }
                });
                if active {
                    ui.label(
                        egui::RichText::new(
                            "(Stop the server to change this — it applies on the next Start.)",
                        )
                        .small()
                        .color(egui::Color32::GRAY),
                    );
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                let engine_name = self
                    .registry
                    .engine(ENGINE_ID)
                    .map(|e| e.doc.engine.name.clone())
                    .unwrap_or_default();
                egui::CollapsingHeader::new(format!("Advanced: {engine_name}"))
                    .default_open(false)
                    .show(ui, |ui| {
                        let mut edits = Vec::new();
                        let advanced: Vec<FormField> = form
                            .as_ref()
                            .and_then(|f| f.section("advanced"))
                            .map(|s| s.fields.iter().filter(|f| f.visible).cloned().collect())
                            .unwrap_or_default();
                        for (i, field) in advanced.iter().enumerate() {
                            if i > 0 {
                                ui.add_space(8.0);
                            }
                            if field.widget.as_deref() == Some("runtime_manager") {
                                self.show_runtime_manager(ui, field);
                                continue;
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
                        self.values.apply(edits);
                    });

                if let Some(msg) = &self.settings_error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(192, 57, 43), msg);
                }

                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            });
        self.show_settings = open && !close_clicked;
        if !self.show_settings {
            self.save_settings();
        }
    }
}

/// How many rows the "Downloaded models"/"Installed builds" delete lists
/// (⚙ Settings) show before scrolling, so a long download history doesn't
/// push the rest of the window down.
pub(super) const DELETE_LIST_MAX_ROWS: usize = 4;

/// Height for a `ScrollArea` that shows exactly `rows` list rows — each
/// row is a `ui.horizontal` containing an interactive widget, so its
/// height is governed by `interact_size.y` plus the spacing between rows.
pub(super) fn list_scroll_height(ui: &egui::Ui, rows: usize) -> f32 {
    (ui.spacing().interact_size.y + ui.spacing().item_spacing.y) * rows as f32
}
