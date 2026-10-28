//! The main window: a menu bar with the engine, the theme, the guides and Settings at its right end; the models in a
//! list down the left, under the button that adds one; and beside them the chosen model, with Start next to its name,
//! one line that says how it will run, and the chat and the log as tabs under it. The status bar at the foot always
//! says what the server is doing (UI v2, C175).
//!
//! What a person does first is on the page and nothing else: an engine that is not installed has its Install button
//! alone, and a first run has the box that adds a model with the models suggested for this computer. An engine that
//! runs on its own (ComfyUI) keeps files of several kinds rather than one model to start, and has a page of its own
//! for them (the workbench): it has the whole of the middle, no list on the left, and the log at the foot.

use eframe::egui;

use super::icons::{self, Icon};
use super::theme;
use super::{App, Tab};
use crate::events::ServerState;

/// Where the documentation and the privacy policy are read.
pub(super) const DOCS_URL: &str = "https://github.com/everybodyai/quickai/tree/main/docs";
const PRIVACY_URL: &str = "https://github.com/everybodyai/quickai/blob/main/PRIVACY.md";
const WEBSITE_URL: &str = "https://everybodyai.dev/quickai";

/// The height of a row of the list of models: the name, and under it its size and what else is known.
const MODEL_ROW: f32 = 42.0;

/// From how many models on the list it has a box that narrows it.
const FILTER_FROM: usize = 8;

/// The size of the buttons that are a picture at the right of the menu bar.
const BAR_ICON: egui::Vec2 = egui::vec2(30.0, 26.0);

