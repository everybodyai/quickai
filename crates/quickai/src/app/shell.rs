//! The main window (UI v3, C203): the sidebar down
//! the left (`sidebar.rs`: the QuickAI menu, the computer and the engine, Recommended, MCP servers and the models), and
//! at its right one bar over the page (`model_bar.rs`: the model's name and its (i), what it is doing, Start or Stop, a
//! new chat and the chat's ⋯), the panel at the right (`inspector.rs`: how the model runs, and Connect an app while it
//! runs), the log under the page when it is asked for (︽ Log in the strip at its foot), and the page itself: the chat.
//! There is no menu bar and no status bar: what they had is in the QuickAI menu, the + of the list, the bar and the
//! sidebar.
//!
//! What a person does first is on the page and nothing else: an engine that is not installed has its Install button
//! alone, and a first run has the box that adds a model with the models suggested for this computer. An engine that
//! runs on its own (ComfyUI) keeps files of several kinds rather than one model to start, and has a page of its own
//! for them (the workbench).

use eframe::egui;

use super::help::{mark_spot, Spot};
use super::theme::{self, Tone};
use super::{App, MainView, Tab};
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

/// The height of a row of the list of models: the name, and under it its size and what else is known (one line of it: a
/// row whose facts need two lines is a line taller).
pub(super) const MODEL_ROW: f32 = 44.0;
/// What the … at the right end of a row takes of its width, with the gap before it: the facts never reach under it.
const MORE_ROOM: f32 = 37.0;
/// Where the words of a row begin: after its mark.
const ROW_TEXT: f32 = 28.0;

/// How wide the window is at least for the panel at the right to show by itself: narrower, the chat would have too little
/// room, and the bar's button opens it.
pub(super) const PANEL_FROM: f32 = 900.0;

/// From how many models on the list it has a box that narrows it.
pub(super) const FILTER_FROM: usize = 8;

