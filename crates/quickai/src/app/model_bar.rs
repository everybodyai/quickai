//! The bar over the page (UI v3, C203, the user's A6 of `the mockups`): the chosen model's name (or the
//! engine's, for one that runs on its own or has no model yet) with (i) for its details, and what it is doing; then what
//! can be done with it as buttons with words (Start, Load model or Use this model; Stop; Open ComfyUI); then a new chat
//! and ⋯, the chat's (Keep chats, Options, Export; C208). A side of the window put away is brought back from the bar's
//! end on its side: » at the left for the sidebar, « at the right for the panel (H4). The log is under the page, brought
//! up from the strip at its foot.

use eframe::egui;

use super::help::{self, Spot};
use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::events::ServerState;

/// The height of the bar.
pub(super) const BAR_HEIGHT: f32 = 56.0;
/// The height of its buttons with words.
const BUTTON: f32 = 32.0;
/// The size of its pictures.
const ICON: egui::Vec2 = egui::vec2(32.0, 32.0);

impl App {
    /// The bar over the page and the panel at its right.
    pub(super) fn show_top_bar(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        if self.view != super::MainView::Model {
            self.show_page_bar(ui);
            return;
        }
        let title = self.bar_title();
        let (said, color) = self.bar_state(state);
        let status = self.run_status_text(state);
        let alone = self.runs_alone();
        let model_page = !alone && !self.needs_install() && !self.models.is_empty();
        let mut pressed = Press::default();
        ui.horizontal_centered(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(2.0);
                // the panel at the right, put away (or a window too narrow for it to show by itself): « at the right end
                // brings it back (H4 of `the mockups`); shown, its own » puts it away
                if self.panel_possible() && !self.panel_shown {
                    let show = icons::icon_button(ui, Icon::DoubleLeft, ICON, true, false)
                        .on_hover_text(
                            "Show the panel: how the model runs, and Connect an app while it runs.",
                        );
                    help::mark_spot(ui, Spot::Change, show.rect);
                    // Connect an app is the panel while the model runs
                    if state == ServerState::Running {
                        help::mark_spot(ui, Spot::Plug, show.rect);
                    }
                    pressed.panel = show.clicked();
                }
                // the pictures, from the right: ⋯ (the chat's: M1 of `the mockups`; none when there is no
                // chat and no chat page to set up) and a new chat
                if self.bar_menu_has_items(model_page) {
                    let more = icons::icon_button(ui, Icon::More, ICON, true, false)
                        .on_hover_text("The chat: Keep chats, Options and Export");
                    help::mark_spot(ui, Spot::ChatMenu, more.rect);
                    egui::Popup::menu(&more)
                        .align(egui::RectAlign::BOTTOM_END)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .width(250.0)
                        .show(|ui| {
                            ui.set_min_width(250.0);
                            pressed.more = self.bar_menu(ui, model_page);
                        });
                }
                if model_page && self.can_chat() {
                    pressed.new_chat = icons::icon_button(
                        ui,
                        Icon::Pencil,
                        ICON,
                        self.can_begin_new_chat(),
                        false,
                    )
                    .on_hover_text(self.new_chat_hover())
                    .on_disabled_hover_text("New chat: once this one has a message.")
                    .clicked();
                }
                if !self.needs_install() && (model_page || alone || self.lists_models()) {
                    separator(ui);
                    self.bar_actions(ui, state, active, &mut pressed);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if self.sidebar_open {
                        ui.add_space(18.0);
                    } else {
                        ui.add_space(4.0);
                        pressed.sidebar = show_sidebar_button(ui);
                        ui.add_space(8.0);
                    }
                    // the name and what it is doing share what the buttons leave: the name keeps at least a third of it, and
                    // the state is drawn only where there is room for its dot and some of its words
                    let p = theme::palette();
                    let state_width = if said.is_empty() {
                        0.0
                    } else {
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        ui.painter()
                            .layout_no_wrap(said.clone(), font, p.muted)
                            .size()
                            .x
                            + 8.0
                            + 8.0
                            + 8.0
                    };
                    // (i) beside the name opens the model's details (I1 of `the mockups`)
                    let info_width = if model_page { 30.0 } else { 0.0 };
                    let room = ui.available_width();
                    let name_room = (room - state_width - info_width).max(room * 0.35).max(10.0);
                    ui.scope(|ui| {
                        ui.set_max_width(name_room);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&title)
                                    .size(16.0)
                                    .family(theme::semibold(ui))
                                    .color(p.text),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&title);
                    });
                    if model_page {
                        ui.add_space(2.0);
                        pressed.details = icons::icon_button(
                            ui,
                            Icon::Info,
                            egui::vec2(26.0, 26.0),
                            true,
                            self.details_of.is_some(),
                        )
                        .on_hover_text("Details of this model")
                        .clicked();
                    }
                    if !said.is_empty() && ui.available_width() >= 40.0 {
                        ui.add_space(8.0);
                        theme::dot(ui, 8.0, 4.0, color);
                        ui.add(
                            egui::Label::new(egui::RichText::new(&said).color(p.muted)).truncate(),
                        )
                        .on_hover_text(&status);
                    }
                });
            });
        });
        self.do_bar_press(ui.ctx(), pressed);
    }

    /// The bar over a page of the sidebar: the way back to the model, the page's name, and what it is about.
    fn show_page_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let (title, said) = match self.view {
            super::MainView::Recommended => (
                super::catalog_view::CATALOG_TITLE.to_string(),
                format!(
                    "for this computer · {} of memory",
                    crate::hardware::format_bytes(self.total_ram_bytes)
                ),
            ),
            _ => (
                "MCP servers".to_string(),
                "tools the model can use".to_string(),
            ),
        };
        let mut back = false;
        let mut sidebar = false;
        ui.horizontal_centered(|ui| {
            if self.sidebar_open {
                ui.add_space(6.0);
            } else {
                ui.add_space(4.0);
                sidebar = show_sidebar_button(ui);
                ui.add_space(2.0);
            }
            back = icons::icon_button(ui, Icon::Back, ICON, true, false)
                .on_hover_text("Back to the model (Escape)")
                .clicked();
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&title)
                    .size(16.0)
                    .family(theme::semibold(ui))
                    .color(p.text),
            );
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(&said).color(p.muted)).truncate());
        });
        if back {
            self.view = super::MainView::Model;
        }
        if sidebar {
            self.sidebar_open = true;
        }
    }

    /// What the bar is named: the chosen model, or the engine when it runs on its own, is not installed or has no model.
    fn bar_title(&self) -> String {
        if self.runs_alone() || self.needs_install() || self.models.is_empty() {
            return self.engine_name();
        }
        self.models
            .get(self.selected)
            .map(|m| m.display.clone())
            .unwrap_or_else(|| self.engine_name())
    }

    /// What the bar says the model (or the engine) is doing, and the colour of its dot.
    fn bar_state(&self, state: ServerState) -> (String, egui::Color32) {
        let p = theme::palette();
        if self.needs_install() {
            return ("Not installed".to_string(), p.faint);
        }
        if self.remote_opening() {
            return (format!("Connecting to {}…", self.remote_label()), p.warn);
        }
        let port = self.session.port();
        let running_where = || match (self.remote.tunnel.as_ref(), port) {
            (Some(_), _) => format!("Running on {}", self.remote_label()),
            (None, Some(port)) => format!("Running on port {port}"),
            (None, None) => "Running".to_string(),
        };
        let model_page = !self.runs_alone() && !self.models.is_empty();
        if model_page {
            if let Some(model) = self.models.get(self.selected) {
                let loading = self.session.loading() == Some(model.display.as_str())
                    || self.session.loading() == Some(model.filename.as_str());
                let named = self.session.model_name() == Some(model.display.as_str());
                if self.runs_model(self.selected) {
                    return (running_where(), p.ok);
                }
                if loading {
                    return ("Loading…".to_string(), p.warn);
                }
                if state == ServerState::Starting && (named || self.lists_models()) {
                    return ("Starting…".to_string(), p.warn);
                }
                if state == ServerState::Running
                    && self.lists_models()
                    && self.session.model_name().is_none()
                {
                    return (
                        format!("{} ready, no model loaded", self.engine_name()),
                        p.ok,
                    );
                }
                if state == ServerState::Running {
                    if let Some(other) = self.session.model_name() {
                        return (format!("Stopped · {other} is running"), p.faint);
                    }
                }
            }
        } else if state == ServerState::Running {
            return (running_where(), p.ok);
        } else if state == ServerState::Starting {
            return ("Starting…".to_string(), p.warn);
        }
        if self.models.is_empty()
            && !self.runs_alone()
            && state == ServerState::Idle
            && !self.lists_models()
        {
            return ("No models yet".to_string(), p.faint);
        }
        (state.label().to_string(), theme::state_color(state))
    }

    /// Start (Load model, Use this model), Stop, and the engine's own page (Open ComfyUI), drawn from the right.
    fn bar_actions(
        &mut self,
        ui: &mut egui::Ui,
        state: ServerState,
        active: bool,
        pressed: &mut Press,
    ) {
        let server = self.server_name();
        let can_stop = matches!(state, ServerState::Starting | ServerState::Running);
        let model_page = !self.runs_alone() && !self.models.is_empty();
        if can_stop {
            pressed.stop =
                icons::labelled_button(ui, Role::Danger, true, Icon::Stop, "Stop", BUTTON)
                    .on_hover_text(format!("Stop the running {server} (Ctrl+Enter)"))
                    .clicked();
        }
        if !model_page {
            // an engine that runs on its own (ComfyUI), or one that lists its models and has none yet (Ollama)
            let web_ui = self
                .engine_doc()
                .and_then(|e| e.doc.provides.web_ui.clone())
                .filter(|_| self.runs_alone());
            if state == ServerState::Running {
                if let Some(web_ui) = &web_ui {
                    pressed.web_ui = icons::labelled_button(
                        ui,
                        Role::Primary,
                        true,
                        Icon::External,
                        web_ui.label.as_str(),
                        BUTTON,
                    )
                    .on_hover_text(format!("Open the running {server} in your browser."))
                    .clicked();
                }
            }
            if !active {
                let start = icons::labelled_button(
                    ui,
                    Role::Primary,
                    !self.engine_missing(),
                    Icon::Play,
                    "Start",
                    BUTTON,
                )
                .on_hover_text(format!("Start {server} (Ctrl+Enter)."));
                help::mark_spot(ui, Spot::Play, start.rect);
                pressed.start = start.clicked();
            }
            return;
        }
        let loadable = self.can_load_selected();
        let switchable = self.can_switch_selected();
        let can_press = (!active && !self.engine_missing()) || loadable || switchable;
        if !active || loadable || switchable {
            let (label, hint) = if loadable {
                (
                    "Load model",
                    format!("Load this model into the running {server} (Ctrl+Enter)."),
                )
            } else if switchable {
                (
                    "Use this model",
                    format!("Start {server} again with this model."),
                )
            } else {
                (
                    "Start",
                    format!("Start {server} with this model (Ctrl+Enter)."),
                )
            };
            let start =
                icons::labelled_button(ui, Role::Primary, can_press, Icon::Play, label, BUTTON)
                    .on_hover_text(&hint)
                    .on_disabled_hover_text(&hint);
            help::mark_spot(ui, Spot::Play, start.rect);
            pressed.start = start.clicked();
            pressed.load = loadable;
            pressed.switch = switchable;
        }
    }

    /// The ⋯ menu of the bar: the chat's options and the model's menu on a model's page; the configurations and the
    /// engine's builds otherwise. What the model's part asked for comes back.
    fn bar_menu(&mut self, ui: &mut egui::Ui, model_page: bool) -> Option<BarMenu> {
        let mut asked = None;
        let chat = model_page && self.can_chat();
        if chat {
            self.chat_menu_items(ui);
        }
        // the chat page of an engine without one of its own (Open WebUI beside Ollama): set it up, or open it while it runs.
        // It is in the panel too while the model runs; here it is also before, as the Server menu had it (UI v2)
        if let Some(page) = self.chat_button().filter(|_| !self.runs_alone()) {
            if chat {
                ui.separator();
            }
            let hover = format!("{}: {}", page.label.trim_end_matches('.'), page.hover);
            if icons::menu_item(ui, Icon::Globe, &page.label, page.enabled, false)
                .on_hover_text(&hover)
                .on_disabled_hover_text(&hover)
                .clicked()
                && page.enabled
            {
                asked = Some(BarMenu::BrowserChat);
                ui.close();
            }
        }
        asked
    }

    /// Whether ⋯ has anything: the chat's items, or the chat page to set up or open. The model's items are on its row in
    /// the sidebar and its details on (i), Configurations in QuickAI ▾ and the panel, the engines in the engine's menu.
    fn bar_menu_has_items(&self, model_page: bool) -> bool {
        (model_page && self.can_chat())
            || self.chat_button().filter(|_| !self.runs_alone()).is_some()
    }

    /// Does what was pressed in the bar.
    fn do_bar_press(&mut self, ctx: &egui::Context, pressed: Press) {
        if pressed.start {
            self.start_pressed_at = Some(std::time::Instant::now());
            if pressed.load {
                self.load_selected();
            } else if pressed.switch {
                self.switch_to_selected();
            } else {
                self.start_selected(ctx);
            }
        }
        if pressed.stop {
            self.press_stop();
        }
        if pressed.web_ui {
            if let Some(url) = self
                .engine_doc()
                .and_then(|e| e.doc.provides.web_ui.clone())
                .and_then(|w| self.web_ui_url(&w))
            {
                let _ = webbrowser::open(&url);
            }
        }
        if pressed.panel {
            // not shown (put away, or a window too narrow): it is opened, and stays
            self.panel_open = true;
            self.panel_asked = true;
        }
        if pressed.sidebar {
            self.sidebar_open = true;
        }
        if pressed.new_chat {
            self.begin_new_chat();
        }
        if pressed.details {
            self.do_row_action(ctx, self.selected, super::shell::RowAction::Details);
        }
        match pressed.more {
            Some(BarMenu::BrowserChat) => self.open_browser_chat(),
            None => {}
        }
    }

    /// Whether the panel at the right has something to show: an engine that is installed, and a model to run (or an
    /// engine that runs on its own).
    pub(super) fn panel_possible(&self) -> bool {
        // (an engine that lists its models and has listed none yet has Connect an app while it runs)
        !self.needs_install()
            && (self.runs_alone()
                || !self.models.is_empty()
                || self.session.state() == ServerState::Running)
    }

    /// Stop, pressed in the bar: the server stops. Not the second click of a double-click on Start (within 0.8 s), as Stop
    /// takes the place of Start under the pointer.
    fn press_stop(&mut self) {
        let just_started = self
            .start_pressed_at
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(800));
        if !just_started {
            self.stop_server();
        }
    }
}

/// Pressed in the bar this frame.
#[derive(Default)]
struct Press {
    start: bool,
    load: bool,
    switch: bool,
    stop: bool,
    web_ui: bool,
    panel: bool,
    sidebar: bool,
    details: bool,
    new_chat: bool,
    more: Option<BarMenu>,
}

/// What the bar's ⋯ menu was asked to do.
enum BarMenu {
    BrowserChat,
}

/// The button at the left end of the bar that brings back the sidebar, when it was put away.
fn show_sidebar_button(ui: &mut egui::Ui) -> bool {
    icons::icon_button(ui, Icon::DoubleRight, ICON, true, false)
        .on_hover_text("Show the sidebar")
        .clicked()
}

/// The thin upright line between the buttons with words and the pictures.
fn separator(ui: &mut egui::Ui) {
    let p = theme::palette();
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 22.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.line);
    ui.add_space(4.0);
}
