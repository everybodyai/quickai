//! The model picker group of the main window (panel `model_bar`).

use eframe::egui;

use super::icons::{self, Icon};
use super::shell::RowAction;
use super::theme::{self, Role, Tone};
use super::App;
use crate::events::ServerState;
use crate::hardware;

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
    /// done with it: Start (or Load model, or Use this model), Connect an app and Stop while it runs, and its menu.
    /// Under the name, what is known of it as chips.
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
                let more = icons::icon_button(ui, Icon::More, egui::vec2(30.0, height), true, false)
                    .on_hover_text("Details, Show in folder, Copy name, Delete");
                egui::Popup::menu(&more)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                    .show(|ui| {
                        pressed.menu = self.model_menu(ui, index, lists);
                    });
                if can_stop {
                    pressed.stop = theme::button_sized(
                        ui,
                        Role::Danger,
                        true,
                        "Stop",
                        egui::vec2(64.0, height),
                    )
                    .on_hover_text(format!("Stop the running {server} (Ctrl+Enter)."))
                    .clicked();
                }
                if state == ServerState::Running {
                    pressed.connect = theme::button_sized(
                        ui,
                        Role::Normal,
                        true,
                        "Connect an app",
                        egui::vec2(0.0, height),
                    )
                    .on_hover_text(
                        "The address to give another app, and a chat in your browser when the engine has one.",
                    )
                    .clicked();
                }
                if show_start {
                    pressed.start = theme::button_sized(
                        ui,
                        if can_press_start && first_model {
                            Role::Primary
                        } else {
                            Role::Normal
                        },
                        can_press_start,
                        start_label,
                        egui::vec2(76.0, height),
                    )
                    .on_hover_text(start_hint)
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
        self.show_header_facts(ui, &model);
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
        if let Some(action) = pressed.menu {
            self.do_row_action(ui.ctx(), index, action);
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
                if can_stop {
                    stop = theme::button_sized(
                        ui,
                        Role::Danger,
                        true,
                        "Stop",
                        egui::vec2(64.0, height),
                    )
                    .on_hover_text(format!("Stop the running {server} (Ctrl+Enter)."))
                    .clicked();
                }
                if state == ServerState::Running {
                    connect = theme::button_sized(
                        ui,
                        Role::Normal,
                        true,
                        "Connect an app",
                        egui::vec2(0.0, height),
                    )
                    .on_hover_text(format!(
                        "Show how to connect an app to the running {server}."
                    ))
                    .clicked();
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
                    start = theme::button_sized(
                        ui,
                        Role::Primary,
                        !self.engine_missing(),
                        "Start",
                        egui::vec2(76.0, height),
                    )
                    .on_hover_text(format!("Start {server} (Ctrl+Enter)."))
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

    /// What is known of the chosen model, under its name: its size and quantization in words, then chips (how fast it
    /// will be here, what it can do, that it is the one recommended), then the first sentence of its note.
    fn show_header_facts(&self, ui: &mut egui::Ui, model: &crate::models::ModelEntry) {
        let mut lead = vec![super::shell::size_text(model.size_bytes)];
        if let Some(q) = super::model_details::quantization(&model.filename) {
            lead.push(q);
        }
        let mut chips = Vec::new();
        let mut sentence = None;
        if self.lists_models() {
            // what the engine says the model can do is a chip each; `completion` is what every model that chats
            // does, and is not worth a chip
            chips.extend(
                model
                    .note
                    .split(", ")
                    .filter(|word| !word.is_empty() && *word != "completion")
                    .map(|word| Fact {
                        text: word.to_string(),
                        tone: None,
                        hover: super::named_models::capability_help(word).to_string(),
                    }),
            );
            if self.named.stale {
                chips.push(Fact {
                    text: "from the last run".to_string(),
                    tone: None,
                    hover: format!(
                        "(as {} listed them the last time it ran)",
                        self.engine_name()
                    ),
                });
            }
        } else {
            let (note, mode) = split_mode(&model.note);
            chips.extend(mode);
            sentence = (!note.is_empty()).then_some(note);
        }
        if self.recommended_filename.as_deref() == Some(model.filename.as_str()) {
            chips.push(self.recommended_fact());
        }
        self.show_model_facts(ui, Some(lead.join(" · ")), chips, sentence, true);
    }

    /// How fast the selected model is likely to run here: the words of the chip, its tone and what its hover text says. It is
    /// said of the model, so it is drawn with the model (it was an icon in the title row).
    fn speed_chip_parts(&self) -> (String, Option<Tone>, String) {
        let size = self
            .models
            .get(self.selected)
            .map(|m| m.size_bytes)
            .unwrap_or(0);
        let tier = hardware::speed_tier(size, self.total_ram_bytes, self.gpu_offload());
        let tone = match tier {
            hardware::SpeedTier::Fast => Some(Tone::Good),
            hardware::SpeedTier::Medium => None,
            hardware::SpeedTier::Slow => Some(Tone::Warn),
        };
        (
            format!("{} on this computer", tier.label()),
            tone,
            format!(
                "Estimated speed for the selected model on this machine: {}. \
                 Rough estimate (model size vs. detected RAM, and whether GPU \
                 offload is on) — not a live benchmark.",
                tier.label()
            ),
        )
    }

    /// The badge that says the picked model is the one this computer is recommended: what it rests on is said when the
    /// pointer is on it.
    pub(super) fn recommended_fact(&self) -> Fact {
        Fact {
            text: "★ Recommended".to_string(),
            tone: Some(Tone::Good),
            hover: format!(
                "Recommended for this machine (~{} RAM detected{}). A starting point, not a guarantee: \
                 based on total RAM and whether GPU offload is enabled, favoring a smaller/faster \
                 model on CPU-only setups since a big model stays slow token-by-token no matter how \
                 much RAM is free. This can't see actual VRAM, so a model with GPU offload on can \
                 still run slower than expected if it doesn't fully fit on the GPU.",
                hardware::format_bytes(self.total_ram_bytes),
                if self.gpu_offload() { "" } else { ", CPU only" }
            ),
        }
    }

    /// What is said of the picked model under the picker, on one row: the chips (what it can do, that it is the
    /// recommended one, that what is said of it is from the last run), then a sentence of what the application knows
    /// of it, cut at its first full stop and whole when the pointer is on it. `with_speed` puts the speed chip in
    /// front, for a picker whose row had no room for it. Nothing is drawn when there is nothing to say.
    pub(super) fn show_model_facts(
        &self,
        ui: &mut egui::Ui,
        lead: Option<String>,
        chips: Vec<Fact>,
        sentence: Option<&str>,
        with_speed: bool,
    ) {
        // how fast a model is, is this computer's guess, and says nothing of one that runs on another
        let with_speed = with_speed && self.remote_in_use().is_none();
        if chips.is_empty() && sentence.is_none() && !with_speed {
            return;
        }
        ui.add_space(2.0);
        // a row of small print and chips is as high as they are and not as a button (the height of a wrapped row is
        // the room it is begun with, so it is set on the ui that begins it)
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().interact_size.y = 20.0;
            ui.horizontal_wrapped(|ui| {
                if let Some(lead) = &lead {
                    ui.label(theme::muted(lead.as_str()));
                }
                if with_speed {
                    let (text, tone, hover) = self.speed_chip_parts();
                    theme::chip(ui, &text, tone).on_hover_text(hover);
                }
                for fact in chips {
                    let chip = theme::chip(ui, &fact.text, fact.tone);
                    if !fact.hover.is_empty() {
                        chip.on_hover_text(fact.hover);
                    }
                }
                if let Some(sentence) = sentence {
                    let first = first_sentence(sentence);
                    let label = ui.label(theme::muted(first));
                    if first.len() < sentence.len() {
                        label.on_hover_text(sentence);
                    }
                }
            });
        });
    }
}

/// Pressed in the head of the page this frame.
#[derive(Default)]
struct HeaderPress {
    start: bool,
    stop: bool,
    connect: bool,
    menu: Option<RowAction>,
}

/// What the model is doing, after its name: a dot and a word in the colour of the state.
fn state_words(ui: &mut egui::Ui, color: egui::Color32, words: &str) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(egui::RichText::new(words).color(color));
}