impl App {
    /// Everything of the main window but the windows of its own (Settings, About, the recommended models...).
    pub(super) fn show_window(&mut self, ui: &mut egui::Ui) {
        let active = self.session.is_active();
        let state = self.session.state();
        if state == ServerState::Running {
            self.run_reached_running = true;
        }
        self.take_shortcuts(ui.ctx(), state, active);

        // the sidebar slides away with the button at its head, and back with the bar's (as in the A6 samples); the page
        // of a computer QuickAI sets up has no bar, and keeps it
        if self.setup_in_use().is_some() {
            self.sidebar_open = true;
        }
        // (dragged narrower than it can be, it is put away too, and dragged out of the window's edge it comes back)
        let was = self.sidebar_open;
        let mut open = was;
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(240.0)
            .size_range(200.0..=340.0)
            .frame(theme::side_frame(egui::Margin::symmetric(10, 10)))
            .show_collapsible(ui, &mut open, |ui| self.show_sidebar(ui, active));
        if open != was {
            self.sidebar_open = open;
        }
        // a computer QuickAI sets up, chosen in the sidebar: the page of its run (WP4.33), which has its own head
        let remote_view = self.setup_in_use().is_some();
        let wide = ui.ctx().content_rect().width() >= PANEL_FROM;
        self.panel_shown = !remote_view
            && self.view == MainView::Model
            && self.panel_possible()
            && self.panel_open
            && (wide || self.panel_asked);
        if !remote_view {
            egui::Panel::top("bar")
                .exact_size(super::model_bar::BAR_HEIGHT)
                .frame(
                    egui::Frame::new()
                        .fill(theme::palette().page)
                        .inner_margin(egui::Margin::symmetric(8, 0)),
                )
                .show(ui, |ui| self.show_top_bar(ui, state, active));
            if self.panel_shown {
                egui::Panel::right("inspector")
                    .resizable(true)
                    .default_size(300.0)
                    .size_range(260.0..=460.0)
                    .frame(theme::side_frame(egui::Margin::symmetric(16, 14)))
                    .show(ui, |ui| self.show_inspector(ui));
            }
        }
        // the log under the page, or the strip at its foot that brings it up
        if self.dock_open {
            self.show_dock(ui);
        } else {
            self.show_log_strip(ui);
        }
        if self.view != MainView::Model && !remote_view {
            // a page of the sidebar: the recommended models, or the MCP servers
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::palette().page))
                .show(ui, |ui| match self.view {
                    MainView::Recommended => self.show_catalog_page(ui),
                    _ => self.show_tools_view(ui),
                });
        } else if remote_view {
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
            // the chat is the page of the model: from a page of the sidebar, Ctrl+L goes back to it
            self.view = MainView::Model;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("chat_input")));
        }
        if settings {
            self.toggle_settings();
        }
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

    /// What the list says of a model under its name (C185: the facts left
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

    /// The items of a model's menu (right-click its row in the list, or the … on the row): Details, Show in folder, Copy
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
        // what went wrong (a crash, a computer that cannot be reached) is over the chat, in the page's margin
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 20,
                right: 16,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| self.show_diagnostics(ui, state));
        self.show_page_body(ui);
    }

    /// The page of an engine that is not installed: what it is, one button that installs it, and the way back.
    fn show_setup_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("setup_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space((ui.available_height() * 0.12).max(24.0));
                page_margin(ui, |ui| {
                    theme::column(ui, super::install_page::CARD_WIDTH, |ui| {
                        self.show_engine_setup(ui)
                    })
                });
                ui.add_space(24.0);
            });
    }

    /// The page of a first run (no models yet): the box that adds a model, with the models that suit this computer.
    fn show_first_run_page(&mut self, ui: &mut egui::Ui, state: ServerState, _active: bool) {
        // an engine that lists its models (Ollama) has none to show until it runs: its row, with Start, is over the box (one
        // whose Ollama has models already lists them once started), and its tabs under it, the log first: what it said when
        // it stopped before it was ready is there
        let lists = self.lists_models();
        if lists {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 16,
                    top: 0,
                    bottom: 0,
                })
                .show(ui, |ui| self.show_diagnostics(ui, state));
        }
        let area = egui::ScrollArea::vertical()
            .id_salt("first_run_page")
            .auto_shrink([false, false]);
        area.show(ui, |ui| {
            ui.add_space(if lists { 24.0 } else { 48.0 });
            // a margin at each side in a page narrower than the column (between the sidebar and the panel, C203)
            page_margin(ui, |ui| {
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
                })
            });
        });
    }

    /// The page of an engine that runs on its own (ComfyUI): its settings, the workbench, and what went wrong.
    fn show_workbench_page(&mut self, ui: &mut egui::Ui, state: ServerState, _active: bool) {
        egui::ScrollArea::vertical()
            .id_salt("page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                page_margin(ui, |ui| {
                    // not installed: the card that installs it, alone (its models are got once there is a ComfyUI to use them)
                    if self.needs_install() {
                        ui.add_space(28.0);
                        theme::column(ui, super::install_page::CARD_WIDTH, |ui| {
                            self.show_engine_setup(ui)
                        });
                        ui.add_space(24.0);
                        return;
                    }
                    // what went wrong (a crash, a computer that cannot be reached) first, then the page
                    theme::column(ui, theme::PAGE_WIDTH, |ui| {
                        ui.add_space(14.0);
                        self.show_diagnostics(ui, state);
                        self.show_workbench(ui, state);
                        ui.add_space(24.0);
                    })
                });
            });
    }

    /// The page of the MCP servers (MCP servers in the sidebar): what Settings had for them, in the middle of the
    /// window. Escape goes back to the model.
    fn show_tools_view(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.view = MainView::Model;
        }
        egui::ScrollArea::vertical()
            .id_salt("tools_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(24, 18))
                    .show(ui, |ui| {
                        theme::column(ui, 760.0, |ui| self.show_mcp_page(ui));
                    });
            });
    }

    // ---- the page's body, and the log under it ---------------------------------------------------------

    /// The page under the bar: the chat, for an engine that has one, and the log for one that has not.
    pub(super) fn show_page_body(&mut self, ui: &mut egui::Ui) {
        if self.can_chat() {
            self.chat.opened = true;
            self.show_chat_tab(ui);
        } else {
            egui::Frame::new()
                .fill(theme::palette().log)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.show_log_pane(ui);
                });
        }
    }

    /// The log under the page (︽ Log at its foot brings it up), with the live traffic beside it when it is on: tabs
    /// over it, a close button at their right, and one height for both.
    fn show_dock(&mut self, ui: &mut egui::Ui) {
        let live = self.traffic_proxy_on();
        let shown = match self.active_tab {
            Tab::Live if live => Tab::Live,
            _ => Tab::Main,
        };
        let on_live = shown == Tab::Live;
        if !on_live {
            self.log_unread = false;
        }
        let mut close = false;
        // dragged up, it leaves the page the chat's box (as high as it was last drawn, with the files over it) and a
        // little of the conversation, so that the box is never under it
        let chat_box = ui
            .data(|d| d.get_temp::<f32>(egui::Id::new(super::chat::CHAT_BOX_HEIGHT)))
            .unwrap_or(64.0);
        let most = (ui.available_height() - chat_box - 80.0).clamp(160.0, 640.0);
        // One panel for both tabs, so switching between them keeps the height it was given.
        egui::Panel::bottom("dock")
            .resizable(true)
            .default_size(260.0)
            .size_range(160.0..=most)
            .frame(theme::side_frame(egui::Margin::ZERO))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let log = dock_tab(ui, "Log", !on_live).on_hover_text("What the server says as it starts and runs.");
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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        close = super::icons::icon_button(
                            ui,
                            super::icons::Icon::DoubleDown,
                            egui::vec2(26.0, 24.0),
                            true,
                            false,
                        )
                        .on_hover_text("Put the log away. ︽ Log at the foot of the page brings it up again.")
                        .clicked();
                    });
                });
                let rule = ui.max_rect().x_range();
                ui.painter().hline(
                    rule,
                    ui.cursor().top(),
                    egui::Stroke::new(1.0, theme::palette().line),
                );
                match shown {
                    Tab::Live => self.show_live_traffic_tab(ui),
                    _ => egui::Frame::new().fill(theme::palette().log).show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        self.show_log_pane(ui);
                    }).inner,
                }
            });
        if close {
            self.dock_open = false;
        }
    }
}

