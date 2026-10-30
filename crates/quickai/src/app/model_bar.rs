//! The head of the page: the name of the chosen model (or of the engine that runs on its own), what it is doing, and
//! the buttons of what can be done with it.

use eframe::egui;

use super::help::{self, Spot};
use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::events::ServerState;

/// The side of the buttons of the head.
const BUTTON: f32 = 30.0;

impl App {
    /// The head of the page: the chosen model's name, what it is doing, and at the right end of its row what can be
    /// done with it, as pictures: play (Start, or Load model, or Use this model), and while it runs the chat in a
    /// browser, Connect an app and Stop. What is known of the model is said in its row of the list (C185).
    pub(super) fn show_model_header(
        &mut self,
        ui: &mut egui::Ui,
        state: ServerState,
        active: bool,
    ) {
        let Some(model) = self.models.get(self.selected) else {
            return;
        };
        let display = model.display.clone();
        let loading_this = self.session.loading() == Some(model.display.as_str())
            || self.session.loading() == Some(model.filename.as_str());
        let index = self.selected;
        let lists = self.lists_models();
        let server = self.server_name();
        let loadable = self.can_load_selected();
        let switchable = self.can_switch_selected();
        let runs_this = self.runs_model(index);
        let named_this = self.session.model_name() == Some(display.as_str());
        let can_start = !self.engine_missing();
        let can_press_start = (!active && can_start) || loadable || switchable;
        let can_stop = matches!(state, ServerState::Starting | ServerState::Running);
        let show_start = !active || loadable || switchable;
        // green while it is the thing to do: Start, or loading into an engine that has nothing loaded. With a model
        // running, the one that changes the model is not
        let first_model = !active || self.session.model_name().is_none();
        let start_label = if loadable {
            "Load model"
        } else if switchable {
            "Use this model"
        } else {
            "Start"
        };
        let start_hint = if loadable {
            format!("Load this model into the running {server} (Ctrl+Enter).")
        } else if switchable {
            format!("Start {server} again with this model.")
        } else {
            format!("Start {server} with this model (Ctrl+Enter).")
        };
        let browser = (state == ServerState::Running)
            .then(|| self.browser_chat())
            .flatten();
        let mut pressed = HeaderPress::default();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // while it runs, the things to do with it are pictures, each named when the pointer rests on it
                if can_stop {
                    pressed.stop = stop_button(ui, &server);
                }
                if state == ServerState::Running {
                    pressed.connect = super::api_info::connect_button(
                        ui,
                        egui::Vec2::splat(BUTTON),
                        "Connect an app: the address to give another app (an editor, a script, a chat app), with an \
                         example.",
                    );
                    if let Some(chat) = &browser {
                        let hover = format!("{}: {}", chat.label.trim_end_matches('.'), chat.hover);
                        pressed.browser = icons::framed_icon_button(
                            ui,
                            Role::Normal,
                            chat.enabled,
                            Icon::Globe,
                            egui::Vec2::splat(BUTTON),
                        )
                        .on_hover_text(&hover)
                        .on_disabled_hover_text(&hover)
                        .clicked();
                    }
                }
                if show_start {
                    // green while it is the thing to do
                    let role = if can_press_start && first_model {
                        Role::Primary
                    } else {
                        Role::Normal
                    };
                    pressed.start = play_button(
                        ui,
                        role,
                        can_press_start,
                        &format!("{start_label}: {start_hint}"),
                    );
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    title(ui, &display);
                    let p = theme::palette();
                    if runs_this {
                        state_words(ui, p.ok, "Running");
                    } else if loading_this {
                        state_words(ui, p.warn, "Loading...");
                    } else if state == ServerState::Starting && (named_this || lists) {
                        state_words(ui, p.warn, "Starting...");
                    }
                });
            });
        });
        if pressed.start {
            self.start_pressed_at = Some(std::time::Instant::now());
            if loadable {
                self.load_selected();
            } else if switchable {
                self.switch_to_selected();
            } else {
                self.start_selected(ui.ctx());
            }
        }
        if pressed.stop {
            self.press_stop();
        }
        if pressed.connect {
            self.show_api_info = !self.show_api_info;
        }
        if pressed.browser {
            self.open_browser_chat();
        }
    }

    /// The head of the page of an engine that runs on its own (ComfyUI), or of one that lists its models and has listed
    /// none yet (Ollama): its name, what it is doing, and Start, its page in the browser, Connect an app and Stop.
    pub(super) fn show_alone_header(
        &mut self,
        ui: &mut egui::Ui,
        state: ServerState,
        active: bool,
    ) {
        let name = self.engine_name();
        let server = self.server_name();
        let can_stop = matches!(state, ServerState::Starting | ServerState::Running);
        let web_ui = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.clone());
        let can_start = !self.engine_missing();
        let mut pressed = HeaderPress::default();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if can_stop {
                    pressed.stop = stop_button(ui, &server);
                }
                if state == ServerState::Running {
                    pressed.connect = super::api_info::connect_button(
                        ui,
                        egui::Vec2::splat(BUTTON),
                        &format!("Connect an app: how to connect an app to the running {server}."),
                    );
                    if let Some(web_ui) = &web_ui {
                        pressed.browser = theme::button_sized(
                            ui,
                            Role::Primary,
                            true,
                            web_ui.label.as_str(),
                            egui::vec2(0.0, BUTTON),
                        )
                        .on_hover_text(format!("Open the running {server} in your browser."))
                        .clicked();
                    }
                }
                if !active {
                    pressed.start = play_button(
                        ui,
                        Role::Primary,
                        can_start,
                        &format!("Start: start {server} (Ctrl+Enter)."),
                    );
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    title(ui, &name);
                    let p = theme::palette();
                    match state {
                        ServerState::Running => state_words(ui, p.ok, "Running"),
                        ServerState::Starting => state_words(ui, p.warn, "Starting..."),
                        _ => {}
                    }
                });
            });
        });
        if pressed.start {
            self.start_pressed_at = Some(std::time::Instant::now());
            self.start_selected(ui.ctx());
        }
        if pressed.stop {
            self.press_stop();
        }
        if pressed.connect {
            self.show_api_info = !self.show_api_info;
        }
        if pressed.browser {
            if let Some(url) = web_ui.as_ref().and_then(|w| self.web_ui_url(w)) {
                let _ = webbrowser::open(&url);
            }
        }
    }

    /// Stop, pressed in the head of the page: the server stops. Not the second click of a double-click on Start
    /// (within 0.8 s), as Stop takes the place of Start under the pointer.
    fn press_stop(&mut self) {
        let just_started = self
            .start_pressed_at
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(800));
        if !just_started {
            self.stop_server();
        }
    }
}

