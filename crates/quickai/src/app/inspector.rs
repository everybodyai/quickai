//! The panel at the right of the page (UI v3, C203): how the model runs while
//! it is stopped (the configuration, the presets or the engine's fields, the capabilities and the MCP servers in a line,
//! and how it fits this computer at its foot), and Connect an app while it runs (the addresses, the model's name, the
//! key, the examples, the chat in a browser and the tools), with How it runs at its foot (C223: its chips and the
//! capabilities). It is what *Change* under the model's name and the Connect an app popover were.

use eframe::egui;

use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::events::ServerState;

impl App {
    /// The panel at the right of the page: This computer at its foot while the model is stopped, How it runs while it
    /// runs (C223), the rest over it.
    pub(super) fn show_inspector(&mut self, ui: &mut egui::Ui) {
        self.refresh_facts();
        self.poll_devices(ui.ctx());
        let connect = self.connect_now();
        if connect {
            egui::Panel::bottom("inspector_run_foot")
                .frame(egui::Frame::new().inner_margin(egui::Margin {
                    left: 0,
                    right: 0,
                    top: 12,
                    bottom: 0,
                }))
                .show(ui, |ui| self.show_run_foot(ui));
        }
        // a computer QuickAI sets up has its own foot: what it is and what the model would take of it (C228)
        let foot = !connect
            && !self.runs_alone()
            && (self.setup_in_use().is_some() || self.models.get(self.selected).is_some());
        // the meters of the foot take at most a quarter of the panel's height, and scroll beyond it
        let meters = (ui.available_height() * 0.25).clamp(48.0, 170.0);
        if foot {
            egui::Panel::bottom("inspector_foot")
                .frame(egui::Frame::new().inner_margin(egui::Margin {
                    left: 0,
                    right: 0,
                    top: 10,
                    bottom: 0,
                }))
                // the foot's own edge is the one line over it (a second one, drawn 4 px under it, made two)
                .show(ui, |ui| {
                    let foot = ui
                        .scope(|ui| self.show_this_computer(ui, meters))
                        .response
                        .rect;
                    super::help::mark_spot(ui, super::help::Spot::ThisComputer, foot);
                });
        }
        // a scroll bar that takes its own room when the panel is longer than the window (several cards), so that it is
        // not drawn over the right end of the rows
        ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
        egui::ScrollArea::vertical()
            .id_salt("inspector")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.connect_now() {
                    self.show_connect_section(ui);
                } else {
                    self.show_how_section(ui);
                }
            });
    }

    /// Whether the panel shows Connect an app: the model chosen runs (or the engine that runs on its own does, or one
    /// that lists its models runs with none loaded yet).
    fn connect_now(&self) -> bool {
        // a computer QuickAI sets up: while its run is up and its ports are here (C228)
        if self.setup_in_use().is_some() {
            return self.setup_run().is_some_and(|r| r.running());
        }
        if self.session.state() != ServerState::Running {
            return false;
        }
        self.runs_alone()
            || self.models.is_empty()
            || self.runs_model(self.selected)
            || (self.lists_models() && self.session.model_name().is_none())
    }

    /// How the model runs: the configuration, the context and the graphics, then the tools with a switch each.
    fn show_how_section(&mut self, ui: &mut egui::Ui) {
        self.panel_head(ui, "How it runs", true, None);
        self.show_change_panel(ui);
        for problem in self.field_problems() {
            ui.colored_label(super::widgets::problem_color(), problem);
        }
        // what is added when it starts: the projector, the mode, the person's arguments and environment (X1)
        if self.remote_in_use().is_none() {
            let extras = ui.scope(|ui| self.show_extras_rows(ui)).response.rect;
            if extras.height() > 1.0 {
                super::help::mark_spot(ui, super::help::Spot::Extras, extras);
            }
        }
        // what the chat does besides asking for replies (C223): its switches
        if !self.runs_alone() && self.can_chat() {
            let capabilities = ui.scope(|ui| self.show_capabilities(ui)).response.rect;
            super::help::mark_spot(ui, super::help::Spot::Capabilities, capabilities);
        }
        // the tools are the chat's: an engine that does not chat (ComfyUI) has none to show. Under Capabilities, with no line
        // between them (C224): both are the chat's, and with the line Tools went past the foot of the window's first size
        if !self.runs_alone() && self.can_chat() {
            let hover = if self.engine == crate::settings::DEFAULT_ENGINE {
                "The MCP servers the model can use when you chat here, and in Open Chat UI. QuickAI asks before a tool \
                 runs, unless you turned that off in MCP servers."
            } else {
                "The MCP servers the model can use when you chat here. QuickAI asks before a tool runs, unless you \
                 turned that off in MCP servers."
            };
            let tools = ui.scope(|ui| self.show_tools_rows(ui, hover)).response.rect;
            super::help::mark_spot(ui, super::help::Spot::Tools, tools);
        }
    }

    /// Connect an app, while it runs (C213), in few words:
    /// the address, the model's name and the key in one box, with a picture each for its
    /// label; the example closed, copied without being shown; the chat in a browser; and the tools. How it runs is at the
    /// panel's foot (C223). The sentences are in the (i) and the hovers.
    fn show_connect_section(&mut self, ui: &mut egui::Ui) {
        let panel = self.connection_panel();
        let openai = panel.as_ref().is_some_and(|p| p.base_url.ends_with("/v1"));
        self.panel_head(
            ui,
            "Connect an app",
            false,
            Some(if openai {
                "Give these to any app that works with OpenAI's API: the address, the model's name and the key."
            } else {
                "Give this address to an app that uses its API."
            }),
        );
        if let Some(panel) = &panel {
            super::api_info::show_connection_box(ui, panel);
        }
        // a run on a computer QuickAI sets up: the address is its port, brought here (C228)
        if let Some(spec) = self.setup_in_use() {
            ui.add_space(4.0);
            ui.label(
                theme::muted(format!(
                    "{}'s port, brought to this computer. Apps here use this address.",
                    spec.name
                ))
                .small(),
            );
        }
        ui.add_space(10.0);
        // the chat in a browser: the engine's own page, or its companion's (Open WebUI)
        if let Some(chat) = self.browser_chat().filter(|_| !self.runs_alone()) {
            let hover = format!("{}: {}", chat.label.trim_end_matches('.'), chat.hover);
            if icons::labelled_button(
                ui,
                Role::Normal,
                chat.enabled,
                Icon::Globe,
                &chat.label,
                30.0,
            )
            .on_hover_text(&hover)
            .on_disabled_hover_text(&hover)
            .clicked()
            {
                self.open_browser_chat();
            }
        }
        if let Some((text, state)) = self.companion_status() {
            ui.horizontal(|ui| {
                theme::dot(ui, 8.0, 3.5, theme::state_color(state));
                ui.label(theme::muted(text).small());
            });
        }
        if !self.runs_alone() {
            rule(ui);
            let hover = if self.engine == crate::settings::DEFAULT_ENGINE
                && self.remote_in_use().is_none()
            {
                "The MCP servers the model can use when you chat here, and in Open Chat UI. Apps you connect bring \
                 their own."
            } else {
                "The MCP servers the model can use when you chat here. Apps you connect bring their own."
            };
            self.show_tools_rows(ui, hover);
        }
    }

    /// How it runs, at the foot of the panel while the model runs (C223), as This computer is while it is stopped: its
    /// heading as large as Connect an app's, with the lock that says the chips change once the model is stopped; the
    /// chips (the context with how much of it the chat takes); and the capabilities, which change while it runs.
    fn show_run_foot(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("How it runs")
                    .size(15.0)
                    .family(theme::semibold(ui))
                    .color(p.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                icons::paint(ui.painter(), rect.shrink(4.0), Icon::Lock, p.muted);
                response.on_hover_text(
                    "Stop the model to change the context, the graphics and the extras. The capabilities change \
                     while it runs.",
                );
            });
        });
        ui.add_space(6.0);
        self.show_run_chips(ui);
        if !self.runs_alone() && self.can_chat() {
            let capabilities = ui.scope(|ui| self.show_capabilities(ui)).response.rect;
            super::help::mark_spot(ui, super::help::Spot::Capabilities, capabilities);
        }
        ui.add_space(4.0);
    }

    /// Shows the page of the MCP servers (MCP servers in the sidebar).
    pub(super) fn open_mcp_servers(&mut self) {
        self.view = super::MainView::Tools;
    }
}

