//! How to connect an external app (panel `api_info`), from the engine's `[api_info]`.

use eframe::egui;
use quickai_core::api_info::{api_panel, ApiPanel};
use quickai_core::plan::ModelInfo;

use super::App;
use crate::events::ServerState;
use crate::launch;

impl App {
    /// How to connect an app to the server that is running: what the engine's `[api_info]` says, for the model that is
    /// loaded, by the name the engine knows it by.
    pub(super) fn connection_panel(&self) -> Option<ApiPanel> {
        let selection = launch::selection_from_values(
            &self.engine,
            self.session.model_name().map(|name| ModelInfo {
                id: name.to_string(),
                display: name.to_string(),
                path: String::new(),
                size_bytes: 0,
            }),
            self.values.map(),
            self.traffic_proxy_on(),
        );
        api_panel(&self.registry, &selection, &self.form_facts())
    }

    /// The "Connect an app to this model" box, shown while the server runs and Connect an app is on.
    pub(super) fn show_api_info_panel(&mut self, ui: &mut egui::Ui, state: ServerState) {
        if !(self.show_api_info && state == ServerState::Running) {
            return;
        }
        let Some(panel) = self.connection_panel() else {
            return;
        };
        ui.add_space(8.0);
        super::theme::card(ui, |ui| {
            // close set: the card is long (a title, four values with their notes and the example), and it is read
            // with the window's buttons above it
            super::theme::compact(ui);
            ui.label(
                egui::RichText::new(if self.runs_alone() {
                    format!("Connect an app to {}", self.engine_name())
                } else {
                    "Connect an app to this model".to_string()
                })
                .size(15.0)
                .color(super::theme::palette().accent_text),
            );
            ui.horizontal(|ui| {
                ui.label("Base URL:");
                ui.code(&panel.base_url);
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(panel.base_url.clone());
                }
            });
            for (label, url) in &panel.extra_urls {
                ui.horizontal(|ui| {
                    ui.label(format!("{label}:"));
                    ui.code(url);
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                });
            }
            if let Some(key) = &panel.api_key {
                ui.horizontal(|ui| {
                    ui.label("API key:");
                    ui.code(&key.value);
                });
                note(ui, key.note.as_deref());
            }
            if let Some(model) = &panel.model {
                ui.horizontal(|ui| {
                    ui.label("Model:");
                    ui.code(&model.value);
                });
                note(ui, model.note.as_deref());
            }
            if let Some(example) = &panel.example {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("curl example:");
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(example.curl.clone());
                    }
                });
                ui.code(&example.curl);
            }
        });
    }
}

/// The small grey sentence under a value.
fn note(ui: &mut egui::Ui, text: Option<&str>) {
    if let Some(text) = text {
        ui.label(
            egui::RichText::new(text)
                .small()
                .color(super::theme::palette().muted),
        );
    }
}
