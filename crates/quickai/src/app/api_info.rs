//! How to connect an external app, from the engine's `[api_info]`: a popover under the plug button beside the running
//! model (C184). The chat in a browser is the globe button beside it.

use eframe::egui;
use quickai_core::api_info::{api_panel, ApiPanel};
use quickai_core::plan::ModelInfo;

use super::companion::ChatAction;
use super::icons::{self, Icon};
use super::theme::{self, Role};
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

    /// The chat in a browser there is for this engine: the words of its button, what it does, and whether it can be
    /// pressed now.
    pub(super) fn browser_chat(&self) -> Option<(String, String, bool)> {
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
    pub(super) fn open_browser_chat(&mut self, _state: ServerState) {
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

    /// *Connect an app*, while the server runs and it was asked for: a popover under the plug button beside the model
    /// (option A of the samples, `mockups/connect/`): the address and the model's name, each with its copy button, the
    /// API key in a word, and the example in curl, Python and JavaScript, a tab each. A click outside it, Escape, or
    /// the plug again closes it.
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
        let title = if self.runs_alone() {
            format!("Connect an app to {}", self.engine_name())
        } else {
            "Connect an app".to_string()
        };
        let openai = panel.base_url.ends_with("/v1");
        let width = 520.0;
        // under the plug button, its right edge with the button's; where the button is not drawn, at the top right
        let anchor: Option<egui::Rect> = ctx.data(|d| d.get_temp(connect_anchor_id()));
        let screen = ctx.content_rect();
        let pos = match anchor {
            Some(button) => egui::pos2(
                (button.right() - width).clamp(screen.left() + 8.0, screen.right() - width - 8.0),
                button.bottom() + 6.0,
            ),
            None => egui::pos2(screen.right() - width - 16.0, screen.top() + 90.0),
        };
        let area = egui::Area::new(egui::Id::new("connect_an_app"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(width - 32.0);
                        let p = theme::palette();
                        ui.label(
                            egui::RichText::new(title)
                                .size(15.0)
                                .family(theme::semibold(ui))
                                .color(p.text),
                        );
                        ui.label(theme::muted(if openai {
                            "Give these to any app that works with OpenAI's API."
                        } else {
                            "Give this address to an app that uses its API."
                        }));
                        ui.add_space(6.0);
                        show_connection_details(ui, &panel);
                    });
            });
        // a press outside it closes it; not one on the plug, which opens and closes it itself
        let pressed_at = ctx.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        if let Some(at) = pressed_at {
            let on_plug = anchor.is_some_and(|r| r.contains(at));
            if !area.response.rect.contains(at) && !on_plug {
                self.show_api_info = false;
            }
        }
    }
}

/// Where the plug button was drawn, kept for the popover to hang from.
pub(super) fn connect_anchor_id() -> egui::Id {
    egui::Id::new("connect_an_app_anchor")
}

/// The rows (the address, the other addresses, the model's name, the key) and the example, a tab for each language.
fn show_connection_details(ui: &mut egui::Ui, panel: &ApiPanel) {
    let p = theme::palette();
    // the engine's note on a value (Ollama's: name exactly this model) is said when the pointer rests on its row
    let row = |ui: &mut egui::Ui, key: &str, value: &str, copy: bool, note: Option<&str>| {
        ui.separator();
        ui.horizontal(|ui| {
            let label = ui.add_sized(
                egui::vec2(70.0, 22.0),
                egui::Label::new(egui::RichText::new(key).color(p.muted)),
            );
            if let Some(note) = note {
                label.on_hover_text(note);
            }
            let room = ui.available_width() - if copy { 36.0 } else { 0.0 };
            ui.add_sized(
                egui::vec2(room, 22.0),
                egui::Label::new(egui::RichText::new(value).monospace().color(p.text)).truncate(),
            );
            if copy {
                copy_button(ui, key, value);
            }
        });
    };
    row(ui, "Address", &panel.base_url, true, None);
    for (label, url) in &panel.extra_urls {
        row(ui, label, url, true, None);
    }
    if let Some(model) = &panel.model {
        row(ui, "Model", &model.value, true, model.note.as_deref());
    }
    if let Some(key) = &panel.api_key {
        ui.separator();
        ui.horizontal(|ui| {
            let label = ui.add_sized(
                egui::vec2(70.0, 22.0),
                egui::Label::new(egui::RichText::new("API key").color(p.muted)),
            );
            if let Some(note) = &key.note {
                label.on_hover_text(note);
            }
            if key.value == "none" {
                ui.label(theme::muted("Any, or leave it blank"));
            } else {
                ui.label(egui::RichText::new(&key.value).monospace().color(p.text));
                copy_button(ui, "API key", &key.value);
            }
        });
    }
    let Some(example) = &panel.example else {
        return;
    };
    // the languages there are, curl first; the one chosen last is kept for the next time
    let mut codes: Vec<(&str, &str)> = vec![("curl", example.curl.as_str())];
    if let Some(python) = &example.python {
        codes.push(("Python", python));
    }
    if let Some(javascript) = &example.javascript {
        codes.push(("JavaScript", javascript));
    }
    let tab_id = egui::Id::new("connect_an_app_tab");
    let mut tab: usize = ui
        .data(|d| d.get_temp(tab_id))
        .unwrap_or(0)
        .min(codes.len() - 1);
    ui.separator();
    ui.horizontal(|ui| {
        for (i, (name, _)) in codes.iter().enumerate() {
            if ui.selectable_label(tab == i, *name).clicked() {
                tab = i;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            copy_button(ui, "example", codes[tab].1);
        });
    });
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
    egui::Frame::new()
        .fill(p.well)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(egui::RichText::new(codes[tab].1).monospace().color(p.text))
                    .wrap(),
            );
        });
}

/// A copy button: a picture of two sheets, a tick for a moment once it has copied `text`.
fn copy_button(ui: &mut egui::Ui, key: &str, text: &str) {
    let id = egui::Id::new(("connect_an_app_copied", key));
    let now = ui.input(|i| i.time);
    let copied_at: Option<f64> = ui.data(|d| d.get_temp(id));
    let just = copied_at.is_some_and(|at| now - at < 1.5);
    let icon = if just { Icon::Check } else { Icon::Copy };
    let response = icons::framed_icon_button(ui, Role::Normal, true, icon, egui::vec2(28.0, 24.0))
        .on_hover_text(if just { "Copied" } else { "Copy" });
    if response.clicked() {
        ui.ctx().copy_text(text.to_string());
        ui.data_mut(|d| d.insert_temp(id, now));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1600));
    } else if just {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
    }
}