impl App {
    /// Everything of the main window but the windows of its own (Settings, About, the recommended models...).
    pub(super) fn show_window(&mut self, ui: &mut egui::Ui) {
        let active = self.session.is_active();
        let state = self.session.state();
        if state == ServerState::Running {
            self.run_reached_running = true;
        }
        self.take_shortcuts(ui.ctx(), state, active);

        egui::Panel::top("menu_bar")
            .frame(theme::bar_frame(egui::Margin::symmetric(6, 3)))
            .show(ui, |ui| self.show_menu_bar(ui, active));
        egui::Panel::bottom("status_bar")
            .frame(theme::bar_frame(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.show_status_bar(ui, state));
        if self.runs_alone() {
            // while the results of a look-up are on the workbench they have the whole of it: the log is not what is
            // being read then
            if !self.lookup.open {
                self.show_dock(ui);
            }
        } else {
            egui::Panel::left("models")
                .resizable(true)
                .default_size(220.0)
                .size_range(170.0..=340.0)
                .frame(theme::bar_frame(egui::Margin::ZERO))
                .show(ui, |ui| self.show_models_list(ui, active));
        }
        if self.runs_alone() {
            egui::CentralPanel::default()
                .frame(theme::page_frame())
                .show(ui, |ui| self.show_workbench_page(ui, state, active));
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::palette().page))
                .show(ui, |ui| self.show_page(ui, state, active));
        }
    }

    /// The keys that do what a button does: Ctrl+Enter starts or stops, Ctrl+K adds a model, Ctrl+L goes to the
    /// chat's box and Ctrl+, opens Settings (A6).
    fn take_shortcuts(&mut self, ctx: &egui::Context, state: ServerState, active: bool) {
        let command = egui::Modifiers::COMMAND;
        let (run, add, chat, settings) = ctx.input_mut(|i| {
            (
                i.consume_key(command, egui::Key::Enter),
                i.consume_key(command, egui::Key::K),
                i.consume_key(command, egui::Key::L),
                i.consume_key(command, egui::Key::Comma),
            )
        });
        if run {
            if matches!(state, ServerState::Starting | ServerState::Running) {
                self.stop_companion();
                self.session.stop();
            } else if self.start_possible(active).0 {
                self.start_selected(ctx);
            }
        }
        if add && !self.runs_alone() {
            self.open_add_model();
        }
        if chat && self.can_chat() {
            self.active_tab = Tab::Chat;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("chat_input")));
        }
        if settings {
            self.toggle_settings();
        }
    }

    // ---- the menu bar -------------------------------------------------------------------------------

    fn show_menu_bar(&mut self, ui: &mut egui::Ui, active: bool) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Configurations…").clicked() {
                    self.show_configurations = true;
                    self.raise_configurations = true;
                }
                if ui.button("Settings…").clicked() {
                    self.toggle_settings();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Models", |ui| {
                if ui
                    .add_enabled(!self.runs_alone(), egui::Button::new("Add a model…"))
                    .clicked()
                {
                    self.open_add_model();
                }
                if ui
                    .add_enabled(self.can_recommend(), egui::Button::new("Recommended…"))
                    .clicked()
                {
                    self.open_catalog();
                }
                ui.separator();
                if ui
                    .add_enabled(!active, egui::Button::new("Refresh the list"))
                    .clicked()
                {
                    self.refresh_list(ui.ctx());
                }
                if ui.button("Open the models folder").clicked() {
                    super::workbench::open_folder(&self.models_dir);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Getting started").clicked() {
                    self.open_guide(super::guide::Guide::GettingStarted);
                }
                if ui.button("Keyboard shortcuts").clicked() {
                    self.open_guide(super::guide::Guide::Shortcuts);
                }
                ui.separator();
                if ui.button("Documentation").clicked() {
                    let _ = webbrowser::open(DOCS_URL);
                }
                if ui.button("Privacy policy").clicked() {
                    let _ = webbrowser::open(PRIVACY_URL);
                }
                if ui.button("QuickAI on the web").clicked() {
                    let _ = webbrowser::open(WEBSITE_URL);
                }
                ui.separator();
                if ui.button("About QuickAI").clicked() {
                    self.show_about = true;
                }
            });
            // at the right end, what is set once in a while: the engine, the theme, the guides and Settings
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if icons::icon_button(ui, Icon::Gear, BAR_ICON, true, self.show_settings)
                    .on_hover_text("Settings (Ctrl+,)")
                    .clicked()
                {
                    self.toggle_settings();
                }
                let help = icons::icon_button(ui, Icon::Help, BAR_ICON, true, false)
                    .on_hover_text("Getting started and guides");
                egui::Popup::menu(&help)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                    .width(270.0)
                    .show(|ui| {
                        ui.set_width(270.0);
                        self.show_guide_menu(ui);
                    });
                let dark = self.dark_mode;
                if icons::icon_button(
                    ui,
                    if dark { Icon::Sun } else { Icon::Moon },
                    BAR_ICON,
                    true,
                    false,
                )
                .on_hover_text(if dark {
                    "Switch to the light theme"
                } else {
                    "Switch to the dark theme"
                })
                .clicked()
                {
                    self.set_dark_mode(ui.ctx(), !dark);
                }
                ui.add_space(4.0);
                self.show_engine_dropdown(ui);
            });
        });
    }

    /// Makes the theme `dark` or light, at once, and keeps it.
    pub(super) fn set_dark_mode(&mut self, ctx: &egui::Context, dark: bool) {
        self.dark_mode = dark;
        theme::apply(ctx, dark);
        self.save_settings();
    }

    /// Whether Start can be pressed now, and whether what it does is load the chosen model into the running engine.
    pub(super) fn start_possible(&self, active: bool) -> (bool, bool) {
        let loadable = self.can_load_selected();
        let can_start = (!self.models.is_empty() || self.lists_models() || self.runs_alone())
            && !self.engine_missing();
        ((!active && can_start) || loadable, loadable)
    }

    /// Reads the list of models again: the folders, or what the running engine says it has.
    pub(super) fn refresh_list(&mut self, ctx: &egui::Context) {
        if self.lists_models() {
            self.refresh_models(ctx);
        } else {
            self.rescan_models();
        }
    }

    // ---- the status bar --------------------------------------------------------------------------------

    fn show_status_bar(&mut self, ui: &mut egui::Ui, state: ServerState) {
        let status = self.run_status_text(state);
        let companion = self.companion_status();
        // a tunnel to another computer being opened is a start that is on its way
        let shown = if self.remote_opening() {
            ServerState::Starting
        } else {
            state
        };
        ui.horizontal(|ui| {
            ui.set_min_height(20.0);
            theme::state_indicator(ui, shown, &status).on_hover_text(
                "What the server is doing now. Start and Stop are beside the model's name.",
            );
            if let Some((text, companion_state)) = companion {
                ui.separator();
                ui.colored_label(theme::state_color(companion_state), text);
            }
            // where the models run, at the right end, and the menu that changes it
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.show_where_switch(ui);
            });
        });
    }

    // ---- the list of models ----------------------------------------------------------------------------

    fn show_models_list(&mut self, ui: &mut egui::Ui, active: bool) {
        let p = theme::palette();
        ui.add_space(10.0);
        // the one way to add a model, wherever it comes from: a search, a link, or a file of this computer
        let ready = !self.needs_install();
        let server = self.server_name();
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let width = ui.available_width() - 10.0;
            let button = add_model_button(ui, width, ready);
            let button = if ready {
                button.on_hover_text(
                    "Search for a model, paste a link, or add a file you already have (Ctrl+K).",
                )
            } else {
                button.on_hover_text(format!("Install {server} first: the page has its button."))
            };
            if ready && button.clicked() {
                self.open_add_model();
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let family = theme::semibold(ui);
            ui.label(
                egui::RichText::new("MODELS")
                    .size(11.5)
                    .family(family)
                    .color(p.muted),
            );
            // how much room they take, all of them
            if !self.models.is_empty() {
                ui.label(
                    egui::RichText::new(size_text(super::model_details::total_bytes(&self.models)))
                        .size(11.0)
                        .color(p.faint),
                )
                .on_hover_text("The room the models listed take on the disk.");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if icons::icon_button(ui, Icon::Refresh, egui::vec2(24.0, 20.0), !active, false)
                    .on_hover_text(if self.lists_models() {
                        format!(
                            "Ask the running {} which models it has. Stopped, this shows what it listed the \
                             last time it ran.",
                            self.server_name()
                        )
                    } else {
                        "Read the models folders again, for a file you put there by hand.".to_string()
                    })
                    .clicked()
                {
                    self.refresh_list(ui.ctx());
                }
            });
        });
        ui.add_space(2.0);
        // a long list has a box that narrows it, by name
        let long = self.models.len() > FILTER_FROM;
        if long {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.model_filter)
                        .hint_text("Filter models...")
                        .desired_width(ui.available_width() - 8.0),
                );
            });
            ui.add_space(4.0);
        } else {
            self.model_filter.clear();
        }
        let words = self.model_filter.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("models_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if self.models.is_empty() {
                    // in a margin of its own: wrapped in a row, only the first line kept the indent
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 8,
                            top: 4,
                            bottom: 0,
                        })
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("No models yet").color(p.faint));
                        });
                    return;
                }
                let mut picked = None;
                let mut asked: Option<(usize, RowAction)> = None;
                let lists = self.lists_models();
                let mut shown = 0;
                for (i, m) in self.models.iter().enumerate() {
                    let named = format!("{} {}", m.display, m.filename).to_lowercase();
                    if !words.is_empty() && !words.split_whitespace().all(|w| named.contains(w)) {
                        continue;
                    }
                    shown += 1;
                    let recommended =
                        self.recommended_filename.as_deref() == Some(m.filename.as_str());
                    let mut detail = vec![size_text(m.size_bytes)];
                    if recommended {
                        detail.push("recommended".to_string());
                    }
                    if m.bundled {
                        detail.push("bundled".to_string());
                    }
                    let running = self.runs_model(i);
                    let row = model_row(
                        ui,
                        &m.display,
                        &detail.join(" · "),
                        i == self.selected,
                        running,
                    );
                    if row.clicked() {
                        picked = Some(i);
                    }
                    // what can be done with a model, where it is: right-click it
                    row.context_menu(|ui| {
                        if let Some(action) = self.model_menu(ui, i, lists) {
                            asked = Some((i, action));
                        }
                    });
                }
                if shown == 0 {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(10.0);
                        ui.label(theme::muted("No model matches."));
                    });
                }
                if let Some((i, action)) = asked {
                    self.do_row_action(ui.ctx(), i, action);
                }
                if let Some(i) = picked.filter(|i| *i != self.selected) {
                    self.selected = i;
                    // a model picked anew gets its own defaults (a smaller context for a bigger model), still only a
                    // starting point: the boxes stay free to change
                    self.apply_model_defaults();
                }
            });
    }

    /// The items of a model's menu (right-click in the list, or the ... beside its name): Details, Show in folder, Copy
    /// name, Delete. What was chosen comes back; the menu is closed.
    pub(super) fn model_menu(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        lists: bool,
    ) -> Option<RowAction> {
        let mut asked = None;
        if ui.button("Details…").clicked() {
            asked = Some(RowAction::Details);
        }
        if !lists && ui.button("Show in folder").clicked() {
            asked = Some(RowAction::ShowInFolder);
        }
        if ui.button("Copy name").clicked() {
            asked = Some(RowAction::CopyName);
        }
        // what came with the application is never the person's to delete: no item, rather than one always greyed
        let shipped = self
            .models
            .get(index)
            .is_some_and(|m| m.bundled || m.path.starts_with(&self.bundled_models_dir));
        if !shipped {
            ui.separator();
            let deletable = self.delete_request(index);
            let item = ui
                .add_enabled(deletable.is_ok(), egui::Button::new("Delete…"))
                .on_disabled_hover_text(deletable.err().unwrap_or_default());
            if item.clicked() {
                asked = Some(RowAction::Delete);
            }
        }
        if asked.is_some() {
            ui.close();
        }
        asked
    }

    /// Does what a model's menu was asked to.
    pub(super) fn do_row_action(&mut self, ctx: &egui::Context, index: usize, action: RowAction) {
        match action {
            RowAction::Delete => self.ask_to_delete_from_window(index),
            RowAction::ShowInFolder => {
                if let Some(folder) = self.models.get(index).and_then(|m| m.path.parent()) {
                    super::workbench::open_folder(folder);
                }
            }
            RowAction::Details => {
                self.details_of = self.models.get(index).map(|m| m.filename.clone());
            }
            RowAction::CopyName => {
                if let Some(m) = self.models.get(index) {
                    ctx.copy_text(m.filename.clone());
                }
            }
        }
    }

    /// Whether the model at `index` is the one the server runs (or, for an engine that lists them, has loaded).
    pub(super) fn runs_model(&self, index: usize) -> bool {
        if self.session.state() != ServerState::Running {
            return false;
        }
        self.models.get(index).is_some_and(|m| {
            self.session.model_name() == Some(m.display.as_str())
                || (self.lists_models() && self.is_loaded_model(&m.filename))
        })
    }

    // ---- the page ----------------------------------------------------------------------------------------

    /// The middle of the window: what is to be done first, or the chosen model with its chat and its log.
    fn show_page(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        if self.needs_install() {
            self.show_setup_page(ui);
            return;
        }
        if self.models.is_empty() {
            self.show_first_run_page(ui, state, active);
            return;
        }
        // the head of the page scrolls in at most three fifths of the window (a crash said is long), so that the tabs keep
        // the rest, the chat's box with them; with Change open, what is changed is what is looked at, and it has more
        let room = ui.available_height();
        let share = if self.change_open { 0.85 } else { 0.6 };
        egui::ScrollArea::vertical()
            .id_salt("page_head")
            .max_height((room * share).max(140.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 16,
                        top: 14,
                        bottom: 8,
                    })
                    .show(ui, |ui| {
                        self.show_model_header(ui, state, active);
                        self.show_run_line(ui);
                        self.show_diagnostics(ui, state);
                    });
            });
        self.show_tabs(ui);
    }

    /// The page of an engine that is not installed: what it is, one button that installs it, and the way back.
    fn show_setup_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("setup_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space((ui.available_height() * 0.22).max(24.0));
                theme::column(ui, 560.0, |ui| self.show_engine_setup(ui));
            });
    }

    /// The page of a first run (no models yet): the box that adds a model, with the models that suit this computer.
    fn show_first_run_page(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        // an engine that lists its models (Ollama) has none to show until it runs: its row, with Start, is over the box (one
        // whose Ollama has models already lists them once started), and its tabs under it, the log first: what it said when
        // it stopped before it was ready is there
        let lists = self.lists_models();
        if lists {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 16,
                    top: 14,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    self.show_alone_header(ui, state, active);
                    self.show_run_line(ui);
                    self.show_diagnostics(ui, state);
                });
        }
        let mut area = egui::ScrollArea::vertical()
            .id_salt("first_run_page")
            .auto_shrink([false, false]);
        if lists {
            area = area.max_height((ui.available_height() * 0.62).max(160.0));
        }
        area.show(ui, |ui| {
            ui.add_space(if lists { 24.0 } else { 48.0 });
            theme::column(ui, 640.0, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Add your first model")
                            .size(22.0)
                            .family(theme::semibold(ui))
                            .color(theme::palette().text),
                    );
                    ui.add_space(4.0);
                    ui.label(theme::muted(self.first_run_sentence()));
                });
                ui.add_space(14.0);
                self.show_add_model_body(ui);
            });
        });
        if lists {
            self.show_tabs(ui);
        }
    }

    /// The page of an engine that runs on its own (ComfyUI): its settings, the workbench, and what went wrong.
    fn show_workbench_page(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        egui::ScrollArea::vertical()
            .id_salt("page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::column(ui, theme::PAGE_WIDTH, |ui| {
                    // its files can be got before it is installed (they are kept in a folder of their own): the models
                    // area stays under the button that installs it
                    if self.needs_install() {
                        ui.add_space(24.0);
                        self.show_engine_setup(ui);
                        ui.add_space(24.0);
                        theme::card(ui, |ui| self.show_workbench(ui, active));
                        return;
                    }
                    self.show_alone_header(ui, state, active);
                    self.show_run_line(ui);
                    ui.add_space(10.0);
                    theme::card(ui, |ui| self.show_workbench(ui, active));
                    self.show_diagnostics(ui, state);
                });
            });
    }

    // ---- the tabs: the chat, the log and the live traffic ----------------------------------------------

    /// The tabs under the model: the chat first, for an engine that has one, then the log, and the live traffic
    /// when it is on. A run that went wrong puts a red dot on the log until it is read.
    fn show_tabs(&mut self, ui: &mut egui::Ui) {
        let live = self.traffic_proxy_on();
        let chat = self.can_chat();
        // a tab that is not there now shows the log, and stays the one chosen (it is kept in the settings)
        let shown = match self.active_tab {
            Tab::Live if !live => Tab::Main,
            Tab::Chat if !chat => Tab::Main,
            tab => tab,
        };
        let on_live = shown == Tab::Live;
        let on_chat = shown == Tab::Chat;
        let on_log = !on_live && !on_chat;
        if on_log {
            self.log_unread = false;
        }
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            // a model that runs and a conversation never opened: a dot says it is there to be tried
            let fresh = self.session.state() == ServerState::Running
                && self.session.model_name().is_some()
                && !self.chat.opened;
            if on_chat {
                self.chat.opened = true;
            }
            if chat
                && dock_tab_marked(ui, "Chat", on_chat, fresh && !on_chat, false)
                    .on_hover_text("Talk to the model that runs, right here (Ctrl+L).")
                    .clicked()
            {
                self.active_tab = Tab::Chat;
            }
            if dock_tab_marked(ui, "Log", on_log, self.log_unread && !on_log, true)
                .on_hover_text("What the server says as it starts and runs.")
                .clicked()
            {
                self.active_tab = Tab::Main;
            }
            if live
                && dock_tab(ui, "Live traffic", on_live)
                    .on_hover_text(
                        "Watch chat/completion requests and replies in real time as any client talks to this model.",
                    )
                    .clicked()
            {
                self.active_tab = Tab::Live;
            }
            if on_chat {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                    self.show_chat_actions(ui);
                });
            }
        });
        let rule = ui.max_rect().x_range();
        ui.painter().hline(
            rule,
            ui.cursor().top(),
            egui::Stroke::new(1.0, theme::palette().line),
        );
        match shown {
            Tab::Live => self.show_live_traffic_tab(ui),
            Tab::Chat => self.show_chat_tab(ui),
            Tab::Main => {
                egui::Frame::new()
                    .fill(theme::palette().log)
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        self.show_log_pane(ui);
                    });
            }
        }
    }

    // ---- the dock of the workbench: the log and the live traffic ----------------------------------------

    fn show_dock(&mut self, ui: &mut egui::Ui) {
        let live = self.traffic_proxy_on();
        // the workbench has no chat: the log is shown for it, and the tab chosen stays chosen
        let shown = match self.active_tab {
            Tab::Live if live => Tab::Live,
            _ => Tab::Main,
        };
        let on_live = shown == Tab::Live;
        // each tab keeps the height it was given
        let (id, height) = match shown {
            Tab::Live => ("dock_live", 320.0),
            _ => ("dock_log", 160.0),
        };
        egui::Panel::bottom(id)
            .resizable(true)
            .default_size(height)
            .size_range(90.0..=640.0)
            .frame(egui::Frame::new().fill(theme::palette().page))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    if dock_tab(ui, "Log", !on_live).clicked() {
                        self.active_tab = Tab::Main;
                    }
                    if live
                        && dock_tab(ui, "Live traffic", on_live)
                            .on_hover_text(
                                "Watch requests and replies in real time as any client talks to this engine.",
                            )
                            .clicked()
                    {
                        self.active_tab = Tab::Live;
                    }
                });
                let rule = ui.max_rect().x_range();
                ui.painter().hline(
                    rule,
                    ui.cursor().top(),
                    egui::Stroke::new(1.0, theme::palette().line),
                );
                match shown {
                    Tab::Live => self.show_live_traffic_tab(ui),
                    _ => self.show_log_pane(ui),
                }
            });
    }
}

