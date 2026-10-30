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

use super::help::{mark_spot, Spot};
use super::icons::{self, Icon};
use super::theme::{self, Tone};
use super::{App, Tab};
use crate::events::ServerState;
use crate::hardware::SpeedTier;
use crate::models::ModelEntry;

/// Where the documentation and the privacy policy are read (from Help), and the website (from About).
pub(super) const DOCS_URL: &str = "https://github.com/everybodyai/quickai/tree/main/docs";
// QuickAI's public documents are on its own site (quickai.everybodyai.dev), the policy of the released version
pub(super) const PRIVACY_URL: &str = "https://quickai.everybodyai.dev/privacy";
pub(super) const WEBSITE_URL: &str = "https://quickai.everybodyai.dev";
/// Where Help → Report a problem files an issue.
pub(super) const NEW_ISSUE_URL: &str = "https://github.com/everybodyai/quickai/issues/new";

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
            .show(ui, |ui| self.show_menu_bar(ui));
        // the first start asks about telemetry once, under the menu bar, over nothing (WP4.42)
        if self.tel.notice {
            egui::Panel::top("telemetry_notice")
                .frame(theme::bar_frame(egui::Margin::symmetric(12, 8)))
                .show(ui, |ui| self.show_telemetry_notice(ui));
        }
        egui::Panel::bottom("status_bar")
            .frame(theme::bar_frame(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.show_status_bar(ui, state));
        // a computer QuickAI sets up, chosen in the status bar: the page of its run (WP4.33)
        let remote_view = self.setup_in_use().is_some();
        if self.runs_alone() && !remote_view {
            // while the results of a look-up are on the workbench they have the whole of it: the log is not what is
            // being read then
            if !self.lookup.open {
                self.show_dock(ui);
            }
        } else if !self.runs_alone() {
            egui::Panel::left("models")
                .resizable(true)
                .default_size(220.0)
                .size_range(170.0..=340.0)
                .frame(theme::bar_frame(egui::Margin::ZERO))
                .show(ui, |ui| self.show_models_list(ui, active));
        }
        if remote_view {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::palette().page))
                .show(ui, |ui| self.show_remote_page(ui));
        } else if self.runs_alone() {
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
    /// chat's box, Ctrl+, opens Settings (A6) and F1 Help.
    fn take_shortcuts(&mut self, ctx: &egui::Context, state: ServerState, active: bool) {
        let command = egui::Modifiers::COMMAND;
        let (run, add, chat, settings, help) = ctx.input_mut(|i| {
            (
                i.consume_key(command, egui::Key::Enter),
                i.consume_key(command, egui::Key::K),
                i.consume_key(command, egui::Key::L),
                i.consume_key(command, egui::Key::Comma),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F1),
            )
        });
        if help {
            self.open_help();
        }
        if run && self.setup_in_use().is_some() {
            // the run on the computer QuickAI set up that is shown
            if self.setup_run().is_some_and(|r| r.running() || r.working()) {
                self.stop_setup_run();
            } else {
                self.start_setup_run(ctx, false);
            }
        } else if run {
            if matches!(state, ServerState::Starting | ServerState::Running) {
                self.stop_server();
            } else if self.start_possible(active) {
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

    fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Configurations…").clicked() {
                    self.open_configurations();
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
                // the list is read again by the small picture at the end of its heading
                if ui.button("Open the models folder").clicked() {
                    super::workbench::open_folder(&self.models_dir);
                }
            });
            // the rest (Getting started, the keys, the documentation online, the privacy policy) is in Help's own
            // window, and the address of the website in About
            ui.menu_button("Help", |ui| {
                if ui
                    .add(egui::Button::new("Help…").shortcut_text("F1"))
                    .clicked()
                {
                    self.open_help();
                }
                if ui.button("About QuickAI").clicked() {
                    // open already, somewhere behind the main window: it comes to the front
                    self.raise_about = self.show_about;
                    self.show_about = true;
                }
                // last, so that the items above stay where they were (WP4.42)
                if ui.button("Report a problem…").clicked() {
                    self.open_report_problem();
                }
            });
            // at the right end, what is set once in a while: the engine, the theme, Help and Settings
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let gear = icons::icon_button(ui, Icon::Gear, BAR_ICON, true, self.show_settings)
                    .on_hover_text("Settings (Ctrl+,)");
                mark_spot(ui, Spot::Settings, gear.rect);
                if gear.clicked() {
                    self.toggle_settings();
                }
                let help = icons::icon_button(ui, Icon::Help, BAR_ICON, true, self.help.open)
                    .on_hover_text("Help (F1)");
                mark_spot(ui, Spot::Help, help.rect);
                if help.clicked() {
                    self.open_help();
                }
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

    /// Whether Start can be pressed now (with a model running, it loads the chosen one into the engine).
    pub(super) fn start_possible(&self, active: bool) -> bool {
        let can_start = (!self.models.is_empty() || self.lists_models() || self.runs_alone())
            && !self.engine_missing();
        (!active && can_start) || self.can_load_selected()
    }

    /// What the speed of the models of the list is guessed from, worked out once for the list (it builds the engine's
    /// form): whether the graphics card is used, `None` while they run on another computer, of which nothing is guessed.
    pub(super) fn speed_basis(&self) -> Option<bool> {
        self.remote_in_use().is_none().then(|| self.gpu_offload())
    }

    /// How fast `m` should be on this computer, from `offload` (`speed_basis`); `None` when nothing is guessed.
    fn row_speed(&self, m: &ModelEntry, offload: Option<bool>) -> Option<SpeedTier> {
        let offload = offload.filter(|_| m.size_bytes > 0)?;
        Some(crate::hardware::speed_tier(
            m.size_bytes,
            self.total_ram_bytes,
            offload,
        ))
    }

    /// What the list says of a model under its name (option D of the samples in `mockups/header/`, C185: the facts left
    /// the head of the page for the list): its size, how fast it should be here (only when it is fast or slower),
    /// recommended, bundled, and how it is started when that is not as a chat model.
    pub(super) fn row_facts(
        &self,
        m: &ModelEntry,
        lists: bool,
        offload: Option<bool>,
    ) -> Vec<(String, Option<egui::Color32>)> {
        let p = theme::palette();
        let mut detail: Vec<(String, Option<egui::Color32>)> =
            vec![(size_text(m.size_bytes), None)];
        match self.row_speed(m, offload) {
            Some(tier @ SpeedTier::Fast) => {
                detail.push((tier.label().to_lowercase(), Some(p.accent_text)))
            }
            Some(tier @ SpeedTier::Slow) => {
                detail.push((tier.label().to_lowercase(), Some(p.warn)))
            }
            Some(SpeedTier::Medium) | None => {}
        }
        if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
            detail.push(("recommended".to_string(), Some(p.accent_text)));
        }
        if m.bundled {
            detail.push(("bundled".to_string(), None));
        }
        if !lists {
            if let (_, Some(mode)) = split_mode(&m.note) {
                detail.push((mode.text, mode.tone.map(|t| t.color())));
            }
        }
        detail
    }

    /// What the list says of a model when the pointer rests on its row: its name, what the engine says it can do or the
    /// first sentence of its note, and what the speed rests on.
    pub(super) fn row_hover(&self, m: &ModelEntry, lists: bool, offload: Option<bool>) -> String {
        let mut hover = m.display.clone();
        if let Some(tier) = self.row_speed(m, offload) {
            hover.push_str(&format!(
                "\n{} on this computer: a rough guess from its size, this computer's memory ({}) and whether the \
                 graphics card is used, not a measurement.",
                tier.label(),
                crate::hardware::format_bytes(self.total_ram_bytes)
            ));
        }
        if lists {
            // what the engine says it can do; `completion` is what every model that chats does
            for word in m
                .note
                .split(", ")
                .filter(|w| !w.is_empty() && *w != "completion")
            {
                hover.push_str(&format!("\n{word}"));
                let help = super::named_models::capability_help(word);
                if !help.is_empty() {
                    hover.push_str(&format!(": {help}"));
                }
            }
        } else {
            let (note, mode) = split_mode(&m.note);
            let first = first_sentence(note);
            if !first.is_empty() {
                hover.push_str(&format!("\n{first}"));
            }
            if let Some(mode) = mode {
                hover.push_str(&format!("\n{}", mode.hover));
            }
        }
        hover
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
        let (shown, status) = match self.remote_run_status() {
            Some(said) => said,
            None => {
                // a tunnel to another computer being opened is a start that is on its way
                let shown = if self.remote_opening() {
                    ServerState::Starting
                } else {
                    state
                };
                (shown, self.run_status_text(state))
            }
        };
        let companion = self
            .companion_status()
            .filter(|_| self.setup_in_use().is_none());
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
            mark_spot(ui, Spot::AddModel, button.rect);
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
            // what an engine that lists its models listed the last time it ran, while it is stopped
            if self.lists_models() && self.named.stale {
                ui.label(egui::RichText::new("from the last run").size(11.0).color(p.faint))
                    .on_hover_text(format!(
                        "As {} listed them the last time it ran. Start it to see what it has now.",
                        self.engine_name()
                    ));
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
                let offload = self.speed_basis();
                let mut shown = 0;
                for (i, m) in self.models.iter().enumerate() {
                    let named = format!("{} {}", m.display, m.filename).to_lowercase();
                    if !words.is_empty() && !words.split_whitespace().all(|w| named.contains(w)) {
                        continue;
                    }
                    shown += 1;
                    let detail = self.row_facts(m, lists, offload);
                    let running = self.runs_model(i);
                    let (row, more) =
                        model_row(ui, &m.display, &detail, i == self.selected, running);
                    // what is said of it when the pointer rests on it, made only then
                    let row = row.on_hover_ui(|ui| {
                        ui.label(self.row_hover(m, lists, offload));
                    });
                    if row.clicked() {
                        picked = Some(i);
                    }
                    // what can be done with a model, where it is: its … (on the row the pointer is on, and on the
                    // chosen one), or a right-click
                    row.context_menu(|ui| {
                        if let Some(action) = self.model_menu(ui, i, lists) {
                            asked = Some((i, action));
                        }
                    });
                    if let Some(more) = more {
                        egui::Popup::menu(&more)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                            .show(|ui| {
                                if let Some(action) = self.model_menu(ui, i, lists) {
                                    asked = Some((i, action));
                                }
                            });
                    }
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

    /// The items of a model's menu (right-click its row in the list, or the … on the row): Details, Show in folder, Copy
    /// name, Delete. What was chosen comes back; the menu is closed.
    fn model_menu(&self, ui: &mut egui::Ui, index: usize, lists: bool) -> Option<RowAction> {
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
    fn do_row_action(&mut self, ctx: &egui::Context, index: usize, action: RowAction) {
        match action {
            RowAction::Delete => self.ask_to_delete_from_window(index),
            RowAction::ShowInFolder => {
                if let Some(folder) = self.models.get(index).and_then(|m| m.path.parent()) {
                    super::workbench::open_folder(folder);
                }
            }
            RowAction::Details => {
                self.raise_details = self.details_of.is_some();
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
    pub(super) fn show_tabs(&mut self, ui: &mut egui::Ui) {
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
            let log = dock_tab_marked(ui, "Log", on_log, self.log_unread && !on_log, true)
                .on_hover_text("What the server says as it starts and runs.");
            mark_spot(ui, Spot::LogTab, log.rect);
            if log.clicked() {
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
enum RowAction {
    Delete,
    ShowInFolder,
    CopyName,
    Details,
}

/// How a model is started, when that is not as a chat model: the word of its row, the tone it is said in, and what is
/// said of it when the pointer rests on the row.
pub(super) struct ModeWord {
    pub text: String,
    pub tone: Option<Tone>,
    pub hover: String,
}

/// What the end of a note says of how the model is started (`models::say_modes` puts the sentence there), as a word of
/// its own, since it is not a part of the note that can be cut and left for the hover. The note without it comes with it.
pub(super) fn split_mode(note: &str) -> (&str, Option<ModeWord>) {
    if let Some(at) = note.rfind("Starts as an ") {
        let sentence = &note[at..];
        let label = sentence
            .trim_start_matches("Starts as an ")
            .trim_end_matches('.');
        let word = ModeWord {
            text: label.to_string(),
            tone: None,
            hover: sentence.to_string(),
        };
        return (note[..at].trim_end(), Some(word));
    }
    if let Some(at) = note.rfind("It is meant to start in the mode") {
        let word = ModeWord {
            text: "mode this engine lacks".to_string(),
            tone: Some(Tone::Warn),
            hover: note[at..].to_string(),
        };
        return (note[..at].trim_end(), Some(word));
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

/// A model of the list: its name (a green dot after it when it runs), and under it what is known of it as words, a
/// few of them coloured (`fast` and `recommended` green, `slower` amber). Chosen, it is green at its left edge. On the
/// chosen row and the one the pointer is on there is a … at its right end, whose response comes back with the row's
/// for the menu of what can be done with the model.
fn model_row(
    ui: &mut egui::Ui,
    name: &str,
    detail: &[(String, Option<egui::Color32>)],
    chosen: bool,
    running: bool,
) -> (egui::Response, Option<egui::Response>) {
    let p = theme::palette();
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, MODEL_ROW), egui::Sense::click());
    let more_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 20.0, rect.center().y),
        egui::vec2(26.0, 24.0),
    );
    let more_id = response.id.with("more");
    let menu_open = egui::Popup::is_id_open(ui.ctx(), more_id.with("popup"));
    let more = (chosen || response.contains_pointer() || menu_open)
        .then(|| ui.interact(more_rect, more_id, egui::Sense::click()));
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
        let right = if more.is_some() {
            more_rect.left() - 4.0
        } else {
            rect.right() - 8.0
        };
        let room = (right - left).max(10.0);
        let title = egui::WidgetText::from(egui::RichText::new(name).color(p.text)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            room,
            egui::TextStyle::Body,
        );
        let font = egui::TextStyle::Small.resolve(ui.style());
        let mut job = egui::text::LayoutJob::default();
        for (i, (text, color)) in detail.iter().enumerate() {
            if i > 0 {
                job.append(" · ", 0.0, egui::TextFormat::simple(font.clone(), p.muted));
            }
            job.append(
                text,
                0.0,
                egui::TextFormat::simple(font.clone(), color.unwrap_or(p.muted)),
            );
        }
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let small = painter.layout_job(job);
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
        if let Some(more) = &more {
            let lit = more.hovered() || menu_open;
            if lit {
                painter.rect_filled(more_rect, 4.0, p.raised_hover);
            }
            let icon = egui::Rect::from_center_size(more_rect.center(), egui::vec2(15.0, 15.0));
            icons::paint(
                painter,
                icon,
                Icon::More,
                if lit { p.text } else { p.muted },
            );
        }
    }
    (
        response,
        more.map(|m| m.on_hover_text("Details, Show in folder, Copy name, Delete")),
    )
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
