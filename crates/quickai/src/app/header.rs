//! The title row of the main window (panel `header`).

use eframe::egui;

use super::{App, Tab};
use crate::hardware;

impl App {
    /// The title row: the heading, the Main/Live tab strip (only with live traffic on) and the theme,
    /// speed, settings and About icons, then the rule under it.
    pub(super) fn show_header(&mut self, ui: &mut egui::Ui) {
        // llama.cpp's settings are described as they always were; another engine's by its own name
        let settings_hover = if self.engine == crate::settings::DEFAULT_ENGINE {
            "App and llama.cpp settings — downloaded-models folder, \
             custom llama-server binary, extra command-line arguments"
                .to_string()
        } else {
            format!(
                "App and {} settings — the engine, where {} is found and its options",
                self.engine_name(),
                self.server_name()
            )
        };
        ui.horizontal(|ui| {
            ui.heading("QuickAI");

            if self.enable_traffic_proxy {
                ui.add_space(12.0);
                ui.selectable_value(&mut self.active_tab, Tab::Main, "Main");
                ui.selectable_value(&mut self.active_tab, Tab::Live, "📡 Live")
                    .on_hover_text(
                        "Watch chat/completion requests and replies in real time as any \
                         client talks to this model.",
                    );
            } else if self.active_tab != Tab::Main {
                self.active_tab = Tab::Main;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let theme_icon = if self.dark_mode { "☀" } else { "🌙" };
                let theme_hover = if self.dark_mode {
                    "Switch to light theme"
                } else {
                    "Switch to dark theme"
                };
                if ui
                    .add(egui::Button::new(theme_icon).frame(false))
                    .on_hover_text(theme_hover)
                    .clicked()
                {
                    self.dark_mode = !self.dark_mode;
                    ui.ctx().set_visuals(if self.dark_mode {
                        egui::Visuals::dark()
                    } else {
                        egui::Visuals::light()
                    });
                    self.save_settings();
                }

                ui.add_space(6.0);

                let size = self
                    .models
                    .get(self.selected)
                    .map(|m| m.size_bytes)
                    .unwrap_or(0);
                let tier = hardware::speed_tier(size, self.total_ram_bytes, self.gpu_offload());
                ui.label(egui::RichText::new(tier.emoji()).size(16.0))
                    .on_hover_text(format!(
                        "Estimated speed for the selected model on this machine: {}. \
                     Rough estimate (model size vs. detected RAM, and whether GPU \
                     offload is on) — not a live benchmark.",
                        tier.label()
                    ));

                ui.add_space(6.0);
                if ui
                    .add(egui::Button::new("⚙").frame(false))
                    .on_hover_text(settings_hover)
                    .clicked()
                {
                    self.show_settings = !self.show_settings;
                }

                ui.add_space(6.0);
                if ui
                    .add(egui::Button::new("ℹ").frame(false))
                    .on_hover_text("About QuickAI: usage, disclaimer, and license")
                    .clicked()
                {
                    self.show_about = !self.show_about;
                }
            });
        });
        ui.add_space(6.0);

        if self.enable_traffic_proxy {
            ui.separator();
            ui.add_space(6.0);
        }
    }
}