/// The button at the head of the list that adds a model: a dashed outline, a plus and its words.
fn add_model_button(ui: &mut egui::Ui, width: f32, enabled: bool) -> egui::Response {
    let p = theme::palette();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = enabled && response.hovered();
        let ink = if enabled { p.text } else { p.faint };
        if hovered {
            painter.rect_filled(rect, 6.0, p.raised_hover.gamma_multiply(0.6));
        }
        let stroke = egui::Stroke::new(1.0, if hovered { p.muted } else { p.outline });
        let r = rect.shrink(0.5);
        // a dashed outline: the sides as dashes, the corners left open
        for (from, to) in [
            (
                r.left_top() + egui::vec2(5.0, 0.0),
                r.right_top() - egui::vec2(5.0, 0.0),
            ),
            (
                r.left_bottom() + egui::vec2(5.0, 0.0),
                r.right_bottom() - egui::vec2(5.0, 0.0),
            ),
            (
                r.left_top() + egui::vec2(0.0, 5.0),
                r.left_bottom() - egui::vec2(0.0, 5.0),
            ),
            (
                r.right_top() + egui::vec2(0.0, 5.0),
                r.right_bottom() - egui::vec2(0.0, 5.0),
            ),
        ] {
            painter.extend(egui::Shape::dashed_line(&[from, to], stroke, 4.0, 3.0));
        }
        let font = egui::TextStyle::Button.resolve(ui.style());
        let galley = painter.layout_no_wrap("Add a model".to_string(), font, ink);
        let total = 14.0 + 7.0 + galley.size().x;
        let left = rect.center().x - total / 2.0;
        icons::paint(
            painter,
            egui::Rect::from_min_size(
                egui::pos2(left, rect.center().y - 7.0),
                egui::vec2(14.0, 14.0),
            ),
            Icon::Plus,
            ink,
        );
        painter.galley(
            egui::pos2(left + 21.0, rect.center().y - galley.size().y / 2.0),
            galley,
            ink,
        );
    }
    response
}

