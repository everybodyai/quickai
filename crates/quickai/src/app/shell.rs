//! The main window, laid out as a classic desktop tool is: a menu bar and a toolbar across the top, the models in a
//! list down the left with the chosen one's details beside it, the log and the live traffic as tabs at the foot, and
//! a status bar under them that always says what the server is doing.
//!
//! An engine that runs on its own (ComfyUI) keeps files of several kinds rather than one model to start, and has a
//! page of its own for them (the workbench): it has the whole of the middle, and no list on the left.

use eframe::egui;

use super::theme::{self, Role};
use super::{App, Tab};
use crate::events::ServerState;

/// Where the documentation and the privacy policy are read.
const DOCS_URL: &str = "https://github.com/everybodyai/quickai/tree/main/docs";
const PRIVACY_URL: &str = "https://github.com/everybodyai/quickai/blob/main/PRIVACY.md";
const WEBSITE_URL: &str = "https://everybodyai.dev/quickai";

/// The height of a row of the list of models: the name, and under it its size and what else is known.
const MODEL_ROW: f32 = 42.0;

/// From how many models on the list it has a box that narrows it.
const FILTER_FROM: usize = 8;

impl App {
    /// Everything of the main window but the windows of its own (Settings, About, the recommended models...).
    pub(super) fn show_window(&mut self, ui: &mut egui::Ui) {
        let active = self.session.is_active();
        let state = self.session.state();
        if state == ServerState::Running {
            self.run_reached_running = true;
        }

        egui::Panel::top("menu_bar")
            .frame(theme::bar_frame(egui::Margin::symmetric(6, 2)))
            .show(ui, |ui| self.show_menu_bar(ui, state, active));
        egui::Panel::top("toolbar")
            .frame(theme::bar_frame(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| self.show_toolbar(ui, state, active));
        egui::Panel::bottom("status_bar")
            .frame(theme::bar_frame(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.show_status_bar(ui, state));
        // while the results of a look-up or the way to connect an app are on the page they have the whole of it, as
        // they always had: the log is not what is being read then
        let browsing = self.lookup.open || (self.show_api_info && state == ServerState::Running);
        if !browsing {
            self.show_dock(ui);
        }
        if !self.runs_alone() {
            egui::Panel::left("models")
                .resizable(true)
                .default_size(210.0)
                .size_range(170.0..=340.0)
                .frame(theme::bar_frame(egui::Margin::ZERO))
                .show(ui, |ui| self.show_models_list(ui, active));
        }
        egui::CentralPanel::default()
            .frame(theme::page_frame())
            .show(ui, |ui| self.show_page(ui, state, active));
    }

    // ---- the menu bar -------------------------------------------------------------------------------

    fn show_menu_bar(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
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
            ui.menu_button("Server", |ui| {
                let (can_start, loadable) = self.start_possible(active);
                let switchable = self.can_switch_selected();
                let label = if loadable {
                    "Load model"
                } else if switchable {
                    "Use this model"
                } else {
                    "Start"
                };
                if ui
                    .add_enabled(can_start || switchable, egui::Button::new(label))
                    .clicked()
                {
                    if loadable {
                        self.load_selected();
                    } else if switchable {
                        self.switch_to_selected();
                    } else {
                        self.start_selected(ui.ctx());
                    }
                }
                let can_stop = matches!(state, ServerState::Starting | ServerState::Running);
                if ui
                    .add_enabled(can_stop, egui::Button::new("Stop"))
                    .clicked()
                {
                    self.stop_companion();
                    self.session.stop();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        state == ServerState::Running,
                        egui::Button::new("Connect an app…"),
                    )
                    .clicked()
                {
                    self.show_api_info = true;
                }
                ui.separator();
                self.show_engine_menu(ui);
                ui.separator();
                if ui
                    .button("Remote computers…")
                    .on_hover_text(
                        "Run the models on a more powerful computer, and use them from here.",
                    )
                    .clicked()
                {
                    self.remote.show_window = true;
                }
            });
            ui.menu_button("Models", |ui| {
                if ui
                    .add_enabled(self.can_recommend(), egui::Button::new("Recommended…"))
                    .clicked()
                {
                    self.open_catalog();
                }
                if ui
                    .add_enabled(self.can_look_up(), egui::Button::new("Look up online…"))
                    .clicked()
                {
                    self.start_lookup(ui.ctx(), "");
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
            ui.menu_button("View", |ui| {
                let live = self.traffic_proxy_on();
                let mut on_live = self.active_tab == Tab::Live;
                if ui
                    .add_enabled(live, egui::Checkbox::new(&mut on_live, "Live traffic"))
                    .on_disabled_hover_text("Turn the live traffic on in Settings first.")
                    .changed()
                {
                    self.active_tab = if on_live { Tab::Live } else { Tab::Main };
                }
                let mut dark = self.dark_mode;
                if ui.checkbox(&mut dark, "Dark theme").changed() {
                    self.dark_mode = dark;
                    theme::apply(ui.ctx(), dark);
                    self.save_settings();
                }
                // (after the theme, which keeps the place it had)
                let mut on_chat = self.active_tab == Tab::Chat;
                if ui
                    .add_enabled(self.can_chat(), egui::Checkbox::new(&mut on_chat, "Chat"))
                    .on_disabled_hover_text(format!(
                        "{} has no chat to talk to.",
                        self.engine_name()
                    ))
                    .changed()
                {
                    self.active_tab = if on_chat { Tab::Chat } else { Tab::Main };
                }
            });
            ui.menu_button("Help", |ui| {
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
        });
    }

    /// Whether Start can be pressed now, and whether what it does is load the chosen model into the running engine.
    pub(super) fn start_possible(&self, active: bool) -> (bool, bool) {
        let loadable = self.can_load_selected();
        let can_start = (!self.models.is_empty() || self.lists_models() || self.runs_alone())
            && !self.engine_missing();
        ((!active && can_start) || loadable, loadable)
    }

    /// Reads the list of models again: the folders, or what the running engine says it has.
    fn refresh_list(&mut self, ctx: &egui::Context) {
        if self.lists_models() {
            self.refresh_models(ctx);
        } else {
            self.rescan_models();
        }
    }

    // ---- the toolbar ----------------------------------------------------------------------------------

    fn show_toolbar(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        ui.horizontal(|ui| {
            self.show_run_buttons(ui, state, active);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::button_sized(ui, Role::Normal, true, "Settings", egui::vec2(0.0, 28.0))
                    .on_hover_text(format!(
                        "The engine ({}), where it is found, the models folder and their options",
                        self.engine_name()
                    ))
                    .clicked()
                {
                    self.toggle_settings();
                }
                self.show_engine_switch(ui, 28.0);
            });
        });
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
            theme::state_indicator(ui, shown, &status)
                .on_hover_text("What the server is doing now. Start and Stop are in the toolbar.");
            if let Some((text, companion_state)) = companion {
                ui.separator();
                ui.colored_label(theme::state_color(companion_state), text);
            }
            // the engine at the right end (the address is in the state's words while it runs)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(theme::muted(self.engine_name()));
            });
        });
    }

    // ---- the list of models ----------------------------------------------------------------------------

    fn show_models_list(&mut self, ui: &mut egui::Ui, active: bool) {
        let p = theme::palette();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
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
                if theme::button_sized(ui, Role::Quiet, !active, "Refresh", egui::vec2(0.0, 20.0))
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
                            left: 10,
                            right: 8,
                            top: 6,
                            bottom: 0,
                        })
                        .show(ui, |ui| {
                            ui.label(theme::muted(if self.lists_models() {
                                "No models yet. Pull one with the box on the right."
                            } else {
                                "No models yet. Get one with Recommended or Look up online, on the right."
                            }));
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
                    let recommended = self.recommended_filename.as_deref() == Some(m.filename.as_str());
                    let mut detail = vec![size_text(m.size_bytes)];
                    if recommended {
                        detail.push("recommended".to_string());
                    }
                    if m.bundled {
                        detail.push("bundled".to_string());
                    }
                    let row = model_row(ui, &m.display, &detail.join(" · "), i == self.selected);
                    if row.clicked() {
                        picked = Some(i);
                    }
                    // what can be done with a model, where it is: right-click it
                    row.context_menu(|ui| {
                        let deletable = self.delete_request(i);
                        let item = ui
                            .add_enabled(deletable.is_ok(), egui::Button::new("Delete…"))
                            .on_disabled_hover_text(deletable.err().unwrap_or_default());
                        if item.clicked() {
                            asked = Some((i, RowAction::Delete));
                            ui.close();
                        }
                        if !lists && ui.button("Show in folder").clicked() {
                            asked = Some((i, RowAction::ShowInFolder));
                            ui.close();
                        }
                                                if ui.button("Copy name").clicked() {
                            asked = Some((i, RowAction::CopyName));
                            ui.close();
                        }
                        if ui.button("Details…").clicked() {
                            asked = Some((i, RowAction::Details));
                            ui.close();
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
                match asked {
                    Some((i, RowAction::Delete)) => self.ask_to_delete_from_window(i),
                    Some((i, RowAction::ShowInFolder)) => {
                        if let Some(folder) = self.models.get(i).and_then(|m| m.path.parent()) {
                            super::workbench::open_folder(folder);
                        }
                    }
                                        Some((i, RowAction::Details)) => {
                        self.details_of = self.models.get(i).map(|m| m.filename.clone());
                    }
                    Some((i, RowAction::CopyName)) => {
                        if let Some(m) = self.models.get(i) {
                            ui.ctx().copy_text(m.filename.clone());
                        }
                    }
                    None => {}
                }
                if let Some(i) = picked.filter(|i| *i != self.selected) {
                    self.selected = i;
                    // a model picked anew gets its own defaults (a smaller context for a bigger model), still only a
                    // starting point: the boxes stay free to change
                    self.apply_model_defaults();
                }
            });
    }

    // ---- the page ----------------------------------------------------------------------------------------

    fn show_page(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        egui::ScrollArea::vertical()
            .id_salt("page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::column(ui, theme::PAGE_WIDTH, |ui| {
                    self.show_configuration_section(ui);
                    self.show_engine_setup(ui);
                    self.show_runs_on(ui);
                    ui.add_space(10.0);
                    if self.runs_alone() {
                        theme::card(ui, |ui| self.show_workbench(ui, active));
                    } else {
                        self.show_model_bar(ui);
                        ui.add_space(10.0);
                        let title = if self.lists_models() {
                            "Pull a model"
                        } else {
                            "Get a model"
                        };
                        theme::group(ui, title, |ui| self.show_acquire(ui, active));
                    }
                    ui.add_space(10.0);
                    self.show_controls(ui);
                    self.show_api_info_panel(ui, state);
                    self.show_diagnostics(ui, state);
                });
            });
    }

    // ---- the dock: the log and the live traffic ------------------------------------------------------

    fn show_dock(&mut self, ui: &mut egui::Ui) {
        let live = self.traffic_proxy_on();
        if !live && self.active_tab == Tab::Live {
            self.active_tab = Tab::Main;
        }
        let chat = self.can_chat();
        if !chat && self.active_tab == Tab::Chat {
            self.active_tab = Tab::Main;
        }
        let on_live = self.active_tab == Tab::Live;
        let on_chat = self.active_tab == Tab::Chat;
        // each tab keeps the height it was given: a conversation wants more room than the log
        let (id, height) = match self.active_tab {
            Tab::Live => ("dock_live", 320.0),
            Tab::Chat => ("dock_chat", 320.0),
            Tab::Main => ("dock_log", 160.0),
        };
        egui::Panel::bottom(id)
            .resizable(true)
            .default_size(height)
            .size_range(90.0..=640.0)
            .frame(egui::Frame::new().fill(theme::palette().page))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    if dock_tab(ui, "Log", !on_live && !on_chat).clicked() {
                        self.active_tab = Tab::Main;
                    }
                    if live
                        && dock_tab(ui, "Live traffic", on_live)
                            .on_hover_text(
                                "Watch chat/completion requests and replies in real time as any client talks \
                                 to this model.",
                            )
                            .clicked()
                    {
                        self.active_tab = Tab::Live;
                    }
                    // (after the others, which keep the places they had). A model that runs and a conversation that was
                    // never opened: a dot says it is there to be tried
                    let fresh = self.session.state() == crate::events::ServerState::Running
                        && self.session.model_name().is_some()
                        && !self.chat.opened;
                    if on_chat {
                        self.chat.opened = true;
                    }
                    if chat
                        && dock_tab_marked(ui, "Chat", on_chat, fresh && !on_chat)
                            .on_hover_text(
                                "Talk to the model that runs, right here: nothing to install, nothing kept.",
                            )
                            .clicked()
                    {
                        self.active_tab = Tab::Chat;
                    }
                });
                let rule = ui.max_rect().x_range();
                ui.painter().hline(
                    rule,
                    ui.cursor().top(),
                    egui::Stroke::new(1.0, theme::palette().line),
                );
                match self.active_tab {
                    Tab::Live => self.show_live_traffic_tab(ui),
                    Tab::Chat => self.show_chat_tab(ui),
                    Tab::Main => self.show_log_pane(ui),
                }
            });
    }
}

/// What the menu of a model of the list was asked to do.
#[derive(Clone, Copy)]
enum RowAction {
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
fn model_row(ui: &mut egui::Ui, name: &str, detail: &str, chosen: bool) -> egui::Response {
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
        painter.galley(egui::pos2(left, top), title, p.text);
        painter.galley(egui::pos2(left, below), small, p.muted);
    }
    response.on_hover_text(name)
}

/// A tab of the dock with a dot after its name, when `marked`: something new is there.
fn dock_tab_marked(ui: &mut egui::Ui, label: &str, open: bool, marked: bool) -> egui::Response {
    let response = dock_tab(ui, label, open);
    if marked && ui.is_rect_visible(response.rect) {
        let centre = egui::pos2(response.rect.right() - 4.0, response.rect.center().y - 5.0);
        ui.painter()
            .circle_filled(centre, 3.0, theme::palette().accent_text);
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
