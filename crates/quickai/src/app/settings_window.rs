//! The Settings window (C190): a sidebar of pages with the engine in use at its head, and the page that is chosen
//! beside it. **General** has the models folder and live traffic; **Program** the builds of the engine's program;
//! **Starting it** what the program is given besides what QuickAI gives it, and the command line that makes; the
//! program that goes with the engine (Open WebUI beside Ollama) has a page of its own; **Other computers** has the
//! switch of WP4.33. Each setting is a row: its name and what it does on the left, its control on the right.
//!
//! It is a window of its own, not a panel drawn over the main one: it can be put beside the main window,
//! made any size, and left open while the main window is used. (It is an immediate viewport: the frame
//! that draws the main window draws it too, so both always show the same state.)

use std::path::PathBuf;

use eframe::egui;

use quickai_core::form::{FormField, FormModel};

use super::icons::{self, Icon};
use super::runtime_manager::INSTALL_PYTHON_FIELD;
use super::theme;
use super::{App, PendingDelete};
use crate::hardware;

/// What the window is called to the window manager: its title bar, and what a taskbar lists.
pub(super) const SETTINGS_TITLE: &str = "QuickAI Settings";

/// The size the window opens at, and the least it can be made. A page that is taller than the window scrolls.
const OPENS_AT: [f32; 2] = [760.0, 600.0];
const LEAST: [f32; 2] = [400.0, 320.0];

/// Narrower than this, the sidebar is its pictures alone (and says each page's name when the pointer rests on it).
const NARROW: f32 = 560.0;
const SIDEBAR_WIDE: f32 = 196.0;
const SIDEBAR_NARROW: f32 = 52.0;

/// The margin of a group of settings, and so how far the line between two rows reaches past them.
const GROUP_MARGIN: i8 = 14;

/// The pages of the window, in the order of the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum SettingsPage {
    #[default]
    General,
    Program,
    Start,
    Companion,
    Computers,
}

/// Which window the Settings are: the id the display knows it by.
pub(super) fn settings_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_settings")
}

