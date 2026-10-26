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
use quickai_core::profile::{label_of, Configuration};

use super::configurations_ui::{modal_width, modified_color, NameFor, UNSAVED_MARK};
use super::App;
use crate::configurations::{
    self, export_text, import, import_is_taken, Entry, OnTaken, Origin, DEFAULT_ID,
};
use crate::gpu::GpuVendor;

/// What the window is called to the window manager: its title bar, and what a taskbar lists.
pub(super) const CONFIGURATIONS_TITLE: &str = "QuickAI Configurations";

/// The size the window opens at, and the least it can be made.
const OPENS_AT: [f32; 2] = [580.0, 640.0];
const LEAST: [f32; 2] = [440.0, 360.0];

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
    /// Replace this configuration with what the window holds now.
    Overwrite(String),
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

    /// A note, and after it the button it has, if it has one.
    fn show_note(&mut self, ui: &mut egui::Ui, note: &Note, configuration: &Configuration) {
        let text = self.note_text(note);
        let action = self.action_for(note, configuration);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(text)
                    .small()
                    .color(level_color(note.level)),
            );
            if let Some((action, label, disabled)) = action {
                let response = ui.add_enabled(disabled.is_none(), egui::Button::new(label).small());
                let response = match &disabled {
                    Some(why) => response.on_disabled_hover_text(why),
                    None => response,
                };
                if response.clicked() {
                    self.configs.in_window = true;
                    let ctx = ui.ctx().clone();
                    self.run_note_action(action, &ctx);
                }
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
        let builder = crate::with_app_icon(
            egui::ViewportBuilder::default()
                .with_title(CONFIGURATIONS_TITLE)
                .with_app_id(crate::APP_ID)
                .with_inner_size(OPENS_AT)
                .with_min_inner_size(LEAST),
        );
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
                egui::Panel::bottom("configurations_footer").show(ui, |ui| {
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
                });
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("configurations_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.show_configurations_body(ui));
                });
            }
            // the questions of the window are asked in it
            self.show_configuration_modals(ui.ctx(), true);
            self.show_import_dialog(ui.ctx());
            self.show_export_dialog(ui.ctx());
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
        self.configs.import = None;
        self.configs.export = None;
        if self.configs.in_window {
            self.configs.question = None;
            self.configs.name_box = None;
            self.configs.confirm = None;
            self.configs.in_window = false;
        }
        self.save_settings();
    }

    /// What the window holds, in the order it is read: the commands that make one, then each
    /// configuration, then the files that could not be used.
    fn show_configurations_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .button("New from this...")
                .on_hover_text("Save what you have set now as a new configuration, under a name.")
                .clicked()
            {
                self.configs.in_window = true;
                let name = self.suggested_name();
                self.ask_for_a_name(NameFor::SaveAs, &name);
            }
            if ui
                .button("Import...")
                .on_hover_text(
                    "Read a configuration someone gave you, and say what to take from it.",
                )
                .clicked()
            {
                self.configs.in_window = true;
                self.configs.import = Some(ImportDialog::default());
            }
        });
        if let Some(why) = &self.configs.error {
            ui.label(
                egui::RichText::new(why)
                    .small()
                    .color(super::theme::palette().bad),
            );
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        let entries = self.configuration_entries();
        let modified = self.configuration_modified();
        for entry in &entries {
            self.show_configuration_row(ui, entry, modified);
            ui.add_space(2.0);
        }
        if !self.configs.unusable.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Files that could not be used").strong())
                .on_hover_text(
                    "They are in your configurations folder. Fix them or remove them there.",
                );
            for bad in &self.configs.unusable.clone() {
                ui.label(egui::RichText::new(&bad.file).small().weak());
                ui.label(
                    egui::RichText::new(&bad.why)
                        .small()
                        .color(super::theme::palette().bad),
                );
            }
        }
    }

    /// One configuration: its line, and opened out the details and the commands.
    fn show_configuration_row(&mut self, ui: &mut egui::Ui, entry: &Entry, modified: bool) {
        let active = entry.id == self.configs.active;
        let selected = self.configs.selected.as_deref() == Some(entry.id.as_str());
        let report = self
            .configs
            .reports
            .get(&entry.id)
            .cloned()
            .unwrap_or_default();
        let engine_name = self
            .registry
            .engine(&entry.configuration.engine)
            .map_or_else(
                || entry.configuration.engine.clone(),
                |e| e.doc.engine.name.clone(),
            );
        let summary = self.summary_of(&entry.configuration, &engine_name);
        let notes = report
            .notes
            .iter()
            .filter(|n| n.level != Level::Says)
            .count();
        ui.horizontal(|ui| {
            let arrow = if selected { "v" } else { ">" };
            if ui
                .selectable_label(selected, format!("{arrow} {}", entry.name))
                .on_hover_text(if active {
                    "In use now"
                } else {
                    "Click for details"
                })
                .clicked()
            {
                self.configs.selected = if selected {
                    None
                } else {
                    Some(entry.id.clone())
                };
            }
            if active {
                ui.label(
                    egui::RichText::new("in use")
                        .small()
                        .color(super::theme::palette().ok),
                );
                if modified {
                    ui.label(
                        egui::RichText::new(format!("{UNSAVED_MARK} modified"))
                            .small()
                            .color(modified_color(ui.visuals().dark_mode)),
                    )
                    .on_hover_text("What the window holds is not what was saved.");
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(entry.origin.label()).small().weak());
                if notes > 0 {
                    ui.label(
                        egui::RichText::new(format!(
                            "{notes} note{}",
                            if notes == 1 { "" } else { "s" }
                        ))
                        .small()
                        .color(super::theme::palette().warn),
                    );
                }
            });
        });
        ui.label(egui::RichText::new(summary).small().weak());
        if !selected {
            return;
        }
        ui.indent(("configuration_details", &entry.id), |ui| {
            self.show_configuration_details(ui, entry, &report, active, modified);
        });
    }

    /// `Ollama - qwen3-vl:8b - Open WebUI`: the engine, the model and what goes with it.
    fn summary_of(&self, configuration: &Configuration, engine_name: &str) -> String {
        let mut parts = vec![engine_name.to_string()];
        if let Some(model) = configuration.models.first() {
            parts.push(model.display().to_string());
        }
        for companion in self.registry.companions_of(&configuration.engine) {
            let on = configuration
                .values
                .get(&quickai_core::profile::value_key(&companion.id, "enabled"))
                .and_then(quickai_core::value::Value::as_bool)
                .unwrap_or(false);
            if on {
                parts.push(companion.doc.engine.name.clone());
            }
        }
        parts.join(" - ")
    }

    fn show_configuration_details(
        &mut self,
        ui: &mut egui::Ui,
        entry: &Entry,
        report: &Report,
        active: bool,
        modified: bool,
    ) {
        let c = &entry.configuration;
        let made = made_line(c);
        if !made.is_empty() {
            ui.label(egui::RichText::new(made).small());
        }
        if let Some(note) = &c.meta.note {
            ui.label(egui::RichText::new(note).small().italics());
        }
        // what choosing it would change
        let differ = self.differences_from(entry);
        if !active {
            ui.label(
                egui::RichText::new(match differ.len() {
                    0 => "Nothing differs from what you have set now.".to_string(),
                    1 => "1 setting differs from what you have set now.".to_string(),
                    n => format!("{n} settings differ from what you have set now."),
                })
                .small(),
            )
            .on_hover_text(
                differ
                    .iter()
                    .map(Change::describe)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        // the commands
        let locked = self.engine_locked();
        ui.horizontal_wrapped(|ui| {
            let can_use = !active || modified;
            // a configuration the notes stop cannot be chosen, and the first of them says why
            let stopped = report.stop().map(|n| n.text(self.registry.vocabulary()));
            let reason = if locked {
                self.engine_locked_sentence()
            } else if let Some(why) = &stopped {
                why.clone()
            } else {
                "It is the one in use, and nothing was changed.".to_string()
            };
            if ui
                .add_enabled(
                    !locked && can_use && stopped.is_none(),
                    egui::Button::new(if active { "Revert" } else { "Use" }),
                )
                .on_hover_text(if active {
                    "Go back to what was saved."
                } else {
                    "Make this the configuration in use: its engine, model and settings become the window's. Nothing is started."
                })
                .on_disabled_hover_text(reason)
                .clicked()
            {
                self.configs.in_window = true;
                if active {
                    self.configs.confirm = Some(Confirm::Revert(entry.id.clone()));
                } else {
                    self.request_choose(&entry.id);
                    self.refresh_configurations();
                }
            }
            let save_label = if active { "Save" } else { "Save here" };
            if ui
                .add_enabled(!active || modified, egui::Button::new(save_label))
                .on_hover_text(if active {
                    "Save what you have set over this configuration."
                } else {
                    "Replace this configuration with what you have set now."
                })
                .clicked()
            {
                self.configs.in_window = true;
                if active {
                    let _ = self.save_active();
                    self.refresh_configurations();
                } else {
                    self.configs.confirm = Some(Confirm::Overwrite(entry.id.clone()));
                }
            }
            let yours = entry.origin == Origin::Yours;
            if ui
                .add_enabled(yours, egui::Button::new("Rename"))
                .on_disabled_hover_text("Built in, or from the package: save it under another name instead.")
                .clicked()
            {
                self.configs.in_window = true;
                self.ask_for_a_name(NameFor::Rename(entry.id.clone()), &entry.name);
            }
            if ui.button("Duplicate").on_hover_text("A copy under a name of its own.").clicked() {
                self.configs.in_window = true;
                let suggestion = format!("{} copy", entry.name);
                self.ask_for_a_name(NameFor::Duplicate(entry.id.clone()), &suggestion);
            }
            if ui
                .button("Export...")
                .on_hover_text("Write it to a file to give to someone else: no folder, program or secret goes in it.")
                .clicked()
            {
                self.configs.in_window = true;
                self.configs.export = Some(self.export_dialog_for(entry));
            }
            if entry.id == DEFAULT_ID {
                if ui
                    .add_enabled(yours, egui::Button::new("Reset to factory"))
                    .on_hover_text("Put the Default that comes with QuickAI back.")
                    .on_disabled_hover_text("It is the Default that comes with QuickAI already.")
                    .clicked()
                {
                    self.configs.in_window = true;
                    self.configs.confirm = Some(Confirm::Reset);
                }
            } else if ui
                .add_enabled(yours, egui::Button::new("Delete"))
                .on_disabled_hover_text("Built in, or from the package: it cannot be deleted.")
                .clicked()
            {
                self.configs.in_window = true;
                self.configs.confirm = Some(Confirm::Delete(entry.id.clone()));
            }
        });
        // what it sets
        let sets = self.sets_line(c);
        ui.label(egui::RichText::new(format!("Sets: {sets}")).small());
        if let Some(build) = &c.runtime.build {
            ui.label(egui::RichText::new(format!("Program: {build}")).small());
        }
        // what a person should be told about it on this computer
        for note in &report.notes {
            self.show_note(ui, note, c);
        }
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

    fn sets_line(&self, c: &Configuration) -> String {
        let mut parts = Vec::new();
        for (key, field) in carried(&self.registry, &c.engine) {
            // what goes with the engine is named, or its settings read as the engine's own
            let owner = key.rsplit_once('.').map_or("", |(owner, _)| owner);
            let owner_name = (owner != c.engine)
                .then(|| {
                    self.registry
                        .engine(owner)
                        .map(|e| e.doc.engine.name.clone())
                })
                .flatten();
            if let Some(value) = c.values.get(&key) {
                let text = match value {
                    quickai_core::value::Value::Str(s) => {
                        let first = s.lines().next().unwrap_or_default();
                        if s.lines().count() > 1 || first.chars().count() > 30 {
                            format!("{}...", first.chars().take(30).collect::<String>())
                        } else {
                            first.to_string()
                        }
                    }
                    other => quickai_core::profile::shown(other),
                };
                let label = label_of(field);
                parts.push(match owner_name {
                    // unless the label says whose it is already
                    Some(name) if !label.to_lowercase().contains(&name.to_lowercase()) => {
                        format!("{name} {label} {text}")
                    }
                    _ => format!("{label} {text}"),
                });
            }
        }
        if c.traffic_proxy == Some(true) {
            parts.push("Live traffic proxy on".to_string());
        }
        if parts.is_empty() {
            "everything at its default".to_string()
        } else {
            parts.join(", ")
        }
    }

    pub(super) fn note_text(&self, note: &Note) -> String {
        format!(
            "{}  {}",
            note.level.label(),
            note.text(self.registry.vocabulary())
        )
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
        let text = match crate::configuration_cli::read_file(&path) {
            Ok(text) => text,
            Err(e) => {
                dialog.error = Some(e);
                return;
            }
        };
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

    /// The dialog that imports a file from outside: the path, the report, what to take, the name.
    fn show_import_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.configs.import.clone() else {
            return;
        };
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
    fn finish_import(&mut self, dialog: &mut ImportDialog, and_use: bool) {
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
        ) {
            Ok(done) => {
                self.reload_configurations();
                self.refresh_configurations();
                let id = done.configuration.id.clone();
                self.configs.selected = Some(id.clone());
                self.configs.status = Some(format!("Imported \"{}\".", done.configuration.name));
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

    /// Delete, reset and overwrite ask before anything is done.
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
            Confirm::Overwrite(id) => (
                "Replace this configuration?".to_string(),
                format!(
                    "\"{}\" is replaced by what you have set now: the engine, the model and the settings.",
                    name_of(id)
                ),
                "Replace",
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
            Confirm::Overwrite(id) => {
                let installed = self.installed_now();
                let captured = self.captured_now(&installed);
                let host = self.host();
                configurations::save(&self.config_dirs, &self.registry, id, captured, &host)
                    .map(|_| "Replaced with what you have set now.".to_string())
                    .map_err(|e| e.to_string())
            }
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
fn computer_line(os: &str, arch: &str, gpus: &[String], memory_gb: Option<u64>) -> String {
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
        for want in ["New from this...", "Import...", "Close"] {
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
        // each configuration with what it is and where it is from
        assert!(
            said_now
                .iter()
                .any(|t| t.starts_with("> Default") || t == "v Default"),
            "{said_now:?}"
        );
        assert!(said_now.iter().any(|t| t == "> Vision"), "{said_now:?}");
        assert!(
            said_now
                .iter()
                .any(|t| t == "llama.cpp - Bonsai 1.7B (1-bit Q1_0)" || t == "llama.cpp"),
            "{said_now:?}"
        );
        assert!(
            said_now
                .iter()
                .any(|t| t == "Ollama - qwen3-vl:8b - Open WebUI"),
            "{said_now:?}"
        );
        assert!(
            said_now.iter().any(|t| t == "built in") && said_now.iter().any(|t| t == "yours"),
            "{said_now:?}"
        );
        // the files that could not be used are listed with why
        assert!(
            said_now.iter().any(|t| t == "Files that could not be used"),
            "{said_now:?}"
        );
        assert!(
            said_now
                .iter()
                .any(|t| t.contains("missing field `engine`")),
            "{said_now:?}"
        );
        // opened out: the details and the commands
        press(&mut app, &ctx, &display, "> Vision");
        settle(&mut app, &ctx, &display);
        let said_now = said(&display);
        for want in [
            "Made with QuickAI 0.1.0 on Linux x86_64 (AMD GPU, 32 GB)",
            "For pictures.",
            "Use",
            "Save here",
            "Rename",
            "Duplicate",
            "Export...",
            "Delete",
        ] {
            assert!(said_now.iter().any(|t| t == want), "{want} in {said_now:?}");
        }
        let sets = said_now
            .iter()
            .find(|t| t.starts_with("Sets: "))
            .expect("what it sets");
        assert!(
            sets.contains("Context size 16384")
                && sets.contains("Port 11600")
                && sets.contains("Start Open WebUI with Ollama true"),
            "{sets}"
        );
        assert!(
            said_now
                .iter()
                .any(|t| t.contains("Warns") || t.contains("Says")),
            "the notes about it: {said_now:?}"
        );
        // a setting differs from what the window holds
        assert!(
            said_now
                .iter()
                .any(|t| t.ends_with("differ from what you have set now.")
                    || t.ends_with("differs from what you have set now.")),
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
            said(&display)
                .iter()
                .any(|t| t == "> Given chat" || t == "v Given chat"),
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
        press(&mut app, &ctx, &display, "> Vision");
        settle(&mut app, &ctx, &display);
        let before = tree(&dir);
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
                .find(|(t, _)| t.starts_with("> Configuration number 30"))
                .map(|(_, r)| r)
        };
        assert!(
            last(&display).is_none(),
            "the last row is below the fold: nothing is painted there"
        );
        for _ in 0..40 {
            display.give(
                configurations_viewport(),
                vec![
                    egui::Event::PointerMoved(egui::pos2(100.0, 200.0)),
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
        // the row of the one in use has Revert; it asks before anything is lost; and it says in words that it
        // has changes that are not saved (a row that is in use and unchanged says only that it is in use)
        let said_now = said(&display);
        assert!(said_now.iter().any(|t| t == "Revert"), "{said_now:?}");
        assert!(
            said_now.iter().any(|t| t == "• modified"),
            "the row of the one in use says it is modified: {said_now:?}"
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
