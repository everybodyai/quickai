//! How to connect an external app, from the engine's `[api_info]`: a window of its own, opened from *Connect an app*
//! beside the running model, with the chat in a browser at its head.

use eframe::egui;
use quickai_core::api_info::{api_panel, ApiPanel};
use quickai_core::plan::ModelInfo;

use super::companion::ChatAction;
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
            &self.run_values(),
            self.traffic_proxy_on(),
        );
        api_panel(&self.registry, &selection, &self.form_facts())
    }

    /// The ways to chat in a browser, as items of a menu: the engine's own page (llama.cpp's), or the companion's
    /// (Open WebUI beside Ollama), to set up or to open.
    pub(super) fn show_browser_chat_items(&mut self, ui: &mut egui::Ui, state: ServerState) {
        if let Some((label, hover, enabled)) = self.browser_chat() {
            if ui
                .add_enabled(enabled, egui::Button::new(label))
                .on_hover_text(hover)
                .clicked()
            {
                self.open_browser_chat(state);
                ui.close();
            }
        }
        let _ = state;
    }

    /// The chat in a browser there is for this engine: the words of its button, what it does, and whether it can be
    /// pressed now.
    fn browser_chat(&self) -> Option<(String, String, bool)> {
        let running = self.session.state() == ServerState::Running;
        if let Some(web_ui) = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.clone())
            .filter(|_| !self.runs_alone())
        {
            return Some((
                web_ui.label.clone(),
                format!(
                    "Open {}'s built-in web chat for the running model in your browser.",
                    self.server_name()
                ),
                running,
            ));
        }
        self.chat_button()
            .map(|chat| (chat.label, chat.hover, chat.enabled))
    }

    /// What the button of the chat in a browser does.
    fn open_browser_chat(&mut self, _state: ServerState) {
        if let Some(web_ui) = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.clone())
            .filter(|_| !self.runs_alone())
        {
            if let Some(port) = self.session.port() {
                // the page at the root is the address without a path, as it always was
                let path = if web_ui.path == "/" { "" } else { &web_ui.path };
                let _ = webbrowser::open(&format!("http://127.0.0.1:{port}{path}"));
            }
            return;
        }
        if let Some(chat) = self.chat_button() {
            match chat.action {
                ChatAction::Open(url) => {
                    let _ = webbrowser::open(&url);
                }
                ChatAction::SetUp => self.set_up_companion(),
            }
        }
    }

    /// The window of *Connect an app*, while the server runs and it was asked for: the chat in a browser, then the
    /// address and the rest of what an app needs.
    pub(super) fn show_connect_window(&mut self, ctx: &egui::Context) {
        let state = self.session.state();
        if !self.show_api_info {
            return;
        }
        if state != ServerState::Running {
            self.show_api_info = false;
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_api_info = false;
            return;
        }
        let Some(panel) = self.connection_panel() else {
            return;
        };
        let browser = self.browser_chat();
        let mut open = true;
        let mut open_chat = false;
        let title = if self.runs_alone() {
            format!("Connect an app to {}", self.engine_name())
        } else {
            "Connect an app".to_string()
        };
        egui::Window::new(title)
            .id(egui::Id::new("connect_an_app"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
            .show(ctx, |ui| {
                super::theme::compact(ui);
                if let Some((label, hover, enabled)) = &browser {
                    ui.horizontal(|ui| {
                        ui.label("Chat in your browser:");
                        open_chat = ui
                            .add_enabled(*enabled, egui::Button::new(label.as_str()))
                            .on_hover_text(hover.as_str())
                            .clicked();
                    });
                    ui.separator();
                }
                ui.label(super::theme::muted(
                    "Any app that works with OpenAI's API can use this model: give it the address and the name.",
                ));
                ui.add_space(4.0);
                show_connection_details(ui, &panel);
            });
        if open_chat {
            self.open_browser_chat(state);
        }
        if !open {
            self.show_api_info = false;
        }
    }
}

/// The address, the other addresses, the key, the model's name and the example, each with its Copy.
fn show_connection_details(ui: &mut egui::Ui, panel: &ApiPanel) {
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