/// What the menu of a model of the list was asked to do.
#[derive(Clone, Copy)]
pub(super) enum RowAction {
    Delete,
    ShowInFolder,
    CopyName,
    Details,
}

/// A size in the words a person reads: "1.1 GB", "250 MB".
pub(super) fn size_text(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{:.0} MB", mb.max(1.0))
    }
}

/// One model of the list: its name, and under it its size and what else is known, the chosen one tinted with a bar of
/// the accent along its left edge.
fn model_row(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    chosen: bool,
    running: bool,
) -> egui::Response {
    let p = theme::palette();
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, MODEL_ROW), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if chosen {
            painter.rect_filled(rect, 0, p.accent_soft);
            // the green of text, not the accent: the one thing filled with the accent is the button that matters
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
                0,
                p.accent_text,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, 0, p.raised_hover.gamma_multiply(0.6));
        }
        let left = rect.left() + 12.0;
        let room = (rect.right() - 8.0 - left).max(10.0);
        let title = egui::WidgetText::from(egui::RichText::new(name).color(p.text)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            room,
            egui::TextStyle::Body,
        );
        let small = egui::WidgetText::from(egui::RichText::new(detail).small().color(p.muted))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                room,
                egui::TextStyle::Small,
            );
        let top = rect.top() + (MODEL_ROW - title.size().y - small.size().y - 2.0) / 2.0;
        let below = top + title.size().y + 2.0;
        let title_right = left + title.size().x;
        let title_mid = top + title.size().y / 2.0;
        painter.galley(egui::pos2(left, top), title, p.text);
        painter.galley(egui::pos2(left, below), small, p.muted);
        // the model that runs has a green dot after its name
        if running {
            painter.circle_filled(egui::pos2(title_right + 8.0, title_mid), 3.5, p.ok);
        }
    }
    response.on_hover_text(name)
}

