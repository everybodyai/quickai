//! The window that keeps the configurations: the list with what each one is and says about this
//! computer, and the commands that make, change, share and take away (docs/configurations.md,
//! section 10).
//!
//! It is a window of its own, as the Settings are (an immediate viewport: the frame that draws the main
//! window draws it too, so both always show the same state), and what is changed in it is written when it
//! closes, however it is closed. The dialogs of importing and exporting have a box for the path of the
//! file as well as the button that opens the system's dialog, because that dialog cannot be driven by a
//! test and a person may want to paste a path.

use std::collections::BTreeSet;
use std::path::PathBuf;

use eframe::egui;

use quickai_core::profile::check::{open_text, review, Opened, Report};
use quickai_core::profile::notes::{Level, Note};
use quickai_core::profile::state::{carried, changes, Change};
use quickai_core::profile::{label_of, CommunityRef, Configuration};

use super::configurations_ui::{modal_width, modified_color, NameFor, UNSAVED_MARK};
use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::configurations::{
    self, export_text, import, import_is_taken, Entry, OnTaken, Origin, DEFAULT_ID,
};
use crate::gpu::GpuVendor;

/// What the window is called to the window manager: its title bar, and what a taskbar lists.
pub(super) const CONFIGURATIONS_TITLE: &str = "QuickAI Configurations";

/// The size the window opens at, and the least it can be made.
const OPENS_AT: [f32; 2] = [780.0, 700.0];
const LEAST: [f32; 2] = [440.0, 240.0];

/// Which window the configurations are: the id the display knows it by.
pub(super) fn configurations_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_configurations")
}

/// What is read from a file that is being imported.
#[derive(Clone, Debug)]
pub(super) struct Loaded {
    pub text: String,
    pub opened: Opened,
    pub report: Report,
    pub changes: Vec<Change>,
}

/// The dialog that imports a file from outside.
#[derive(Clone, Debug, Default)]
pub(super) struct ImportDialog {
    pub path: String,
    pub loaded: Option<Loaded>,
    /// Why the file cannot be used, when it cannot.
    pub error: Option<String>,
    /// The settings that are asked about and that the person said yes to.
    pub ticked: BTreeSet<String>,
    pub name: String,
    /// What to do when the name is used.
    pub when_taken: WhenTaken,
    /// The configuration of the community the text was fetched from (Get... of the Community window), and its title:
    /// there is no path, and the stored file says where it came from.
    pub community: Option<(CommunityRef, String)>,
}

/// The three answers to a name that is in use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WhenTaken {
    #[default]
    KeepBoth,
    Replace,
    Cancel,
}

/// The dialog that gives a configuration away.
#[derive(Clone, Debug)]
pub(super) struct ExportDialog {
    pub id: String,
    pub name: String,
    pub note: String,
    pub path: String,
    pub replace: bool,
    pub error: Option<String>,
}

/// What a question of the window is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Confirm {
    /// Go back to what the configuration in use says: what was changed since is lost.
    Revert(String),
    /// Take the person's own file for this configuration away.
    Delete(String),
    /// The person's Default is replaced by the built-in one.
    Reset,
}

/// What a note offers to do about what it says: the one thing it names (getting a model, looking
/// something up, setting something up). Nothing is done until the button is pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NoteAction {
    /// Fetch a model of the recommended list, the way its window does.
    GetCatalog { id: String, file: String },
    /// The engine pulls a model by its name.
    GetNamed { name: String },
    /// Look a file up online, with this in the box.
    LookUp { query: String },
    /// Open the manager of builds with this in its filter.
    LookUpBuild { label: String },
    /// Open the settings of the program that goes with the engine, to set it up.
    SetUp,
}

impl App {
    /// The button a note has, with the words on it and, when it cannot be pressed now, why not.
    pub(super) fn action_for(
        &self,
        note: &Note,
        configuration: &Configuration,
    ) -> Option<(NoteAction, &'static str, Option<String>)> {
        use quickai_core::profile::ModelSource;
        let pick = configuration.models.first();
        let (action, label) = match note.id {
            "model_to_get" => match pick.map(|p| &p.source)? {
                ModelSource::Catalog { id, file } => (
                    NoteAction::GetCatalog {
                        id: id.clone(),
                        file: file.clone()?,
                    },
                    "Get",
                ),
                ModelSource::Named { name, .. } => {
                    (NoteAction::GetNamed { name: name.clone() }, "Get")
                }
                ModelSource::File { name } => (
                    NoteAction::LookUp {
                        query: lookup_words(pick?.from.as_deref(), name),
                    },
                    "Look up online",
                ),
            },
            "model_unknown" => match pick.map(|p| &p.source)? {
                ModelSource::File { name } => (
                    NoteAction::LookUp {
                        query: lookup_words(None, name),
                    },
                    "Look up online",
                ),
                _ => return None,
            },
            "build_missing" | "build_none" => (
                NoteAction::LookUpBuild {
                    label: note.subject.clone()?,
                },
                "Look up builds",
            ),
            "companion_missing" => (NoteAction::SetUp, "Set up"),
            _ => return None,
        };
        // what the window can do is about the engine in use: another engine's builds, models and
        // settings are not what its windows show
        let fetching = matches!(
            action,
            NoteAction::GetCatalog { .. } | NoteAction::GetNamed { .. }
        );
        let disabled = if configuration.engine != self.engine {
            Some("Choose this configuration first: it is for another engine.".to_string())
        } else if matches!(action, NoteAction::GetNamed { .. }) && self.endpoint().is_none() {
            Some(format!(
                "Start {} first: it does the fetching.",
                self.server_name()
            ))
        } else if fetching && (self.download.is_some() || self.named.pull.is_some()) {
            Some("Something is being fetched already.".to_string())
        } else {
            None
        };
        Some((action, label, disabled))
    }

    /// Does what a note's button says.
    pub(super) fn run_note_action(&mut self, action: NoteAction, ctx: &egui::Context) {
        match action {
            NoteAction::GetCatalog { id, file } => {
                let Some(engine) = self.engine_doc() else {
                    return;
                };
                let entries = quickai_core::models::recommended(&self.registry, engine);
                // the weights of that file, with what goes with it when the list pairs them
                let fetch = quickai_core::models::catalog_fetch(&entries, &id, &file);
                match fetch {
                    Some(fetch) => {
                        self.fetch_offer(&fetch, ctx);
                        self.configs.status = Some(format!("Fetching {file}..."));
                    }
                    None => {
                        self.configs.status = Some(format!(
                            "The recommended list has no {file} for this engine."
                        ));
                    }
                }
            }
            NoteAction::GetNamed { name } => {
                if self.pull_named(&name, ctx) {
                    self.configs.status = Some(format!("Fetching {name}..."));
                }
            }
            NoteAction::LookUp { query } => {
                self.model_input = query.clone();
                self.start_lookup(ctx, &query);
                self.configs.status = Some(format!("Looking {query} up online..."));
            }
            NoteAction::LookUpBuild { label } => {
                self.runtime.filter = label;
                self.runtime.show_lookup = true;
                self.open_settings();
            }
            NoteAction::SetUp => self.set_up_companion(),
        }
    }