/// Pressed in the head of the page this frame.
#[derive(Default)]
struct HeaderPress {
    start: bool,
    stop: bool,
    connect: bool,
    browser: bool,
}

/// The name in the head of the page, cut when there is no room for all of it.
fn title(ui: &mut egui::Ui, name: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(name)
                .size(20.0)
                .family(theme::semibold(ui))
                .color(theme::palette().text),
        )
        .truncate(),
    );
}

/// The play button: `hint` says what it does when the pointer rests on it, also while it cannot be pressed.
fn play_button(ui: &mut egui::Ui, role: Role, enabled: bool, hint: &str) -> bool {
    let play = icons::framed_icon_button(ui, role, enabled, Icon::Play, egui::Vec2::splat(BUTTON))
        .on_hover_text(hint)
        .on_disabled_hover_text(hint);
    help::mark_spot(ui, Spot::Play, play.rect);
    play.clicked()
}

/// The stop button of the running `server`.
fn stop_button(ui: &mut egui::Ui, server: &str) -> bool {
    icons::framed_icon_button(
        ui,
        Role::Danger,
        true,
        Icon::Stop,
        egui::Vec2::splat(BUTTON),
    )
    .on_hover_text(format!("Stop the running {server} (Ctrl+Enter)"))
    .clicked()
}

/// What the model is doing, after its name: a dot and a word in the colour of the state.
fn state_words(ui: &mut egui::Ui, color: egui::Color32, words: &str) {
    ui.add_space(6.0);
    theme::dot(ui, 8.0, 4.0, color);
    ui.label(egui::RichText::new(words).color(color));
}