impl App {
    /// Draws the Settings window, if it is open: a window of its own (see the head of this file).
    pub(super) fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let id = settings_viewport();
        // asked for again while open, somewhere behind the main window: they come to the front
        if std::mem::take(&mut self.raise_settings) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        // the page that what opened the window asked for: the builds ("Manage engines...", "Choose a version..."), or the
        // program that goes with the engine ("Set up chat UI...")
        if std::mem::take(&mut self.advanced_section_wanted) {
            self.settings_page = SettingsPage::Program;
        }
        if std::mem::take(&mut self.companion_section_wanted) && self.companion_name().is_some() {
            self.settings_page = SettingsPage::Companion;
        }
        // a page this engine has not (no program goes with it) is the first one
        if !self
            .settings_pages()
            .iter()
            .any(|nav| nav.page == self.settings_page)
        {
            self.settings_page = SettingsPage::General;
        }
        let form = self.form();
        let mut close = false;
        let builder = crate::with_app_icon(
            egui::ViewportBuilder::default()
                .with_title(SETTINGS_TITLE)
                .with_app_id(crate::APP_ID)
                .with_inner_size(OPENS_AT)
                .with_min_inner_size(LEAST),
        );
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                // A context that cannot open a window (a test draws on one with no display) shows it inside
                // the main window: the pages in a row over the page, and its Close button under it.
                ui.horizontal_wrapped(|ui| {
                    for nav in self.settings_pages() {
                        if ui
                            .selectable_label(self.settings_page == nav.page, &nav.label)
                            .clicked()
                        {
                            self.settings_page = nav.page;
                        }
                    }
                });
                let room = (ui.ctx().content_rect().height() - 150.0).max(200.0);
                egui::ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.show_settings_page(ui, &form));
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            } else {
                // the window manager's own close button asks to close it, and it is closed by not being drawn
                if ui.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
                egui::Panel::bottom("settings_footer").show(ui, |ui| {
                    ui.add_space(6.0);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    ui.add_space(6.0);
                });
                let narrow = ui.available_width() < NARROW;
                egui::Panel::left("settings_sidebar")
                    .exact_size(if narrow { SIDEBAR_NARROW } else { SIDEBAR_WIDE })
                    .resizable(false)
                    .frame(theme::bar_frame(egui::Margin::symmetric(8, 12)))
                    .show(ui, |ui| self.show_settings_sidebar(ui, narrow));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::palette().page).inner_margin(
                        egui::Margin {
                            left: 18,
                            right: 14,
                            top: 14,
                            bottom: 10,
                        },
                    ))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("settings_page", self.settings_page))
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.show_settings_page(ui, &form));
                    });
            }
            // every delete button of the window asks its question in the window, where it was pressed
            if !self.delete_asked_in_window {
                self.show_confirm_delete_modal(ui.ctx());
            }
        });
        if close {
            self.close_settings();
        }
    }

    /// Opens the Settings, or brings them to the front when they are open already (a window of their
    /// own can be behind the main one).
    pub(super) fn open_settings(&mut self) {
        if !self.show_settings {
            self.settings_opening();
        }
        self.raise_settings = self.show_settings;
        self.show_settings = true;
    }

    /// What the window opens on: the page it was on last, or the program that goes with the engine when it is wanted
    /// and not there yet (the thing to do).
    pub(super) fn settings_opening(&mut self) {
        if self.companion_enabled() && !self.companion_installed() {
            self.companion_section_wanted = true;
        }
    }

    /// Closes the Settings and writes down what was changed in them, however they were closed: by their
    /// Close button, their window's own close button or the Settings button. A question asked in them (a delete
    /// waiting for its yes) goes with them: nobody is left to answer it.
    pub(super) fn close_settings(&mut self) {
        self.show_settings = false;
        self.raise_settings = false;
        // a question asked in the main window is the main window's to answer
        if !self.delete_asked_in_window {
            self.pending_delete = None;
            self.delete_of_companion = false;
        }
        // what the manager of builds shows of the disk is not looked at while the window is closed: it is read again when it is opened
        self.runtime.disk.stale = true;
        self.as_companion(|app| app.runtime.disk.stale = true);
        self.save_settings();
    }

    /// The pages of the sidebar for the engine in use: the program that goes with it has one when there is one, and other
    /// computers when this build sets them up.
    pub(super) fn settings_pages(&self) -> Vec<NavItem> {
        let mut pages = vec![
            NavItem::new(SettingsPage::General, Icon::Sliders, "General"),
            NavItem {
                count: (self.runtime.disk.known && !self.runtime.disk.builds.is_empty())
                    .then_some(self.runtime.disk.builds.len()),
                ..NavItem::new(SettingsPage::Program, Icon::Package, "Program")
            },
            NavItem::new(SettingsPage::Start, Icon::Terminal, "Starting it"),
        ];
        if let Some(name) = self.companion_name() {
            pages.push(NavItem::new(SettingsPage::Companion, Icon::Chat, &name));
        }
        if cfg!(feature = "remote-setup") {
            pages.push(NavItem::new(
                SettingsPage::Computers,
                Icon::Computer,
                "Other computers",
            ));
        }
        pages
    }

    /// The sidebar: whose settings these are, and the pages.
    fn show_settings_sidebar(&mut self, ui: &mut egui::Ui, narrow: bool) {
        let p = theme::palette();
        // the count of the builds is the disk's, read on a thread of its own whatever page is shown
        self.refresh_disk(ui.ctx());
        let engine = self.engine.clone();
        ui.horizontal(|ui| {
            ui.add_space(if narrow { 3.0 } else { 6.0 });
            self.engine_badge(ui, &engine);
            if !narrow {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(
                        egui::RichText::new(self.engine_name())
                            .strong()
                            .color(p.text),
                    );
                    ui.label(
                        egui::RichText::new("the engine in use")
                            .small()
                            .color(p.faint),
                    );
                });
            }
        })
        .response
        .on_hover_text(
            "These are the settings of the engine in use. Switch engines in the menu bar.",
        );
        ui.add_space(8.0);
        let line = ui.cursor().top();
        let wide = ui.max_rect();
        ui.painter().hline(
            (wide.left() - 8.0)..=(wide.right() + 8.0),
            line,
            egui::Stroke::new(1.0, p.line),
        );
        ui.add_space(8.0);
        for nav in self.settings_pages() {
            let chosen = self.settings_page == nav.page;
            if nav_item(ui, &nav, chosen, narrow).clicked() {
                self.settings_page = nav.page;
            }
            ui.add_space(2.0);
        }
    }

    /// The page that is chosen, and under it what went wrong in it.
    fn show_settings_page(&mut self, ui: &mut egui::Ui, form: &Option<FormModel>) {
        match self.settings_page {
            SettingsPage::General => self.show_general_page(ui, form),
            SettingsPage::Program => self.show_program_page(ui, form),
            SettingsPage::Start => self.show_start_page(ui, form),
            SettingsPage::Companion => self.show_companion_page(ui),
            SettingsPage::Computers => self.show_computers_page(ui),
        }
        if let Some(msg) = &self.settings_error {
            ui.add_space(8.0);
            ui.colored_label(theme::palette().bad, msg);
        }
    }

    /// General: where the models are, and the live traffic.
    fn show_general_page(&mut self, ui: &mut egui::Ui, form: &Option<FormModel>) {
        page_head(ui, "General", None);
        group(ui, |ui| {
            if self.lists_models() {
                self.show_listed_models_row(ui);
            } else {
                self.show_models_folder(ui, form);
                if self.runs_alone() {
                    self.show_downloaded_models(ui);
                }
            }
            // an engine with no conversation to show has nothing for the proxy to do
            if self.reads_traffic() {
                row_line(ui);
                self.show_live_traffic_row(ui);
            }
        });
    }

    /// Program: the builds of the engine's program, and the Python an install is made with when it has one.
    fn show_program_page(&mut self, ui: &mut egui::Ui, form: &Option<FormModel>) {
        let name = self.engine_name();
        page_head(
            ui,
            "Program",
            Some(&format!("The build of {name} that Start runs.")),
        );
        let (program, _) = advanced_fields(form);
        let (manager, install): (Vec<FormField>, Vec<FormField>) = program
            .into_iter()
            .partition(|f| f.widget.as_deref() == Some("runtime_manager"));
        for field in &manager {
            self.show_runtime_manager(ui, field);
        }
        if !install.is_empty() {
            ui.add_space(10.0);
            group(ui, |ui| self.show_fields(ui, &install, Some(&name)));
        }
    }

    /// Starting it: what the program is given besides what QuickAI gives it, and the command line that makes.
    fn show_start_page(&mut self, ui: &mut egui::Ui, form: &Option<FormModel>) {
        let name = self.engine_name();
        page_head(ui, &format!("Starting {}", self.server_name()), None);
        let (_, others) = advanced_fields(form);
        if !others.is_empty() {
            group(ui, |ui| self.show_fields(ui, &others, Some(&name)));
        }
        self.show_command_line(ui);
    }

    /// Other computers: whether QuickAI sets them up (WP4.33).
    fn show_computers_page(&mut self, ui: &mut egui::Ui) {
        page_head(ui, "Other computers", None);
        group(ui, |ui| {
            let mut on = self.remote_setup_on();
            let turned = switch_row(
                ui,
                "Set up other computers and run configurations there",
                Some(
                    "QuickAI copies its headless program to a computer you add and runs the configuration you \
                     select there; the status bar switches between the runs.",
                ),
                Some(
                    "Off, the computers you add are ones you run an engine on yourself. Switching it off stops \
                     nothing: a run there goes on until it is stopped.",
                ),
                &mut on,
                true,
            );
            if turned {
                self.set_remote_setup(on);
            }
        });
    }

    /// The live traffic, on or off: a switch that is grey while the server runs, and says so.
    fn show_live_traffic_row(&mut self, ui: &mut egui::Ui) {
        let active = self.session.is_active();
        let server = self.server_name();
        let desc = if active {
            format!("Stop {server} to change this: it applies at the next Start.")
        } else {
            format!("A Live tab with each request and reply, through a small proxy in front of {server}.")
        };
        let hover = format!(
            "Adds a small local proxy in front of {server} so the \"Live traffic\" tab can show chat/completion \
             requests and replies live, from any client — the built-in chat UI or an external app connected with Connect \
             an app. Off by default: clients then connect straight to {server}, with no extra hop or overhead. Takes \
             effect the next time you click Start."
        );
        let mut on = self.enable_traffic_proxy;
        let turned = switch_row(
            ui,
            "Show live conversation traffic",
            Some(&desc),
            Some(&hover),
            &mut on,
            !active,
        );
        if turned {
            self.enable_traffic_proxy = on;
            self.save_settings();
        }
    }

    /// The folder the downloaded models are in: how many there are, the buttons that change it and open it, and the
    /// path, which can be typed (Apply then appears). Deleting a model is its own menu's, in the main window.
    fn show_models_folder(&mut self, ui: &mut egui::Ui, form: &Option<FormModel>) {
        let field = form.as_ref().and_then(|f| f.field("models_dir"));
        let title = field.map_or_else(|| "Models folder".to_string(), |f| f.label.clone());
        let downloaded: Vec<&crate::models::ModelEntry> = self
            .models
            .iter()
            .filter(|m| !m.path.starts_with(&self.bundled_models_dir))
            .collect();
        let size: u64 = downloaded.iter().map(|m| m.size_bytes).sum();
        let desc = match downloaded.len() {
            0 => "No models in it yet. Downloads go here.".to_string(),
            1 => format!(
                "1 model, {}. Downloads go here.",
                hardware::format_bytes(size)
            ),
            n => format!(
                "{n} models, {}. Downloads go here.",
                hardware::format_bytes(size)
            ),
        };
        // a download writes into the folder it began in: the folder changes when it is done
        let idle = self.download.is_none();
        let busy = "Wait for the download to finish: it is written into the folder in use.";
        let help = field.and_then(|f| f.help.clone());
        let mut choose = false;
        let mut open = false;
        let _ = setting_row(ui, &title, None, help.as_deref(), |ui| {
            if ui.button("Open").on_hover_text("Open the folder").clicked() {
                open = true;
            }
            if ui
                .add_enabled(idle, egui::Button::new("Change…"))
                .on_hover_text("Choose another folder for the models")
                .on_disabled_hover_text(busy)
                .clicked()
            {
                choose = true;
            }
        });
        if open {
            super::workbench::open_folder(&self.models_dir);
        }
        if choose {
            if let Some(dir) = rfd::FileDialog::new()
                .set_directory(&self.models_dir)
                .pick_folder()
            {
                self.apply_models_dir(dir);
            }
        }
        ui.add_space(4.0);
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.models_dir_input)
                .desired_width(ui.available_width()),
        );
        // how many models it holds, under the box (the box is right under the name, as every box of the window is)
        ui.label(theme::muted(desc));
        let typed = self.models_dir_input.trim().to_string();
        let edited = typed != self.models_dir.display().to_string();
        let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let mut apply = edited && entered && idle;
        if !edited && self.models_dir_override.is_none() {
            return;
        }
        ui.horizontal(|ui| {
            if edited {
                if ui
                    .add_enabled(idle, egui::Button::new("Apply"))
                    .on_hover_text("Use this folder from now on and re-scan it for models.")
                    .on_disabled_hover_text(busy)
                    .clicked()
                {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    self.models_dir_input = self.models_dir.display().to_string();
                }
            } else if self.models_dir_override.is_some()
                && ui
                    .add_enabled(idle, egui::Button::new("Reset to default"))
                    .on_hover_text("Use QuickAI's own folder for the models again.")
                    .on_disabled_hover_text(busy)
                    .clicked()
            {
                let default_dir = self.default_models_dir();
                let _ = std::fs::create_dir_all(&default_dir);
                self.use_models_dir(default_dir, false);
            }
        });
        if apply {
            self.apply_models_dir(PathBuf::from(typed));
        }
    }

    /// The downloaded models, newest first, each with Delete in its ⋯ menu: only for an engine whose main window lists
    /// none (ComfyUI says how many it has), where this is the one place a model is deleted. The other engines' are
    /// deleted from the menu of their row in the main window.
    fn show_downloaded_models(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let mut downloaded: Vec<&crate::models::ModelEntry> = self
            .models
            .iter()
            .filter(|m| !m.path.starts_with(&self.bundled_models_dir))
            .collect();
        if downloaded.is_empty() {
            return;
        }
        // a model has no other notion of "latest" than its file's own time
        downloaded.sort_by_key(|m| {
            std::cmp::Reverse(std::fs::metadata(&m.path).and_then(|m| m.modified()).ok())
        });
        row_line(ui);
        ui.label(
            egui::RichText::new("Downloaded models")
                .strong()
                .color(p.text),
        )
        .on_hover_text("The models in the folder above, newest first. Bundled models aren't listed: they can't be deleted.");
        let mut to_delete: Option<PendingDelete> = None;
        egui::ScrollArea::vertical()
            .id_salt("downloaded_models_scroll")
            .auto_shrink([false, true])
            .max_height(30.0 * DELETE_LIST_MAX_ROWS as f32)
            .show(ui, |ui| {
                for m in downloaded {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 28.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let more = icons::icon_button(
                                ui,
                                Icon::More,
                                egui::vec2(26.0, 24.0),
                                true,
                                false,
                            )
                            .on_hover_text("Delete it");
                            egui::Popup::menu(&more)
                                .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                                .show(|ui| {
                                    ui.set_min_width(150.0);
                                    ui.set_max_width(170.0);
                                    if icons::menu_item(ui, Icon::Trash, "Delete…", true, true)
                                        .on_hover_text("Delete this model from disk")
                                        .clicked()
                                    {
                                        to_delete = Some(PendingDelete::Model {
                                            filename: m.filename.clone(),
                                            path: m.path.clone(),
                                        });
                                    }
                                });
                            ui.label(
                                egui::RichText::new(hardware::format_bytes(m.size_bytes))
                                    .small()
                                    .color(p.faint),
                            );
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&m.display).color(p.body),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&m.filename);
                                },
                            );
                        },
                    );
                }
            });
        if let Some(pending) = to_delete {
            self.pending_delete = Some(pending);
        }
    }

    /// Uses a folder the person gave for the models, made when it is not there; one that cannot be made is said.
    fn apply_models_dir(&mut self, candidate: PathBuf) {
        if std::fs::create_dir_all(&candidate).is_ok() {
            self.use_models_dir(candidate, true);
        } else {
            self.settings_error = Some(format!("Could not create/use {}", candidate.display()));
        }
    }
}