    /// A note: a sign of how much it matters, its words, and at the right the button it has, if it has one.
    fn show_note(&mut self, ui: &mut egui::Ui, note: &Note, configuration: &Configuration) {
        let text = note.text(self.registry.vocabulary());
        let color = level_color(note.level);
        let action = self.action_for(note, configuration);
        ui.horizontal(|ui| {
            let (square, _) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), egui::Sense::hover());
            if note.level != Level::Says {
                icons::paint(ui.painter(), square, Icon::Warn, color);
            }
            // the words give the button its room, and wrap in what is left
            let button = action.as_ref().map_or(0.0, |(_, label, _)| {
                let font = egui::TextStyle::Button.resolve(ui.style());
                let words = ui
                    .painter()
                    .layout_no_wrap(label.to_string(), font, color)
                    .size()
                    .x;
                words + 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x
            });
            let room = (ui.available_width() - button).max(80.0);
            // as high as the button, so that a line of words is level with it
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(egui::vec2(room, 24.0), layout, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(text).small().color(color)).wrap());
            });
            if let Some((action, label, disabled)) = action {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = theme::button_sized(
                        ui,
                        Role::Normal,
                        disabled.is_none(),
                        label,
                        egui::vec2(0.0, 24.0),
                    );
                    let response = match &disabled {
                        Some(why) => response.on_disabled_hover_text(why),
                        None => response,
                    };
                    if response.clicked() {
                        self.configs.in_window = true;
                        let ctx = ui.ctx().clone();
                        self.run_note_action(action, &ctx);
                    }
                });
            }
        });
    }

    /// Draws the window of the configurations, if it is open: a window of its own.
    pub(super) fn show_configurations_window(&mut self, ctx: &egui::Context) {
        if !self.show_configurations {
            return;
        }
        let id = configurations_viewport();
        if std::mem::take(&mut self.raise_configurations) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let mut close = false;
        let builder = theme::own_window(CONFIGURATIONS_TITLE, OPENS_AT, LEAST);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                let room = (ui.ctx().content_rect().height() - 120.0).max(200.0);
                egui::ScrollArea::vertical()
                    .id_salt("configurations_scroll")
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.show_configurations_body(ui));
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            } else {
                if ui.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
                let footer = egui::Panel::bottom("configurations_footer")
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("Close").clicked() {
                                close = true;
                            }
                            if let Some(status) = &self.configs.status {
                                ui.label(egui::RichText::new(status).small().weak());
                            }
                        });
                        ui.add_space(6.0);
                    })
                    .response
                    .rect
                    .height();
                let frame = egui::Frame::central_panel(&ui.ctx().global_style());
                super::theme::fitted_page(
                    ui,
                    "configurations_scroll",
                    frame,
                    footer,
                    LEAST[1],
                    |ui| self.show_configurations_body(ui),
                );
            }
            // the questions of the window are asked in it
            self.show_configuration_modals(ui.ctx(), true);
            self.show_import_dialog(ui.ctx(), false);
            self.show_export_dialog(ui.ctx());
            self.show_share_dialog(ui.ctx());
        });
        if close {
            self.close_configurations();
        }
    }

    /// Closes the window and writes down what was changed, however it was closed. What was asked in it and
    /// not answered goes with it.
    pub(super) fn close_configurations(&mut self) {
        self.show_configurations = false;
        self.raise_configurations = false;
        if self
            .configs
            .import
            .as_ref()
            .is_some_and(|d| d.community.is_none())
        {
            self.configs.import = None;
        }
        self.configs.export = None;
        self.community.share = None;
        if self.configs.in_window {
            self.configs.question = None;
            self.configs.name_box = None;
            self.configs.confirm = None;
            self.configs.in_window = false;
        }
        self.save_settings();
    }

    /// What the window holds, in the order it is read: the commands that make one, then a card for each
    /// configuration, then the files that could not be used.
    fn show_configurations_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if icons::labelled_button(ui, Role::Normal, true, Icon::Plus, "New from current", 28.0)
                .on_hover_text("Save what you have set now as a new configuration, under a name.")
                .clicked()
            {
                self.configs.in_window = true;
                let name = self.suggested_name();
                self.ask_for_a_name(NameFor::SaveAs, &name);
            }
            if icons::labelled_button(ui, Role::Normal, true, Icon::Download, "Import...", 28.0)
                .on_hover_text(
                    "Read a configuration someone gave you, and say what to take from it.",
                )
                .clicked()
            {
                self.configs.in_window = true;
                self.configs.import = Some(ImportDialog::default());
            }
            if icons::labelled_button(ui, Role::Normal, true, Icon::Globe, "Community...", 28.0)
                .on_hover_text(
                    "Configurations people shared on quickai.everybodyai.dev. The list is fetched when you press this.",
                )
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.open_community(&ctx);
            }
        });
        if let Some(why) = &self.configs.error {
            ui.label(egui::RichText::new(why).small().color(theme::palette().bad));
        }
        ui.add_space(6.0);

        let entries = self.configuration_entries();
        let modified = self.configuration_modified();
        for entry in &entries {
            self.show_configuration_card(ui, entry, modified);
        }
        ui.add_space(4.0);
        if !self.configs.unusable.is_empty() {
            let n = self.configs.unusable.len();
            let heading = format!("{n} file{} couldn't be used", if n == 1 { "" } else { "s" });
            egui::CollapsingHeader::new(theme::muted(heading))
                .id_salt("configurations_unusable")
                .show(ui, |ui| {
                    ui.label(theme::muted(
                        "They are in your configurations folder. Fix them or remove them there.",
                    ));
                    for bad in &self.configs.unusable {
                        ui.label(egui::RichText::new(&bad.file).small().weak());
                        ui.label(
                            egui::RichText::new(&bad.why)
                                .small()
                                .color(theme::palette().bad),
                        );
                    }
                });
        }
    }

    /// One configuration, as a card: the badge of its engine, its name and what is said of it, a line of the
    /// engine, the model and what goes with it, and at the right what can be done (Use; Revert and Save for the
    /// one in use while it has changes; the rest in its menu). A click on the card opens it: what it sets and
    /// its notes. The one in use has a green frame.
    fn show_configuration_card(&mut self, ui: &mut egui::Ui, entry: &Entry, modified: bool) {
        let p = theme::palette();
        let active = entry.id == self.configs.active;
        let open = self.configs.selected.as_deref() == Some(entry.id.as_str());
        let report = self
            .configs
            .reports
            .get(&entry.id)
            .cloned()
            .unwrap_or_default();
        // the outline is coloured once the card knows whether the pointer is on it, and is as wide from the start
        let mut card = egui::Frame::new()
            .fill(p.card)
            .stroke(egui::Stroke::new(1.0, p.line))
            .corner_radius(8.0)
            .inner_margin(egui::Margin {
                left: 10,
                right: 8,
                top: 7,
                bottom: 7,
            })
            .begin(ui);
        let header = {
            let ui = &mut card.content_ui;
            ui.set_width(ui.available_width());
            // the click of the card is under its buttons: a button that is pressed is not a click on the card
            let header = ui
                .scope_builder(
                    egui::UiBuilder::new()
                        .id_salt(("configuration_card", &entry.id))
                        .sense(egui::Sense::click()),
                    |ui| self.show_card_header(ui, entry, &report, active, modified),
                )
                .response;
            if open {
                self.show_card_details(ui, entry, &report);
            }
            header
        };
        card.frame.stroke = egui::Stroke::new(
            1.0,
            if active {
                p.accent
            } else if open || header.contains_pointer() {
                p.outline
            } else {
                p.line
            },
        );
        card.end(ui);
        if header
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.configs.selected = if open { None } else { Some(entry.id.clone()) };
        }
    }

    /// The first rows of a card: the badge, the name and its tags over the line that says what it is, and at the
    /// right its buttons and its menu. The name and the line give up their width to the buttons, and are cut.
    fn show_card_header(
        &mut self,
        ui: &mut egui::Ui,
        entry: &Entry,
        report: &Report,
        active: bool,
        modified: bool,
    ) {
        let p = theme::palette();
        let c = &entry.configuration;
        // a configuration the notes stop cannot be chosen, and the first of them says why
        let stopped = report.stop().map(|n| n.text(self.registry.vocabulary()));
        let notes = report
            .notes
            .iter()
            .filter(|n| n.level != Level::Says)
            .count();
        // what the community's list says of it, once the list was fetched
        let (shared, withdrawn) = self.community_of(entry);
        let shared = shared.map(|(c, _)| super::community_window::stars_words(c));
        let height = BADGE + 4.0;
        let layout = egui::Layout::right_to_left(egui::Align::Center);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), layout, |ui| {
            self.card_menu(ui, entry);
            if active && modified {
                self.card_save_and_revert(ui);
            } else if !active && stopped.is_none() {
                self.card_use(ui, entry);
            }
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.with_layout(layout, |ui| {
                self.engine_badge(ui, &c.engine);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.horizontal(|ui| {
                        // each tag with the order it gives way in when the name has no room: the notes first, then
                        // where it is from, then In use (its frame says it too); Modified never does
                        let mut tags: Vec<(u8, Option<Icon>, String, egui::Color32, egui::Color32)> =
                            Vec::new();
                        let warn = modified_color(ui.visuals().dark_mode);
                        let warn_fill = p.card.lerp_to_gamma(warn, 0.16);
                        if active {
                            tags.push((3, Some(Icon::Check), "In use".into(), p.accent_text, p.accent_soft));
                            if modified {
                                tags.push((u8::MAX, None, format!("{UNSAVED_MARK} Modified"), warn, warn_fill));
                            }
                        }
                        if entry.origin != Origin::Yours {
                            let origin = entry.origin.label().to_string();
                            tags.push((2, Some(Icon::Lock), origin, p.faint, egui::Color32::TRANSPARENT));
                        }
                        if notes > 0 && stopped.is_none() {
                            tags.push((1, Some(Icon::Warn), notes.to_string(), warn, warn_fill));
                        }
                        if withdrawn.is_some() {
                            tags.push((4, None, WITHDRAWN.into(), p.bad, p.card.lerp_to_gamma(p.bad, 0.16)));
                        }
                        if let Some(stars) = &shared {
                            tags.push((0, Some(Icon::Star), stars.clone(), p.muted, egui::Color32::TRANSPARENT));
                        }
                        let spacing = ui.spacing().item_spacing.x;
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let wanted = ui
                            .painter()
                            .layout_no_wrap(entry.name.clone(), font, p.text)
                            .size()
                            .x
                            .min(90.0);
                        let room = loop {
                            let room = ui.available_width()
                                - tags
                                    .iter()
                                    .map(|(_, icon, text, ..)| tag_width(ui, icon.is_some(), text) + spacing)
                                    .sum::<f32>();
                            let yields = tags
                                .iter()
                                .enumerate()
                                .filter(|(_, t)| t.0 != u8::MAX)
                                .min_by_key(|(_, t)| t.0)
                                .map(|(i, _)| i);
                            match yields {
                                Some(i) if room < wanted => {
                                    tags.remove(i);
                                }
                                _ => break room,
                            }
                        };
                        let name = egui::RichText::new(&entry.name)
                            .strong()
                            .color(if stopped.is_some() { p.faint } else { p.text });
                        // the words are not for selecting: a click on them is a click on the card
                        ui.allocate_ui(egui::vec2(room.max(40.0), 20.0), |ui| {
                            ui.add(egui::Label::new(name).selectable(false).truncate());
                        });
                        for (_, icon, text, color, fill) in &tags {
                            let shown = tag(ui, *icon, text, *color, *fill);
                            if text.starts_with(UNSAVED_MARK) {
                                shown.on_hover_text("What the window holds is not what was saved.");
                            } else if text == WITHDRAWN {
                                shown.on_hover_text(
                                    "The community withdrew the version it came from: open it to read why.",
                                );
                            } else if icon == &Some(Icon::Star) {
                                shown.on_hover_text("Shared on the community: its stars and how many rated it.");
                            } else if icon == &Some(Icon::Warn) {
                                shown.on_hover_text(format!(
                                    "{notes} note{} about it on this computer: open it to read them.",
                                    if notes == 1 { "" } else { "s" }
                                ));
                            }
                        }
                    });
                    let line = stopped.clone().unwrap_or_else(|| self.summary_of(c));
                    let color = if stopped.is_some() { p.faint } else { p.muted };
                    ui.add(
                        egui::Label::new(egui::RichText::new(line).small().color(color))
                            .selectable(false)
                            .truncate(),
                    );
                });
            });
        });
    }

    /// *Use* of a card that is not the one in use: grey while something runs, with why; its hover says what
    /// would change.
    fn card_use(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        let locked = self.engine_locked();
        let shown = theme::button_sized(ui, Role::Normal, !locked, "Use", egui::vec2(0.0, 26.0))
            .on_disabled_hover_text(self.engine_locked_sentence());
        let shown = shown.on_hover_ui(|ui| {
            ui.set_max_width(320.0);
            ui.label("Make this the configuration in use: its engine, model and settings become the window's. Nothing is started.");
            let differ = self.differences_from(entry);
            if !differ.is_empty() {
                ui.add_space(4.0);
                ui.label(theme::muted(match differ.len() {
                    1 => "1 setting changes:".to_string(),
                    n => format!("{n} settings change:"),
                }));
                for change in differ.iter().take(8) {
                    ui.label(egui::RichText::new(change.describe()).small());
                }
                if differ.len() > 8 {
                    ui.label(theme::muted(format!("and {} more", differ.len() - 8)));
                }
            }
        });
        if shown.clicked() {
            self.configs.in_window = true;
            self.request_choose(&entry.id);
            self.refresh_configurations();
        }
    }

    /// *Save* and *Revert* of the card of the one in use, while it has changes (laid out from the right: Save is
    /// the last). Revert is grey while something runs, with why.
    fn card_save_and_revert(&mut self, ui: &mut egui::Ui) {
        if icons::labelled_button(ui, Role::Primary, true, Icon::Save, "Save", 26.0)
            .on_hover_text("Save what you have set over this configuration.")
            .clicked()
        {
            self.configs.in_window = true;
            let _ = self.save_active();
            self.refresh_configurations();
        }
        let locked = self.engine_locked();
        if icons::labelled_button(ui, Role::Normal, !locked, Icon::Undo, "Revert", 26.0)
            .on_hover_text("Go back to what was saved.")
            .on_disabled_hover_text(self.engine_locked_sentence())
            .clicked()
        {
            self.configs.in_window = true;
            self.configs.confirm = Some(Confirm::Revert(self.configs.active.clone()));
        }
    }

    /// The … of a card and its menu: Rename, Duplicate, Export, Share with the community, and Delete (Reset to factory,
    /// for the Default).
    /// What is built in or comes with the package cannot be renamed or deleted, and says so.
    fn card_menu(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        let yours = entry.origin == Origin::Yours;
        let more = icons::icon_button(ui, Icon::More, egui::vec2(28.0, 26.0), true, false)
            .on_hover_text("Rename, duplicate, export, share or delete it");
        egui::Popup::menu(&more)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                ui.set_min_width(190.0);
                if icons::menu_item(ui, Icon::Pencil, "Rename...", yours, false)
                    .on_disabled_hover_text(
                        "Built in, or from the package: duplicate it to have one of your own.",
                    )
                    .clicked()
                {
                    self.configs.in_window = true;
                    self.ask_for_a_name(NameFor::Rename(entry.id.clone()), &entry.name);
                }
                if icons::menu_item(ui, Icon::Copy, "Duplicate...", true, false)
                    .on_hover_text("A copy under a name of its own.")
                    .clicked()
                {
                    self.configs.in_window = true;
                    let suggestion = format!("{} copy", entry.name);
                    self.ask_for_a_name(NameFor::Duplicate(entry.id.clone()), &suggestion);
                }
                if icons::menu_item(ui, Icon::Upload, "Export...", true, false)
                    .on_hover_text("Write it to a file to give to someone else: no folder, program or secret goes in it.")
                    .clicked()
                {
                    self.configs.in_window = true;
                    self.configs.export = Some(self.export_dialog_for(entry));
                }
                if icons::menu_item(ui, Icon::Globe, "Share with the community...", true, false)
                    .on_hover_text("Upload it to quickai.everybodyai.dev for others to use. You see the file first.")
                    .clicked()
                {
                    self.configs.in_window = true;
                    self.open_share(entry);
                }
                ui.separator();
                if entry.id == DEFAULT_ID {
                    if icons::menu_item(ui, Icon::Undo, "Reset to factory...", yours, true)
                        .on_hover_text("Put the Default that comes with QuickAI back.")
                        .on_disabled_hover_text("It is the Default that comes with QuickAI already.")
                        .clicked()
                    {
                        self.configs.in_window = true;
                        self.configs.confirm = Some(Confirm::Reset);
                    }
                } else if icons::menu_item(ui, Icon::Trash, "Delete...", yours, true)
                    .on_disabled_hover_text("Built in, or from the package: it cannot be deleted.")
                    .clicked()
                {
                    self.configs.in_window = true;
                    self.configs.confirm = Some(Confirm::Delete(entry.id.clone()));
                }
            });
    }

    /// What an opened card says under its first rows, in line with its name: the note of whoever made it, the
    /// computer it was made on when that is another kind, what it sets as chips, and the notes about it here.
    fn show_card_details(&mut self, ui: &mut egui::Ui, entry: &Entry, report: &Report) {
        let p = theme::palette();
        let c = &entry.configuration;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(BADGE + ui.spacing().item_spacing.x);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                if let Some(note) = &c.meta.note {
                    ui.label(egui::RichText::new(note).italics().color(p.body));
                }
                if self.made_elsewhere(c) {
                    ui.label(theme::muted(made_line(c)));
                }
                // what goes with the model first: it is on the command line as the settings are
                let goes_with = self.model_chips(c);
                let chips = self.settings_chips(c);
                if !goes_with.is_empty() || !chips.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                        for (label, value) in goes_with.iter().chain(&chips) {
                            setting_chip(ui, label, value);
                        }
                    });
                }
                if chips.is_empty() {
                    ui.label(theme::muted("Every setting at its default."));
                }
                self.show_card_community(ui, entry);
                for note in &report.notes {
                    self.show_note(ui, note, c);
                }
            });
        });
    }

    /// What the community's list says of an opened card, once it was fetched: that its version was withdrawn, and why;
    /// that it is shared, with its stars and downloads. The application changes nothing by itself.
    fn show_card_community(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        let p = theme::palette();
        let (shared, withdrawn) = self.community_of(entry);
        if let Some(reason) = withdrawn {
            let from = entry
                .configuration
                .meta
                .community
                .as_ref()
                .map_or(0, |c| c.version);
            ui.label(
                egui::RichText::new(format!(
                    "The community withdrew version {from} of the configuration this came from: {reason}"
                ))
                .small()
                .color(p.bad),
            );
        }
        let Some((shared, version)) = shared else {
            return;
        };
        let (title, url) = (shared.title.clone(), shared.url.clone());
        let mut line = format!(
            "On the community as \"{title}\"{}: {}, {} download{}.",
            if version == shared.version {
                String::new()
            } else {
                format!(" (version {version}; it is at {} now)", shared.version)
            },
            super::community_window::stars_words(shared),
            shared.downloads,
            if shared.downloads == 1 { "" } else { "s" },
        );
        if shared.ratings == 0 {
            line = line.replace("No ratings yet", "no ratings yet");
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::muted(line));
            if !url.is_empty() && ui.small_button("Open its page").clicked() {
                let _ = webbrowser::open(&url);
            }
        });
    }

    /// `Ollama · qwen3-vl:8b · with Open WebUI`: the engine, the model (by its name in the recommended list when
    /// it is one of it) and what goes with it.
    fn summary_of(&self, configuration: &Configuration) -> String {
        let engine = self.registry.engine(&configuration.engine).map_or_else(
            || configuration.engine.clone(),
            |e| e.doc.engine.name.clone(),
        );
        let mut parts = vec![engine];
        parts.push(match configuration.models.first() {
            Some(model) => self.model_name(model),
            None => "any model".to_string(),
        });
        for companion in self.registry.companions_of(&configuration.engine) {
            if companion_on(configuration, &companion.id) {
                parts.push(format!("with {}", companion.doc.engine.name));
            }
        }
        parts.join(" · ")
    }

    /// What a person calls the model a configuration names: the name the file gives, else the name of the entry
    /// of the recommended list, else the file or the name the engine knows.
    fn model_name(&self, model: &quickai_core::profile::ModelPick) -> String {
        if let (None, quickai_core::profile::ModelSource::Catalog { id, .. }) =
            (&model.name, &model.source)
        {
            if let Some(listed) = self.registry.catalog().model(id) {
                return listed.name.clone();
            }
        }
        model.display().to_string()
    }

    /// What goes with the model on its command line (C189): each file fetched with it (a projector), by its name, and the
    /// mode it is started in. From the record beside its weights when the model is here, else from the entry of the
    /// recommended list it is; nothing for a model the engine keeps itself.
    fn model_chips(&self, c: &Configuration) -> Vec<(String, String)> {
        use quickai_core::profile::ModelSource;
        use quickai_core::registry::catalog::Run;
        let (Some(engine), Some(model)) = (self.registry.engine(&c.engine), c.models.first())
        else {
            return Vec::new();
        };
        let file = match &model.source {
            ModelSource::File { name } => Some(name.as_str()),
            ModelSource::Catalog { file, .. } => file.as_deref(),
            ModelSource::Named { .. } => None,
        };
        // the models listed are the engine's in use
        let here = file
            .filter(|_| c.engine == self.engine)
            .and_then(|f| self.models.iter().find(|m| m.filename == f));
        let (files, mode): (Vec<(String, String)>, Option<String>) = match (here, &model.source) {
            (Some(here), _) => (
                here.companions
                    .iter()
                    .map(|f| (f.kind.clone(), f.name.clone()))
                    .collect(),
                here.mode.clone(),
            ),
            (None, ModelSource::Catalog { id, .. }) => match self
                .registry
                .catalog()
                .model(id)
                .and_then(|m| m.runs.get(&c.engine))
            {
                Some(Run::Files(run)) => (
                    run.companions
                        .iter()
                        .map(|(kind, f)| {
                            let name = f.path.rsplit('/').next().unwrap_or(&f.path);
                            (
                                kind.clone(),
                                f.store_as.clone().unwrap_or_else(|| name.to_string()),
                            )
                        })
                        .collect(),
                    run.mode.clone(),
                ),
                _ => (Vec::new(), None),
            },
            _ => (Vec::new(), None),
        };
        let mut chips: Vec<(String, String)> = files
            .into_iter()
            .map(|(kind, name)| {
                let label = quickai_core::models::bundles(engine)
                    .iter()
                    .flat_map(|b| b.parts.iter())
                    .find(|part| part.kind == kind)
                    .map_or(kind, |part| part.label.clone());
                let mut letters = label.chars();
                let label = letters.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(letters).collect()
                });
                (label, name)
            })
            .collect();
        if let Some(mode) = mode {
            let label = quickai_core::models::modes(engine)
                .iter()
                .find(|m| m.id == mode)
                .map_or(mode, |m| m.label.clone());
            chips.push(("Started as".to_string(), label));
        }
        chips
    }

    /// The configuration was made on another kind of computer than this one (another system, processor or GPU).
    fn made_elsewhere(&self, c: &Configuration) -> bool {
        let here = self.host().made_on();
        c.meta
            .made_on
            .as_ref()
            .is_some_and(|m| (&m.os, &m.arch, &m.gpu) != (&here.os, &here.arch, &here.gpu))
    }

    /// What differs between what the window holds and this configuration, for the engine it is for.
    fn differences_from(&self, entry: &Entry) -> Vec<Change> {
        let installed = self.installed_cache_or_scan();
        let state = configurations::working_state(&self.registry, &self.settings_now(), &installed);
        changes(
            &self.registry,
            &state,
            &self.configs.first_run,
            None,
            &entry.configuration,
        )
    }

    /// What a configuration sets, as the chips of its card say it: a label and a value for each setting it carries
    /// (a program that goes with the engine named before the label, unless the label names it; the hint of a label in
    /// brackets left out), the program it was made with, and the live traffic proxy. That a program goes with the
    /// engine is not one: the line of the card says it.
    fn settings_chips(&self, c: &Configuration) -> Vec<(String, String)> {
        let switches: Vec<String> = self
            .registry
            .companions_of(&c.engine)
            .map(|companion| quickai_core::profile::value_key(&companion.id, "enabled"))
            .collect();
        let mut chips = Vec::new();
        for (key, field) in carried(&self.registry, &c.engine) {
            if switches.contains(&key) {
                continue;
            }
            let Some(value) = c.values.get(&key) else {
                continue;
            };
            let owner = key.rsplit_once('.').map_or("", |(owner, _)| owner);
            let owner_name = (owner != c.engine)
                .then(|| {
                    self.registry
                        .engine(owner)
                        .map(|e| e.doc.engine.name.clone())
                })
                .flatten();
            let text = match value {
                quickai_core::value::Value::Str(s) => {
                    let first = s.lines().next().unwrap_or_default();
                    if s.lines().count() > 1 || first.chars().count() > 30 {
                        format!("{}...", first.chars().take(30).collect::<String>())
                    } else {
                        first.to_string()
                    }
                }
                quickai_core::value::Value::Bool(on) => if *on { "on" } else { "off" }.to_string(),
                other => quickai_core::profile::shown(other),
            };
            let full = label_of(field);
            let label = full
                .split_once(" (")
                .map_or(full.as_str(), |(short, _)| short)
                .to_string();
            chips.push((
                match owner_name {
                    // unless the label says whose it is already
                    Some(name) if !label.to_lowercase().contains(&name.to_lowercase()) => {
                        format!("{name} {label}")
                    }
                    _ => label,
                },
                text,
            ));
        }
        if let Some(build) = &c.runtime.build {
            chips.push(("Program".to_string(), build.clone()));
        }
        if c.traffic_proxy == Some(true) {
            chips.push(("Live traffic proxy".to_string(), "on".to_string()));
        }
        chips
    }

    // ---- exporting -----------------------------------------------------------------------------------

    fn export_dialog_for(&self, entry: &Entry) -> ExportDialog {
        let home = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_default();
        ExportDialog {
            id: entry.id.clone(),
            name: entry.name.clone(),
            note: entry.configuration.meta.note.clone().unwrap_or_default(),
            path: home
                .join(format!("{}.toml", entry.id))
                .display()
                .to_string(),
            replace: false,
            error: None,
        }
    }

    /// The dialog that writes a configuration to a file for someone else.
    fn show_export_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.configs.export.clone() else {
            return;
        };
        let host = self.host();
        let preview = export_text(
            &self.registry,
            &dialog.id,
            Some(&dialog.name),
            Some(&dialog.note)
                .filter(|n| !n.trim().is_empty())
                .map(String::as_str),
            &host,
        );
        let exists = !dialog.path.trim().is_empty() && PathBuf::from(dialog.path.trim()).exists();
        let (mut write, mut cancel, mut browse) = (false, false, false);
        let modal = egui::Modal::new(egui::Id::new("export_configuration_modal")).show(ctx, |ui| {
            ui.set_width(modal_width(ctx, 480.0));
            ui.label(egui::RichText::new("Export a configuration").strong());
            ui.add_space(6.0);
            ui.label("Name in the file:");
            ui.add(
                egui::TextEdit::singleline(&mut dialog.name).desired_width(ui.available_width()),
            );
            ui.label("A note for whoever gets it:");
            ui.add(
                egui::TextEdit::multiline(&mut dialog.note)
                    .desired_rows(2)
                    .desired_width(ui.available_width()),
            );
            ui.label("File:");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.path)
                        .desired_width((ui.available_width() - 90.0).max(120.0)),
                );
                if ui.button("Browse...").clicked() {
                    browse = true;
                }
            });
            match &preview {
                Ok((text, left_out)) => {
                    for note in left_out {
                        ui.label(
                            egui::RichText::new(note.text(self.registry.vocabulary())).small(),
                        );
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("What will be written:").small().weak());
                    egui::ScrollArea::vertical()
                        .id_salt("export_preview")
                        .max_height((ctx.content_rect().height() - 250.0).clamp(60.0, 180.0))
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut text.as_str())
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(ui.available_width()),
                            );
                        });
                }
                Err(e) => {
                    ui.label(egui::RichText::new(e.to_string()).color(super::theme::palette().bad));
                }
            }
            if exists {
                ui.checkbox(
                    &mut dialog.replace,
                    "That file is there already: replace it",
                );
            }
            if let Some(why) = &dialog.error {
                ui.label(
                    egui::RichText::new(why)
                        .small()
                        .color(super::theme::palette().bad),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                let ready = preview.is_ok()
                    && !dialog.path.trim().is_empty()
                    && (!exists || dialog.replace);
                if ui.add_enabled(ready, egui::Button::new("Export")).clicked() {
                    write = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if browse {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(format!("{}.toml", dialog.id))
                .save_file()
            {
                dialog.path = path.display().to_string();
                dialog.replace = true;
            }
        }
        if write {
            if let Ok((text, _)) = &preview {
                match std::fs::write(dialog.path.trim(), text) {
                    Ok(()) => {
                        self.configs.status = Some(format!("Exported to {}", dialog.path.trim()));
                        self.configs.export = None;
                        return;
                    }
                    Err(e) => {
                        dialog.error = Some(format!("Could not write {}: {e}", dialog.path.trim()));
                    }
                }
            }
        }
        if cancel {
            self.configs.export = None;
        } else {
            self.configs.export = Some(dialog);
        }
    }

    // ---- importing -----------------------------------------------------------------------------------

    /// Reads the file the dialog names, and looks at it for this computer.
    fn read_for_import(&mut self, dialog: &mut ImportDialog) {
        dialog.error = None;
        dialog.loaded = None;
        dialog.ticked.clear();
        let path = dialog.path.trim().to_string();
        if path.is_empty() {
            return;
        }
        match crate::configuration_cli::read_file(&path) {
            Ok(text) => self.load_for_import(dialog, text),
            Err(e) => dialog.error = Some(e),
        }
    }

    /// Looks at the text of a file from outside for this computer: what the dialog shows before anything is written.
    pub(super) fn load_for_import(&mut self, dialog: &mut ImportDialog, text: String) {
        dialog.error = None;
        dialog.loaded = None;
        dialog.ticked.clear();
        let opened = match open_text(&self.registry, &text, env!("CARGO_PKG_VERSION")) {
            Ok(opened) => opened,
            Err(stop) => {
                dialog.error = Some(stop.text(self.registry.vocabulary()));
                return;
            }
        };
        let installed = self.installed_now();
        let here = self.configuration_inventory(Some(&opened.configuration), &installed);
        let report = review(
            &self.registry,
            &opened.configuration,
            &opened.notes,
            &here,
            true,
        );
        if let Some(stop) = report.stop() {
            dialog.error = Some(stop.text(self.registry.vocabulary()));
            return;
        }
        let state = configurations::working_state(&self.registry, &self.settings_now(), &installed);
        let diff = changes(
            &self.registry,
            &state,
            &self.configs.first_run,
            None,
            &opened.configuration,
        );
        dialog.name = opened.configuration.name.clone();
        dialog.loaded = Some(Loaded {
            text,
            opened,
            report,
            changes: diff,
        });
    }

    /// The dialog that imports a file from outside: the path, the report, what to take, the name. A file of the
    /// community (`from_community`) is reviewed in the Community window, and has its title where the path is.
    pub(super) fn show_import_dialog(&mut self, ctx: &egui::Context, from_community: bool) {
        let Some(mut dialog) = self.configs.import.clone() else {
            return;
        };
        if dialog.community.is_some() != from_community {
            return;
        }
        let (mut read, mut browse, mut cancel, mut go, mut go_and_use) =
            (false, false, false, false, false);
        let taken = dialog.loaded.as_ref().is_some_and(|l| {
            let called =
                (dialog.name != l.opened.configuration.name).then_some(dialog.name.as_str());
            import_is_taken(
                &self.config_dirs,
                &self.registry,
                &l.opened.configuration,
                called,
            )
        });
        let modal = egui::Modal::new(egui::Id::new("import_configuration_modal")).show(ctx, |ui| {
            ui.set_width(modal_width(ctx, 500.0));
            ui.label(egui::RichText::new("Import a configuration").strong());
            ui.add_space(6.0);
            if let Some((from, title)) = &dialog.community {
                ui.label(format!(
                    "\"{title}\" (version {}) from quickai.everybodyai.dev, checked against the community's list.",
                    from.version
                ));
            } else {
            ui.label("File:");
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut dialog.path)
                        .id(egui::Id::new("import_configuration_path"))
                        .desired_width((ui.available_width() - 170.0).max(120.0)),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    read = true;
                }
                if ui.button("Browse...").clicked() {
                    browse = true;
                }
                if ui.button("Read").clicked() {
                    read = true;
                }
            });
            }
            if let Some(why) = &dialog.error {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(why).color(super::theme::palette().bad));
            }
            if let Some(loaded) = &dialog.loaded {
                ui.add_space(6.0);
                ui.separator();
                let c = &loaded.opened.configuration;
                ui.label(egui::RichText::new(&c.name).strong());
                let made = made_line(c);
                if !made.is_empty() {
                    ui.label(egui::RichText::new(made).small());
                }
                ui.label(
                    egui::RichText::new(format!("This computer: {}", self.this_computer())).small(),
                );
                if let Some(note) = &c.meta.note {
                    ui.label(egui::RichText::new(note).small().italics());
                }
                if !loaded.changes.is_empty() {
                    let list: Vec<String> = loaded
                        .changes
                        .iter()
                        .take(6)
                        .map(Change::describe)
                        .collect();
                    ui.label(
                        egui::RichText::new(format!(
                            "Changes {} setting{}: {}",
                            loaded.changes.len(),
                            if loaded.changes.len() == 1 { "" } else { "s" },
                            list.join("; ")
                        ))
                        .small(),
                    );
                }
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("import_report")
                    .max_height((ctx.content_rect().height() - 300.0).clamp(60.0, 260.0))
                    .auto_shrink([true, true])
                    .show(ui, |ui| {
                        for note in loaded
                            .report
                            .notes
                            .iter()
                            .filter(|n| n.level != Level::Asks)
                        {
                            self.show_note(ui, note, c);
                        }
                        for ask in loaded.report.asks() {
                            let Some(key) = ask.subject.clone() else {
                                continue;
                            };
                            let mut on = dialog.ticked.contains(&key);
                            if ui
                                .checkbox(&mut on, ask.text(self.registry.vocabulary()))
                                .on_hover_text("Taken only if you tick it.")
                                .changed()
                            {
                                if on {
                                    dialog.ticked.insert(key.clone());
                                } else {
                                    dialog.ticked.remove(&key);
                                }
                            }
                            if let Some(value) = c.values.get(&key) {
                                let mut shown = value.to_string();
                                ui.add(
                                    egui::TextEdit::multiline(&mut shown)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_rows(1)
                                        .desired_width(ui.available_width()),
                                );
                            }
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .desired_width(ui.available_width()),
                    );
                });
                if taken {
                    ui.label(egui::RichText::new("That name is used already:").small());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut dialog.when_taken, WhenTaken::KeepBoth, "Keep both");
                        ui.radio_value(&mut dialog.when_taken, WhenTaken::Replace, "Replace");
                        ui.radio_value(&mut dialog.when_taken, WhenTaken::Cancel, "Cancel");
                    });
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                let ready = dialog.loaded.is_some()
                    && !dialog.name.trim().is_empty()
                    && !(taken && dialog.when_taken == WhenTaken::Cancel);
                if ui.add_enabled(ready, egui::Button::new("Import")).clicked() {
                    go = true;
                }
                if ui
                    .add_enabled(ready, egui::Button::new("Import and use"))
                    .clicked()
                {
                    go_and_use = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if browse {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                dialog.path = path.display().to_string();
                read = true;
            }
        }
        if read {
            self.read_for_import(&mut dialog);
        }
        if go || go_and_use {
            self.finish_import(&mut dialog, go_and_use);
        }
        if cancel {
            self.configs.import = None;
        } else if self.configs.import.is_some() {
            self.configs.import = Some(dialog);
        }
    }

    /// Stores what was read, with what the person ticked, as one of their configurations.
    pub(super) fn finish_import(&mut self, dialog: &mut ImportDialog, and_use: bool) {
        let Some(loaded) = dialog.loaded.clone() else {
            return;
        };
        let installed = self.installed_now();
        let here = self.configuration_inventory(Some(&loaded.opened.configuration), &installed);
        let taken_policy = match dialog.when_taken {
            WhenTaken::KeepBoth => OnTaken::KeepBoth,
            WhenTaken::Replace => OnTaken::Replace,
            WhenTaken::Cancel => OnTaken::Refuse,
        };
        let called =
            (dialog.name != loaded.opened.configuration.name).then_some(dialog.name.as_str());
        match import(
            &self.config_dirs,
            &self.registry,
            &loaded.text,
            &here,
            &dialog.ticked,
            called,
            taken_policy,
            true,
            dialog.community.as_ref().map(|(from, _)| from.clone()),
        ) {
            Ok(done) => {
                self.reload_configurations();
                self.refresh_configurations();
                let id = done.configuration.id.clone();
                self.configs.selected = Some(id.clone());
                self.configs.status = Some(format!("Imported \"{}\".", done.configuration.name));
                if dialog.community.is_some() {
                    self.community.status = Some(format!(
                        "Imported \"{}\": it is in Configurations.",
                        done.configuration.name
                    ));
                }
                self.push_log(format!(
                    "[quickai] Imported the configuration \"{}\".",
                    done.configuration.name
                ));
                self.configs.import = None;
                if and_use {
                    self.configs.in_window = true;
                    self.request_choose(&id);
                    self.refresh_configurations();
                }
            }
            Err(report) => {
                dialog.error = report
                    .notes
                    .iter()
                    .map(|n| n.text(self.registry.vocabulary()))
                    .next();
            }
        }
    }

    /// What this computer is, as a line says it: `Linux x86_64 (AMD GPU, 16 GB)`.
    fn this_computer(&self) -> String {
        let host = self.host();
        computer_line(
            &host.os,
            &host.arch,
            &host.gpu_vendors,
            (host.ram_bytes > 0).then_some(host.ram_bytes / (1024 * 1024 * 1024)),
        )
    }

    // ---- the questions of the window ---------------------------------------------------------------------

    /// Revert, delete and reset ask before anything is done.
    pub(super) fn show_configuration_confirm(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.configs.confirm.clone() else {
            return;
        };
        let name_of = |id: &str| {
            self.configuration_entries()
                .into_iter()
                .find(|e| e.id == id)
                .map_or(id.to_string(), |e| e.name)
        };
        let (title, body, button) = match &confirm {
            Confirm::Revert(id) => (
                "Go back to what was saved?".to_string(),
                format!(
                    "What you changed since \"{}\" was saved is lost: the engine, the model and the settings go back to it.",
                    name_of(id)
                ),
                "Revert",
            ),
            Confirm::Delete(id) => (
                "Delete configuration?".to_string(),
                format!("\"{}\" is removed from your configurations folder. This can't be undone.", name_of(id)),
                "Delete",
            ),
            Confirm::Reset => (
                "Reset to factory?".to_string(),
                "Your Default is replaced by the Default that comes with QuickAI. This can't be undone.".to_string(),
                "Reset",
            ),
        };
        let (mut yes, mut cancel) = (false, false);
        let modal =
            egui::Modal::new(egui::Id::new("configuration_confirm_modal")).show(ctx, |ui| {
                ui.set_width(modal_width(ctx, 340.0));
                ui.label(egui::RichText::new(title).strong());
                ui.add_space(6.0);
                ui.label(body);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button(button).clicked() {
                        yes = true;
                    }
                });
            });
        if modal.should_close() {
            cancel = true;
        }
        if yes {
            self.configs.confirm = None;
            self.do_confirmed(confirm);
        } else if cancel {
            self.configs.confirm = None;
        }
    }

    /// Does what the person said yes to.
    pub(super) fn do_confirmed(&mut self, confirm: Confirm) {
        self.configs.error = None;
        let done: Result<String, String> = match &confirm {
            Confirm::Revert(id) => {
                self.choose_configuration(id);
                match self.configs.error.clone() {
                    Some(why) => Err(why),
                    None => Ok("Back to what was saved.".to_string()),
                }
            }
            Confirm::Delete(id) => configurations::delete(&self.config_dirs, &self.registry, id)
                .map(|()| {
                    // the one in use is gone: the Default is the one in use now, and what the window holds is as it was
                    let mut stored = self.settings_now();
                    if configurations::forget_if_active(&mut stored, id) {
                        self.write_settings(&stored);
                        self.stored = stored;
                        self.configs.active = DEFAULT_ID.to_string();
                    }
                    if self.configs.selected.as_deref() == Some(id.as_str()) {
                        self.configs.selected = None;
                    }
                    "Deleted.".to_string()
                })
                .map_err(|e| e.to_string()),
            Confirm::Reset => configurations::reset_to_factory(&self.config_dirs, &self.registry)
                .map(|()| "The Default is the one that comes with QuickAI again.".to_string())
                .map_err(|e| e.to_string()),
        };
        match done {
            Ok(said) => {
                self.reload_configurations();
                self.refresh_configurations();
                self.configs.status = Some(said);
            }
            Err(why) => self.configs.error = Some(why),
        }
    }
}

