//! How to connect an external app (panel `api_info`).

use eframe::egui;

use super::App;
use crate::events::ServerState;

impl App {
    /// The "Connect an app to this model" box, shown while the server runs and 🔌 API is on.
    pub(super) fn show_api_info_panel(&mut self, ui: &mut egui::Ui, state: ServerState) {
        if self.show_api_info && state == ServerState::Running {
            if let Some(port) = self.session.port() {
                ui.add_space(6.0);
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Connect an app to this model").strong());
                    let base_url = format!("http://127.0.0.1:{port}/v1");
                    let model = self.session.model_name().unwrap_or("local-model").to_string();

                    ui.horizontal(|ui| {
                        ui.label("Base URL:");
                        ui.code(&base_url);
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(base_url.clone());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("API key:");
                        ui.code("none");
                    });
                    ui.label(
                        egui::RichText::new(
                            "llama.cpp doesn't require one by default — leave it blank, or use \
                             any placeholder value if your client insists on a non-empty string.",
                        )
                        .small()
                        .color(egui::Color32::GRAY),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Model:");
                        ui.code(&model);
                    });
                    ui.label(
                        egui::RichText::new(
                            "Only one model is ever loaded at a time, so most OpenAI-compatible \
                             clients work with any value here.",
                        )
                        .small()
                        .color(egui::Color32::GRAY),
                    );

                    ui.add_space(4.0);
                    let curl = format!(
                        "curl {base_url}/chat/completions \\\n  -H \"Content-Type: application/json\" \\\n  -d '{{\"model\":\"{model}\",\"messages\":[{{\"role\":\"user\",\"content\":\"Hello\"}}]}}'"
                    );
                    ui.horizontal(|ui| {
                        ui.label("curl example:");
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(curl.clone());
                        }
                    });
                    ui.code(&curl);
                });
            }
        }
    }
}
