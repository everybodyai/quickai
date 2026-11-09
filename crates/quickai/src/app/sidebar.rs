//! The sidebar of UI v3 (C203, the user's A6 with the QuickAI menu of S2, `the mockups`): at its head the
//! QuickAI menu (Settings, Configurations and Quit) and the button that puts the sidebar away, then where the models run
//! and the engine that runs them, in one box, a box that narrows the list, and the models, with + for what the Models
//! menu had. At its foot (C206, `the mockups`) Recommended and MCP servers, and under them Help (its
//! menu: Help, the tour, the keys, Report a problem), About and the theme. The menu bar and the status bar are gone.

use eframe::egui;

use super::help::{mark_spot, Spot};
use super::icons::{self, Icon};
use super::shell::{model_row, size_text, FILTER_FROM};
use super::theme;
use super::App;

/// The height of the head of the sidebar, the QuickAI menu's row.
const HEAD: f32 = 34.0;
/// The button at the right of the head that puts the sidebar away.
const HIDE: egui::Vec2 = egui::vec2(30.0, 30.0);

impl App {
    /// The sidebar: the QuickAI menu, the computer and the engine, the box that narrows the list, and the models (none
    /// for an engine that runs on its own, which keeps its files on its page), with Recommended and MCP servers at its
    /// foot.
    pub(super) fn show_sidebar(&mut self, ui: &mut egui::Ui, active: bool) {
        self.show_app_menu_row(ui);
        ui.add_space(8.0);
        self.show_where_and_engine(ui);
        // Recommended and MCP servers at the foot, under a line, and the models in the room above them (the user, on
        // 2026-11-09)
        egui::Panel::bottom("sidebar_foot")
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                ui.add_space(6.0);
                let y = ui.cursor().top();
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    y,
                    egui::Stroke::new(1.0, theme::palette().line),
                );
                ui.add_space(7.0);
                self.show_nav_rows(ui);
                ui.add_space(2.0);
                self.show_help_row(ui);
            });
        let there = self.setup_in_use().is_some();
        let alone = self.runs_alone();
        if !alone && !there && self.models.len() > FILTER_FROM {
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.model_filter)
                    .id(egui::Id::new("model_search"))
                    .hint_text("Search models")
                    .desired_width(ui.available_width()),
            );
        } else if self.models.len() <= FILTER_FROM {
            self.model_filter.clear();
        }
        if alone {
            return;
        }
        ui.add_space(12.0);
        if there {
            self.show_models_there(ui);
            return;
        }
        self.show_models_head(ui, active);
        self.show_models_list(ui);
    }

    /// The head of the sidebar: the logo and QuickAI, a menu (S2 of the samples): Settings, Configurations, the theme,
    /// Help, Report a problem, About QuickAI and Quit.
    fn show_app_menu_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let width = ui.available_width();
        // the menu, and at its right the button that puts the sidebar away (as in the A6 samples)
        let mut hide = false;
        let (rect, response) = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let menu = ui.allocate_exact_size(
                    egui::vec2((width - HIDE.x - 4.0).max(60.0), HEAD),
                    egui::Sense::click(),
                );
                hide = icons::icon_button(ui, Icon::DoubleLeft, HIDE, true, false)
                    .on_hover_text(
                        "Hide the sidebar. The » at the left end of the bar brings it back.",
                    )
                    .clicked();
                menu
            })
            .inner;
        if hide {
            self.sidebar_open = false;
        }
        mark_spot(ui, Spot::Settings, rect);
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if ui.is_rect_visible(rect) {
            if open || response.hovered() {
                ui.painter().rect_filled(rect, 8.0, p.selected);
            }
            let logo = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 6.0, rect.center().y - 11.0),
                egui::vec2(22.0, 22.0),
            );
            self.paint_logo(ui, logo);
            let family = theme::semibold(ui);
            let name = egui::WidgetText::from(
                egui::RichText::new("QuickAI")
                    .size(15.0)
                    .family(family)
                    .color(p.text),
            )
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            );
            let at = egui::pos2(logo.right() + 9.0, rect.center().y - name.size().y / 2.0);
            let chevron_left = at.x + name.size().x + 6.0;
            ui.painter().galley(at, name, p.text);
            icons::paint(
                ui.painter(),
                egui::Rect::from_min_size(
                    egui::pos2(chevron_left, rect.center().y - 6.0),
                    egui::vec2(12.0, 12.0),
                ),
                Icon::Chevron,
                p.muted,
            );
        }
        let response = response.on_hover_text("Settings (Ctrl+,), configurations, and Quit.");
        let mut asked: Option<AppMenu> = None;
        egui::Popup::menu(&response)
            .align(egui::RectAlign::BOTTOM_START)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(240.0)
            .show(|ui| {
                ui.set_width(240.0);
                asked = self.app_menu(ui);
                if asked.is_some() {
                    ui.close();
                }
            });
        match asked {
            Some(AppMenu::Settings) => self.toggle_settings(),
            Some(AppMenu::Configurations) => {
                self.open_configurations();
                self.raise_configurations = true;
            }
            Some(AppMenu::Quit) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
            None => {}
        }
    }

    /// The items of the QuickAI menu: Settings, Configurations and Quit (the theme, Help, Report a problem and About are
    /// at the foot of the sidebar, C206).
    fn app_menu(&mut self, ui: &mut egui::Ui) -> Option<AppMenu> {
        let mut asked = None;
        if icons::menu_item_keyed(ui, Some(Icon::Gear), "Settings…", "Ctrl+,").clicked() {
            asked = Some(AppMenu::Settings);
        }
        if icons::menu_item_keyed(ui, Some(Icon::Sliders), "Configurations…", "").clicked() {
            asked = Some(AppMenu::Configurations);
        }
        ui.separator();
        if icons::menu_item_keyed(ui, Some(Icon::Close), "Quit", "").clicked() {
            asked = Some(AppMenu::Quit);
        }
        asked
    }

    /// The last row of the sidebar (C206): **Help** with its word at the left, a menu that opens upwards (Help, the tour,
    /// the keys, Report a problem), and at the right About QuickAI and the theme, as pictures.
    fn show_help_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let mut asked: Option<HelpMenu> = None;
        let mut about = false;
        let mut theme_flip = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // Help: its picture and its word, lit while its menu is open or the pointer is on it
            let font = egui::TextStyle::Body.resolve(ui.style());
            let word = ui
                .painter()
                .layout_no_wrap("Help".to_string(), font, p.body);
            let (rect, help) = ui.allocate_exact_size(
                egui::vec2(36.0 + word.size().x + 12.0, 30.0),
                egui::Sense::click(),
            );
            mark_spot(ui, Spot::Help, rect);
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&help));
            if ui.is_rect_visible(rect) {
                if open {
                    ui.painter().rect_filled(rect, 8.0, p.selected);
                } else if help.hovered() {
                    ui.painter()
                        .rect_filled(rect, 8.0, p.selected.gamma_multiply(0.55));
                }
                icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 18.0, rect.center().y),
                        egui::vec2(15.0, 15.0),
                    ),
                    Icon::Help,
                    p.muted,
                );
                ui.painter().galley(
                    egui::pos2(rect.left() + 36.0, rect.center().y - word.size().y / 2.0),
                    word,
                    p.body,
                );
            }
            let help = help.on_hover_text("Help (F1), the tour, the keys, and Report a problem.");
            egui::Popup::menu(&help)
                .align(egui::RectAlign::TOP_START)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                .width(230.0)
                .show(|ui| {
                    ui.set_width(230.0);
                    if icons::menu_item_keyed(ui, Some(Icon::Help), "Help", "F1").clicked() {
                        asked = Some(HelpMenu::Help);
                    }
                    if icons::menu_item_keyed(ui, Some(Icon::Route), "Take the tour", "").clicked()
                    {
                        asked = Some(HelpMenu::Tour);
                    }
                    if icons::menu_item_keyed(ui, Some(Icon::Keyboard), "Keyboard shortcuts", "")
                        .clicked()
                    {
                        asked = Some(HelpMenu::Keys);
                    }
                    ui.separator();
                    if icons::menu_item_keyed(ui, Some(Icon::Warn), "Report a problem…", "")
                        .clicked()
                    {
                        asked = Some(HelpMenu::Report);
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dark = self.dark_mode;
                theme_flip = icons::icon_button(
                    ui,
                    if dark { Icon::Moon } else { Icon::Sun },
                    egui::vec2(30.0, 30.0),
                    true,
                    false,
                )
                .on_hover_text(if dark {
                    "Switch to the light theme"
                } else {
                    "Switch to the dark theme"
                })
                .clicked();
                about = icons::icon_button(ui, Icon::Info, egui::vec2(30.0, 30.0), true, false)
                    .on_hover_text("About QuickAI: its version, its site and its license.")
                    .clicked();
            });
        });
        if theme_flip {
            let dark = !self.dark_mode;
            self.set_dark_mode(ui.ctx(), dark);
        }
        if about {
            // open already, somewhere behind the main window: it comes to the front
            self.raise_about = self.show_about;
            self.show_about = true;
        }
        match asked {
            Some(HelpMenu::Help) => self.open_help(),
            Some(HelpMenu::Tour) => self.start_tour(ui.ctx()),
            Some(HelpMenu::Keys) => self.open_help_at(super::help::HelpPage::Shortcuts),
            Some(HelpMenu::Report) => self.open_report_problem(),
            None => {}
        }
    }

    /// Where the models run and the engine that runs them, in one box: the computer (with whether it answers) and its
    /// menu, and under it the engine (with a dot while it runs) and its menu. An engine alone in the list has no row.
    fn show_where_and_engine(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let engines = self.engine_choices().len();
        let frame = egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, p.line))
            .corner_radius(egui::CornerRadius::same(9));
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            self.show_computer_row(ui);
            if engines >= 2 {
                let y = ui.cursor().top();
                ui.painter()
                    .hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, p.line));
                self.show_engine_row(ui);
            }
        });
    }

    /// The computer the models run on: its name, a line of what it is, a dot of whether it answers, and its menu.
    fn show_computer_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let (name, dot) = self.computer_now();
        let line = self.switch_line();
        let width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 46.0), egui::Sense::click());
        mark_spot(ui, Spot::Computer, rect);
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if ui.is_rect_visible(rect) {
            if open || response.hovered() {
                ui.painter()
                    .rect_filled(rect, card_corners(true), p.selected);
            }
            let icon = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 19.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            );
            icons::paint(ui.painter(), icon, Icon::Computer, p.muted);
            let left = rect.left() + 36.0;
            let room = (rect.right() - 28.0 - left).max(10.0);
            let title = egui::WidgetText::from(egui::RichText::new(&name).color(p.text))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    room - 14.0,
                    egui::TextStyle::Body,
                );
            let small = egui::WidgetText::from(egui::RichText::new(&line).color(p.muted))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    room,
                    egui::TextStyle::Small,
                );
            let top = rect.top() + (46.0 - title.size().y - small.size().y - 1.0) / 2.0;
            let title_right = left + title.size().x;
            let title_mid = top + title.size().y / 2.0;
            let below = top + title.size().y + 1.0;
            ui.painter().galley(egui::pos2(left, top), title, p.text);
            ui.painter().galley(egui::pos2(left, below), small, p.muted);
            if let Some(answers) = dot {
                ui.painter().circle_filled(
                    egui::pos2(title_right + 8.0, title_mid),
                    3.5,
                    if answers { p.ok } else { p.bad },
                );
            }
            paint_chevron(ui, rect, p.muted);
        }
        let response = response.on_hover_text(
            "Where the models run: this computer, or a more powerful one you added (reached over SSH). The list \
             under it is that computer's models; the chat and Connect an app work the same either way.",
        );
        egui::Popup::menu(&response)
            .align(egui::RectAlign::BOTTOM_START)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .width(300.0)
            .show(|ui| {
                ui.set_width(300.0);
                self.show_where_menu(ui);
            });
    }

    /// The engine in use: its name, a dot while it runs, and its menu (each engine with whether it is ready, and Manage
    /// engines). Greyed with why while the server runs.
    fn show_engine_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let lock = self.engine_lock_reason();
        let state = self.session.state();
        let width = ui.available_width();
        let sense = if lock.is_none() {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), sense);
        mark_spot(ui, Spot::EngineMenu, rect);
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if ui.is_rect_visible(rect) {
            if lock.is_none() && (open || response.hovered()) {
                ui.painter()
                    .rect_filled(rect, card_corners(false), p.selected);
            }
            let icon = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 19.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            );
            icons::paint(ui.painter(), icon, Icon::Package, p.muted);
            let name = ui.painter().layout_no_wrap(
                self.engine_name(),
                egui::TextStyle::Body.resolve(ui.style()),
                p.text,
            );
            let at = egui::pos2(rect.left() + 36.0, rect.center().y - name.size().y / 2.0);
            ui.painter().galley(at, name, p.text);
            let color = match state {
                crate::events::ServerState::Running => Some(p.ok),
                crate::events::ServerState::Starting => Some(p.warn),
                _ => None,
            };
            if let Some(color) = color {
                ui.painter().circle_filled(
                    egui::pos2(rect.right() - 34.0, rect.center().y),
                    4.0,
                    color,
                );
            }
            paint_chevron(ui, rect, if lock.is_none() { p.muted } else { p.faint });
        }
        let response = match lock {
            Some(why) => response.on_hover_text(format!("Engine. {why}")),
            None => response.on_hover_text(
                "Engine: the program that runs the models. Each keeps its own models and settings.",
            ),
        };
        if lock.is_some() {
            return;
        }
        let mut chosen = None;
        egui::Popup::menu(&response)
            .align(egui::RectAlign::BOTTOM_START)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .width(270.0)
            .show(|ui| {
                ui.set_width(270.0);
                chosen = self.engine_menu_items(ui);
            });
        self.do_engine_choice(chosen);
    }

    /// Recommended and MCP servers, a row each (option A6 of the samples): the recommended models of this computer, and
    /// the tools the model can use, with how many servers are on (llama.cpp's for now).
    fn show_nav_rows(&mut self, ui: &mut egui::Ui) {
        if self.can_recommend() {
            let on = self.view == super::MainView::Recommended;
            let row = nav_row(ui, Icon::Bulb, "Recommended", None, true, on)
                .on_hover_text("The models that suit this computer, for what you want to do.");
            if row.clicked() {
                if self.catalog.open {
                    self.view = super::MainView::Recommended;
                } else {
                    self.open_catalog();
                }
            }
        }
        let said = format!("{} on", self.mcp_server_count().unwrap_or(0));
        let on = self.view == super::MainView::Tools;
        let row = nav_row(ui, Icon::Wrench, "MCP servers", Some(&said), true, on).on_hover_text(
            "Tools the model can use when you chat, with any engine: read files, fetch a web page, the time.",
        );
        if row.clicked() {
            self.open_mcp_servers();
        }
    }

    /// MODELS, the room they take (and "from the last run" for an engine that lists them while it is stopped), and +:
    /// Add a model, Recommended, Open the models folder and Refresh the list (the Models menu of the menu bar was these).
    fn show_models_head(&mut self, ui: &mut egui::Ui, active: bool) {
        let p = theme::palette();
        let ready = !self.needs_install();
        let mut asked: Option<PlusMenu> = None;
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let family = theme::semibold(ui);
            ui.label(egui::RichText::new("MODELS").size(11.5).family(family).color(p.muted));
            if !self.models.is_empty() {
                ui.label(
                    egui::RichText::new(format!(
                        "· {}",
                        size_text(super::model_details::total_bytes(&self.models))
                    ))
                    .size(11.0)
                    .color(p.faint),
                )
                .on_hover_text("The room the models listed take on the disk.");
            }
            if self.lists_models() && self.named.stale {
                ui.label(egui::RichText::new("· from the last run").size(11.0).color(p.faint))
                    .on_hover_text(format!(
                        "As {} listed them the last time it ran. Start it to see what it has now.",
                        self.engine_name()
                    ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let plus = icons::icon_button(ui, Icon::Plus, egui::vec2(26.0, 24.0), true, false);
                mark_spot(ui, Spot::AddModel, plus.rect);
                let plus = if ready {
                    plus.on_hover_text(
                        "Add a model (Ctrl+K): search for one, paste a link, or add a file you already have. And the \
                         recommended models, and the models folder.",
                    )
                } else {
                    plus.on_hover_text(format!("Install {} first: the page has its button.", self.server_name()))
                };
                egui::Popup::menu(&plus)
                    .align(egui::RectAlign::BOTTOM_END)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                    .width(250.0)
                    .show(|ui| {
                        ui.set_width(250.0);
                        if icons::menu_item_keyed(ui, Some(Icon::Plus), "Add a model…", "Ctrl+K")
                            .clicked()
                            && ready
                        {
                            asked = Some(PlusMenu::Add);
                        }
                        if self.can_recommend()
                            && icons::menu_item_keyed(ui, Some(Icon::Bulb), "Recommended…", "").clicked()
                        {
                            asked = Some(PlusMenu::Recommended);
                        }
                        if !self.lists_models()
                            && icons::menu_item_keyed(ui, Some(Icon::Folder), "Open the models folder", "")
                                .clicked()
                        {
                            asked = Some(PlusMenu::Folder);
                        }
                        let refresh = icons::menu_item(ui, Icon::Refresh, "Refresh the list", !active, false);
                        let refresh = refresh.on_hover_text(if self.lists_models() {
                            format!(
                                "Ask the running {} which models it has. Stopped, this shows what it listed the last \
                                 time it ran.",
                                self.server_name()
                            )
                        } else {
                            "Read the models folders again, for a file you put there by hand.".to_string()
                        });
                        if refresh.clicked() {
                            asked = Some(PlusMenu::Refresh);
                        }
                    });
            });
        });
        ui.add_space(2.0);
        match asked {
            Some(PlusMenu::Add) => self.open_add_model(),
            Some(PlusMenu::Recommended) => self.open_catalog(),
            Some(PlusMenu::Folder) => super::workbench::open_folder(&self.models_dir),
            Some(PlusMenu::Refresh) => self.refresh_list(ui.ctx()),
            None => {}
        }
    }

    /// The models of the list, each with what is known of it, its mark (green while it runs) and its menu.
    fn show_models_list(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let words = self.model_filter.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("models_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if self.models.is_empty() {
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 8,
                            right: 8,
                            top: 4,
                            bottom: 0,
                        })
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("No models yet").color(p.faint));
                            if !self.needs_install() && ui.link("Add your first model").clicked() {
                                self.open_add_model();
                            }
                        });
                    return;
                }
                let mut picked = None;
                let mut asked = None;
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
                    let row = row.on_hover_ui(|ui| {
                        ui.label(self.row_hover(m, lists, offload));
                    });
                    if row.clicked() {
                        picked = Some(i);
                        self.view = super::MainView::Model;
                    }
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
                        ui.add_space(8.0);
                        ui.label(theme::muted("No model matches."));
                    });
                    if !self.needs_install() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(8.0);
                            if ui.link("Search for it online").clicked() {
                                self.open_add_model();
                            }
                        });
                    }
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

    /// The logo in `rect`.
    fn paint_logo(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let texture = self.logo_texture(ui.ctx());
        ui.painter().image(
            texture,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

/// What the QuickAI menu was asked to do.
#[derive(Clone, Copy)]
enum AppMenu {
    Settings,
    Configurations,
    Quit,
}

/// What the menu of Help at the foot of the sidebar was asked to do.
#[derive(Clone, Copy)]
enum HelpMenu {
    Help,
    Tour,
    Keys,
    Report,
}

/// What the + of the list was asked to do.
#[derive(Clone, Copy)]
enum PlusMenu {
    Add,
    Recommended,
    Folder,
    Refresh,
}

/// The corners of a row of the box of the computer and the engine: the top row has the box's top corners, the other
/// its bottom ones.
fn card_corners(top: bool) -> egui::CornerRadius {
    if top {
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        }
    } else {
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: 8,
            se: 8,
        }
    }
}

/// The chevron at the right end of a row that opens a menu.
fn paint_chevron(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let chevron = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 16.0, rect.center().y),
        egui::vec2(12.0, 12.0),
    );
    icons::paint(ui.painter(), chevron, Icon::Chevron, color);
}

/// A row of the sidebar that goes somewhere: its picture, its name, and what it says at its right end (`said`).
fn nav_row(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    said: Option<&str>,
    enabled: bool,
    on: bool,
) -> egui::Response {
    let p = theme::palette();
    let width = ui.available_width();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), sense);
    if ui.is_rect_visible(rect) {
        if on {
            ui.painter().rect_filled(rect, 8.0, p.selected);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(rect, 8.0, p.selected.gamma_multiply(0.55));
        }
        let ink = if enabled { p.body } else { p.faint };
        let square = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            egui::vec2(15.0, 15.0),
        );
        icons::paint(
            ui.painter(),
            square,
            icon,
            if enabled { p.muted } else { p.faint },
        );
        ui.painter().text(
            egui::pos2(rect.left() + 36.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            ink,
        );
        if let Some(said) = said {
            ui.painter().text(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                said,
                egui::TextStyle::Small.resolve(ui.style()),
                p.faint,
            );
        }
    }
    response
}