/// What to look a file up by: the name of the repository it came from, when that is known, else the
/// file's name without its quantization and ending.
fn lookup_words(from: Option<&str>, file: &str) -> String {
    if let Some(repo) = from.and_then(|f| f.split_once(':')).map(|(_, name)| name) {
        return repo.rsplit('/').next().unwrap_or(repo).to_string();
    }
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    // `Qwen3-VL-8B-Q4_K_M` is looked up as `Qwen3-VL-8B`
    let keep: Vec<&str> = stem
        .split('-')
        .take_while(|w| {
            let upper = w.to_ascii_uppercase();
            let quantization = (upper.starts_with('Q')
                && upper.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
                || upper.starts_with("IQ")
                || upper == "F16"
                || upper == "BF16";
            !quantization
        })
        .collect();
    if keep.is_empty() {
        stem.to_string()
    } else {
        keep.join("-")
    }
}

/// `Made with QuickAI 0.1.0 on Linux x86_64 (AMD GPU, 32 GB)`, from what the file says of itself.
pub(super) fn made_line(c: &Configuration) -> String {
    let with = c.meta.made_with.as_deref();
    let on = c.meta.made_on.as_ref().map(|m| {
        computer_line(
            m.os.as_deref().unwrap_or(""),
            m.arch.as_deref().unwrap_or(""),
            &m.gpu,
            m.memory_gb,
        )
    });
    match (with, on) {
        (Some(with), Some(on)) if !on.is_empty() => format!("Made with {with} on {on}"),
        (Some(with), _) => format!("Made with {with}"),
        (None, Some(on)) if !on.is_empty() => format!("Made on {on}"),
        _ => String::new(),
    }
}

/// `Linux x86_64 (AMD GPU, 32 GB)`.
pub(super) fn computer_line(
    os: &str,
    arch: &str,
    gpus: &[String],
    memory_gb: Option<u64>,
) -> String {
    let os = match os {
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
        other => other,
    };
    let mut details: Vec<String> = gpus
        .iter()
        .map(|g| {
            GpuVendor::from_key(g).map_or_else(|| g.to_string(), |v| format!("{} GPU", v.label()))
        })
        .collect();
    if let Some(gb) = memory_gb {
        details.push(format!("{gb} GB"));
    }
    let base = [os, arch]
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    if details.is_empty() {
        base
    } else {
        format!("{base} ({})", details.join(", "))
    }
}

fn level_color(level: Level) -> egui::Color32 {
    match level {
        Level::Stops => super::theme::palette().bad,
        Level::Asks => super::theme::palette().warn,
        Level::Warns => super::theme::palette().warn,
        Level::Says => super::theme::palette().muted,
    }
}

/// The side of the badge of an engine on a card.
const BADGE: f32 = 30.0;

/// The tag of a configuration whose version of the community was withdrawn.
const WITHDRAWN: &str = "Withdrawn";

/// The program `companion` is switched on in `configuration`.
fn companion_on(configuration: &Configuration, companion: &str) -> bool {
    configuration
        .values
        .get(&quickai_core::profile::value_key(companion, "enabled"))
        .and_then(quickai_core::value::Value::as_bool)
        .unwrap_or(false)
}

impl App {
    /// The badge of the engine a configuration is for: the first letter of its name on the engine's tint (by its
    /// place in the selector, so that no two engines of this QuickAI share one), or in an outline for an engine it
    /// does not have.
    pub(super) fn engine_badge(&self, ui: &mut egui::Ui, engine: &str) {
        let p = theme::palette();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(BADGE, BADGE), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let name = self
            .registry
            .engine(engine)
            .map_or(engine, |e| e.doc.engine.name.as_str());
        let letter: String = name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let (fill, color, stroke) = match self.registry.selectable().position(|e| e.id == engine) {
            Some(place) => {
                let (fill, color) = theme::tint(place);
                (fill, color, egui::Stroke::NONE)
            }
            None => (
                egui::Color32::TRANSPARENT,
                p.faint,
                egui::Stroke::new(1.0, p.outline),
            ),
        };
        ui.painter()
            .rect(rect, 7.0, fill, stroke, egui::StrokeKind::Inside);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            letter,
            egui::FontId::new(BADGE * 0.45, theme::semibold(ui)),
            color,
        );
    }
}