impl App {
    /// Uses `dir` as the models folder from now on (`chosen`: the person's, else the engine's own): the list is read
    /// from it, the settings say it, and so does the value the next run is planned from (an engine that is told its
    /// folder on its command line, as ComfyUI is, was started with the old one until the application started again).
    pub(super) fn use_models_dir(&mut self, dir: PathBuf, chosen: bool) {
        self.models_dir = dir.clone();
        self.models_dir_input = dir.display().to_string();
        if chosen {
            self.values.set("models_dir", dir.display().to_string());
            self.models_dir_override = Some(dir);
        } else {
            self.values.clear("models_dir");
            self.models_dir_override = None;
        }
        self.rescan_models();
        self.save_settings();
        self.settings_error = None;
    }

    /// The models an engine that keeps its own lists: how many and how big. Removing one is its menu's, in the main
    /// window's list, as it is for a downloaded file.
    fn show_listed_models_row(&mut self, ui: &mut egui::Ui) {
        let name = self.engine_name();
        let server = self.server_name();
        let size: u64 = self.models.iter().map(|m| m.size_bytes).sum();
        let desc = match self.models.len() {
            0 => format!("{name} has not listed any models yet."),
            1 => format!(
                "1 model, {}, as {server} listed it. Remove it from its ⋯ menu in the model list.",
                hardware::format_bytes(size)
            ),
            n => format!(
                "{n} models, {}, as {server} listed them. Remove one from its ⋯ menu in the model list.",
                hardware::format_bytes(size)
            ),
        };
        let _ = setting_row(ui, "Models", Some(&desc), None, |_| ());
    }
}

/// A page of the sidebar: which, its picture, its name, and a count beside the name (the builds installed).
pub(super) struct NavItem {
    pub page: SettingsPage,
    pub icon: Icon,
    pub label: String,
    pub count: Option<usize>,
}

impl NavItem {
    fn new(page: SettingsPage, icon: Icon, label: &str) -> NavItem {
        NavItem {
            page,
            icon,
            label: label.to_string(),
            count: None,
        }
    }
}

/// A page in the sidebar: its picture and its name (its picture alone when the sidebar is narrow), on the grey of what is
/// chosen.
fn nav_item(ui: &mut egui::Ui, nav: &NavItem, chosen: bool, narrow: bool) -> egui::Response {
    let p = theme::palette();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if chosen || response.hovered() {
            let fill = if chosen {
                p.raised
            } else {
                p.raised.gamma_multiply(0.5)
            };
            ui.painter().rect_filled(rect, 6.0, fill);
        }
        let icon_left = if narrow {
            rect.center().x - 8.0
        } else {
            rect.left() + 10.0
        };
        icons::paint(
            ui.painter(),
            egui::Rect::from_min_size(
                egui::pos2(icon_left, rect.center().y - 8.0),
                egui::vec2(16.0, 16.0),
            ),
            nav.icon,
            if chosen { p.text } else { p.muted },
        );
        if !narrow {
            let font = egui::FontId::new(
                egui::TextStyle::Body.resolve(ui.style()).size,
                if chosen {
                    theme::semibold(ui)
                } else {
                    egui::FontFamily::Proportional
                },
            );
            ui.painter().text(
                egui::pos2(rect.left() + 36.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &nav.label,
                font,
                if chosen { p.text } else { p.body },
            );
            if let Some(count) = nav.count {
                ui.painter().text(
                    egui::pos2(rect.right() - 10.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    count.to_string(),
                    egui::TextStyle::Small.resolve(ui.style()),
                    p.faint,
                );
            }
        }
    }
    if narrow {
        response.on_hover_text(&nav.label)
    } else {
        response
    }
}

/// The head of a page: its title, and a line under it when it has one.
pub(super) fn page_head(ui: &mut egui::Ui, title: &str, lead: Option<&str>) {
    let p = theme::palette();
    ui.label(
        egui::RichText::new(title)
            .size(18.0)
            .family(theme::semibold(ui))
            .color(p.text),
    );
    if let Some(lead) = lead {
        ui.label(egui::RichText::new(lead).small().color(p.muted));
    }
    ui.add_space(8.0);
}

/// Settings that belong together, in a box; `row_line` goes between two of them.
pub(super) fn group<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    theme::card_frame()
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(GROUP_MARGIN, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The line between two rows of a group, from one side of its box to the other.
pub(super) fn row_line(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    let y = ui.cursor().top();
    let room = ui.max_rect();
    let margin = f32::from(GROUP_MARGIN);
    ui.painter().hline(
        (room.left() - margin)..=(room.right() + margin),
        y,
        egui::Stroke::new(1.0, theme::palette().line),
    );
    ui.add_space(9.0);
}

/// A setting on a row: its name, and what it does in small words under it, on the left; `control` on the right, drawn
/// from the right edge. `hover` is said when the pointer rests on the name. What comes back is what `control` gave, and
/// the name's response: a click on the name of a switch turns it over, as a click on the words of a tick box did.
pub(super) fn setting_row<R>(
    ui: &mut egui::Ui,
    title: &str,
    desc: Option<&str>,
    hover: Option<&str>,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, egui::Response) {
    let p = theme::palette();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reply = control(ui);
            ui.add_space(12.0);
            let name = ui
                .with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let name = ui.add(
                        egui::Label::new(egui::RichText::new(title).color(p.text))
                            .sense(egui::Sense::click()),
                    );
                    let name = match hover {
                        Some(hover) => name.on_hover_text(hover),
                        None => name,
                    };
                    if let Some(desc) = desc {
                        ui.add(
                            egui::Label::new(egui::RichText::new(desc).small().color(p.muted))
                                .wrap(),
                        );
                    }
                    name
                })
                .inner;
            (reply, name)
        })
        .inner
    })
    .inner
}