impl App {
    /// The strip at the foot of the page while the log is put away: ︽ Log at its right end brings it up, with a red dot
    /// when the log has something new (a run that stopped by itself).
    fn show_log_strip(&mut self, ui: &mut egui::Ui) {
        let mut open = false;
        egui::Panel::bottom("log_strip")
            .exact_size(28.0)
            .resizable(false)
            .frame(theme::side_frame(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let p = theme::palette();
                    let font = egui::TextStyle::Small.resolve(ui.style());
                    let word = ui
                        .painter()
                        .layout_no_wrap("Log".to_string(), font, p.muted);
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(8.0 + 13.0 + 6.0 + word.size().x + 8.0, 22.0),
                        egui::Sense::click(),
                    );
                    mark_spot(ui, Spot::LogTab, rect);
                    let ink = if response.hovered() { p.text } else { p.muted };
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 6.0, p.selected);
                    }
                    super::icons::paint(
                        ui.painter(),
                        egui::Rect::from_center_size(
                            egui::pos2(rect.left() + 8.0 + 6.5, rect.center().y),
                            egui::vec2(13.0, 13.0),
                        ),
                        super::icons::Icon::DoubleUp,
                        ink,
                    );
                    ui.painter().galley(
                        egui::pos2(rect.left() + 27.0, rect.center().y - word.size().y / 2.0),
                        word,
                        ink,
                    );
                    if self.log_unread {
                        ui.painter().circle_filled(
                            egui::pos2(rect.right() - 3.0, rect.top() + 4.0),
                            3.5,
                            p.bad,
                        );
                    }
                    open = response
                        .on_hover_text(
                            "The log, and the live traffic when it is on: under the page.",
                        )
                        .clicked();
                });
            });
        if open {
            self.dock_open = true;
            if matches!(self.active_tab, Tab::Chat) {
                self.active_tab = Tab::Main;
            }
        }
    }
}

/// What the menu of a model of the list was asked to do.
#[derive(Clone, Copy)]
pub(super) enum RowAction {
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

/// Room at each side of a page's column, which a column narrower than the page has already but one in a page between
/// the sidebar and the panel (C203) would not: what it shows would touch them both.
fn page_margin<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 0))
        .show(ui, add)
        .inner
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

