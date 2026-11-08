//! The panel at the right of the page (UI v3, C203, the user's A6 of `the mockups`): how the model runs while
//! it is stopped (the configuration, the presets or the engine's fields, the MCP servers in a line, and how it fits this
//! computer), and Connect an app while it runs (the addresses, the model's name, the key, the examples and the chat in a
//! browser). It is what *Change* under the model's name and the Connect an app popover were.

use eframe::egui;

use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::events::ServerState;

impl App {
    /// The panel at the right of the page.
    pub(super) fn show_inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("inspector")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.connect_now() {
                    self.show_connect_section(ui);
                } else {
                    self.show_how_section(ui);
                }
            });
    }

    /// Whether the panel shows Connect an app: the model chosen runs (or the engine that runs on its own does, or one
    /// that lists its models runs with none loaded yet).
    fn connect_now(&self) -> bool {
        if self.session.state() != ServerState::Running {
            return false;
        }
        self.runs_alone()
            || self.models.is_empty()
            || self.runs_model(self.selected)
            || (self.lists_models() && self.session.model_name().is_none())
    }

    /// How the model runs: what *Change* had (the configuration, the presets or the fields), then the MCP servers and how
    /// it fits this computer.
    fn show_how_section(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        section_title(ui, "How it runs");
        self.show_change_panel(ui);
        for problem in self.field_problems() {
            ui.colored_label(super::widgets::problem_color(), problem);
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Sliders, p.faint);
            ui.label(theme::muted(self.run_summary()).small());
        });
        if let Some(count) = self.mcp_server_count() {
            rule(ui);
            let mut manage = false;
            ui.horizontal(|ui| {
                ui.label(theme::muted("Tools (MCP)").small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    manage = ui.link("Manage").clicked();
                });
            });
            ui.label(egui::RichText::new(match count {
                1 => "1 server on".to_string(),
                n => format!("{n} servers on"),
            }));
            ui.label(
                theme::muted("Used in llama.cpp's chat page (Open Chat UI), from the next Start.")
                    .small(),
            );
            if manage {
                self.open_mcp_servers();
            }
        }
        if let Some(model) = self
            .models
            .get(self.selected)
            .filter(|_| !self.runs_alone())
        {
            if let Some(offload) = self.speed_basis() {
                rule(ui);
                let tier =
                    crate::hardware::speed_tier(model.size_bytes, self.total_ram_bytes, offload);
                ui.horizontal(|ui| {
                    ui.label(theme::muted("On this computer").small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = match tier {
                            crate::hardware::SpeedTier::Fast => p.accent_text,
                            crate::hardware::SpeedTier::Slow => p.warn,
                            crate::hardware::SpeedTier::Medium => p.text,
                        };
                        ui.label(egui::RichText::new(tier.label()).color(color));
                    });
                });
                let total = self.total_ram_bytes.max(1);
                ui.horizontal(|ui| {
                    ui.label(theme::muted("Size").small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} of {} memory",
                                super::shell::size_text(model.size_bytes),
                                crate::hardware::format_bytes(total)
                            ))
                            .small(),
                        );
                    });
                });
                meter(ui, model.size_bytes as f32 / total as f32);
            }
        }
        ui.add_space(10.0);
        if ui.link("All settings for starting it…").clicked() {
            self.settings_page = super::settings_window::SettingsPage::Start;
            if !self.show_settings {
                self.toggle_settings();
            }
        }
    }

    /// Connect an app, while it runs: the rows and the example, the chat in a browser, and how it runs, in a line.
    fn show_connect_section(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Connect an app");
        let panel = self.connection_panel();
        let openai = panel.as_ref().is_some_and(|p| p.base_url.ends_with("/v1"));
        ui.label(
            theme::muted(if openai {
                "Give these to any app that works with OpenAI's API."
            } else {
                "Give this address to an app that uses its API."
            })
            .small(),
        );
        ui.add_space(4.0);
        if let Some(panel) = &panel {
            super::api_info::show_connection_details(ui, panel);
        }
        ui.add_space(8.0);
        // the chat in a browser: the engine's own page, or its companion's (Open WebUI)
        if let Some(chat) = self.browser_chat().filter(|_| !self.runs_alone()) {
            let hover = format!("{}: {}", chat.label.trim_end_matches('.'), chat.hover);
            if icons::labelled_button(
                ui,
                Role::Normal,
                chat.enabled,
                Icon::Globe,
                &chat.label,
                30.0,
            )
            .on_hover_text(&hover)
            .on_disabled_hover_text(&hover)
            .clicked()
            {
                self.open_browser_chat();
            }
        }
        if let Some((text, state)) = self.companion_status() {
            ui.horizontal(|ui| {
                theme::dot(ui, 8.0, 3.5, theme::state_color(state));
                ui.label(theme::muted(text).small());
            });
        }
        rule(ui);
        ui.label(theme::muted("How it runs").small());
        ui.label(self.run_summary());
        ui.label(theme::muted("Stop the model to change these.").small());
        if let Some(count) = self
            .mcp_server_count()
            .filter(|_| self.remote_in_use().is_none())
        {
            rule(ui);
            let mut manage = false;
            ui.horizontal(|ui| {
                ui.label(theme::muted("Tools (MCP)").small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    manage = ui.link("Manage").clicked();
                });
            });
            ui.label(match count {
                0 => "None on".to_string(),
                1 => "1 server on".to_string(),
                n => format!("{n} servers on"),
            });
            ui.label(theme::muted("The model can use them in Open Chat UI.").small());
            if manage {
                self.open_mcp_servers();
            }
        }
    }

    /// Opens Settings at the MCP servers.
    pub(super) fn open_mcp_servers(&mut self) {
        self.settings_page = super::settings_window::SettingsPage::Tools;
        if !self.show_settings {
            self.toggle_settings();
        }
    }
}

/// The name of the panel's section, at its head.
fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(15.0)
            .family(theme::semibold(ui))
            .color(theme::palette().text),
    );
    ui.add_space(4.0);
}

/// A line between two parts of the panel.
fn rule(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    let y = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        y,
        egui::Stroke::new(1.0, theme::palette().line),
    );
    ui.add_space(9.0);
}

/// A thin bar, filled as far as `share` (0 to 1) of it.
fn meter(ui: &mut egui::Ui, share: f32) {
    let p = theme::palette();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, p.raised);
    let filled = egui::Rect::from_min_size(
        rect.min,
        egui::vec2((width * share.clamp(0.0, 1.0)).max(4.0), rect.height()),
    );
    ui.painter().rect_filled(filled, 3.0, p.accent_text);
}