impl App {
    /// The head of the panel: the name of its section, an (i) that says `info` when there is one, and at its right » that
    /// puts the panel away (« at the right end of the bar brings it back), with
    /// the gear of All settings for starting it beside it when `settings`.
    fn panel_head(&mut self, ui: &mut egui::Ui, text: &str, settings: bool, info: Option<&str>) {
        let mut hide = false;
        let mut open_settings = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(15.0)
                    .family(theme::semibold(ui))
                    .color(theme::palette().text),
            );
            if let Some(info) = info {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                icons::paint(ui.painter(), rect, Icon::Info, theme::palette().faint);
                response.on_hover_text(info);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button =
                    icons::icon_button(ui, Icon::DoubleRight, egui::vec2(28.0, 28.0), true, false)
                        .on_hover_text(
                            "Hide the panel. The « at the right end of the bar brings it back.",
                        );
                super::help::mark_spot(ui, super::help::Spot::Change, button.rect);
                // Connect an app is the panel while the model runs
                if self.session.state() == ServerState::Running {
                    super::help::mark_spot(ui, super::help::Spot::Plug, button.rect);
                }
                hide = button.clicked();
                if settings {
                    open_settings =
                        icons::icon_button(ui, Icon::Gear, egui::vec2(28.0, 28.0), true, false)
                            .on_hover_text("All settings for starting it…")
                            .clicked();
                }
            });
        });
        ui.add_space(4.0);
        if open_settings {
            self.settings_page = super::settings_window::SettingsPage::Start;
            if !self.show_settings {
                self.toggle_settings();
            }
        }
        if hide {
            self.panel_open = false;
            self.panel_asked = false;
        }
    }
}

/// A line between two parts of the panel.
fn rule(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    let y = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        y,
        egui::Stroke::new(1.0, theme::palette().line),
    );
    ui.add_space(9.0);
}

/// The head of a section of the panel: its icon and its word, an (i) that says what it is when `hover`, and what
/// `right` draws at its right end (drawn from the right).
pub(super) fn section_head(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    hover: Option<&str>,
    right: impl FnOnce(&mut egui::Ui),
) {
    let p = theme::palette();
    // a little closer than they were (8 and 4 points round them), so that the window's first size still shows Tools under
    // Capabilities (C224) with no scrolling
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.set_min_height(24.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        icons::paint(ui.painter(), rect, icon, p.muted);
        ui.label(egui::RichText::new(text).color(p.text));
        if let Some(hover) = hover {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Info, p.faint);
            response.on_hover_text(hover);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(3.0);
}