/// A model of the list (UI v3, C203): a mark at its left (a ring, green while it runs), its name, and under it what is
/// known of it as words, a few of them coloured (`fast` and `recommended` green, `slower` amber), on a second line too
/// when they do not fit on one (the shipped model is `1.1 GB · fast · recommended`, then `bundled`). Chosen, it is
/// filled. On the chosen row and the one the pointer is on there is a … at its right end, whose response comes back with
/// the row's for the menu of what can be done with the model.
pub(super) fn model_row(
    ui: &mut egui::Ui,
    name: &str,
    detail: &[(String, Option<egui::Color32>)],
    chosen: bool,
    running: bool,
) -> (egui::Response, Option<egui::Response>) {
    let p = theme::palette();
    let width = ui.available_width();
    // the facts are measured as if the … were there, so that a row keeps its height when the pointer comes onto it
    let facts = fact_lines(ui, detail, (width - ROW_TEXT - MORE_ROOM).max(10.0));
    let facts_height: f32 = facts.iter().map(|g| g.size().y).sum();
    let line = facts.first().map_or(0.0, |g| g.size().y);
    let height = MODEL_ROW + facts_height - line;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let more_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 18.0, rect.center().y),
        egui::vec2(26.0, 24.0),
    );
    let more_id = response.id.with("more");
    let menu_open = egui::Popup::is_id_open(ui.ctx(), more_id.with("popup"));
    let more = (chosen || response.contains_pointer() || menu_open)
        .then(|| ui.interact(more_rect, more_id, egui::Sense::click()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if chosen {
            painter.rect_filled(rect, 8.0, p.selected);
        } else if response.hovered() {
            painter.rect_filled(rect, 8.0, p.selected.gamma_multiply(0.55));
        }
        let left = rect.left() + ROW_TEXT;
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
        let top = rect.top() + (height - title.size().y - facts_height - 2.0) / 2.0;
        let mut below = top + title.size().y + 2.0;
        let title_mid = top + title.size().y / 2.0;
        // its mark: a ring, and green while it runs
        let mark = egui::pos2(rect.left() + 14.0, title_mid);
        if running {
            painter.circle_filled(mark, 6.5, p.ok.gamma_multiply(0.25));
            painter.circle_filled(mark, 4.5, p.ok);
        } else {
            painter.circle_stroke(mark, 4.0, egui::Stroke::new(1.5, p.outline));
        }
        painter.galley(egui::pos2(left, top), title, p.text);
        for galley in facts {
            let y = galley.size().y;
            painter.galley(egui::pos2(left, below), galley, p.muted);
            below += y;
        }
        if let Some(more) = &more {
            let lit = more.hovered() || menu_open;
            if lit {
                painter.rect_filled(more_rect, 5.0, p.raised_hover);
            }
            let icon = egui::Rect::from_center_size(more_rect.center(), egui::vec2(15.0, 15.0));
            super::icons::paint(
                painter,
                icon,
                super::icons::Icon::More,
                if lit { p.text } else { p.muted },
            );
        }
    }
    (
        response,
        more.map(|m| m.on_hover_text("Details, Show in folder, Copy name, Delete")),
    )
}

/// The facts under a model's name, laid out in lines no wider than `room`: as many to a line as fit, with " · " between
/// them, and a fact never cut between two lines ("1.1 GB · fast · recommended" then "bundled", where one line cut
/// "bundled" off). Only a fact wider than the room by itself is cut short. There is always at least one line.
fn fact_lines(
    ui: &egui::Ui,
    detail: &[(String, Option<egui::Color32>)],
    room: f32,
) -> Vec<std::sync::Arc<egui::Galley>> {
    let p = theme::palette();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let job_of = |facts: &[(String, Option<egui::Color32>)], wrap: egui::text::TextWrapping| {
        let mut job = egui::text::LayoutJob::default();
        for (i, (text, color)) in facts.iter().enumerate() {
            if i > 0 {
                job.append(" · ", 0.0, egui::TextFormat::simple(font.clone(), p.muted));
            }
            job.append(
                text,
                0.0,
                egui::TextFormat::simple(font.clone(), color.unwrap_or(p.muted)),
            );
        }
        job.wrap = wrap;
        job
    };
    let painter = ui.painter();
    let fits = |facts: &[(String, Option<egui::Color32>)]| {
        painter
            .layout_job(job_of(facts, egui::text::TextWrapping::default()))
            .size()
            .x
            <= room
    };
    let mut lines = Vec::new();
    let mut start = 0;
    while start < detail.len() {
        let mut end = start + 1;
        while end < detail.len() && fits(&detail[start..=end]) {
            end += 1;
        }
        let cut = egui::text::TextWrapping::truncate_at_width(room);
        lines.push(painter.layout_job(job_of(&detail[start..end], cut)));
        start = end;
    }
    if lines.is_empty() {
        lines.push(painter.layout_job(job_of(&[], egui::text::TextWrapping::default())));
    }
    lines
}

/// A tab of the dock: its name, and a line of the accent under the one that is open.
pub(super) fn dock_tab(ui: &mut egui::Ui, label: &str, open: bool) -> egui::Response {
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
