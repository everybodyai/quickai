//! How to connect an external app, from the engine's `[api_info]`: a popover under the plug button beside the running
//! model (C184). The chat in a browser is the globe button beside it.

use eframe::egui;
use quickai_core::api_info::{api_panel, ApiPanel};
use quickai_core::doc::engine::WebUi;
use quickai_core::plan::ModelInfo;

use super::companion::{ChatAction, ChatButton};
use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::events::ServerState;
use crate::launch;

impl App {
    /// How to connect an app to the server that is running: what the engine's `[api_info]` says, for the model that is
    /// loaded, by the name the engine knows it by.
    pub(super) fn connection_panel(&self) -> Option<ApiPanel> {
        let mut values = self.run_values();
        let mut model = self.session.model_name().map(str::to_string);
        let mut proxy = self.traffic_proxy_on();
        // a run on a computer QuickAI set up, shown: its API here, through the tunnel, and the model it runs
        if self.setup_in_use().is_some() {
            let (port, there) = self.setup_run()?.chat_target()?;
            values.insert(
                format!("{}.port", self.engine),
                quickai_core::value::Value::Int(i64::from(port)),
            );
            model = Some(there).filter(|m| !m.is_empty());
            proxy = false;
        }
        let selection = launch::selection_from_values(
            &self.engine,
            model.map(|name| ModelInfo {
                id: name.clone(),
                display: name,
                path: String::new(),
                size_bytes: 0,
            }),
            &values,
            proxy,
        );
        api_panel(&self.registry, &selection, &self.form_facts())
    }

    /// The address of the engine's own page (`[provides.web_ui]`) while it runs, as the browser is given it.
    pub(super) fn web_ui_url(&self, web_ui: &WebUi) -> Option<String> {
        let port = self.session.port()?;
        // the page at the root is the address without a path, as it always was
        let path = if web_ui.path == "/" { "" } else { &web_ui.path };
        Some(format!("http://127.0.0.1:{port}{path}"))
    }

    /// The chat in a browser there is for this engine, as its button: the engine's own page, or its companion's.
    pub(super) fn browser_chat(&self) -> Option<ChatButton> {
        let own = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.as_ref())
            .filter(|_| !self.runs_alone());
        let Some(web_ui) = own else {
            return self.chat_button();
        };
        let url = self.web_ui_url(web_ui);
        Some(ChatButton {
            label: web_ui.label.clone(),
            enabled: url.is_some() && self.session.state() == ServerState::Running,
            hover: format!(
                "Open {}'s built-in web chat for the running model in your browser.",
                self.server_name()
            ),
            action: ChatAction::Open(url.unwrap_or_default()),
        })
    }

    /// What the button of the chat in a browser does.
    pub(super) fn open_browser_chat(&mut self) {
        match self.browser_chat().map(|chat| chat.action) {
            Some(ChatAction::Open(url)) if !url.is_empty() => {
                let _ = webbrowser::open(&url);
            }
            Some(ChatAction::SetUp) => self.set_up_companion(),
            _ => {}
        }
    }

    /// *Connect an app*, while the server runs and it was asked for: a popover under the plug button beside the model
    /// (option A of the samples, `mockups/connect/`): the address and the model's name, each with its copy button, the
    /// API key in a word, and the example in curl, Python and JavaScript, a tab each. A click outside it, Escape, or
    /// the plug again closes it.
    pub(super) fn show_connect_popover(&mut self, ctx: &egui::Context) {
        let running = if self.setup_in_use().is_some() {
            self.setup_run().is_some_and(|r| r.running())
        } else {
            self.session.state() == ServerState::Running
        };
        if !running {
            self.show_api_info = false;
        }
        if !self.show_api_info {
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
        // under the plug button, its right edge with the button's; where the button is not drawn, at the top right
        let anchor: egui::PopupAnchor = match ctx.data(|d| d.get_temp::<egui::Rect>(anchor_id())) {
            Some(plug) => plug.into(),
            None => {
                let screen = ctx.content_rect();
                egui::pos2(screen.right() - 16.0, screen.top() + 90.0).into()
            }
        };
        let frame =
            egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::symmetric(16, 12));
        egui::Popup::new(
            egui::Id::new("connect_an_app"),
            ctx.clone(),
            anchor,
            egui::LayerId::background(),
        )
        .open_bool(&mut self.show_api_info)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(frame)
        .show(|ui| {
            ui.set_width(488.0);
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
    }
}

/// The plug button beside the running model, `size` big: Connect an app opens and closes under it (where it is drawn
/// is kept for the popover to hang from). `hover` is said when the pointer rests on it.
pub(super) fn connect_button(ui: &mut egui::Ui, size: egui::Vec2, hover: &str) -> bool {
    let plug =
        icons::framed_icon_button(ui, Role::Normal, true, Icon::Plug, size).on_hover_text(hover);
    ui.data_mut(|d| d.insert_temp(anchor_id(), plug.rect));
    super::help::mark_spot(ui, super::help::Spot::Plug, plug.rect);
    plug.clicked()
}

/// Where the plug button was drawn.
fn anchor_id() -> egui::Id {
    egui::Id::new("connect_an_app_anchor")
}

/// The rows (the address, the other addresses, the model's name, the key) and the example, a tab for each language.
fn show_connection_details(ui: &mut egui::Ui, panel: &ApiPanel) {
    let p = theme::palette();
    // what a row is, at its left; the engine's note on a value (Ollama's: name exactly this model) is said when the
    // pointer rests on it
    let key_label = |ui: &mut egui::Ui, key: &str, note: Option<&str>| {
        let label = ui.add_sized(
            egui::vec2(70.0, 22.0),
            egui::Label::new(egui::RichText::new(key).color(p.muted)),
        );
        if let Some(note) = note {
            label.on_hover_text(note);
        }
    };
    let row = |ui: &mut egui::Ui, key: &str, value: &str, note: Option<&str>| {
        ui.separator();
        ui.horizontal(|ui| {
            key_label(ui, key, note);
            let room = ui.available_width() - 36.0;
            ui.add_sized(
                egui::vec2(room, 22.0),
                egui::Label::new(egui::RichText::new(value).monospace().color(p.text)).truncate(),
            );
            icons::copy_button(ui, key, value);
        });
    };
    row(ui, "Address", &panel.base_url, None);
    for (label, url) in &panel.extra_urls {
        row(ui, label, url, None);
    }
    if let Some(model) = &panel.model {
        row(ui, "Model", &model.value, model.note.as_deref());
    }
    if let Some(key) = &panel.api_key {
        ui.separator();
        ui.horizontal(|ui| {
            key_label(ui, "API key", key.note.as_deref());
            if key.value == "none" {
                ui.label(theme::muted("Any, or leave it blank"));
            } else {
                ui.label(egui::RichText::new(&key.value).monospace().color(p.text));
                icons::copy_button(ui, "API key", &key.value);
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
            icons::copy_button(ui, "example", codes[tab].1);
        });
    });
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
    theme::well_frame(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(
            egui::Label::new(egui::RichText::new(codes[tab].1).monospace().color(p.text)).wrap(),
        );
    });
}