/// A setting that is on or off, on a row: a switch at the right, which a click on its name turns over too. `true` when
/// it was turned over (`on` is then what it is now).
pub(super) fn switch_row(
    ui: &mut egui::Ui,
    title: &str,
    desc: Option<&str>,
    hover: Option<&str>,
    on: &mut bool,
    enabled: bool,
) -> bool {
    let (switched, name) = setting_row(ui, title, desc, hover, |ui| {
        theme::switch(ui, on, enabled).changed()
    });
    if !switched && enabled && name.clicked() {
        *on = !*on;
        return true;
    }
    switched
}

/// The fields of the engine's Advanced section, shown, as the Program page has them (the manager of its builds and the
/// Python an install is made with) and as Starting it has them (the rest).
fn advanced_fields(form: &Option<FormModel>) -> (Vec<FormField>, Vec<FormField>) {
    form.as_ref()
        .and_then(|f| f.section("advanced"))
        .map(|s| {
            s.fields
                .iter()
                .filter(|f| f.visible)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .partition(is_program_field)
}

/// Whether a field is about the program itself: the manager of its builds, or the Python its install is made with.
pub(super) fn is_program_field(field: &FormField) -> bool {
    field.widget.as_deref() == Some("runtime_manager") || field.id == INSTALL_PYTHON_FIELD
}

/// How many rows the list of installed builds shows before it scrolls, so a long history of installs doesn't push the
/// rest of the page down.
pub(super) const DELETE_LIST_MAX_ROWS: usize = 4;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashSet};
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::super::runtime_manager::Scan;
    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::prims::install::{InstallSpec, InstalledBuild};
    use crate::settings::Settings;
    use crate::testkit::{scratch, wait_until};

    const GB: u64 = 1024 * 1024 * 1024;

    /// What a window of its own was asked to be and what it painted.
    struct Window {
        builder: egui::ViewportBuilder,
        shapes: Vec<egui::epaint::ClippedShape>,
        /// The size the display gave it.
        size: egui::Vec2,
    }

    #[derive(Default)]
    struct Shown {
        /// The windows drawn in the frame that is going.
        windows: BTreeMap<egui::ViewportId, Window>,
        /// What the person did in a window, for the next pass of it.
        events: BTreeMap<egui::ViewportId, Vec<egui::Event>>,
        /// The windows whose close button was pressed since they were last drawn.
        closed: HashSet<egui::ViewportId>,
        /// The size to open a window at instead of the one it asks for.
        size: Option<egui::Vec2>,
    }

    /// A display that opens a window of its own for a viewport, as the toolkit does on a screen: the code
    /// of the window runs as a pass of its own, on a screen of the size the window asked for, with the
    /// events the person gave that window. It writes down what each window asked for and painted: what a
    /// person would see in it. (A context with no display shows the same code as a window of the main one.)
    struct Display(Rc<RefCell<Shown>>);

    impl Display {
        fn attach(ctx: &egui::Context) -> Display {
            let shown = Rc::new(RefCell::new(Shown::default()));
            ctx.set_embed_viewports(false);
            let inner = Rc::clone(&shown);
            egui::Context::set_immediate_viewport_renderer(move |ctx, viewport| {
                let egui::ImmediateViewport {
                    ids,
                    builder,
                    mut viewport_ui_cb,
                } = viewport;
                let (events, closed, size) = {
                    let mut shown = inner.borrow_mut();
                    (
                        shown.events.remove(&ids.this).unwrap_or_default(),
                        shown.closed.remove(&ids.this),
                        shown.size,
                    )
                };
                let size = size
                    .or(builder.inner_size)
                    .unwrap_or(egui::vec2(500.0, 500.0));
                let mut info = egui::ViewportInfo::default();
                if closed {
                    info.events.push(egui::ViewportEvent::Close);
                }
                let mut raw = egui::RawInput {
                    viewport_id: ids.this,
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                };
                raw.viewports.insert(ids.this, info);
                let mut output = ctx.run_ui(raw, |ui| viewport_ui_cb(ui));
                output.textures_delta.clear();
                inner.borrow_mut().windows.insert(
                    ids.this,
                    Window {
                        builder,
                        shapes: output.shapes,
                        size,
                    },
                );
            });
            Display(shown)
        }

        /// What the person does in a window, in the next frame.
        fn give(&self, window: egui::ViewportId, events: Vec<egui::Event>) {
            self.0.borrow_mut().events.insert(window, events);
        }

        /// The person presses the window manager's close button on a window.
        fn press_close_button(&self, window: egui::ViewportId) {
            self.0.borrow_mut().closed.insert(window);
        }

        fn open_windows_at(&self, size: egui::Vec2) {
            self.0.borrow_mut().size = Some(size);
        }
    }

    fn app_in(dir: &Path, engine: &str) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        )
    }

    /// One frame of the application on the display: the main window, with the Settings window drawn as
    /// the application draws it. The windows the frame drew are the display's afterwards.
    fn frame(app: &mut App, ctx: &egui::Context, display: &Display) -> egui::FullOutput {
        display.0.borrow_mut().windows.clear();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 560.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            app.show_window(ui);
            app.show_settings_window(ui.ctx());
        });
        output.textures_delta.clear();
        output
    }

    /// A few frames with nothing done in them: a window that was just opened finds its size and where
    /// everything goes over its first frames, as a real one does.
    fn settle(app: &mut App, ctx: &egui::Context, display: &Display) {
        for _ in 0..4 {
            frame(app, ctx, display);
        }
    }

    /// Every text the shapes paint, with where.
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => into.push((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                _ => {}
            }
        }
        let mut into = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    fn said(shapes: &[egui::epaint::ClippedShape]) -> String {
        texts(shapes)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Where `text` is painted in the window of the display, if it is.
    fn where_is(display: &Display, text: &str) -> Option<egui::Rect> {
        let shown = display.0.borrow();
        let window = shown.windows.get(&settings_viewport())?;
        texts(&window.shapes)
            .into_iter()
            .find(|(painted, _)| painted == text)
            .map(|(_, rect)| rect)
    }

    /// The person clicks `at` in the Settings window: the pointer goes there, then presses, then lets go
    /// (a frame each, as a pointer arrives).
    fn click_in_settings(app: &mut App, ctx: &egui::Context, display: &Display, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
            vec![],
        ] {
            display.give(settings_viewport(), events);
            frame(app, ctx, display);
        }
    }

    #[test]
    fn p3_f85_the_settings_are_a_window_of_their_own_and_none_of_them_is_drawn_in_the_main_window()
    {
        let dir = scratch("f85");
        let mut app = app_in(&dir, "llamacpp");
        // the engine's program is there (a file is all the window looks for): the main window's page is the model's, not
        // the one that installs the engine (WP4.31)
        app.binary = dir.join("llama-server");
        std::fs::write(&app.binary, "").unwrap();
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);

        // closed, the display is asked for nothing
        let out = frame(&mut app, &ctx, &display);
        assert!(display.0.borrow().windows.is_empty());
        assert!(!out.viewport_output.contains_key(&settings_viewport()));
        assert!(said(&out.shapes).contains("Add a model"), "the main window");

        // open, it is asked for one window, named and sized, with the icon of the application...
        app.toggle_settings();
        let out = frame(&mut app, &ctx, &display);
        assert!(out.viewport_output.contains_key(&settings_viewport()));
        {
            let shown = display.0.borrow();
            assert_eq!(shown.windows.len(), 1);
            let window = &shown.windows[&settings_viewport()];
            assert_eq!(window.builder.title.as_deref(), Some("QuickAI Settings"));
            assert_eq!(window.builder.app_id.as_deref(), Some(crate::APP_ID));
            assert_eq!(
                window.builder.icon.is_some(),
                !cfg!(target_os = "macos"),
                "the icon of the application (on macOS the app's own, in the Dock)"
            );
            assert!(
                window.builder.decorations != Some(false)
                    && window.builder.resizable != Some(false),
                "the window manager's own title bar, and a size the person can change"
            );
            // a size to open at, wide enough for its sidebar and a page beside it (C190)
            let size = window.builder.inner_size.expect("a size to open at");
            assert_eq!(size, egui::vec2(760.0, 600.0));
            // ...and what is in it is there, and in no other window: the pages, and the first of them
            let inside = said(&window.shapes);
            for text in [
                "General",
                "Program",
                "Starting it",
                "Downloaded models folder",
                "Show live conversation traffic",
                "Close",
            ] {
                assert!(inside.contains(text), "{text:?} in the window: {inside}");
            }
            // the engine is chosen in the menu bar, not here (it had a row of its own)
            assert!(!inside.contains("Engine:"), "{inside}");
            let main = said(&out.shapes);
            for text in [
                "Show live conversation traffic",
                "Starting it",
                "Downloaded models folder",
            ] {
                assert!(!main.contains(text), "{text:?} in the main window: {main}");
            }
            assert!(
                main.contains("Add a model") && main.contains("Add your first model"),
                "{main}"
            );
        }

        // closed again, the window is not asked for and so goes
        app.toggle_settings();
        let out = frame(&mut app, &ctx, &display);
        assert!(display.0.borrow().windows.is_empty());
        assert!(!out.viewport_output.contains_key(&settings_viewport()));
    }

    /// B22 (C189): the engine's Advanced settings end with the command line of the next Start, made as Start makes it:
    /// the projector a vision model was fetched with is on it, on a row of its own that says it is the model's, after
    /// the settings' rows and before the person's own words; the variables are under it, the person's lines with the
    /// document's; and llama.cpp takes lines of environment as the other engines do.
    #[test]
    fn p3_b22_the_settings_show_the_command_line_of_the_next_start_with_the_projector_on_it() {
        let dir = scratch("b22");
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("tiny-vl-Q4_K_M.gguf"), "GGUF").unwrap();
        std::fs::write(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf"), "GGUF").unwrap();
        std::fs::write(
            models.join("tiny-vl-Q4_K_M.gguf.bundle.json"),
            r#"{ "version": 1, "bundle": "vision", "source": "https://huggingface.co/me/tiny-vl-GGUF",
                 "parts": [ { "kind": "mmproj", "label": "projector", "file": "me--tiny-vl-GGUF--mmproj-f16.gguf", "name": "mmproj-f16.gguf" } ] }"#,
        )
        .unwrap();
        let mut app = app_in(&dir, "llamacpp");
        app.binary = dir.join("llama-server");
        std::fs::write(&app.binary, "").unwrap();
        app.rescan_models();
        app.values.set("extra_args", "--threads 4");
        app.values
            .set("extra_env", "GGML_VK_VISIBLE_DEVICES=0\nHF_TOKEN=abc");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        display.open_windows_at(egui::vec2(1000.0, 1600.0));
        app.toggle_settings();
        app.settings_page = SettingsPage::Start;
        settle(&mut app, &ctx, &display);

        let at = |text: &str| {
            where_is(&display, text).unwrap_or_else(|| {
                let shown = display.0.borrow();
                panic!(
                    "{text:?} in {}",
                    said(&shown.windows[&settings_viewport()].shapes)
                )
            })
        };
        at("Extra environment");
        at("Command line");
        // the projector by the name it came with, not the one it is kept under
        let projector = at("--mmproj …/mmproj-f16.gguf");
        let said_by = at("the model's projector");
        assert!(
            (projector.center().y - said_by.center().y).abs() < 4.0,
            "on the projector's row: {projector:?} {said_by:?}"
        );
        assert!(at("--model …/tiny-vl-Q4_K_M.gguf").top() < projector.top());
        assert!(
            at("Context size").top() < projector.top(),
            "the settings' rows first"
        );
        assert!(
            at("--threads 4").top() > projector.top(),
            "the person's words last"
        );
        // the variables, folded: QuickAI's library path and the person's two lines
        at("Environment (3)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// I28 (C190): the window is a sidebar of pages beside the page that is chosen, on groups as the main window's
    /// boxes are; the models folder has its buttons on the row of its name and its box under them, and only what can be
    /// done: Reset to default with a folder of the person's, Apply once the box says another.
    #[test]
    fn p3_i28_the_models_folder_has_its_buttons_on_the_row_of_its_name_and_the_window_is_pages_in_a_sidebar(
    ) {
        let dir = scratch("i28");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        let label = where_is(&display, "Downloaded models folder").expect("the name of the folder");
        let change = where_is(&display, "Change…").expect("a button that changes it");
        let open = where_is(&display, "Open").expect("a button that opens it");
        // at the end of the name's row, one beside the other
        for button in [change, open] {
            assert!(
                (button.center().y - label.center().y).abs() < 10.0,
                "{button:?} is not on the row of {label:?}"
            );
        }
        assert!(
            label.right() < change.left() && change.right() < open.left(),
            "{label:?} {change:?} {open:?}"
        );
        // the folder is QuickAI's own: nothing to reset, and nothing typed to apply
        assert!(where_is(&display, "Reset to default").is_none());
        assert!(where_is(&display, "Apply").is_none());
        // the pages are in a sidebar on the left of the page, the one shown marked
        let general = texts(&display.0.borrow().windows[&settings_viewport()].shapes)
            .into_iter()
            .filter(|(text, _)| text == "General")
            .map(|(_, rect)| rect)
            .collect::<Vec<_>>();
        let program = where_is(&display, "Program").expect("the page of the program");
        assert!(
            general.iter().any(|r| r.right() < label.left()) && program.right() < label.left(),
            "the sidebar is on the left: {general:?} {program:?} {label:?}"
        );
        assert_eq!(app.settings_page, SettingsPage::General);
        {
            let shown = display.0.borrow();
            let window = &shown.windows[&settings_viewport()];
            // the settings are on a group, as the main window's are on its boxes
            fn cards(shape: &egui::epaint::Shape, into: &mut Vec<egui::Rect>) {
                match shape {
                    egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| cards(s, into)),
                    egui::epaint::Shape::Rect(r)
                        if r.fill == super::super::theme::palette_of(true).card
                            || r.fill == super::super::theme::palette_of(false).card =>
                    {
                        into.push(r.rect)
                    }
                    _ => {}
                }
            }
            let mut found = Vec::new();
            for clipped in &window.shapes {
                cards(&clipped.shape, &mut found);
            }
            assert!(
                found.iter().any(|c| c.contains(label.center())),
                "the folder is not on a group: {found:?}"
            );
        }
        // the page of the program, chosen in the sidebar, is the one shown
        click_in_settings(&mut app, &ctx, &display, program.center());
        settle(&mut app, &ctx, &display);
        assert_eq!(app.settings_page, SettingsPage::Program);
        assert!(where_is(&display, "The build of llama.cpp that Start runs.").is_some());
        assert!(where_is(&display, "Downloaded models folder").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// I30 (C190): in a window narrower than a sidebar and a page want, the sidebar is the pages' pictures alone (each
    /// says its name when the pointer rests on it), and the page keeps the room; wide again, the names are back.
    #[test]
    fn p3_i30_in_a_narrow_window_the_sidebar_is_its_pictures_and_the_page_keeps_the_room() {
        let dir = scratch("i30");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        display.open_windows_at(egui::vec2(480.0, 600.0));
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        for name in ["Program", "Starting it", "the engine in use"] {
            assert!(where_is(&display, name).is_none(), "{name} is painted");
        }
        let page = where_is(&display, "Downloaded models folder").expect("the page");
        assert!(page.left() < 100.0, "the page has the room: {page:?}");
        display.open_windows_at(egui::vec2(760.0, 600.0));
        settle(&mut app, &ctx, &display);
        assert!(where_is(&display, "Starting it").is_some());
        let page = where_is(&display, "Downloaded models folder").expect("the page");
        assert!(page.left() > 190.0, "beside the sidebar: {page:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// I32 (C190): an engine whose main window lists no models (ComfyUI says how many it has) has them listed on the
    /// General page, each with Delete in its ⋯ menu, which asks in the window; an engine whose main window lists them
    /// (llama.cpp) does not, its rows' own menus deleting them.
    #[test]
    fn p3_i32_the_models_are_listed_in_the_settings_only_for_an_engine_whose_window_lists_none() {
        for (engine, listed) in [("comfyui", true), ("llamacpp", false)] {
            let dir = scratch(&format!("i32-{engine}"));
            let mut app = app_in(&dir, engine);
            let folder = if engine == "comfyui" {
                app.models_dir.join("diffusion_models")
            } else {
                app.models_dir.clone()
            };
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("zeta-model.gguf"), vec![0u8; 2048]).unwrap();
            app.rescan_models();
            assert!(
                app.models.iter().any(|m| m.filename == "zeta-model.gguf"),
                "{engine}"
            );
            let ctx = egui::Context::default();
            let display = Display::attach(&ctx);
            app.toggle_settings();
            settle(&mut app, &ctx, &display);
            assert_eq!(
                where_is(&display, "Downloaded models").is_some(),
                listed,
                "{engine}"
            );
            if listed {
                choose_in_menu_of_row(&mut app, &ctx, &display, "zeta-model.gguf", "Delete…");
                frame(&mut app, &ctx, &display);
                let inside = said(&display.0.borrow().windows[&settings_viewport()].shapes);
                assert!(
                    inside.contains("Delete model?") && inside.contains("zeta-model.gguf"),
                    "{inside}"
                );
                assert!(
                    folder.join("zeta-model.gguf").exists(),
                    "not before the yes"
                );
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn p3_i29_a_choice_of_the_settings_is_beside_its_label_and_a_path_is_under_its() {
        let dir = scratch("i29");
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        let page = where_is(&display, "Starting it").expect("the page of how it is started");
        click_in_settings(&mut app, &ctx, &display, page.center());
        settle(&mut app, &ctx, &display);
        let label =
            where_is(&display, "When Ollama is already running").expect("the choice's name");
        let shown = where_is(&display, "auto").expect("what the choice shows");
        assert!(
            (shown.center().y - label.center().y).abs() < 10.0 && shown.left() > label.right(),
            "{shown:?} is not beside {label:?}"
        );
        // a path wants the width of the window, and is under its name
        let path = where_is(&display, "Model store").expect("the name of the store");
        let browse = where_is(&display, "Browse…").expect("the box's button");
        assert!(path.top() > label.bottom() && browse.top() > path.bottom());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_f86_however_the_settings_window_is_closed_what_was_changed_in_it_is_written() {
        let dir = scratch("f86");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        let saved = |name: &str| -> serde_json::Value {
            std::fs::read_to_string(dir.join("settings.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .map(|json: serde_json::Value| json["values"][name].clone())
                .unwrap_or(serde_json::Value::Null)
        };
        for way in ["its Close button", "the close button of the window manager"] {
            let mut app = app_in(&dir, "comfyui");
            app.toggle_settings();
            settle(&mut app, &ctx, &display);
            assert!(app.show_settings && !display.0.borrow().windows.is_empty());
            let python = format!("/usr/bin/python3.{}", way.len());
            app.values.set("python", python.clone());
            if way == "its Close button" {
                let close = where_is(&display, "Close")
                    .expect("a Close button")
                    .center();
                click_in_settings(&mut app, &ctx, &display, close);
            } else {
                display.press_close_button(settings_viewport());
                frame(&mut app, &ctx, &display);
            }
            assert!(!app.show_settings, "closed with {way}");
            assert_eq!(
                saved("comfyui.python"),
                serde_json::Value::String(python),
                "what was typed is kept, closed with {way}"
            );
            let out = frame(&mut app, &ctx, &display);
            assert!(
                display.0.borrow().windows.is_empty()
                    && !out.viewport_output.contains_key(&settings_viewport()),
                "the window is gone once closed with {way}"
            );
        }
    }

    /// A reader of the disk that finds one build of llama.cpp.
    fn one_build_reader(spec: &InstallSpec) -> Scan {
        let dir = spec.root.join("b1-llama-b1-bin-ubuntu-vulkan-x64");
        Scan {
            builds: vec![InstalledBuild {
                label: "b1-llama-b1-bin-ubuntu-vulkan-x64".into(),
                binary_path: dir.join("llama-server"),
                install_dir: dir,
                size_bytes: 1_000_000,
            }],
            unfinished: Vec::new(),
        }
    }

    /// The ⋯ of the row of the list of builds that `name` is on, at the right of the window's group, is clicked, and then
    /// what its menu says `item`.
    fn choose_in_menu_of_row(
        app: &mut App,
        ctx: &egui::Context,
        display: &Display,
        name: &str,
        item: &str,
    ) {
        let row = where_is(display, name).unwrap_or_else(|| panic!("the row of {name}"));
        let width = display.0.borrow().windows[&settings_viewport()].size.x;
        click_in_settings(app, ctx, display, egui::pos2(width - 41.0, row.center().y));
        let item = where_is(display, item).unwrap_or_else(|| panic!("{item} in the menu"));
        click_in_settings(app, ctx, display, item.center());
    }

    #[test]
    fn p3_f87_a_delete_asked_in_the_settings_is_asked_in_their_window_and_goes_with_it() {
        let dir = scratch("f87");
        let mut app = app_in(&dir, "llamacpp");
        app.runtime.disk.reader = Some(one_build_reader);
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        app.settings_page = SettingsPage::Program;
        wait_until(Duration::from_secs(10), "the build to be listed", || {
            frame(&mut app, &ctx, &display);
            where_is(&display, "b1 · Vulkan").is_some()
        });
        settle(&mut app, &ctx, &display);

        // Delete is chosen in the ⋯ menu of the build, in the window...
        choose_in_menu_of_row(&mut app, &ctx, &display, "b1 · Vulkan", "Delete…");
        assert!(app.pending_delete.is_some(), "the question is waiting");
        // ...and the question is in that window, not in the main one
        let out = frame(&mut app, &ctx, &display);
        let inside = said(&display.0.borrow().windows[&settings_viewport()].shapes);
        assert!(
            inside.contains("Delete llama-server build?")
                && inside.contains("b1-llama-b1-bin-ubuntu-vulkan-x64"),
            "{inside}"
        );
        assert!(!said(&out.shapes).contains("Delete llama-server build?"));

        // the window is closed with the question open: nobody is left to answer it, and it is not asked again
        display.press_close_button(settings_viewport());
        frame(&mut app, &ctx, &display);
        assert!(!app.show_settings && app.pending_delete.is_none());
        app.toggle_settings();
        frame(&mut app, &ctx, &display);
        let inside = said(&display.0.borrow().windows[&settings_viewport()].shapes);
        assert!(!inside.contains("Delete llama-server build?"), "{inside}");
    }

    #[test]
    fn p3_f88_asked_for_again_while_open_the_settings_come_to_the_front_and_are_not_opened_twice() {
        let dir = scratch("f88");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        let commands = |out: &egui::FullOutput| {
            out.viewport_output
                .get(&settings_viewport())
                .map(|v| v.commands.clone())
                .unwrap_or_default()
        };

        // opened, the window is not told to take the focus (it is given it by being new)
        app.invoke("open:settings");
        let out = frame(&mut app, &ctx, &display);
        assert!(app.show_settings && display.0.borrow().windows.len() == 1);
        assert!(!commands(&out).contains(&egui::ViewportCommand::Focus));

        // asked for again (a button of the main window says "open the settings"), it comes to the front, once
        app.invoke("open:settings");
        let out = frame(&mut app, &ctx, &display);
        assert!(
            commands(&out).contains(&egui::ViewportCommand::Focus),
            "{:?}",
            commands(&out)
        );
        assert_eq!(display.0.borrow().windows.len(), 1, "one window");
        let out = frame(&mut app, &ctx, &display);
        assert!(
            !commands(&out).contains(&egui::ViewportCommand::Focus),
            "once"
        );
        assert!(app.show_settings, "asking is not closing");
    }

    #[test]
    fn p3_f89_the_close_button_stays_at_the_foot_of_a_window_that_is_too_short_for_what_is_in_it() {
        let dir = scratch("f89");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        // a person made the window small: the settings scroll, and Close is where it was
        display.open_windows_at(egui::vec2(480.0, 150.0));
        settle(&mut app, &ctx, &display);
        let close = where_is(&display, "Close").expect("a Close button");
        let size = display.0.borrow().windows[&settings_viewport()].size;
        assert!(
            close.max.y <= size.y && close.min.y > size.y - 60.0,
            "Close is at the foot of a window {size:?} tall: {close:?}"
        );
        // and in a window of the size it opens at it is in the same place, at its foot
        display.open_windows_at(egui::vec2(500.0, 620.0));
        settle(&mut app, &ctx, &display);
        let close = where_is(&display, "Close").expect("a Close button");
        assert!(
            close.min.y > 620.0 - 60.0 && close.max.y <= 620.0,
            "{close:?}"
        );
        // what the page says is above it, and not under it
        let live = where_is(&display, "Show live conversation traffic").expect("the last row");
        assert!(live.max.y < close.min.y, "{live:?} {close:?}");
    }

    static SCANS: AtomicUsize = AtomicUsize::new(0);

    /// A reader of the disk that finds nothing and counts how many times it was asked.
    fn counting_reader(_: &InstallSpec) -> Scan {
        SCANS.fetch_add(1, Ordering::SeqCst);
        Scan {
            builds: Vec::new(),
            unfinished: Vec::new(),
        }
    }

    #[test]
    fn p3_f92_the_builds_are_read_from_the_disk_again_when_the_settings_window_is_opened_again() {
        // The window of the Settings is a window of its own, and the passes it counts begin again from none each time it is
        // opened: what tells the manager of builds that it was away (a build installed, an install that stopped, meanwhile)
        // must not be a count of the window's own. (Found by the GUI suite, TC-CFR-013: a stopped install was not listed
        // when the Settings were opened again.)
        let dir = scratch("f92");
        let mut app = app_in(&dir, "comfyui");
        app.runtime.disk.reader = Some(counting_reader);
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        let scans = || SCANS.load(Ordering::SeqCst);
        let scans_after = |app: &mut App, wanted: usize, what: &str| {
            wait_until(Duration::from_secs(10), what, || {
                frame(app, &ctx, &display);
                scans() == wanted
            });
        };

        // opened, the disk is read once (the sidebar counts the builds, whatever page is shown) and not again while the
        // window goes on, the page of the builds chosen or not
        app.toggle_settings();
        scans_after(&mut app, 1, "the first scan");
        settle(&mut app, &ctx, &display);
        let page = where_is(&display, "Program").expect("the page of the builds");
        click_in_settings(&mut app, &ctx, &display, page.center());
        settle(&mut app, &ctx, &display);
        assert_eq!(scans(), 1, "no scan without a reason");

        // closed by any of the ways, and opened again: it is read again
        display.press_close_button(settings_viewport());
        frame(&mut app, &ctx, &display);
        assert!(!app.show_settings);
        app.toggle_settings();
        scans_after(&mut app, 2, "the scan after the window was opened again");

        // and it is, with nothing but the frames of the main window in between, when the window went away some other way
        app.show_settings = false;
        for _ in 0..6 {
            frame(&mut app, &ctx, &display);
        }
        app.show_settings = true;
        scans_after(
            &mut app,
            3,
            "the scan after the window was away for a while",
        );
    }

    // ---- the program that goes with the engine (work package 4.16) ----------------------------------------

    /// Windows as tall as what they hold, as the window manager makes them when they ask (`theme::fit_window_height`,
    /// at most 900 when the screen is not known): this display keeps a window at the size it opens at, and what is under
    /// that is not painted.
    fn tall_enough_for_what_it_holds(display: &Display) {
        display.open_windows_at(egui::vec2(OPENS_AT[0], 900.0));
    }

    /// What the Settings window painted last.
    fn in_settings(display: &Display) -> String {
        said(&display.0.borrow().windows[&settings_viewport()].shapes)
    }

    /// A frame as the application does it between draws: what its parts have to say, then the drawing.
    fn frame_polled(app: &mut App, ctx: &egui::Context, display: &Display) {
        app.poll_runtime_manager();
        app.poll_companion(ctx);
        frame(app, ctx, display);
    }

    /// The launcher of an installed build of Open WebUI, as the document names it for this system.
    fn launcher_file() -> &'static str {
        if cfg!(windows) {
            "open-webui.cmd"
        } else {
            "open-webui"
        }
    }

    fn app_with_companion(dir: &Path, enabled: bool) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        let registry = build_registry(&ConfigDirs::default());
        let values: std::collections::BTreeMap<String, quickai_core::value::Value> = [(
            "openwebui.enabled".to_string(),
            quickai_core::value::Value::Bool(enabled),
        )]
        .into();
        settings
            .set_engine_values(registry.engine("openwebui").unwrap(), &values)
            .unwrap();
        App::build_in(
            settings,
            registry,
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        )
    }

    #[test]
    fn p3_t36_the_settings_of_ollama_offer_open_webui_and_those_of_the_other_engines_do_not() {
        for (engine, offered) in [("llamacpp", false), ("ollama", true), ("comfyui", false)] {
            let dir = scratch(&format!("t36-{engine}"));
            let mut app = app_in(&dir, engine);
            let ctx = egui::Context::default();
            let display = Display::attach(&ctx);
            app.toggle_settings();
            settle(&mut app, &ctx, &display);
            let inside = in_settings(&display);
            // a page of its own in the sidebar, and no word of it in the window of another engine
            assert_eq!(inside.contains("Open WebUI"), offered, "{engine}: {inside}");
            assert_eq!(
                app.settings_pages()
                    .iter()
                    .any(|nav| nav.page == SettingsPage::Companion),
                offered,
                "{engine}"
            );
            assert_eq!(app.companion.is_some(), offered, "{engine}");
        }
        // and the section, opened, says what it is and what it asks
        let dir = scratch("t36-open");
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        tall_enough_for_what_it_holds(&display);
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        let page = where_is(&display, "Open WebUI").expect("its page");
        click_in_settings(&mut app, &ctx, &display, page.center());
        settle(&mut app, &ctx, &display);
        let inside = in_settings(&display);
        for text in [
            "A chat page for your models",
            "License: Open WebUI License",
            "openwebui.org",
            "Start Open WebUI with Ollama",
            "Port",
            "Open WebUI's internet access",
            "Look up Open WebUI builds",
            "Installs a Python environment with",
            "Open WebUI and what it needs",
        ] {
            let text = text.replace("openwebui.org", "openwebui.com");
            assert!(inside.contains(&text), "{text:?} in: {inside}");
        }
        assert!(
            inside.contains("Use a program on disk…")
                && inside.contains("not installed yet")
                && !inside.contains("In use"),
            "its manager is the manager of an engine, and no build is there yet: {inside}"
        );
    }

    #[test]
    fn p3_t37_the_section_is_open_when_it_is_wanted_and_not_there_and_closed_when_it_is_not() {
        let open = |dir: &Path, enabled: bool, installed: bool| -> bool {
            if installed {
                let build = dir.join("data/openwebui/0.11.4");
                std::fs::create_dir_all(&build).unwrap();
                std::fs::write(build.join(launcher_file()), "#!/bin/sh\n").unwrap();
            }
            let mut app = app_with_companion(dir, enabled);
            assert_eq!(app.companion_installed(), installed);
            let ctx = egui::Context::default();
            let display = Display::attach(&ctx);
            app.toggle_settings();
            settle(&mut app, &ctx, &display);
            in_settings(&display).contains("Start Open WebUI with Ollama")
        };
        assert!(
            open(&scratch("t37a"), true, false),
            "asked for and not there: the thing to do"
        );
        assert!(!open(&scratch("t37b"), false, false), "not asked for");
        assert!(
            !open(&scratch("t37c"), true, true),
            "asked for and there: nothing to do"
        );
        // the button that sets it up opens the Settings with the section open, whatever it was (and the person can fold it)
        for (what, enabled, installed) in
            [("a", false, false), ("b", false, true), ("c", true, true)]
        {
            let dir = scratch(&format!("t37{what}2"));
            if installed {
                let build = dir.join("data/openwebui/0.11.4");
                std::fs::create_dir_all(&build).unwrap();
                std::fs::write(build.join(launcher_file()), "#!/bin/sh\n").unwrap();
            }
            let mut app = app_with_companion(&dir, enabled);
            let ctx = egui::Context::default();
            let display = Display::attach(&ctx);
            app.set_up_companion();
            settle(&mut app, &ctx, &display);
            assert!(
                in_settings(&display).contains("Start Open WebUI with Ollama"),
                "the button's click, with the box {enabled} and the program {installed}"
            );
            assert!(
                !app.companion_section_wanted,
                "the wish is for one drawing, not for ever"
            );
        }
    }

    #[test]
    fn p3_t38_ticking_the_box_changes_the_companions_values_only_and_they_are_written_with_the_others(
    ) {
        let dir = scratch("t38");
        let mut app = app_with_companion(&dir, false);
        let ollama_before = app.values.map().clone();
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        let page = where_is(&display, "Open WebUI").expect("its page");
        click_in_settings(&mut app, &ctx, &display, page.center());
        settle(&mut app, &ctx, &display);
        assert!(!app.companion_enabled());

        // its switch is turned over by a click on its name, as a tick box was by a click on its words
        let label = where_is(&display, "Start Open WebUI with Ollama").expect("the switch's name");
        click_in_settings(&mut app, &ctx, &display, label.center());
        settle(&mut app, &ctx, &display);
        assert!(app.companion_enabled(), "the switch is on");
        assert_eq!(
            app.values.map(),
            &ollama_before,
            "Ollama's own values are what they were: the click was the companion's"
        );
        assert!(app.engine == "ollama" && !app.in_companion);

        // closing the window writes both
        display.press_close_button(settings_viewport());
        frame(&mut app, &ctx, &display);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["values"]["openwebui.enabled"], true, "{saved}");
        assert_eq!(saved["values"]["app.engine"], "ollama", "{saved}");
    }

    #[test]
    fn p3_t39_a_build_of_the_companion_is_listed_and_deleted_in_its_own_section_and_ollamas_are_untouched(
    ) {
        let dir = scratch("t39");
        // a build of each: Ollama's, and Open WebUI's
        let ollama_build = dir.join("data/ollama/v0.35.1-ollama-linux-amd64");
        std::fs::create_dir_all(ollama_build.join("bin")).unwrap();
        std::fs::write(ollama_build.join("bin/ollama"), "#!/bin/sh\n").unwrap();
        let build = dir.join("data/openwebui/0.11.4");
        std::fs::create_dir_all(build.join("venv")).unwrap();
        std::fs::write(build.join(launcher_file()), "#!/bin/sh\n").unwrap();
        std::fs::write(build.join("venv/pyvenv.cfg"), "home = x\n").unwrap();
        let mut app = app_with_companion(&dir, true);
        assert!(
            app.companion_installed(),
            "found where the document says builds are kept"
        );
        let ollama_binary = app.binary.clone();
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        tall_enough_for_what_it_holds(&display);
        app.toggle_settings();
        // wanted and there: the window opens where it was; its page is chosen
        settle(&mut app, &ctx, &display);
        let page = where_is(&display, "Open WebUI").expect("its page");
        click_in_settings(&mut app, &ctx, &display, page.center());
        // the disk is read by a thread of its own, and the frame takes the answer in
        wait_until(Duration::from_secs(10), "the builds to be listed", || {
            frame_polled(&mut app, &ctx, &display);
            in_settings(&display).contains("0.11.4")
        });
        let inside = in_settings(&display);
        assert!(
            inside.contains("In use"),
            "its one build is the one in use: {inside}"
        );

        // Delete is chosen in the ⋯ menu of its build: the question is about Open WebUI's build
        choose_in_menu_of_row(&mut app, &ctx, &display, "0.11.4", "Delete…");
        assert!(app.pending_delete.is_some() && app.delete_of_companion);
        frame(&mut app, &ctx, &display);
        let inside = in_settings(&display);
        assert!(
            inside.contains("Delete Open WebUI build?") && inside.contains("0.11.4"),
            "{inside}"
        );
        // and the yes deletes it, on a thread, and not Ollama's
        let yes = where_is(&display, "Delete").expect("the yes");
        click_in_settings(&mut app, &ctx, &display, yes.center());
        wait_until(Duration::from_secs(10), "the build to be deleted", || {
            frame_polled(&mut app, &ctx, &display);
            !build.exists()
        });
        wait_until(Duration::from_secs(10), "the delete to be taken in", || {
            frame_polled(&mut app, &ctx, &display);
            app.companion_slot()
                .is_some_and(|s| s.runtime.delete.is_none())
        });
        assert!(app.pending_delete.is_none() && !app.delete_of_companion);
        assert!(
            ollama_build.join("bin/ollama").is_file(),
            "Ollama's build is where it was"
        );
        assert_eq!(
            app.binary, ollama_binary,
            "and Ollama's program is the one it was"
        );
        assert!(app.runtime.delete.is_none() && app.values.text("bin_override").is_none());
        assert!(
            app.log_lines
                .iter()
                .any(|l| l.contains("Deleted Open WebUI build: 0.11.4")),
            "{:#?}",
            app.log_lines
        );
        assert!(!app.companion_installed(), "it is not installed now");
    }
}