/// How wide a tag of `text` is, with a picture or not.
pub(super) fn tag_width(ui: &egui::Ui, icon: bool, text: &str) -> f32 {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let words = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, egui::Color32::PLACEHOLDER)
        .size()
        .x;
    words + 14.0 + if icon { 15.0 } else { 0.0 }
}

/// A short word after a name, in a pill of `color` on `fill`, with a picture before it when it has one: *In use*,
/// *Modified*, where it is from, how many notes there are.
pub(super) fn tag(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    text: &str,
    color: egui::Color32,
    fill: egui::Color32,
) -> egui::Response {
    let size = egui::vec2(tag_width(ui, icon.is_some(), text), 18.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 9.0, fill);
        let mut left = rect.left() + 7.0;
        if let Some(icon) = icon {
            let square = egui::Rect::from_min_size(
                egui::pos2(left, rect.center().y - 6.0),
                egui::vec2(12.0, 12.0),
            );
            icons::paint(ui.painter(), square, icon, color);
            left += 15.0;
        }
        ui.painter().text(
            egui::pos2(left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::TextStyle::Small.resolve(ui.style()),
            color,
        );
    }
    response
}

/// A setting of a card: its label, and its value a little stronger, in a rounded chip.
fn setting_chip(ui: &mut egui::Ui, label: &str, value: &str) {
    let p = theme::palette();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let strong = egui::FontId::new(font.size, theme::semibold(ui));
    let label_galley = ui.painter().layout_no_wrap(label.to_string(), font, p.body);
    let value_galley = ui
        .painter()
        .layout_no_wrap(value.to_string(), strong, p.text);
    let gap = 5.0;
    let size = egui::vec2(
        label_galley.size().x + gap + value_galley.size().x + 18.0,
        22.0,
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 11.0, p.raised);
        let left = rect.left() + 9.0;
        let value_left = left + label_galley.size().x + gap;
        let label_top = rect.center().y - label_galley.size().y / 2.0;
        let value_top = rect.center().y - value_galley.size().y / 2.0;
        ui.painter()
            .galley(egui::pos2(left, label_top), label_galley, p.body);
        ui.painter()
            .galley(egui::pos2(value_left, value_top), value_galley, p.text);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashSet};
    use std::path::Path;
    use std::rc::Rc;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    struct Window {
        builder: egui::ViewportBuilder,
        shapes: Vec<egui::epaint::ClippedShape>,
        size: egui::Vec2,
    }

    #[derive(Default)]
    struct Shown {
        windows: BTreeMap<egui::ViewportId, Window>,
        events: BTreeMap<egui::ViewportId, Vec<egui::Event>>,
        closed: HashSet<egui::ViewportId>,
        size: Option<egui::Vec2>,
    }

    /// A display that opens a window of its own for a viewport, as the toolkit does on a screen (the same
    /// stand-in the Settings window's tests use).
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

        fn give(&self, window: egui::ViewportId, events: Vec<egui::Event>) {
            self.0.borrow_mut().events.insert(window, events);
        }

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
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        app.config_dirs = ConfigDirs {
            user: Some(dir.join("config")),
            data: Some(dir.join("data")),
            ..ConfigDirs::default()
        };
        app.reload_configurations();
        app.find_active_configuration();
        app
    }

    fn put(dir: &Path, rel: &str, text: &str) {
        let path = dir.join("config").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn saved(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
    }

    fn vision_text() -> &'static str {
        "schema = 1\nkind = \"profile\"\nid = \"vision\"\nname = \"Vision\"\nengine = \"ollama\"\n\
         [meta]\nmade_with = \"QuickAI 0.1.0\"\nmade_on = { os = \"linux\", arch = \"x86_64\", gpu = [\"amd\"], memory_gb = 32 }\nnote = \"For pictures.\"\n\
         [[model]]\nollama = \"qwen3-vl:8b\"\nsize = 6140000000\n\
         [values.ollama]\nctx_size = 16384\nport = 11600\nextra_env = \"OLLAMA_FLASH_ATTENTION=1\"\n\
         [values.openwebui]\nenabled = true\n"
    }

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

    /// The texts of a window `width` wide as they are seen: a text clipped to something narrower than the
    /// window (a box of one line, which scrolls its text inside its own frame) is cut to it. The rest are
    /// whole, so that a label wider than the window is still found.
    fn seen_texts(shapes: &[egui::epaint::ClippedShape], width: f32) -> Vec<(String, egui::Rect)> {
        shapes
            .iter()
            .flat_map(|clipped| {
                let inner = clipped.clip_rect.width() < width - 1.0;
                texts(std::slice::from_ref(clipped))
                    .into_iter()
                    .map(move |(t, r)| {
                        (
                            t,
                            if inner {
                                r.intersect(clipped.clip_rect)
                            } else {
                                r
                            },
                        )
                    })
            })
            .filter(|(_, r)| r.is_positive())
            .collect()
    }

    /// What the configurations window painted in the last frame, if it was drawn.
    fn painted(display: &Display) -> Option<Vec<(String, egui::Rect)>> {
        let shown = display.0.borrow();
        let window = shown.windows.get(&configurations_viewport())?;
        Some(texts(&window.shapes))
    }

    fn said(display: &Display) -> Vec<String> {
        painted(display)
            .unwrap_or_default()
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    fn frame(app: &mut App, ctx: &egui::Context, display: &Display) {
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
            app.show_configuration_modals(ui.ctx(), false);
            app.show_configurations_window(ui.ctx());
        });
        output.textures_delta.clear();
    }

    fn settle(app: &mut App, ctx: &egui::Context, display: &Display) {
        for _ in 0..4 {
            frame(app, ctx, display);
        }
    }

    fn rect_of(display: &Display, text: &str) -> Option<egui::Rect> {
        painted(display)?
            .into_iter()
            .find(|(t, _)| t == text)
            .map(|(_, r)| r)
    }

    fn click(app: &mut App, ctx: &egui::Context, display: &Display, at: egui::Pos2) {
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
            display.give(configurations_viewport(), events);
            frame(app, ctx, display);
        }
    }

    /// The same as `press`, for a button of a question: it is drawn above the window, so it is the last
    /// of the texts with that name.
    fn press_in_question(app: &mut App, ctx: &egui::Context, display: &Display, label: &str) {
        let at = painted(display)
            .unwrap_or_default()
            .into_iter()
            .rev()
            .find(|(t, _)| t == label)
            .unwrap_or_else(|| panic!("no `{label}` in {:?}", said(display)))
            .1
            .center();
        click(app, ctx, display, at);
    }

    fn press(app: &mut App, ctx: &egui::Context, display: &Display, label: &str) {
        let at = rect_of(display, label)
            .unwrap_or_else(|| panic!("no `{label}` in {:?}", said(display)))
            .center();
        click(app, ctx, display, at);
    }

    /// Clicks the … of the card of `name`: at the right of the card, level with the badge (the text painted just
    /// before the name), which is in the middle of the card's first rows as the … is.
    fn open_menu_of(app: &mut App, ctx: &egui::Context, display: &Display, name: &str) {
        let width = display
            .0
            .borrow()
            .windows
            .get(&configurations_viewport())
            .expect("drawn")
            .size
            .x;
        let texts = painted(display).unwrap_or_default();
        let at = texts
            .iter()
            .position(|(t, _)| t == name)
            .unwrap_or_else(|| panic!("no `{name}` in {:?}", said(display)));
        let level = texts[at - 1].1.center().y;
        // the panel's margin, the card's and half the button
        click(app, ctx, display, egui::pos2(width - 30.0, level));
        settle(app, ctx, display);
    }

    #[test]
    fn p3_a53_the_configurations_are_a_window_of_their_own_that_closes_any_way_and_writes() {
        let dir = scratch("a53");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        // closed: nothing is drawn
        settle(&mut app, &ctx, &display);
        assert!(painted(&display).is_none());
        // opened: a window of its own, with its title, its size and its Close button
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        {
            let shown = display.0.borrow();
            let window = shown
                .windows
                .get(&configurations_viewport())
                .expect("the window is drawn");
            assert_eq!(window.builder.title.as_deref(), Some(CONFIGURATIONS_TITLE));
            assert_eq!(
                window.builder.inner_size,
                Some(egui::vec2(OPENS_AT[0], OPENS_AT[1]))
            );
            assert_eq!(
                window.builder.min_inner_size,
                Some(egui::vec2(LEAST[0], LEAST[1]))
            );
            assert_eq!(window.size, egui::vec2(OPENS_AT[0], OPENS_AT[1]));
        }
        let said_now = said(&display);
        for want in ["New from current", "Import...", "Close"] {
            assert!(said_now.iter().any(|t| t == want), "{want} in {said_now:?}");
        }
        // asked for again while it is open: it comes to the front, once
        app.open_configurations();
        assert!(app.raise_configurations);
        frame(&mut app, &ctx, &display);
        assert!(!app.raise_configurations);
        assert!(app.show_configurations);
        // closed by its Close button: what was changed is written
        app.values.set("port", 9000u16);
        press(&mut app, &ctx, &display, "Close");
        assert!(!app.show_configurations);
        assert_eq!(saved(&dir)["port"], 9000);
        assert!(painted(&display).is_none());
        // closed by the window manager's own button
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        app.values.set("port", 9100u16);
        display.press_close_button(configurations_viewport());
        frame(&mut app, &ctx, &display);
        assert!(!app.show_configurations);
        assert_eq!(saved(&dir)["port"], 9100);
        // closed by the question that was waiting in it: it goes with it
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        app.configs.in_window = true;
        app.ask_for_a_name(NameFor::SaveAs, "Name");
        app.close_configurations();
        assert!(app.configs.name_box.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a54_the_list_and_the_details_are_built_from_the_store_and_name_no_engine() {
        let dir = scratch("a54");
        put(&dir, "profiles/vision.toml", vision_text());
        put(
            &dir,
            "profiles/broken.toml",
            "schema = 1\nkind = \"profile\"\nid = \"broken\"\n",
        );
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        let said_now = said(&display);
        // each configuration by its name, with a line of its engine, its model and what goes with it; only what is
        // not the person's own says where it is from
        for want in [
            "Default",
            "Vision",
            "Ollama · qwen3-vl:8b · with Open WebUI",
            "built in",
            "Use",
        ] {
            assert!(said_now.iter().any(|t| t == want), "{want} in {said_now:?}");
        }
        assert!(
            said_now.iter().any(|t| t.starts_with("llama.cpp · ")),
            "{said_now:?}"
        );
        assert!(!said_now.iter().any(|t| t == "yours"), "{said_now:?}");
        // the files that could not be used are counted, and say why when they are opened
        assert!(
            said_now.iter().any(|t| t == "1 file couldn't be used"),
            "{said_now:?}"
        );
        assert!(
            !said_now
                .iter()
                .any(|t| t.contains("missing field `engine`")),
            "folded: {said_now:?}"
        );
        press(&mut app, &ctx, &display, "1 file couldn't be used");
        settle(&mut app, &ctx, &display);
        assert!(
            said(&display)
                .iter()
                .any(|t| t.contains("missing field `engine`")),
            "{:?}",
            said(&display)
        );
        // opened by a click on its name: the note, the computer it was made on (another kind: this one has no AMD
        // GPU), what it sets as chips, and the notes about it here
        press(&mut app, &ctx, &display, "Vision");
        settle(&mut app, &ctx, &display);
        assert_eq!(app.configs.selected.as_deref(), Some("vision"));
        let said_now = said(&display);
        for want in [
            "Made with QuickAI 0.1.0 on Linux x86_64 (AMD GPU, 32 GB)",
            "For pictures.",
            "Context size",
            "16384",
            "Port",
            "11600",
        ] {
            assert!(said_now.iter().any(|t| t == want), "{want} in {said_now:?}");
        }
        // that Open WebUI goes with it is the line's to say, not a setting's
        assert!(
            !said_now.iter().any(|t| t.starts_with("Start Open WebUI")),
            "{said_now:?}"
        );
        let report = app
            .configs
            .reports
            .get("vision")
            .cloned()
            .unwrap_or_default();
        assert!(!report.notes.is_empty(), "notes about it here");
        for note in &report.notes {
            let text = note.text(app.registry.vocabulary());
            assert!(said_now.contains(&text), "{text} in {said_now:?}");
        }
        // what is done less often is in its menu
        open_menu_of(&mut app, &ctx, &display, "Vision");
        let said_now = said(&display);
        for want in ["Rename...", "Duplicate...", "Export...", "Delete..."] {
            assert!(said_now.iter().any(|t| t == want), "{want} in {said_now:?}");
        }
        assert!(!said_now.iter().any(|t| t == "Save here"), "{said_now:?}");
        // and the built-in Default's has Reset to factory where the others have Delete
        click(&mut app, &ctx, &display, egui::pos2(4.0, 4.0));
        open_menu_of(&mut app, &ctx, &display, "Default");
        let said_now = said(&display);
        assert!(
            said_now.iter().any(|t| t == "Reset to factory...")
                && !said_now.iter().any(|t| t == "Delete..."),
            "{said_now:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);

        // no word of an engine is in what draws it
        for (name, source) in [
            (
                "configurations_window.rs",
                include_str!("configurations_window.rs"),
            ),
            ("configurations_ui.rs", include_str!("configurations_ui.rs")),
            ("command_line.rs", include_str!("command_line.rs")),
            (
                "configurations.rs",
                include_str!("../../../quickai-runtime/src/configurations.rs"),
            ),
            (
                "configuration_cli.rs",
                include_str!("../../../quickai-runtime/src/configuration_cli.rs"),
            ),
        ] {
            let code = source.split("#[cfg(test)]").next().unwrap();
            for (n, line) in code.lines().enumerate() {
                let line = line.split("//").next().unwrap_or("");
                for word in [
                    "llama",
                    "ollama",
                    "comfy",
                    "openwebui",
                    "open-webui",
                    "Open WebUI",
                ] {
                    assert!(
                        !line.to_lowercase().contains(&word.to_lowercase()),
                        "{name}:{}: `{word}` in {line}",
                        n + 1
                    );
                }
            }
        }
    }

    /// B23 (C189): a card says what goes with its model on the command line: the projector of a vision model of the
    /// list by its name before it is here, the one its record names once it is, and the mode a model of the list is
    /// started in; a model the engine keeps itself has none.
    #[test]
    fn p3_b23_a_card_names_the_projector_and_the_mode_that_go_with_its_model() {
        let dir = scratch("b23");
        let profile = |id: &str, name: &str, model: &str| {
            format!(
                "schema = 1\nkind = \"profile\"\nid = \"{id}\"\nname = \"{name}\"\nengine = \"llamacpp\"\n[[model]]\n{model}\n"
            )
        };
        put(
            &dir,
            "profiles/pictures.toml",
            &profile(
                "pictures",
                "Pictures",
                "catalog = \"qwen3-vl-8b\"\nfile = \"Qwen3VL-8B-Instruct-Q4_K_M.gguf\"",
            ),
        );
        put(
            &dir,
            "profiles/vectors.toml",
            &profile(
                "vectors",
                "Vectors",
                "catalog = \"bge-small-en-v1.5\"\nfile = \"bge-small-en-v1.5-q8_0.gguf\"",
            ),
        );
        put(
            &dir,
            "profiles/here.toml",
            &profile("here", "Here", "file = \"tiny-vl-Q4_K_M.gguf\""),
        );
        put(&dir, "profiles/vision.toml", vision_text());
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
        app.rescan_models();
        let chips = |app: &App, id: &str| {
            let entry = app
                .configuration_entries()
                .into_iter()
                .find(|e| e.configuration.id == id)
                .unwrap_or_else(|| panic!("{id} is read"));
            app.model_chips(&entry.configuration)
        };
        let pair = |a: &str, b: &str| vec![(a.to_string(), b.to_string())];
        assert_eq!(
            chips(&app, "pictures"),
            pair("Projector", "mmproj-Qwen3VL-8B-Instruct-F16.gguf"),
            "the list's, before it is here"
        );
        assert_eq!(
            chips(&app, "here"),
            pair("Projector", "mmproj-f16.gguf"),
            "the record's, by the name it came with"
        );
        assert_eq!(
            chips(&app, "vectors"),
            pair("Started as", "embedding server")
        );
        assert!(chips(&app, "vision").is_empty(), "a model the engine keeps");

        // and it is on the card, first, when the card is opened
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.open_configurations();
        app.configs.selected = Some("pictures".to_string());
        settle(&mut app, &ctx, &display);
        let said_now = said(&display);
        for want in ["Projector", "mmproj-Qwen3VL-8B-Instruct-F16.gguf"] {
            assert!(said_now.iter().any(|t| t == want), "{want} in {said_now:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tree(dir: &Path) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    // one separator on every system, so the lists read the same everywhere
                    let rel = path.strip_prefix(base).unwrap();
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    fn write_file(dir: &Path, name: &str, text: &str) -> String {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    }

    fn given_text() -> String {
        vision_text()
            .replace("id = \"vision\"", "id = \"given\"")
            .replace("name = \"Vision\"", "name = \"Given chat\"")
    }

    #[test]
    fn p3_a55_the_import_dialog_reads_a_path_reports_before_it_writes_and_takes_only_what_is_ticked(
    ) {
        let dir = scratch("a55");
        let given = write_file(&dir, "given/given.toml", &given_text());
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        let before = tree(&dir.join("config"));

        // the dialog: a box for the path, and the button of the system's dialog beside it
        press(&mut app, &ctx, &display, "Import...");
        settle(&mut app, &ctx, &display);
        let now = said(&display);
        for want in [
            "Import a configuration",
            "File:",
            "Browse...",
            "Read",
            "Cancel",
            "Import",
            "Import and use",
        ] {
            assert!(now.iter().any(|t| t == want), "{want} in {now:?}");
        }
        // the path is typed, and read: the report comes before anything is written
        app.configs.import.as_mut().unwrap().path = given.clone();
        press(&mut app, &ctx, &display, "Read");
        settle(&mut app, &ctx, &display);
        let now = said(&display);
        assert!(now.iter().any(|t| t == "Given chat"), "{now:?}");
        assert!(
            now.iter()
                .any(|t| t == "Made with QuickAI 0.1.0 on Linux x86_64 (AMD GPU, 32 GB)"),
            "{now:?}"
        );
        let this = format!("This computer: {}", app.this_computer());
        assert!(now.contains(&this), "{this} in {now:?}");
        assert!(
            now.iter()
                .any(|t| t.starts_with("Changes ") && t.contains("Context size: default to 16384")),
            "{now:?}"
        );
        assert!(
            now.iter()
                .any(|t| t.contains("Made with") || t.contains("Warns")),
            "{now:?}"
        );
        assert!(
            now.iter()
                .any(|t| t == "Lines of environment, set for Ollama as they are written."),
            "the item that is asked about: {now:?}"
        );
        assert!(
            now.iter().any(|t| t == "OLLAMA_FLASH_ATTENTION=1"),
            "in full: {now:?}"
        );
        assert_eq!(
            tree(&dir.join("config")),
            before,
            "nothing is written by reading"
        );

        // imported with nothing ticked: what is asked about is not taken
        press(&mut app, &ctx, &display, "Import");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.import.is_none());
        let stored = std::fs::read_to_string(dir.join("config/profiles/given.toml")).unwrap();
        assert!(
            stored.contains("ctx_size = 16384") && !stored.contains("extra_env"),
            "{stored}"
        );
        assert_eq!(app.configs.selected.as_deref(), Some("given"));
        assert!(app.registry.profile("given").is_some());
        assert!(
            said(&display).iter().any(|t| t == "Given chat"),
            "{:?}",
            said(&display)
        );

        // the same file again: the name is in use, and there are three answers; ticked, it is taken
        press(&mut app, &ctx, &display, "Import...");
        app.configs.import.as_mut().unwrap().path = given.clone();
        press(&mut app, &ctx, &display, "Read");
        settle(&mut app, &ctx, &display);
        let now = said(&display);
        for want in [
            "That name is used already:",
            "Keep both",
            "Replace",
            "Cancel",
        ] {
            assert!(now.iter().any(|t| t == want), "{want} in {now:?}");
        }
        app.configs
            .import
            .as_mut()
            .unwrap()
            .ticked
            .insert("ollama.extra_env".into());
        // Cancel is an answer: nothing is stored
        app.configs.import.as_mut().unwrap().when_taken = WhenTaken::Cancel;
        let files = tree(&dir.join("config"));
        press(&mut app, &ctx, &display, "Import");
        settle(&mut app, &ctx, &display);
        assert_eq!(tree(&dir.join("config")), files);
        assert!(app.configs.import.is_some(), "the dialog is still there");
        app.configs.import.as_mut().unwrap().when_taken = WhenTaken::KeepBoth;
        press(&mut app, &ctx, &display, "Import");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.import.is_none());
        let kept = std::fs::read_to_string(dir.join("config/profiles/given-chat-2.toml"))
            .unwrap_or_else(|_| panic!("{:?}", tree(&dir.join("config"))));
        assert!(
            kept.contains("OLLAMA_FLASH_ATTENTION=1") && kept.contains("name = \"Given chat 2\""),
            "{kept}"
        );

        // and used at once
        let second = write_file(
            &dir,
            "given/second.toml",
            &given_text()
                .replace("id = \"given\"", "id = \"second\"")
                .replace("Given chat", "Second chat"),
        );
        press(&mut app, &ctx, &display, "Import...");
        app.configs.import.as_mut().unwrap().path = second;
        press(&mut app, &ctx, &display, "Read");
        settle(&mut app, &ctx, &display);
        press(&mut app, &ctx, &display, "Import and use");
        settle(&mut app, &ctx, &display);
        assert_eq!(app.configs.active, "second");
        assert_eq!(app.engine, "ollama");
        assert_eq!(saved(&dir)["values"]["app.configuration"], "second");

        // a file that cannot be read, one that is refused and one of a newer QuickAI are said in the dialog
        let alien = write_file(
            &dir,
            "given/alien.toml",
            "schema = 1\nkind = \"profile\"\nid = \"alien\"\nengine = \"whispercpp\"\n",
        );
        let newer = write_file(&dir, "given/newer.toml", "schema = 1\nkind = \"profile\"\nid = \"n\"\nrequires = { app = \">=9.0\" }\nengine = \"llamacpp\"\n");
        let junk = write_file(&dir, "given/junk.toml", "this is = not toml = at all");
        press(&mut app, &ctx, &display, "Import...");
        for (path, says) in [
            (
                dir.join("nothing.toml").display().to_string(),
                "could not read",
            ),
            (
                alien,
                "It is for whispercpp, which this QuickAI does not have.",
            ),
            (newer, "Update QuickAI to use it."),
            (junk, "not a QuickAI configuration"),
        ] {
            app.configs.import.as_mut().unwrap().path = path;
            press(&mut app, &ctx, &display, "Read");
            settle(&mut app, &ctx, &display);
            let now = said(&display);
            assert!(now.iter().any(|t| t.contains(says)), "{says} in {now:?}");
            assert!(app.configs.import.as_ref().unwrap().loaded.is_none());
        }
        // cancelled: gone, and nothing was stored for the files that were refused
        press(&mut app, &ctx, &display, "Cancel");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.import.is_none());
        assert!(app.registry.profile("alien").is_none() && app.registry.profile("n").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a56_the_export_dialog_shows_what_will_be_written_and_writes_it_where_the_person_said() {
        let dir = scratch("a56");
        let secret = vision_text().replace(
            "OLLAMA_FLASH_ATTENTION=1",
            "OLLAMA_FLASH_ATTENTION=1\\nHF_TOKEN=hunter2",
        );
        put(&dir, "profiles/vision.toml", &secret);
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        let before = tree(&dir);
        open_menu_of(&mut app, &ctx, &display, "Vision");
        press(&mut app, &ctx, &display, "Export...");
        settle(&mut app, &ctx, &display);
        let now = said(&display);
        for want in [
            "Export a configuration",
            "Name in the file:",
            "A note for whoever gets it:",
            "File:",
            "Browse...",
            "What will be written:",
            "Cancel",
            "Export",
        ] {
            assert!(now.iter().any(|t| t == want), "{want} in {now:?}");
        }
        // what will be written is shown, without the secret, and what was left out is said
        let preview = now
            .iter()
            .find(|t| t.contains("kind = \"profile\""))
            .expect("the text of the file");
        assert!(
            preview.contains("OLLAMA_FLASH_ATTENTION=1") && !preview.contains("hunter2"),
            "{preview}"
        );
        assert!(
            now.iter()
                .any(|t| t.contains("Lines left out because they look like secrets")),
            "{now:?}"
        );
        // the person's name and note go into it
        let target = dir.join("out/shared.toml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        {
            let d = app.configs.export.as_mut().unwrap();
            d.name = "Pictures".into();
            d.note = "For you.".into();
            d.path = target.display().to_string();
        }
        press(&mut app, &ctx, &display, "Export");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.export.is_none());
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(
            text.contains("name = \"Pictures\"")
                && text.contains("note = \"For you.\"")
                && !text.contains("hunter2"),
            "{text}"
        );
        assert!(app
            .configs
            .status
            .as_deref()
            .is_some_and(|s| s.starts_with("Exported to ")));
        // the file is written where the person chose and nowhere else
        let after = tree(&dir);
        let new: Vec<&String> = after.iter().filter(|f| !before.contains(f)).collect();
        assert_eq!(new, ["out/shared.toml"], "{new:?}");
        // a file that is there is replaced only when the person says so
        open_menu_of(&mut app, &ctx, &display, "Vision");
        press(&mut app, &ctx, &display, "Export...");
        {
            let d = app.configs.export.as_mut().unwrap();
            d.path = target.display().to_string();
            d.note = "Another note.".into();
        }
        settle(&mut app, &ctx, &display);
        assert!(
            said(&display)
                .iter()
                .any(|t| t == "That file is there already: replace it"),
            "{:?}",
            said(&display)
        );
        press(&mut app, &ctx, &display, "Export");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.export.is_some(), "not written without a yes");
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("For you."));
        app.configs.export.as_mut().unwrap().replace = true;
        press(&mut app, &ctx, &display, "Export");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.export.is_none());
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("Another note."));
        // a path that cannot be written is said in the dialog
        open_menu_of(&mut app, &ctx, &display, "Vision");
        press(&mut app, &ctx, &display, "Export...");
        app.configs.export.as_mut().unwrap().path =
            dir.join("no/such/folder/x.toml").display().to_string();
        settle(&mut app, &ctx, &display);
        press(&mut app, &ctx, &display, "Export");
        settle(&mut app, &ctx, &display);
        assert!(
            said(&display)
                .iter()
                .any(|t| t.starts_with("Could not write ")),
            "{:?}",
            said(&display)
        );
        press(&mut app, &ctx, &display, "Cancel");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.export.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a57_every_view_is_whole_the_window_narrow_long_names_many_rows_and_each_dialog() {
        let dir = scratch("a57");
        put(&dir, "profiles/vision.toml", vision_text());
        for n in 1..=30 {
            put(
                &dir,
                &format!("profiles/c{n:02}.toml"),
                &format!(
                    "schema = 1\nkind = \"profile\"\nid = \"c{n:02}\"\nname = \"Configuration number {n:02} with a rather long name indeed\"\nengine = \"llamacpp\"\n"
                ),
            );
        }
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        let narrow = egui::vec2(LEAST[0], LEAST[1]);
        display.open_windows_at(narrow);
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        let whole = |display: &Display, what: &str| {
            let shown = display.0.borrow();
            let window = shown
                .windows
                .get(&configurations_viewport())
                .expect("drawn");
            for (text, rect) in seen_texts(&window.shapes, window.size.x) {
                // a scrolled text may be above or below what is seen; none is wider than the window
                assert!(
                    rect.min.x >= -1.0 && rect.max.x <= window.size.x + 1.0,
                    "{what}: `{}` is {:?} in a window {:?} wide",
                    text.chars().take(60).collect::<String>(),
                    rect,
                    window.size.x
                );
            }
        };
        whole(&display, "the list");
        // the end of the list: the last row is below the fold until the list is scrolled
        let last = |display: &Display| {
            painted(display)
                .unwrap_or_default()
                .into_iter()
                .find(|(t, _)| t.starts_with("Configuration number 30"))
                .map(|(_, r)| r)
        };
        assert!(
            last(&display).is_none(),
            "the last row is below the fold: nothing is painted there"
        );
        // the wheel turned over the list: halfway down the window (the least window is 240 high, and its foot is the
        // footer with Close)
        for _ in 0..40 {
            display.give(
                configurations_viewport(),
                vec![
                    egui::Event::PointerMoved(egui::pos2(100.0, narrow.y / 2.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::vec2(0.0, -20.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame(&mut app, &ctx, &display);
        }
        settle(&mut app, &ctx, &display);
        let end = last(&display).expect("the last row");
        assert!(
            end.max.y <= narrow.y + 1.0 && end.min.y >= 0.0,
            "scrolled to its end: {end:?}"
        );
        whole(&display, "the end of the list");
        // a row opened out, in the narrow window
        app.configs.selected = Some("vision".into());
        settle(&mut app, &ctx, &display);
        whole(&display, "the details");
        // each dialog
        app.configs.import = Some(ImportDialog {
            path: write_file(&dir, "given/given.toml", &given_text()),
            ..ImportDialog::default()
        });
        let mut dialog = app.configs.import.clone().unwrap();
        app.read_for_import(&mut dialog);
        app.configs.import = Some(dialog);
        app.configs.in_window = true;
        settle(&mut app, &ctx, &display);
        whole(&display, "the import dialog");
        app.configs.import = None;
        let entry = app
            .configuration_entries()
            .into_iter()
            .find(|e| e.id == "vision")
            .unwrap();
        app.configs.export = Some(app.export_dialog_for(&entry));
        settle(&mut app, &ctx, &display);
        whole(&display, "the export dialog");
        app.configs.export = None;
        app.configs.confirm = Some(Confirm::Delete("vision".into()));
        settle(&mut app, &ctx, &display);
        whole(&display, "the question");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a64_a_build_that_is_not_installed_has_a_button_that_opens_the_manager_with_its_label() {
        let dir = scratch("a64");
        let mut app = app_in(&dir, "ollama");
        let text = "schema = 1\nkind = \"profile\"\nid = \"given\"\nname = \"Given\"\nengine = \"ollama\"\n[runtime]\nbuild = \"v0.40.0\"\n[values.openwebui]\nenabled = true\n";
        let configuration = open_text(&app.registry, text, "0.1.0")
            .unwrap()
            .configuration;
        let installed = app.installed_now();
        let here = app.configuration_inventory(Some(&configuration), &installed);
        let report = review(&app.registry, &configuration, &[], &here, false);
        // the note says no build is installed, and the button opens the manager with the label
        let note = report
            .notes
            .iter()
            .find(|n| n.id == "build_none")
            .expect("a note");
        let (action, label, disabled) = app.action_for(note, &configuration).expect("a button");
        assert_eq!((label, disabled), ("Look up builds", None));
        assert_eq!(
            action,
            NoteAction::LookUpBuild {
                label: "v0.40.0".into()
            }
        );
        assert!(!app.show_settings);
        let ctx = egui::Context::default();
        app.run_note_action(action, &ctx);
        assert!(app.show_settings, "the Settings are opened");
        assert_eq!(app.runtime.filter, "v0.40.0");
        assert!(app.runtime.show_lookup);
        // nothing is asked of the network by it: the person presses the manager's own button
        assert!(app.runtime.lookup_rx.is_none());
        // Open WebUI that is wanted and not installed: the button sets it up
        let note = report
            .notes
            .iter()
            .find(|n| n.id == "companion_missing")
            .expect("a note");
        let (action, label, disabled) = app.action_for(note, &configuration).expect("a button");
        assert_eq!(
            (label, action.clone(), disabled),
            ("Set up", NoteAction::SetUp, None)
        );
        app.close_settings();
        app.run_note_action(action, &ctx);
        assert!(app.show_settings && app.companion_section_wanted);
        // for another engine the buttons wait
        let mut llama = configuration.clone();
        llama.engine = "llamacpp".into();
        let mut other = app_in(&scratch("a64b"), "llamacpp");
        assert_eq!(
            other.action_for(note, &configuration).unwrap().2.as_deref(),
            Some("Choose this configuration first: it is for another engine.")
        );
        other.close_settings();
        // a note with nothing to do has no button
        let says = Note::new("will_change", &[("list", "x".into())]);
        assert!(app.action_for(&says, &configuration).is_none());
    }

    #[test]
    fn reverting_asks_first_and_then_goes_back_to_what_was_saved_and_deleting_the_one_in_use_keeps_the_window(
    ) {
        let dir = scratch("revert");
        put(&dir, "profiles/vision.toml", vision_text());
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.request_choose("vision");
        assert_eq!(app.configs.active, "vision");
        app.values.set("port", 9000u16);
        assert!(app.configuration_modified());
        app.open_configurations();
        settle(&mut app, &ctx, &display);
        // the card of the one in use has Revert and Save; Revert asks before anything is lost; and the card says in
        // a word that it has changes that are not saved (one that is in use and unchanged says only that it is in use)
        let said_now = said(&display);
        assert!(
            said_now.iter().any(|t| t == "Revert") && said_now.iter().any(|t| t == "Save"),
            "{said_now:?}"
        );
        assert!(
            said_now.iter().any(|t| t == "• Modified"),
            "the card of the one in use says it is modified: {said_now:?}"
        );
        press(&mut app, &ctx, &display, "Revert");
        settle(&mut app, &ctx, &display);
        assert_eq!(app.configs.confirm, Some(Confirm::Revert("vision".into())));
        assert_eq!(
            app.values.int("port"),
            Some(9000),
            "nothing is lost before the answer"
        );
        let now = said(&display);
        assert!(
            now.iter().any(|t| t == "Go back to what was saved?"),
            "{now:?}"
        );
        press_in_question(&mut app, &ctx, &display, "Revert");
        settle(&mut app, &ctx, &display);
        assert!(app.configs.confirm.is_none());
        assert_eq!(
            app.values.int("port"),
            Some(11600),
            "what the configuration says"
        );
        assert!(!app.configuration_modified());
        // deleting the one in use: the Default is the one in use, and the window keeps what it holds
        app.values.set("ctx_size", 2222u32);
        app.do_confirmed(Confirm::Delete("vision".into()));
        assert_eq!(app.configs.active, "default");
        assert!(app.registry.profile("vision").is_none());
        assert_eq!(
            app.values.int("ctx_size"),
            Some(2222),
            "what the window holds is as it was"
        );
        assert!(
            app.configuration_modified(),
            "it differs from the Default now"
        );
        let file = saved(&dir);
        assert!(file["values"].get("app.configuration").is_none(), "{file}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
