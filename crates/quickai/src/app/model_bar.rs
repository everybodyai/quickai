//! The model picker group of the main window (panel `model_bar`).

use eframe::egui;

use super::icons::{self, Icon};
use super::theme::{self, Role, Tone};
use super::App;
use crate::events::ServerState;

/// One thing that is said of the picked model, as a chip: its words, the tone it is drawn in and what is said more of
/// it when the pointer rests on it (nothing when it needs no more).
pub(super) struct Fact {
    pub text: String,
    pub tone: Option<Tone>,
    pub hover: String,
}

/// What the end of a note says of how the model is started (`models::say_modes` puts the sentence there): a chip of its
/// own, since it is not a part of the note that can be cut and left for the hover. The note without it comes with it.
pub(super) fn split_mode(note: &str) -> (&str, Option<Fact>) {
    if let Some(at) = note.rfind("Starts as an ") {
        let sentence = &note[at..];
        let label = sentence
            .trim_start_matches("Starts as an ")
            .trim_end_matches('.');
        let fact = Fact {
            text: label.to_string(),
            tone: None,
            hover: sentence.to_string(),
        };
        return (note[..at].trim_end(), Some(fact));
    }
    if let Some(at) = note.rfind("It is meant to start in the mode") {
        let fact = Fact {
            text: "mode this engine lacks".to_string(),
            tone: Some(Tone::Warn),
            hover: note[at..].to_string(),
        };
        return (note[..at].trim_end(), Some(fact));
    }
    (note, None)
}

/// `note` up to and including its first full stop (all of it when it has none).
pub(super) fn first_sentence(note: &str) -> &str {
    match note.find(". ") {
        Some(end) => &note[..=end],
        None => note,
    }
}

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
        let Some(model) = self.models.get(self.selected).cloned() else {
            return;
        };
        let index = self.selected;
        let lists = self.lists_models();
        let server = self.server_name();
        let loadable = self.can_load_selected();
        let switchable = self.can_switch_selected();
        let runs_this = self.runs_model(index);
        let named_this = self.session.model_name() == Some(model.display.as_str());
        let loading_this = self.session.loading() == Some(model.display.as_str())
            || self.session.loading() == Some(model.filename.as_str());
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
        let height = 30.0;
        let mut pressed = HeaderPress::default();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // while it runs, the things to do with it are pictures, each named when the pointer rests on it
                let square = egui::vec2(height, height);
                if can_stop {
                    pressed.stop =
                        icons::framed_icon_button(ui, Role::Danger, true, Icon::Stop, square)
                            .on_hover_text(format!("Stop the running {server} (Ctrl+Enter)"))
                            .clicked();
                }
                if state == ServerState::Running {
                    let plug = icons::framed_icon_button(
                        ui,
                        Role::Normal,
                        true,
                        Icon::Plug,
                        square,
                    )
                    .on_hover_text(
                        "Connect an app: the address to give another app (an editor, a script, a chat app), \
                         with an example.",
                    );
                    ui.data_mut(|d| d.insert_temp(super::api_info::connect_anchor_id(), plug.rect));
                    pressed.connect = plug.clicked();
                    if let Some((label, hover, enabled)) = self.browser_chat() {
                        pressed.browser =
                            icons::framed_icon_button(ui, Role::Normal, enabled, Icon::Globe, square)
                                .on_hover_text(format!("{}: {hover}", label.trim_end_matches('.')))
                                .on_disabled_hover_text(format!(
                                    "{}: {hover}",
                                    label.trim_end_matches('.')
                                ))
                                .clicked();
                    }
                }
                if show_start {
                    // a play button, green while it is the thing to do; what it does is said when the pointer rests on it
                    let role = if can_press_start && first_model {
                        Role::Primary
                    } else {
                        Role::Normal
                    };
                    let hint = format!("{start_label}: {start_hint}");
                    pressed.start =
                        icons::framed_icon_button(ui, role, can_press_start, Icon::Play, square)
                            .on_hover_text(&hint)
                            .on_disabled_hover_text(&hint)
                            .clicked();
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(model.display.as_str())
                                .size(20.0)
                                .family(theme::semibold(ui))
                                .color(theme::palette().text),
                        )
                        .truncate(),
                    );
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
        // Stop takes the place of Start: a second click of a double-click on Start (within 0.8 s) is not a Stop
        let just_started = self
            .start_pressed_at
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(800));
        if pressed.stop && !just_started {
            // the page goes with what it talks to
            self.stop_companion();
            self.session.stop();
        }
        if pressed.connect {
            self.show_api_info = !self.show_api_info;
        }
        if pressed.browser {
            self.open_browser_chat(state);
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
        let height = 30.0;
        let mut start = false;
        let mut stop = false;
        let mut connect = false;
        let mut open_page = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let square = egui::vec2(height, height);
                if can_stop {
                    stop = icons::framed_icon_button(ui, Role::Danger, true, Icon::Stop, square)
                        .on_hover_text(format!("Stop the running {server} (Ctrl+Enter)"))
                        .clicked();
                }
                if state == ServerState::Running {
                    let plug =
                        icons::framed_icon_button(ui, Role::Normal, true, Icon::Plug, square)
                            .on_hover_text(format!(
                                "Connect an app: how to connect an app to the running {server}."
                            ));
                    ui.data_mut(|d| d.insert_temp(super::api_info::connect_anchor_id(), plug.rect));
                    connect = plug.clicked();
                    if let Some(web_ui) = &web_ui {
                        open_page = theme::button_sized(
                            ui,
                            Role::Primary,
                            true,
                            web_ui.label.as_str(),
                            egui::vec2(0.0, height),
                        )
                        .on_hover_text(format!("Open the running {server} in your browser."))
                        .clicked();
                    }
                }
                if !active {
                    let hint = format!("Start: start {server} (Ctrl+Enter).");
                    start = icons::framed_icon_button(
                        ui,
                        Role::Primary,
                        !self.engine_missing(),
                        Icon::Play,
                        square,
                    )
                    .on_hover_text(&hint)
                    .on_disabled_hover_text(&hint)
                    .clicked();
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(name.as_str())
                            .size(20.0)
                            .family(theme::semibold(ui))
                            .color(theme::palette().text),
                    );
                    let p = theme::palette();
                    match state {
                        ServerState::Running => state_words(ui, p.ok, "Running"),
                        ServerState::Starting => state_words(ui, p.warn, "Starting..."),
                        _ => {}
                    }
                });
            });
        });
        if start {
            self.start_pressed_at = Some(std::time::Instant::now());
            self.start_selected(ui.ctx());
        }
        let just_started = self
            .start_pressed_at
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(800));
        if stop && !just_started {
            self.stop_companion();
            self.session.stop();
        }
        if connect {
            self.show_api_info = !self.show_api_info;
        }
        if open_page {
            if let (Some(web_ui), Some(port)) = (&web_ui, self.session.port()) {
                let path = if web_ui.path == "/" { "" } else { &web_ui.path };
                let _ = webbrowser::open(&format!("http://127.0.0.1:{port}{path}"));
            }
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

/// What the model is doing, after its name: a dot and a word in the colour of the state.
fn state_words(ui: &mut egui::Ui, color: egui::Color32, words: &str) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(egui::RichText::new(words).color(color));
}