/// A tab with a dot after its name, when `marked`: something new is there, in green, or something went wrong, in red
/// (`bad`).
fn dock_tab_marked(
    ui: &mut egui::Ui,
    label: &str,
    open: bool,
    marked: bool,
    bad: bool,
) -> egui::Response {
    let response = dock_tab(ui, label, open);
    if marked && ui.is_rect_visible(response.rect) {
        let p = theme::palette();
        let centre = egui::pos2(response.rect.right() - 4.0, response.rect.center().y - 5.0);
        ui.painter()
            .circle_filled(centre, 3.0, if bad { p.bad } else { p.accent_text });
    }
    response
}

/// A tab of the dock: its name, and a line of the accent under the one that is open.
fn dock_tab(ui: &mut egui::Ui, label: &str, open: bool) -> egui::Response {
    let p = theme::palette();
    let text = egui::RichText::new(label).color(if open { p.text } else { p.muted });
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let size = egui::vec2(galley.size().x + 20.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if open { p.text } else { p.muted };
        ui.painter().galley(
            egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
        if open {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 4.0, rect.bottom() - 2.0),
                    egui::pos2(rect.right() - 4.0, rect.bottom()),
                ),
                0,
                p.accent_text,
            );
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::size_text;

    #[test]
    fn a_size_is_said_in_megabytes_below_a_gigabyte_and_in_gigabytes_above() {
        assert_eq!(size_text(250 * 1024 * 1024), "250 MB");
        assert_eq!(size_text(1_150_000_000), "1.1 GB");
        assert_eq!(size_text(12 * 1024 * 1024 * 1024), "12.0 GB");
        assert_eq!(size_text(0), "1 MB");
    }
}
