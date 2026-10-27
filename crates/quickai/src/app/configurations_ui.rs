//! Configurations in the main window: the section under the title row that shows which one is in use,
//! chooses another and says when what the window holds is no longer what was saved, the question asked
//! when a choice would lose changes that were not saved, and the box that asks for a name
//! (docs/configurations.md, sections 6 and 10). What choosing, saving and sharing *do* is
//! `crate::configurations`; this file is the part that is the window's.
//!
//! The working state is still what the window holds and `settings.json` keeps. A configuration is
//! saved on purpose (Save, Save as): while what the window holds is no longer what was saved, the
//! section says *Modified* in words, with what changed on hover, in a colour of its own, and offers
//! Save, Save as and Revert. What it says it says in the room that was kept for it, so that nothing
//! moves when a change is made.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;

use quickai_core::profile::check::{review, Inventory, Report};
use quickai_core::profile::notes::Level;
use quickai_core::profile::state::{capture, changes, Captured, Change, ModelFacts};
use quickai_core::profile::Configuration;
use quickai_core::value::Value;

use super::configurations_window::Confirm;
use super::App;
use crate::configurations::{
    self, busy_ports, choose, entries, first_run_values, installed_builds, inventory,
    listed_by_engines, models_here, save, save_as, store_state, unusable, working_state, Entry,
    Host, Installed, Now, StoreError, Unusable, DEFAULT_ID,
};
use crate::launch;

/// What the dropdown is made of: its width when there is room, and the least it is made when there is not.
const COMBO_WIDTH: f32 = 240.0;
const COMBO_LEAST: f32 = 110.0;

/// Whether the section has its label, and how wide its dropdown is, in the room its row has. `label` is
/// the width of the label with the space after it, `extras` the width of what a change adds to the row
/// (the badge and the buttons, with the spaces between them). The room for the extras is kept whether
/// there is a change or not, so that the dropdown is where it was and as wide as it was when one is made;
/// with less room than the label, the least dropdown and the extras need, the label goes first, and then
/// the dropdown gives up its width.
pub(super) fn section_shape(room: f32, label: f32, extras: f32) -> (bool, f32) {
    let spare = room - label - extras;
    if spare >= COMBO_WIDTH {
        (true, COMBO_WIDTH)
    } else if spare >= COMBO_LEAST {
        (true, spare)
    } else {
        (false, (room - extras).clamp(COMBO_LEAST, COMBO_WIDTH))
    }
}

/// The mark in front of *Modified*, in the badge that says a configuration has changes that are not saved.
/// It is a character the window's fonts have (the filled circle, U+25CF, is not one of them: it is drawn
/// as an empty box).
pub(super) const UNSAVED_MARK: &str = "•";

/// The colour of what says a configuration has changes that are not saved: amber, a deeper one on the
/// light theme (the one of the dark theme is hard to read on white).
pub(super) fn modified_color(dark: bool) -> egui::Color32 {
    super::theme::palette_of(dark).warn
}

/// What the main window keeps about configurations.
pub(super) struct Configs {
    /// The id of the one in use.
    pub active: String,
    /// What a first run holds for the settings that have always been in the file (the default of a
    /// configuration): worked out once.
    pub first_run: BTreeMap<String, Value>,
    /// A choice that waits for the answer to *Save, Don't save or Cancel*.
    pub question: Option<String>,
    /// The box that asks for a name, when one is asked for.
    pub name_box: Option<NameBox>,
    /// What went wrong the last time, in a sentence, until the next thing the person does.
    pub error: Option<String>,
    /// The builds that are installed, as they were found a moment ago: the title row asks every frame
    /// whether anything changed, and the disk is not read every frame.
    pub installed: std::cell::RefCell<Option<(std::time::Instant, Installed)>>,
    /// What was asked in the window that keeps the configurations is answered there, and what was asked in
    /// the title row is answered in the main window.
    pub in_window: bool,
    /// The row of the window that is opened out.
    pub selected: Option<String>,
    /// What a person is told about each configuration on this computer, made when the window is opened
    /// and after each thing that is done in it.
    pub reports: BTreeMap<String, Report>,
    /// The files of the folder that could not be used, and why.
    pub unusable: Vec<Unusable>,
    pub import: Option<super::configurations_window::ImportDialog>,
    pub export: Option<super::configurations_window::ExportDialog>,
    pub confirm: Option<super::configurations_window::Confirm>,
    /// The last thing that was done in the window, in a sentence.
    pub status: Option<String>,
}

impl Configs {
    pub fn new() -> Configs {
        Configs {
            active: DEFAULT_ID.to_string(),
            first_run: BTreeMap::new(),
            question: None,
            name_box: None,
            error: None,
            installed: std::cell::RefCell::new(None),
            in_window: false,
            selected: None,
            reports: BTreeMap::new(),
            unusable: Vec::new(),
            import: None,
            export: None,
            confirm: None,
            status: None,
        }
    }
}

/// What a name is asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NameFor {
    /// A new configuration from what the window holds.
    SaveAs,
    /// A copy of the configuration with this id.
    Duplicate(String),
    /// A new name for the configuration with this id.
    Rename(String),
}

#[derive(Clone, Debug)]
pub(super) struct NameBox {
    pub asked_for: NameFor,
    pub text: String,
    pub error: Option<String>,
    /// The box was just opened: the cursor goes in it once.
    pub fresh: bool,
}

impl App {
    /// The computer, as the notes and a saved file's `made_on` say it.
    pub(super) fn host(&self) -> Host {
        Host::from_machine(&self.machine)
    }

    /// The configurations the registry has, the Default first. The Default is the engine in use with every setting at its
    /// default (when it names no model and no build, as the application's own does): changing the engine in the toolbar
    /// is not a change of it, and choosing it (or Revert) brings back the defaults of that engine rather than another.
    pub(super) fn configuration_entries(&self) -> Vec<Entry> {
        let mut all = entries(&self.registry);
        for entry in all.iter_mut().filter(|e| e.id == DEFAULT_ID) {
            let generic = entry.configuration.models.is_empty()
                && entry.configuration.runtime.build.is_none();
            if generic && self.registry.engine(&self.engine).is_some() {
                entry.configuration.engine = self.engine.clone();
            }
        }
        all
    }

    /// The configuration in use, when the registry still has it.
    pub(super) fn active_entry(&self) -> Option<Entry> {
        self.configuration_entries()
            .into_iter()
            .find(|e| e.id == self.configs.active)
    }

    /// What is known of the model that is selected, for naming it in a configuration.
    fn selected_model_facts(&self) -> Option<ModelFacts> {
        let model = self.models.get(self.selected)?;
        // where it was fetched from, when the application remembers (a file of a look-up)
        let recalled = crate::sources::recall_where(
            std::path::Path::new(&self.host_facts.data_dir),
            &model.filename,
            model.size_bytes,
        );
        Some(ModelFacts {
            size: Some(model.size_bytes).filter(|s| *s > 0),
            sha256: None,
            display: Some(model.display.clone()).filter(|d| *d != model.filename),
            source: recalled.as_ref().map(|r| r.source.clone()),
            path: recalled.as_ref().and_then(|r| r.path.clone()),
            revision: recalled.and_then(|r| r.revision),
        })
    }

    /// The builds that are installed, as the manager lists them (read from the disk).
    pub(super) fn installed_now(&self) -> Installed {
        installed_builds(&self.registry, &self.host_facts)
    }

    /// The working state as a configuration describes it: from the settings the window would write now.
    pub(super) fn captured_now(&self, installed: &Installed) -> Captured {
        let state = working_state(&self.registry, &self.settings_now(), installed);
        capture(
            &self.registry,
            &state,
            &self.configs.first_run,
            self.selected_model_facts().as_ref(),
        )
    }

    /// The configuration in use as the window can be compared with it: what it names and this computer
    /// cannot hold is not counted as a difference. A model that is not among the models the window has
    /// (not here yet, or not listed because the engine is stopped), and a build that is not installed
    /// (`held`: the label of the build the window runs with, or none), cannot be held by the window, and a
    /// configuration that was just chosen is not said to be modified for that.
    fn comparable(
        &self,
        saved: &Configuration,
        installed: &Installed,
        held: Option<String>,
    ) -> Configuration {
        let mut saved = saved.clone();
        let held_model = saved.models.first().is_some_and(|pick| {
            self.models
                .iter()
                .any(|m| m.filename == pick.source.label())
        });
        if !held_model {
            saved.models.clear();
        }
        let named = saved.runtime.build.as_deref();
        let there = named.is_some_and(|label| {
            installed
                .get(&saved.engine)
                .is_some_and(|builds| builds.iter().any(|b| b.label == label))
        });
        if named.is_some() && !there {
            saved.runtime.build = held;
        }
        saved
    }

    /// Whether what the window holds is no longer what the configuration in use says: whether there is
    /// anything in the list of what changed (`changes_since_saved`), so that the badge and what it says on
    /// hover are never two answers. The settings are compared as the values in force, and not as the file
    /// spells them: a file that says a value that is the default (a hand-written one, or one made by
    /// another version) is not changed by being chosen. Nothing is started or read from the disk for it,
    /// but the installed builds, which are looked at when they change (`installed_cache_or_scan`).
    pub(super) fn configuration_modified(&self) -> bool {
        !self.changes_since_saved().is_empty()
    }

    /// What was changed since the configuration in use was saved, as a person says it (`Port: 8080 to
    /// 9000`: what it was, what it is).
    pub(super) fn changes_since_saved(&self) -> Vec<Change> {
        let Some(saved) = self.active_entry() else {
            return Vec::new();
        };
        let installed = self.installed_cache_or_scan();
        let state = working_state(&self.registry, &self.settings_now(), &installed);
        let saved = self.comparable(&saved.configuration, &installed, state.build.clone());
        let mut list = changes(
            &self.registry,
            &state,
            &self.configs.first_run,
            self.selected_model_facts().as_ref(),
            &saved,
        );
        // a configuration that names no model does not mind which one is selected
        if saved.models.is_empty() {
            list.retain(|c| !matches!(c, Change::Model { .. }));
        }
        list.into_iter().map(Change::flipped).collect()
    }

    /// The installed builds as they were found a moment ago, else read now.
    pub(super) fn installed_cache_or_scan(&self) -> Installed {
        let mut cache = self.configs.installed.borrow_mut();
        if let Some((at, found)) = cache.as_ref() {
            if at.elapsed() < std::time::Duration::from_secs(5) {
                return found.clone();
            }
        }
        let found = self.installed_now();
        *cache = Some((std::time::Instant::now(), found.clone()));
        found
    }

    /// The disk is read again the next time the builds are asked for.
    pub(super) fn forget_installed(&self) {
        *self.configs.installed.borrow_mut() = None;
    }

    /// What this computer has, for the notes about a configuration: the builds installed, the models in
    /// the folders and the lists the engines gave last time. Nothing is asked of a program.
    pub(super) fn configuration_inventory(
        &self,
        configuration: Option<&Configuration>,
        installed: &Installed,
    ) -> Inventory {
        let host = self.host();
        let listed = listed_by_engines(&self.registry, &self.host_facts);
        let stored = self.settings_now();
        let models = models_here(
            &self.registry,
            &self.bundled_models_dir,
            &|engine| {
                stored
                    .engine_values(engine)
                    .get(&format!("{}.models_dir", engine.id))
                    .and_then(Value::as_str)
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(crate::paths::user_models_dir)
            },
            &listed,
        );
        let mut running = BTreeSet::new();
        if self.session.is_active() {
            running.insert(self.engine.clone());
        }
        let data = self.data_dir_override.clone();
        let busy = configuration
            .map(|c| {
                busy_ports(
                    &self.registry,
                    c,
                    &working_state(&self.registry, &stored, installed),
                )
            })
            .unwrap_or_default();
        inventory(
            &self.registry,
            &host,
            installed,
            &models,
            Now {
                running: &running,
                busy_ports: busy,
                first_addresses: configurations::first_addresses(&self.registry, &stored),
            },
            &|engine| {
                launch::locate_binary_in(&self.registry, &engine.id, None, data.as_deref()).exists()
            },
        )
    }

    /// Reads the configurations from the disk again after a file was written or removed.
    pub(super) fn reload_configurations(&mut self) {
        self.registry = crate::config::build_registry(&self.config_dirs);
        // the window that keeps them shows what is there now
        if self.show_configurations {
            self.refresh_configurations();
        }
    }

    /// Opens the window that keeps the configurations (see `configurations_window.rs`), or brings it to the
    /// front when it is open already.
    pub(super) fn open_configurations(&mut self) {
        self.raise_configurations = self.show_configurations;
        if !self.show_configurations {
            self.refresh_configurations();
        }
        self.show_configurations = true;
        self.configs
            .selected
            .get_or_insert_with(|| self.configs.active.clone());
    }

    /// Reads again what the window shows of the disk and of this computer: the notes about each
    /// configuration, and the files that could not be used.
    pub(super) fn refresh_configurations(&mut self) {
        self.forget_installed();
        let installed = self.installed_now();
        let mut reports = BTreeMap::new();
        for entry in self.configuration_entries() {
            let here = self.configuration_inventory(Some(&entry.configuration), &installed);
            reports.insert(
                entry.id.clone(),
                review(
                    &self.registry,
                    &entry.configuration,
                    &entry.notes,
                    &here,
                    false,
                ),
            );
        }
        self.configs.reports = reports;
        self.configs.unusable = unusable(&self.registry);
    }

    // ---- choosing -------------------------------------------------------------------------------------

    /// The person picks a configuration: refused while something runs or is fetched (the Engine row's
    /// rule), asked about when changes would be lost, else chosen.
    pub(super) fn request_choose(&mut self, id: &str) {
        self.configs.error = None;
        if self.engine_locked() {
            self.configs.error = Some(self.engine_locked_sentence());
            return;
        }
        // the one in use, picked again, changes nothing: going back to what it says is Revert's, which asks first
        // (picked again with changes, it threw them away without a word)
        if id == self.configs.active {
            return;
        }
        if self.configuration_modified() {
            self.configs.question = Some(id.to_string());
            return;
        }
        self.choose_configuration(id);
    }

    /// The Engine row's sentence for why nothing can be changed now.
    pub(super) fn engine_locked_sentence(&self) -> String {
        if self.session.is_active() {
            "Stop the server to change this.".to_string()
        } else {
            "Wait for what is being fetched to finish to change this.".to_string()
        }
    }

    /// Chooses a configuration: the engine, the model, the build and the settings it describes become the
    /// window's and are written to `settings.json`. Nothing is started, fetched or installed. What a person
    /// should be told is in the log; what stops it is in `configs.error`.
    pub(super) fn choose_configuration(&mut self, id: &str) {
        self.configs.error = None;
        if self.engine_locked() {
            self.configs.error = Some(self.engine_locked_sentence());
            return;
        }
        let Some(entry) = self
            .configuration_entries()
            .into_iter()
            .find(|e| e.id == id)
        else {
            self.configs.error = Some(StoreError::Missing(id.to_string()).to_string());
            return;
        };
        self.forget_installed();
        let installed = self.installed_now();
        let here = self.configuration_inventory(Some(&entry.configuration), &installed);
        let settings = self.settings_now();
        let choice = match choose(
            &self.registry,
            &settings,
            &entry.configuration,
            &here,
            &installed,
        ) {
            Ok(choice) => choice,
            Err(report) => {
                self.configs.error = report
                    .stop()
                    .map(|n| n.text(self.registry.vocabulary()))
                    .or_else(|| Some("This configuration cannot be used here.".into()));
                return;
            }
        };
        let mut next = settings;
        if let Err(why) = store_state(&self.registry, &mut next, &choice.state) {
            self.configs.error = Some(why);
            return;
        }
        next.set_active_configuration(id);
        self.write_settings(&next);
        self.stored = next;
        self.engine = choice.state.engine.clone();
        self.enable_traffic_proxy = choice.state.traffic_proxy;
        self.values = super::values::FieldValues::from_settings(
            &self.stored,
            self.registry.engine(&self.engine),
        );
        self.configs.active = id.to_string();
        self.load_engine_state();
        if self.show_configurations {
            self.refresh_configurations();
        }
        self.push_log(format!(
            "[quickai] Configuration \"{}\" is in use.",
            entry.name
        ));
        for note in &choice.report.notes {
            if note.level != Level::Asks {
                self.push_log(format!(
                    "[quickai] {}: {}",
                    note.level.label(),
                    note.text(self.registry.vocabulary())
                ));
            }
        }
    }

    /// Saves what the window holds over the configuration in use.
    pub(super) fn save_active(&mut self) -> Result<(), String> {
        self.configs.error = None;
        let id = self.configs.active.clone();
        let installed = self.installed_now();
        let captured = self.captured_now(&installed);
        let host = self.host();
        match save(&self.config_dirs, &self.registry, &id, captured, &host) {
            Ok(_) => {
                self.reload_configurations();
                self.push_log(format!("[quickai] Saved the configuration \"{id}\"."));
                Ok(())
            }
            Err(e) => {
                let said = e.to_string();
                self.configs.error = Some(said.clone());
                Err(said)
            }
        }
    }

    /// Saves what the window holds as a new configuration with this name, and makes it the one in use.
    pub(super) fn save_as_configuration(&mut self, name: &str) -> Result<String, String> {
        self.configs.error = None;
        let installed = self.installed_now();
        let captured = self.captured_now(&installed);
        let host = self.host();
        match save_as(&self.config_dirs, &self.registry, name, captured, &host) {
            Ok(id) => {
                self.reload_configurations();
                self.configs.active = id.clone();
                let mut stored = self.settings_now();
                stored.set_active_configuration(&id);
                self.write_settings(&stored);
                self.stored = stored;
                self.push_log(format!(
                    "[quickai] Saved \"{}\" as a configuration.",
                    name.trim()
                ));
                Ok(id)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    // ---- the box that asks for a name ----------------------------------------------------------------

    pub(super) fn ask_for_a_name(&mut self, asked_for: NameFor, suggestion: &str) {
        self.configs.name_box = Some(NameBox {
            asked_for,
            text: suggestion.to_string(),
            error: None,
            fresh: true,
        });
    }

    /// The name box was answered with a name: does what it was opened for, and closes when that worked.
    pub(super) fn submit_name(&mut self) {
        let Some(open) = self.configs.name_box.clone() else {
            return;
        };
        let done: Result<(), String> = match &open.asked_for {
            NameFor::SaveAs => self.save_as_configuration(&open.text).map(|_| ()),
            NameFor::Duplicate(id) => {
                configurations::duplicate(&self.config_dirs, &self.registry, id, &open.text)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
                    .map(|()| self.reload_configurations())
            }
            NameFor::Rename(id) => {
                configurations::rename(&self.config_dirs, &self.registry, id, &open.text)
                    .map_err(|e| e.to_string())
                    .map(|()| self.reload_configurations())
            }
        };
        match done {
            Ok(()) => self.configs.name_box = None,
            Err(why) => {
                if let Some(open) = &mut self.configs.name_box {
                    open.error = Some(why);
                }
            }
        }
    }

    // ---- what the first start does -------------------------------------------------------------------

    /// The first start with configurations: the working state is written once as the person's Default when
    /// it is not what the built-in Default describes (see `configurations::adopt`). Only the real
    /// application does it, from `App::new`.
    pub(super) fn adopt_working_state(&mut self) {
        let installed = self.installed_now();
        let host = self.host();
        // what the person had set is what the file said: the model the window picked for someone who
        // never chose one is not theirs, and is not put in their Default
        let had = self.stored.clone();
        let named = working_state(&self.registry, &had, &installed)
            .values
            .get(&quickai_core::profile::value_key(
                &self.engine,
                quickai_core::profile::state::MODEL_FIELD,
            ))
            .and_then(Value::as_str)
            .map(str::to_string);
        let facts = self
            .models
            .get(self.selected)
            .filter(|m| Some(&m.filename) == named.as_ref())
            .and_then(|_| self.selected_model_facts());
        match configurations::adopt(
            &self.config_dirs,
            &self.registry,
            &had,
            &installed,
            &host,
            facts.as_ref(),
        ) {
            Ok(Some(path)) => {
                self.reload_configurations();
                self.push_log(format!(
                    "[quickai] Your settings were saved once as your Default configuration ({}).",
                    path.display()
                ));
            }
            Ok(None) => {}
            Err(e) => self.push_log(format!(
                "[quickai] The Default configuration could not be written: {e}"
            )),
        }
    }

    /// Which configuration to start with, and the sentence for the log when the one the settings name is
    /// not there.
    pub(super) fn find_active_configuration(&mut self) {
        let (id, note) = configurations::active_for_start(&self.registry, &self.stored);
        self.configs.active = id;
        self.configs.first_run = first_run_values(&self.registry);
        if let Some(note) = note {
            self.push_log(note);
        }
    }

    // ---- drawing ----------------------------------------------------------------------------------------

    /// The section of the configuration, under the title row of the Main page: the label, the dropdown with
    /// the name of the one in use and, while what the window holds is no longer what was saved, a badge
    /// that says so with *Save*, *Save as...* and *Revert* after it, in a frame of another colour. The
    /// room for the badge and the buttons is kept all the time, so that nothing moves when a change is
    /// made (a field of the page under it may be the one the person is typing in).
    pub(super) fn show_configuration_section(&mut self, ui: &mut egui::Ui) {
        let modified = self.configuration_modified();
        let color = modified_color(ui.visuals().dark_mode);
        let mut frame = egui::Frame::group(ui.style());
        if modified {
            frame = frame
                .stroke(egui::Stroke::new(1.0, color))
                .fill(color.gamma_multiply(0.1));
        }
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.show_section_row(ui, modified, color);
            if let Some(why) = &self.configs.error {
                ui.label(
                    egui::RichText::new(why)
                        .small()
                        .color(super::theme::palette().bad),
                );
            }
        });
    }

    /// The row of the section: what it chooses and what it is told to do is done after it is drawn.
    fn show_section_row(&mut self, ui: &mut egui::Ui, modified: bool, color: egui::Color32) {
        let (label_width, extras) = row_widths(ui);
        let mut chosen: Option<String> = None;
        let mut command: Option<Command> = None;
        ui.horizontal(|ui| {
            let (with_label, width) = section_shape(ui.available_width(), label_width, extras);
            if with_label {
                ui.label("Configuration:").on_hover_text(
                    "The configuration in use: the engine, the model and the settings to run. \
                     Choose another, save what you have set under a name, or manage them.",
                );
            }
            self.show_configuration_combo(ui, width, modified, &mut chosen, &mut command);
            if modified {
                let name = self.active_entry().map_or_else(String::new, |e| e.name);
                self.show_what_a_change_adds(ui, color, &name, &mut command);
            }
        });

        if chosen.is_some() || command.is_some() {
            self.configs.in_window = false;
        }
        if let Some(id) = chosen {
            self.request_choose(&id);
        }
        match command {
            Some(Command::Save) => {
                let _ = self.save_active();
            }
            Some(Command::SaveAs) => {
                let suggestion = self.suggested_name();
                self.ask_for_a_name(NameFor::SaveAs, &suggestion);
            }
            Some(Command::Manage) => self.open_configurations(),
            Some(Command::Revert) => {
                self.configs.error = None;
                self.configs.confirm = Some(Confirm::Revert(self.configs.active.clone()));
            }
            None => {}
        }
    }

    /// The dropdown: the name of the configuration in use, and below it the configurations to choose from
    /// and the commands.
    fn show_configuration_combo(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        modified: bool,
        chosen: &mut Option<String>,
        command: &mut Option<Command>,
    ) {
        let all = self.configuration_entries();
        let active = self.configs.active.clone();
        let locked = self.engine_locked();
        let name = all
            .iter()
            .find(|e| e.id == active)
            .map_or("Default", |e| e.name.as_str());
        let engine_names: BTreeMap<String, String> = self
            .registry
            .engines()
            .map(|e| (e.id.clone(), e.doc.engine.name.clone()))
            .collect();

        // The dropdown is as wide as the room the section kept for it and no wider: a name that is longer is
        // cut (its end replaced by an ellipsis) and not let to push the badge and the buttons out of the row.
        let combo = ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add_enabled_ui(!locked, |ui| {
                    egui::ComboBox::from_id_salt("configuration_combo")
                        .selected_text(name)
                        .width(width)
                        .truncate()
                        .show_ui(ui, |ui| {
                            ui.set_min_width(330.0);
                            egui::ScrollArea::vertical()
                                .id_salt("configuration_dropdown_scroll")
                                .max_height(260.0)
                                .auto_shrink([true, true])
                                .show(ui, |ui| {
                                    for entry in &all {
                                        // one for an engine this version has not cannot be chosen, and says why
                                        let stopped = quickai_core::profile::family(
                                            self.registry.engine_map(),
                                            &entry.configuration.engine,
                                        )
                                        .is_empty();
                                        ui.horizontal(|ui| {
                                            let text = marked(
                                                ui,
                                                &entry.name,
                                                entry.id == active && modified,
                                            );
                                            let row = if stopped {
                                                let reason =
                                                    quickai_core::profile::notes::Note::new(
                                                        "unknown_engine",
                                                        &[(
                                                            "engine",
                                                            entry.configuration.engine.clone(),
                                                        )],
                                                    )
                                                    .text(self.registry.vocabulary());
                                                ui.add_enabled(
                                                    false,
                                                    egui::Button::selectable(false, text),
                                                )
                                                .on_disabled_hover_text(reason)
                                            } else {
                                                ui.selectable_label(entry.id == active, text)
                                            };
                                            if row.clicked() {
                                                *chosen = Some(entry.id.clone());
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(entry.origin.label())
                                                            .small()
                                                            .weak(),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new(
                                                            engine_names
                                                                .get(&entry.configuration.engine)
                                                                .cloned()
                                                                .unwrap_or_else(|| {
                                                                    entry
                                                                        .configuration
                                                                        .engine
                                                                        .clone()
                                                                }),
                                                        )
                                                        .small()
                                                        .weak(),
                                                    );
                                                },
                                            );
                                        });
                                    }
                                });
                            ui.separator();
                            if ui
                                .add_enabled(modified, egui::Button::new("Save"))
                                .on_hover_text("Save what you have set over this configuration.")
                                .clicked()
                            {
                                *command = Some(Command::Save);
                            }
                            if ui
                                .button("Save as...")
                                .on_hover_text(
                                    "Save what you have set as a new configuration, under a name.",
                                )
                                .clicked()
                            {
                                *command = Some(Command::SaveAs);
                            }
                            if ui.button("Manage configurations...").clicked() {
                                *command = Some(Command::Manage);
                            }
                        })
                })
            },
        );
        let shown = combo.inner;
        let name = name.to_string();
        if locked {
            shown.response.on_hover_text(self.engine_locked_sentence());
        } else if modified {
            shown
                .inner
                .response
                .on_hover_ui(|ui| self.show_changes(ui, &name));
        } else {
            shown.inner.response.on_hover_text(format!(
                "\"{name}\": the configuration in use, with the engine, the model and the settings to run. \
                 Choose another, save what you have set under a name, or manage them."
            ));
        }
    }

    /// What a change adds to the row: the badge that says *Modified* (what changed is on its hover) and the
    /// three things to do about it.
    fn show_what_a_change_adds(
        &mut self,
        ui: &mut egui::Ui,
        color: egui::Color32,
        name: &str,
        command: &mut Option<Command>,
    ) {
        ui.label(egui::RichText::new(badge_text()).color(color).strong())
            .on_hover_ui(|ui| self.show_changes(ui, name));
        if ui
            .button("Save")
            .on_hover_text("Save what you have set over this configuration.")
            .clicked()
        {
            *command = Some(Command::Save);
        }
        if ui
            .button("Save as...")
            .on_hover_text("Save what you have set as a new configuration, under a name.")
            .clicked()
        {
            *command = Some(Command::SaveAs);
        }
        let locked = self.engine_locked();
        if ui
            .add_enabled(!locked, egui::Button::new("Revert"))
            .on_hover_text(
                "Go back to what was saved: the engine, the model and the settings go back to it.",
            )
            .on_disabled_hover_text(self.engine_locked_sentence())
            .clicked()
        {
            *command = Some(Command::Revert);
        }
    }

    /// What was changed since the configuration in use was saved, a line for each (the first eight).
    fn show_changes(&self, ui: &mut egui::Ui, name: &str) {
        ui.label(format!("\"{name}\" changed since it was saved:"));
        for line in change_lines(&self.changes_since_saved()) {
            ui.label(line);
        }
    }

    /// A name for a new configuration that nothing has: the one in use with a number after it.
    pub(super) fn suggested_name(&self) -> String {
        let base = self
            .active_entry()
            .map_or_else(|| "My configuration".to_string(), |e| e.name);
        let names: BTreeSet<String> = self
            .configuration_entries()
            .into_iter()
            .map(|e| e.name.to_lowercase())
            .collect();
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|candidate| !names.contains(&candidate.to_lowercase()))
            .unwrap_or(base)
    }

    /// The question, the name box and the confirmation of a thing that cannot be undone: in the main
    /// window when they were asked in its section of the configuration, in the window of the
    /// configurations when they were asked there.
    pub(super) fn show_configuration_modals(&mut self, ctx: &egui::Context, in_window: bool) {
        if self.configs.in_window != in_window {
            return;
        }
        self.show_unsaved_question(ctx);
        self.show_name_box(ctx);
        self.show_configuration_confirm(ctx);
    }

    /// *Save, Don't save, Cancel*: asked when a choice would replace changes that were not saved.
    fn show_unsaved_question(&mut self, ctx: &egui::Context) {
        let Some(target) = self.configs.question.clone() else {
            return;
        };
        let current = self
            .active_entry()
            .map_or_else(|| "This configuration".to_string(), |e| e.name);
        let wanted = self
            .configuration_entries()
            .into_iter()
            .find(|e| e.id == target)
            .map_or(target.clone(), |e| e.name);
        let list: Vec<String> = self
            .changes_since_saved()
            .iter()
            .take(6)
            .map(Change::describe)
            .collect();
        let (mut save_it, mut dont, mut cancel) = (false, false, false);
        let modal =
            egui::Modal::new(egui::Id::new("unsaved_configuration_modal")).show(ctx, |ui| {
                ui.set_width(modal_width(ctx, 340.0));
                ui.label(
                    egui::RichText::new(format!("\"{current}\" has changes that are not saved"))
                        .strong(),
                );
                ui.add_space(6.0);
                if !list.is_empty() {
                    ui.label(egui::RichText::new(list.join("\n")).small());
                    ui.add_space(6.0);
                }
                ui.label(format!("Choosing \"{wanted}\" replaces them."));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Don't save").clicked() {
                        dont = true;
                    }
                    if ui.button("Save").clicked() {
                        save_it = true;
                    }
                });
            });
        if modal.should_close() {
            cancel = true;
        }
        if save_it {
            self.answer_unsaved(Answer::Save);
        } else if dont {
            self.answer_unsaved(Answer::DontSave);
        } else if cancel {
            self.answer_unsaved(Answer::Cancel);
        }
    }

    /// The answer to *Save, Don't save, Cancel*: Save keeps what was set over the configuration in use
    /// and then chooses the one that was asked for, Don't save chooses it at once, Cancel leaves
    /// everything as it was.
    pub(super) fn answer_unsaved(&mut self, answer: Answer) {
        let Some(target) = self.configs.question.take() else {
            return;
        };
        match answer {
            Answer::Cancel => {}
            Answer::DontSave => self.choose_configuration(&target),
            Answer::Save => {
                if self.save_active().is_ok() {
                    self.choose_configuration(&target);
                }
            }
        }
    }

    /// The box that asks for a name: for Save as, Duplicate and Rename.
    fn show_name_box(&mut self, ctx: &egui::Context) {
        let Some(open) = self.configs.name_box.clone() else {
            return;
        };
        let title = match &open.asked_for {
            NameFor::SaveAs => "Save as a new configuration",
            NameFor::Duplicate(_) => "Duplicate configuration",
            NameFor::Rename(_) => "Rename configuration",
        };
        let (mut submit, mut cancel) = (false, false);
        let mut text = open.text.clone();
        let modal = egui::Modal::new(egui::Id::new("configuration_name_modal")).show(ctx, |ui| {
            ui.set_width(modal_width(ctx, 340.0));
            ui.label(egui::RichText::new(title).strong());
            ui.add_space(6.0);
            ui.label("Name:");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id(egui::Id::new("configuration_name_edit"))
                    .desired_width(ui.available_width()),
            );
            if open.fresh {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            if let Some(why) = &open.error {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(why)
                        .small()
                        .color(super::theme::palette().bad),
                );
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if ui.button("Save").clicked() {
                    submit = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if let Some(live) = &mut self.configs.name_box {
            live.text = text;
            live.fresh = false;
        }
        if submit {
            self.submit_name();
        } else if cancel {
            self.configs.name_box = None;
        }
    }
}

/// What the hover of the badge says of what changed: a line for each of the first eight, and how many
/// more there are.
fn change_lines(all: &[Change]) -> Vec<String> {
    const SHOWN: usize = 8;
    let mut lines: Vec<String> = all.iter().take(SHOWN).map(Change::describe).collect();
    if all.len() > SHOWN {
        lines.push(format!("... and {} more", all.len() - SHOWN));
    }
    lines
}

/// The words of the badge that says a configuration has changes that are not saved.
fn badge_text() -> String {
    format!("{UNSAVED_MARK} Modified")
}

/// A name, and after it, when the configuration is modified, the word that says so in the colour of the
/// badge (a row of the dropdown: the badge itself is next to the closed dropdown).
fn marked(ui: &egui::Ui, name: &str, modified: bool) -> egui::WidgetText {
    if !modified {
        return name.into();
    }
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        name,
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            color: egui::Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    job.append(
        " (modified)",
        0.0,
        egui::TextFormat {
            font_id,
            color: modified_color(ui.visuals().dark_mode),
            ..Default::default()
        },
    );
    job.into()
}

/// The width of the label of the section with the space after it, and of what a change adds to the row (the
/// badge and the three buttons, with the spaces before and between them): measured, so that the room
/// that is kept for them is the room they take.
fn row_widths(ui: &egui::Ui) -> (f32, f32) {
    let space = ui.spacing().item_spacing.x;
    let padding = 2.0 * ui.spacing().button_padding.x;
    let width = |text: String, style: egui::TextStyle| {
        ui.painter()
            .layout_no_wrap(text, style.resolve(ui.style()), egui::Color32::WHITE)
            .size()
            .x
    };
    let label = width("Configuration:".to_string(), egui::TextStyle::Body) + space;
    let button = |text: &str| width(text.to_string(), egui::TextStyle::Button) + padding;
    let extras = space
        + width(badge_text(), egui::TextStyle::Body)
        + space
        + button("Save")
        + space
        + button("Save as...")
        + space
        + button("Revert");
    (label, extras)
}

/// How wide a question is: as wide as it likes, but never wider than the window it is asked in.
pub(super) fn modal_width(ctx: &egui::Context, wanted: f32) -> f32 {
    wanted.min(ctx.content_rect().width() - 40.0).max(240.0)
}

/// What the person answers when asked about changes that were not saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Answer {
    Save,
    DontSave,
    Cancel,
}

#[derive(Clone, Copy)]
enum Command {
    Save,
    SaveAs,
    Revert,
    Manage,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::super::Tab;
    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    fn app_in(dir: &Path, engine: &str) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        app_with(dir, settings)
    }

    /// An application on `settings` (the file as it was loaded), models in `dir/models`, and the
    /// configurations in `dir/config/profiles`: what `App::new` makes before its first frame, except for the
    /// first start's adoption, which a test asks for.
    fn app_with(dir: &Path, mut settings: Settings) -> App {
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

    fn file_in(dir: &Path, rel: &str, text: &str) {
        let path = dir.join("config").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn saved(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
    }

    fn vision_text() -> &'static str {
        "schema = 1\nkind = \"profile\"\nid = \"vision\"\nname = \"Vision\"\nengine = \"ollama\"\n\
         [[model]]\nollama = \"qwen3-vl:8b\"\n[values.ollama]\nctx_size = 16384\nport = 11600\n"
    }

    /// Every text the shapes of a frame paint.
    fn texts(output: &egui::FullOutput) -> Vec<String> {
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => into.push(t.galley.text().to_string()),
                _ => {}
            }
        }
        let mut into = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    /// What the page has under the section of the configuration, to see whether it moves.
    const UNDER: &str = "something under the section";

    /// One frame of the top of the page (the title row and the section of the configuration) and the
    /// questions it asks, in a window of the width of a person's first start.
    fn frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        frame_at(app, ctx, 760.0, events)
    }

    fn frame_at(
        app: &mut App,
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 560.0),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                // as the page draws it: in a column of the Main tab's width
                super::super::theme::column(ui, super::super::theme::PAGE_WIDTH, |ui| {
                    app.show_configuration_section(ui);
                    ui.label(UNDER);
                });
            });
            app.show_configuration_modals(ui.ctx(), false);
        });
        output.textures_delta.clear();
        output
    }

    fn settle(app: &mut App, ctx: &egui::Context) -> Vec<String> {
        let mut last = Vec::new();
        for _ in 0..4 {
            last = texts(&frame(app, ctx, vec![]));
        }
        last
    }

    /// The person clicks at a place: the pointer goes there, presses, lets go.
    fn click_at(app: &mut App, ctx: &egui::Context, at: egui::Pos2) -> Vec<String> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut last = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
            vec![],
            vec![],
        ] {
            last = texts(&frame(app, ctx, events));
        }
        last
    }

    fn rect_of(ctx: &egui::Context, app: &mut App, wanted: &str) -> Option<egui::Rect> {
        fn walk(shape: &egui::epaint::Shape, wanted: &str, found: &mut Option<egui::Rect>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => {
                    inner.iter().for_each(|s| walk(s, wanted, found))
                }
                egui::epaint::Shape::Text(t) if t.galley.text() == wanted => {
                    *found = Some(t.galley.rect.translate(t.pos.to_vec2()));
                }
                _ => {}
            }
        }
        let output = frame(app, ctx, vec![]);
        let mut found = None;
        for clipped in &output.shapes {
            walk(&clipped.shape, wanted, &mut found);
        }
        found
    }

    #[test]
    fn p3_a50_the_dropdown_of_the_section_shows_the_one_in_use_and_says_when_it_is_modified() {
        let dir = scratch("a50");
        file_in(&dir, "profiles/vision.toml", vision_text());
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let said = settle(&mut app, &ctx);
        // the mark of the badge is a character the fonts can draw (the filled circle is drawn as an empty box)
        let drawable =
            ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(14.0), UNSAVED_MARK));
        assert!(drawable, "the fonts have no {UNSAVED_MARK}");
        assert!(said.iter().any(|t| t == "Configuration:"), "{said:?}");
        assert!(said.iter().any(|t| t == "Default"), "{said:?}");
        for none in [badge_text().as_str(), "Save", "Save as...", "Revert"] {
            assert!(
                !said.iter().any(|t| t == none),
                "nothing is unsaved, and {none} is there: {said:?}"
            );
        }

        // a setting changed: the badge says it, and what to do about it is there; the hover says what
        app.values.set("port", 9000u16);
        let said = settle(&mut app, &ctx);
        for want in [badge_text().as_str(), "Save", "Save as...", "Revert"] {
            assert!(said.iter().any(|t| t == want), "{want} in {said:?}");
        }
        let list: Vec<String> = app
            .changes_since_saved()
            .iter()
            .map(Change::describe)
            .collect();
        assert_eq!(list, ["Port: 8080 to 9000"]);
        // the same value typed back, and they go
        app.values.set("port", 8080u16);
        let said = settle(&mut app, &ctx);
        assert!(!said.iter().any(|t| t == &badge_text()), "{said:?}");
        // a change of the theme is not a change of the run
        app.dark_mode = !app.dark_mode;
        assert!(!app.configuration_modified());

        // the dropdown, opened: every configuration with where it is from, and the commands
        let at = rect_of(&ctx, &mut app, "Default")
            .expect("the name")
            .center();
        let open = click_at(&mut app, &ctx, at);
        for want in [
            "Vision",
            "yours",
            "built in",
            "Ollama",
            "llama.cpp",
            "Save",
            "Save as...",
            "Manage configurations...",
        ] {
            assert!(open.iter().any(|t| t == want), "{want} in {open:?}");
        }
        // the Default is first
        let first = open.iter().position(|t| t == "Default").unwrap();
        let vision = open.iter().position(|t| t == "Vision").unwrap();
        assert!(first < vision, "{open:?}");
        // while the one in use has changes, its row says so in words
        let _ = click_at(&mut app, &ctx, egui::pos2(700.0, 500.0));
        app.values.set("port", 9000u16);
        let at = rect_of(&ctx, &mut app, "Default")
            .expect("the name")
            .center();
        let open = click_at(&mut app, &ctx, at);
        assert!(
            open.iter().any(|t| t == "Default (modified)"),
            "the row of the one in use says it: {open:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file with the name of an entry of the recommended list and not its size (a person's own copy, or a
    /// stand-in of the GUI suite's), where the models are.
    fn model_file_of_another_size(dir: &Path) {
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let file = std::fs::File::create(dir.join("models/Bonsai-1.7B-Q1_0.gguf")).unwrap();
        file.set_len(262_144_000).unwrap();
    }

    /// The window has this one model and nothing else (not the ones the repository keeps for a run from its tree), and
    /// has picked it: what a first start does with a models folder that holds one file.
    fn only_the_model_file(app: &mut App, dir: &Path) {
        app.bundled_models_dir = dir.join("bundled");
        app.models = app.scan_models();
        app.selected = 0;
    }

    #[test]
    fn p3_a69_the_first_start_keeps_what_the_person_had_set_and_not_what_the_window_picked() {
        // a person who never chose a model, whose window picked one: their Default has their settings and
        // no model, and nothing is unsaved
        let dir = scratch("a69");
        model_file_of_another_size(&dir);
        let settings = Settings {
            port: 9123,
            ..Settings::default()
        };
        let mut app = app_with(&dir, settings);
        only_the_model_file(&mut app, &dir);
        assert_eq!(app.models.len(), 1, "the window has a model to pick");
        app.adopt_working_state();
        app.reload_configurations();
        app.find_active_configuration();
        let written =
            std::fs::read_to_string(dir.join("config/profiles/default.toml")).expect("a Default");
        assert!(written.contains("port = 9123"), "{written}");
        assert!(!written.contains("[[model]]"), "{written}");
        assert!(
            !app.configuration_modified(),
            "{:?}",
            app.changes_since_saved()
        );
        let _ = std::fs::remove_dir_all(&dir);

        // a person who had chosen it: their Default names that file, as the window compares it (by the
        // file it is and its size, not by the entry of the list that has a file of that name)
        let dir = scratch("a69b");
        model_file_of_another_size(&dir);
        let settings = Settings {
            port: 9123,
            last_model_filename: Some("Bonsai-1.7B-Q1_0.gguf".into()),
            ..Settings::default()
        };
        let mut app = app_with(&dir, settings);
        only_the_model_file(&mut app, &dir);
        app.adopt_working_state();
        app.reload_configurations();
        app.find_active_configuration();
        let written =
            std::fs::read_to_string(dir.join("config/profiles/default.toml")).expect("a Default");
        assert!(
            written.contains("file = \"Bonsai-1.7B-Q1_0.gguf\""),
            "{written}"
        );
        assert!(written.contains("size = 262144000"), "{written}");
        assert!(!written.contains("catalog ="), "{written}");
        assert!(
            !app.configuration_modified(),
            "{:?}",
            app.changes_since_saved()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One frame of the top of the page in a window of this width.
    fn texts_at(app: &mut App, ctx: &egui::Context, width: f32) -> egui::FullOutput {
        frame_at(app, ctx, width, vec![])
    }

    /// The rectangles the texts of a frame are drawn in, by text.
    fn rects(output: &egui::FullOutput) -> std::collections::BTreeMap<String, egui::Rect> {
        fn walk(
            shape: &egui::epaint::Shape,
            into: &mut std::collections::BTreeMap<String, egui::Rect>,
        ) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => {
                    into.insert(
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    );
                }
                _ => {}
            }
        }
        let mut into = std::collections::BTreeMap::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    /// The frames (rectangles with an outline) of a frame of the window: where each is and the colour of its
    /// outline.
    fn frames(output: &egui::FullOutput) -> Vec<(egui::Rect, egui::Color32)> {
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<(egui::Rect, egui::Color32)>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Rect(r) if r.stroke.width > 0.0 => {
                    into.push((r.rect, r.stroke.color))
                }
                _ => {}
            }
        }
        let mut into = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    /// A frame of the top of the page, once it has settled in a window of this width.
    fn settled_at(app: &mut App, ctx: &egui::Context, width: f32) -> egui::FullOutput {
        let mut output = texts_at(app, ctx, width);
        for _ in 0..3 {
            output = texts_at(app, ctx, width);
        }
        output
    }

    #[test]
    fn p3_a70_the_menu_bar_the_toolbar_and_the_dock_keep_their_parts_apart_at_every_width_the_window_can_have(
    ) {
        let dir = scratch("a70");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        // every text the whole window paints, with where (a text that is drawn twice is here twice)
        let painted = |app: &mut App, width: f32| -> Vec<(String, egui::Rect)> {
            let mut output = None;
            for _ in 0..4 {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 560.0),
                    )),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| app.show_window(ui));
                out.textures_delta.clear();
                output = Some(out);
            }
            let mut into = Vec::new();
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
            for clipped in &output.unwrap().shapes {
                walk(&clipped.shape, &mut into);
            }
            into
        };
        for live in [false, true] {
            app.enable_traffic_proxy = live;
            // from the least the window can be (main.rs) to a wide screen
            for width in [640.0, 680.0, 760.0, 1000.0, 1400.0] {
                let found = painted(&mut app, width);
                // the first rect of a text in a band of the window
                let at = |text: &str, top: f32, bottom: f32| {
                    found
                        .iter()
                        .find(|(t, r)| t == text && r.center().y > top && r.center().y < bottom)
                        .map(|(_, r)| *r)
                        .unwrap_or_else(|| panic!("{text} at {width} {live}: {found:?}"))
                };
                // the menus, in their order along the first row, inside the window
                let menus: Vec<egui::Rect> = ["File", "Server", "Models", "View", "Help"]
                    .iter()
                    .map(|m| at(m, 0.0, 24.0))
                    .collect();
                assert!(
                    menus
                        .windows(2)
                        .all(|pair| pair[0].right() < pair[1].left()),
                    "{width} {live}: {menus:?}"
                );
                // the toolbar: the buttons in their order, none over another, and Settings whole at the right end
                let tools: Vec<egui::Rect> = [
                    "Start",
                    "Stop",
                    "Open Chat UI",
                    "Connect an app",
                    "Settings",
                ]
                .iter()
                .map(|t| at(t, 24.0, 70.0))
                .collect();
                assert!(
                    tools
                        .windows(2)
                        .all(|pair| pair[0].right() < pair[1].left()),
                    "{width} {live}: {tools:?}"
                );
                assert!(tools[4].right() <= width, "{width} {live}: {:?}", tools[4]);
                // the dock's tabs, the log first, and the live traffic's only when it is on
                let log = at("Log", 70.0, 540.0);
                let tab = found.iter().find(|(t, r)| {
                    t == "Live traffic" && (r.center().y - log.center().y).abs() < 4.0
                });
                assert_eq!(tab.is_some(), live, "{width}: {found:?}");
                if let Some((_, tab)) = tab {
                    assert!(log.right() < tab.left(), "{width}: {log:?} {tab:?}");
                }
                // and the status bar under it all
                let state = at("Stopped", 530.0, 560.0);
                assert!(
                    state.top() > log.bottom(),
                    "{width} {live}: {state:?} {log:?}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a72_the_configuration_is_a_section_at_the_top_of_the_page_whole_at_every_width() {
        let dir = scratch("a72");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        for live in [false, true] {
            app.enable_traffic_proxy = live;
            // the worst case is the widest section: the one with a change in it
            app.values.set("port", 9000u16);
            for width in [520.0, 540.0, 560.0, 600.0, 680.0, 760.0, 1000.0, 1400.0] {
                let output = settled_at(&mut app, &ctx, width);
                let found = rects(&output);
                let at = |text: &str| {
                    *found
                        .get(text)
                        .unwrap_or_else(|| panic!("{text} at {width}: {:?}", found.keys()))
                };
                let (label, name) = (at("Configuration:"), at("Default"));
                let (badge, save, save_as, revert) = (
                    at(&badge_text()),
                    at("Save"),
                    at("Save as..."),
                    at("Revert"),
                );
                // as wide as the page: one outline, from edge to edge
                let section = frames(&output)
                    .into_iter()
                    .filter(|(rect, _)| rect.top() < label.top() && rect.bottom() > label.bottom())
                    .map(|(rect, _)| rect)
                    .next()
                    .unwrap_or_else(|| panic!("{width}: no frame round the section"));
                // (the page is as wide as the window leaves it, and no wider than a column of the Main tab's)
                assert!(
                    (section.width() - (width - 16.0).min(super::super::theme::PAGE_WIDTH)).abs()
                        < 2.0,
                    "{width}: the section is {} wide",
                    section.width()
                );
                // the parts are in their order along the row and none is drawn over another: the name
                // and the badge are far enough apart for the arrow of the dropdown to be between them
                assert!(label.right() <= name.left(), "{width}: {label:?} {name:?}");
                assert!(
                    badge.left() >= name.left() + COMBO_LEAST - 8.0,
                    "{width}: {name:?} {badge:?}"
                );
                assert!(badge.right() <= save.left(), "{width}: {badge:?} {save:?}");
                assert!(
                    save.right() <= save_as.left(),
                    "{width}: {save:?} {save_as:?}"
                );
                assert!(
                    save_as.right() <= revert.left(),
                    "{width}: {save_as:?} {revert:?}"
                );
                // and the row ends inside the outline, whole
                assert!(
                    revert.right() < section.right(),
                    "{width} {live}: {revert:?} is not inside {section:?}"
                );
                // with room the label is there, and the window's least is room enough
                assert!(label.width() > 0.0, "{width}: no label");
            }
        }
        // the shape: the label and the full width, then less width, then no label
        let (label, extras) = (100.0, 270.0);
        assert_eq!(
            section_shape(label + extras + COMBO_WIDTH + 50.0, label, extras),
            (true, COMBO_WIDTH)
        );
        assert_eq!(
            section_shape(label + extras + 130.0, label, extras),
            (true, 130.0)
        );
        assert_eq!(section_shape(extras + 120.0, label, extras), (false, 120.0));
        assert_eq!(section_shape(10.0, label, extras), (false, COMBO_LEAST));
        // a name that is longer than the dropdown is cut at its width, and the badge and the buttons stay whole in the row
        // (the dropdown grew to the length of the name, and at the narrowest window pushed them out of it)
        let long = "Qwen3-VL 8B on Ollama with Open WebUI beside it for work!";
        file_in(
            &dir,
            "profiles/long.toml",
            &format!("schema = 1\nkind = \"profile\"\nid = \"long\"\nname = \"{long}\"\nengine = \"llamacpp\"\n"),
        );
        app.reload_configurations();
        app.choose_configuration("long");
        assert_eq!(app.configs.active, "long", "{:?}", app.configs.error);
        app.values.set("port", 9000u16);
        for width in [520.0, 600.0, 760.0, 1400.0] {
            let output = settled_at(&mut app, &ctx, width);
            let found = rects(&output);
            let at = |text: &str| {
                *found
                    .get(text)
                    .unwrap_or_else(|| panic!("{text} at {width}: {:?}", found.keys()))
            };
            let (name, badge, revert) = (at(long), at(&badge_text()), at("Revert"));
            let section = frames(&output)
                .into_iter()
                .map(|(rect, _)| rect)
                .find(|rect| rect.top() < name.top() && rect.bottom() > name.bottom())
                .unwrap_or_else(|| panic!("{width}: no frame round the section"));
            assert!(
                name.right() < badge.left(),
                "{width}: the name {name:?} runs into the badge {badge:?}"
            );
            assert!(
                revert.right() < section.right(),
                "{width}: {revert:?} is not inside {section:?}"
            );
            assert!(
                name.width() <= COMBO_WIDTH,
                "{width}: the name is {} wide, and the dropdown is at most {COMBO_WIDTH}",
                name.width()
            );
        }
        app.values.set("port", 8080u16);
        app.choose_configuration("default");
        app.values.set("port", 9000u16);
        // the live traffic is a tab of the dock at the foot of the window: with it open the section is where it was
        // (choosing a configuration sets the proxy as it says)
        app.enable_traffic_proxy = true;
        app.active_tab = Tab::Live;
        let found = rects(&settled_at(&mut app, &ctx, 760.0));
        assert!(
            found.contains_key("Configuration:") && found.contains_key(UNDER),
            "{:?}",
            found.keys()
        );
        app.active_tab = Tab::Main;
        let found = rects(&settled_at(&mut app, &ctx, 760.0));
        assert!(found.contains_key("Configuration:") && found.contains_key(UNDER));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a73_a_change_is_said_in_words_in_a_colour_of_its_own_and_nothing_moves() {
        let dir = scratch("a73");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let amber = modified_color(true);
        for width in [520.0, 760.0, 1100.0] {
            app.values.set("port", 8080u16);
            let quiet = settled_at(&mut app, &ctx, width);
            let (said, outlines) = (rects(&quiet), frames(&quiet));
            assert!(
                !said.contains_key(&badge_text()),
                "{width}: {:?}",
                said.keys()
            );
            assert!(
                !outlines.iter().any(|(_, color)| *color == amber),
                "{width}: the section is in the colour of a change with none made"
            );

            // a change: the words, the colour, and what to do about it
            app.values.set("port", 9000u16);
            let loud = settled_at(&mut app, &ctx, width);
            let (changed, loud_outlines) = (rects(&loud), frames(&loud));
            for want in [badge_text().as_str(), "Save", "Save as...", "Revert"] {
                assert!(changed.contains_key(want), "{width}: {want}");
            }
            let section = loud_outlines
                .iter()
                .find(|(_, color)| *color == amber)
                .unwrap_or_else(|| panic!("{width}: no outline in the colour of a change"))
                .0;
            // nothing moves: the label, the dropdown and what the page has under the section are where they
            // were, and so is the section (its outline is the same rectangle)
            for text in ["Configuration:", "Default", UNDER] {
                assert_eq!(
                    said[text], changed[text],
                    "{width}: {text} moved when a change was made"
                );
            }
            let was = outlines
                .iter()
                .map(|(rect, _)| *rect)
                .find(|rect| {
                    rect.top() < said["Configuration:"].top()
                        && rect.bottom() > said["Configuration:"].bottom()
                })
                .expect("the outline of the section");
            assert_eq!(was, section, "{width}: the section changed its size");

            // typed back, and it is as it was: nothing of it is left
            app.values.set("port", 8080u16);
            let back = settled_at(&mut app, &ctx, width);
            assert_eq!(rects(&back), said, "{width}");
            assert!(!frames(&back).iter().any(|(_, color)| *color == amber));
        }
        // the light theme has a colour of its own for it (the other is hard to read on white)
        assert_ne!(modified_color(true), modified_color(false));

        // what the hover says: what changed, a line for each, and how many more than eight
        app.values.set("port", 9000u16);
        app.values.set("ctx_size", 8192u32);
        let lines = change_lines(&app.changes_since_saved());
        assert_eq!(
            lines,
            ["Port: 8080 to 9000", "Context size: 4096 to 8192"],
            "{lines:?}"
        );
        let many: Vec<Change> = (0..11)
            .map(|n| Change::Proxy {
                from: n % 2 == 0,
                to: n % 2 == 1,
            })
            .collect();
        let lines = change_lines(&many);
        assert_eq!(lines.len(), 9, "{lines:?}");
        assert_eq!(lines[8], "... and 3 more");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a74_the_buttons_of_a_modified_configuration_save_save_as_and_revert() {
        let dir = scratch("a74");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        settle(&mut app, &ctx);

        // Revert asks first, in the main window
        app.values.set("port", 9000u16);
        settle(&mut app, &ctx);
        let at = rect_of(&ctx, &mut app, "Revert").expect("Revert").center();
        let asked = click_at(&mut app, &ctx, at);
        assert!(
            asked.iter().any(|t| t == "Go back to what was saved?"),
            "{asked:?}"
        );
        assert_eq!(app.configs.confirm, Some(Confirm::Revert("default".into())));
        assert_eq!(app.values.int("port"), Some(9000), "nothing is lost yet");
        // Cancel leaves everything as it was
        let at = rect_of(&ctx, &mut app, "Cancel").expect("Cancel").center();
        let said = click_at(&mut app, &ctx, at);
        assert!(app.configs.confirm.is_none());
        assert!(!said.iter().any(|t| t == "Go back to what was saved?"));
        assert!(app.configuration_modified());
        assert_eq!(app.values.int("port"), Some(9000));
        // and Revert, answered, goes back
        let at = rect_of(&ctx, &mut app, "Revert").expect("Revert").center();
        click_at(&mut app, &ctx, at);
        assert!(app.configs.confirm.is_some());
        let at = rect_of(&ctx, &mut app, "Revert")
            .expect("the answer")
            .center();
        let said = click_at(&mut app, &ctx, at);
        assert_eq!(app.values.int("port"), Some(8080), "back to what was saved");
        assert!(!app.configuration_modified());
        assert!(!said.iter().any(|t| t == &badge_text()), "{said:?}");

        // Save writes over the one in use, and the badge goes
        app.values.set("port", 9000u16);
        settle(&mut app, &ctx);
        let at = rect_of(&ctx, &mut app, "Save").expect("Save").center();
        let said = click_at(&mut app, &ctx, at);
        let kept = std::fs::read_to_string(dir.join("config/profiles/default.toml"))
            .expect("the Default of the person, written");
        assert!(kept.contains("port = 9000"), "{kept}");
        assert!(!app.configuration_modified());
        assert!(!said.iter().any(|t| t == &badge_text()), "{said:?}");

        // Save as asks for a name, in the main window, and the new one is the one in use
        app.values.set("port", 9100u16);
        settle(&mut app, &ctx);
        let at = rect_of(&ctx, &mut app, "Save as...")
            .expect("Save as")
            .center();
        let asked = click_at(&mut app, &ctx, at);
        assert!(
            asked.iter().any(|t| t == "Save as a new configuration"),
            "{asked:?}"
        );
        app.configs.name_box.as_mut().expect("the box").text = "Studio".into();
        app.submit_name();
        assert_eq!(app.configs.active, "studio");
        let kept = std::fs::read_to_string(dir.join("config/profiles/studio.toml")).unwrap();
        assert!(kept.contains("port = 9100"), "{kept}");
        let said = settle(&mut app, &ctx);
        assert!(said.iter().any(|t| t == "Studio"), "{said:?}");
        assert!(!said.iter().any(|t| t == &badge_text()), "{said:?}");

        // while something is fetched, Revert is grey (the sentence of the Engine row is on its hover) and
        // asks nothing; what went wrong is said in the section
        app.values.set("port", 9200u16);
        app.download = Some(crate::jobs::Job::spawn(
            "x".into(),
            "Downloading",
            || {},
            |_| std::thread::sleep(std::time::Duration::from_millis(300)),
        ));
        settle(&mut app, &ctx);
        let at = rect_of(&ctx, &mut app, "Revert").expect("Revert").center();
        click_at(&mut app, &ctx, at);
        assert!(app.configs.confirm.is_none(), "a grey button asks nothing");
        app.request_choose("default");
        let said = settle(&mut app, &ctx);
        assert!(
            said.iter()
                .any(|t| t == "Wait for what is being fetched to finish to change this."),
            "{said:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A build the window has (a label, and where its program is).
    fn a_build(label: &str) -> crate::prims::install::InstalledBuild {
        crate::prims::install::InstalledBuild {
            label: label.into(),
            binary_path: PathBuf::from(format!("/data/ollama/{label}/bin/ollama")),
            install_dir: PathBuf::from(format!("/data/ollama/{label}")),
            size_bytes: 1,
        }
    }

    /// The builds `app` finds installed, for the next few seconds.
    fn installed_are(app: &App, labels: &[&str]) {
        let found: Installed = [(
            "ollama".to_string(),
            labels.iter().map(|l| a_build(l)).collect(),
        )]
        .into();
        *app.configs.installed.borrow_mut() = Some((std::time::Instant::now(), found));
    }

    #[test]
    fn p3_a75_a_configuration_just_chosen_is_not_modified_for_what_this_computer_lacks() {
        let dir = scratch("a75");
        // it names a model and a build that are not here, and says a value that is the default
        // (a file written by hand, or by another version, can: the port of Open WebUI is 3000)
        file_in(
            &dir,
            "profiles/lacking.toml",
            "schema = 1\nkind = \"profile\"\nid = \"lacking\"\nname = \"Lacking\"\nengine = \"ollama\"\n\
             [runtime]\nbuild = \"v9.9.9\"\n[[model]]\nollama = \"qwen3-vl:8b\"\n\
             [values.ollama]\nctx_size = 16384\n[values.openwebui]\nenabled = true\nport = 3000\n",
        );
        let mut app = app_in(&dir, "llamacpp");
        installed_are(&app, &[]);
        app.request_choose("lacking");
        assert_eq!(app.configs.error, None);
        assert_eq!(app.configs.active, "lacking");
        assert!(
            !app.configuration_modified(),
            "a configuration that was chosen is the one in use as it is: {:?}",
            app.changes_since_saved()
        );
        let said = settle(&mut app, &egui::Context::default());
        assert!(!said.iter().any(|t| t == &badge_text()), "{said:?}");

        // a change of the person's is a change
        app.values.set("ctx_size", 8192u32);
        assert!(app.configuration_modified());
        assert_eq!(
            app.changes_since_saved()
                .iter()
                .map(Change::describe)
                .collect::<Vec<_>>(),
            ["Context size: 16384 to 8192"]
        );
        app.values.set("ctx_size", 16384u32);
        assert!(!app.configuration_modified());

        // a build the file names and that is installed is one the window can hold: another program in
        // the window is a change, and the one the file names is not
        let field = crate::configurations::runtime_field(app.registry.engine("ollama").unwrap())
            .expect("the program of Ollama")
            .to_string();
        installed_are(&app, &["v9.9.9", "v1.0"]);
        app.values
            .set(&field, a_build("v1.0").binary_path.display().to_string());
        assert!(app.configuration_modified());
        assert_eq!(
            app.changes_since_saved()
                .iter()
                .map(Change::describe)
                .collect::<Vec<_>>(),
            ["Program: v9.9.9 to v1.0"]
        );
        app.values
            .set(&field, a_build("v9.9.9").binary_path.display().to_string());
        assert!(!app.configuration_modified());
        // the file names a build that is not installed, and the window runs another: it cannot hold it
        installed_are(&app, &["v1.0"]);
        app.values
            .set(&field, a_build("v1.0").binary_path.display().to_string());
        assert!(
            !app.configuration_modified(),
            "{:?}",
            app.changes_since_saved()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a51_choosing_applies_the_state_writes_the_file_and_starts_nothing() {
        let dir = scratch("a51");
        file_in(&dir, "profiles/vision.toml", vision_text());
        file_in(
            &dir,
            "profiles/alien.toml",
            "schema = 1\nkind = \"profile\"\nid = \"alien\"\nname = \"Alien\"\nengine = \"whispercpp\"\n",
        );
        let mut app = app_in(&dir, "llamacpp");
        let before = tree(&dir);
        assert_eq!(app.engine, "llamacpp");

        // nothing unsaved: it is chosen at once
        app.request_choose("vision");
        assert_eq!(app.engine, "ollama");
        assert_eq!(app.configs.active, "vision");
        assert_eq!(app.values.int("port"), Some(11600));
        assert_eq!(app.values.int("ctx_size"), Some(16384));
        let file = saved(&dir);
        assert_eq!(file["values"]["app.engine"], "ollama");
        assert_eq!(file["values"]["app.configuration"], "vision");
        assert_eq!(file["values"]["ollama.ctx_size"], 16384);
        assert_eq!(
            file["port"], 8080,
            "llama.cpp's own settings are where they were"
        );
        assert!(app
            .log_lines
            .iter()
            .any(|l| l == "[quickai] Configuration \"Vision\" is in use."));
        // and nothing was started, fetched or installed
        assert!(!app.session.is_active());
        assert!(
            app.download.is_none() && app.runtime.install.is_none() && app.named.pull.is_none()
        );
        let after = tree(&dir);
        let new_files: Vec<&String> = after.iter().filter(|f| !before.contains(f)).collect();
        assert!(
            new_files.iter().all(|f| f.ends_with("settings.json")
                || f.contains("models")
                || f.contains("data")),
            "{new_files:?}"
        );
        assert!(
            !app.configuration_modified(),
            "what was chosen is what was saved"
        );

        // a change that is not saved asks first, and nothing moves until it is answered
        app.values.set("ctx_size", 2048u32);
        assert!(app.configuration_modified());
        app.request_choose("default");
        assert_eq!(app.configs.question.as_deref(), Some("default"));
        assert_eq!(app.engine, "ollama");
        assert_eq!(saved(&dir)["values"]["app.configuration"], "vision");
        // Cancel: as it was
        app.answer_unsaved(Answer::Cancel);
        assert!(
            app.configs.question.is_none()
                && app.engine == "ollama"
                && app.configs.active == "vision"
        );
        assert_eq!(app.values.int("ctx_size"), Some(2048));
        // Don't save: the change is lost, the Default is in use, and the file for Vision is as it was
        let vision_before =
            std::fs::read_to_string(dir.join("config/profiles/vision.toml")).unwrap();
        app.request_choose("default");
        app.answer_unsaved(Answer::DontSave);
        // the Default is the engine in use at its defaults (WP4.26): Ollama stays, and what Vision set goes
        assert_eq!(
            (app.engine.as_str(), app.configs.active.as_str()),
            ("ollama", "default")
        );
        assert_eq!(app.values.int("ctx_size"), None, "Ollama's own default");
        assert!(!app.configuration_modified());
        let file = saved(&dir);
        assert!(
            file["values"].get("app.configuration").is_none(),
            "the Default is what a file with no choice means"
        );
        assert_eq!(file["values"]["app.engine"], "ollama");
        assert_eq!(
            std::fs::read_to_string(dir.join("config/profiles/vision.toml")).unwrap(),
            vision_before
        );
        // Save: kept over the one in use, and then the other is chosen
        app.request_choose("vision");
        app.values.set("ctx_size", 4096u32);
        assert!(app.configuration_modified());
        app.request_choose("default");
        app.answer_unsaved(Answer::Save);
        assert_eq!(app.configs.active, "default");
        let kept = std::fs::read_to_string(dir.join("config/profiles/vision.toml")).unwrap();
        assert!(kept.contains("ctx_size = 4096"), "{kept}");
        assert!(kept.contains("name = \"Vision\""), "the name stays: {kept}");

        // a configuration this version cannot use says why and changes nothing
        let settings_before = saved(&dir);
        app.request_choose("alien");
        assert_eq!(
            app.configs.error.as_deref(),
            Some("It is for whispercpp, which this QuickAI does not have.")
        );
        assert_eq!(app.configs.active, "default");
        assert_eq!(saved(&dir), settings_before);

        // while something is fetched, the choice is refused with the Engine row's sentence
        app.download = Some(crate::jobs::Job::spawn(
            "x".into(),
            "Downloading",
            || {},
            |_| std::thread::sleep(std::time::Duration::from_millis(300)),
        ));
        app.request_choose("vision");
        assert_eq!(
            app.configs.error.as_deref(),
            Some("Wait for what is being fetched to finish to change this.")
        );
        assert_eq!(app.configs.active, "default");
        assert_eq!(app.engine, "ollama", "the engine the Default was chosen in");
        assert_eq!(saved(&dir), settings_before);
        assert!(app.download.is_some(), "the fetch goes on");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tree(dir: &Path) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    out.push(path.strip_prefix(base).unwrap().display().to_string());
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn p3_a52_save_writes_the_one_in_use_and_save_as_asks_a_name_and_makes_the_new_one_active() {
        let dir = scratch("a52");
        let mut app = app_in(&dir, "llamacpp");
        let profiles: PathBuf = dir.join("config/profiles");
        // Save over the Default: the person's own stands in for the built-in one
        app.values.set("port", 9100u16);
        assert!(app.configuration_modified());
        app.save_active().unwrap();
        assert!(!app.configuration_modified());
        let text = std::fs::read_to_string(profiles.join("default.toml")).unwrap();
        assert!(
            text.contains("replace = true") && text.contains("port = 9100"),
            "{text}"
        );
        assert_eq!(
            app.configuration_entries()[0].origin,
            crate::configurations::Origin::Yours
        );

        // Save as: the box asks, and says why a name cannot be used, in the box
        app.values.set("port", 9200u16);
        app.ask_for_a_name(NameFor::SaveAs, "Default 2");
        assert!(app.configs.name_box.is_some());
        for (typed, says) in [
            ("   ", "A configuration needs a name."),
            (
                "default",
                "There is already a configuration called default.",
            ),
            (
                "DEFAULT",
                "There is already a configuration called DEFAULT.",
            ),
        ] {
            app.configs.name_box.as_mut().unwrap().text = typed.into();
            app.submit_name();
            assert_eq!(
                app.configs.name_box.as_ref().unwrap().error.as_deref(),
                Some(says)
            );
        }
        app.configs.name_box.as_mut().unwrap().text = "Fast chat".into();
        app.submit_name();
        assert!(app.configs.name_box.is_none());
        assert_eq!(app.configs.active, "fast-chat");
        assert!(
            !app.configuration_modified(),
            "the new one is what the window holds"
        );
        assert_eq!(saved(&dir)["values"]["app.configuration"], "fast-chat");
        // the one left behind is as it was saved
        let left = std::fs::read_to_string(profiles.join("default.toml")).unwrap();
        assert!(
            left.contains("port = 9100") && !left.contains("9200"),
            "{left}"
        );
        assert!(std::fs::read_to_string(profiles.join("fast-chat.toml"))
            .unwrap()
            .contains("port = 9200"));
        // the suggestion for the next name is one nothing has
        assert_eq!(app.suggested_name(), "Fast chat 2");

        // the Default can be put back to factory: the window then has changes the Default does not
        app.request_choose("default");
        assert_eq!(app.configs.active, "default");
        crate::configurations::reset_to_factory(&app.config_dirs, &app.registry).unwrap();
        app.reload_configurations();
        assert!(
            app.configuration_modified(),
            "port 9100 is not the factory Default"
        );
        app.values.set("port", 8080u16);
        assert!(!app.configuration_modified());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a68_choosing_never_stops_starts_or_installs_anything() {
        let dir = scratch("a68");
        file_in(&dir, "profiles/vision.toml", vision_text());
        let mut app = app_in(&dir, "llamacpp");
        assert!(!app.session.is_active());
        for round in ["vision", "default", "vision"] {
            app.request_choose(round);
            assert!(!app.session.is_active(), "{round}");
            assert!(
                app.download.is_none()
                    && app.runtime.install.is_none()
                    && app.runtime.delete.is_none(),
                "{round}"
            );
            assert!(
                app.named.pull.is_none() && app.companion_running_now().is_none(),
                "{round}"
            );
        }
        // the only things written are the settings and, in the data and models folders, what loading an
        // engine's state makes; no build, no model and no download
        let files = tree(&dir);
        assert!(
            !files.iter().any(|f| f.ends_with(".gguf")
                || f.contains("ollama/bin")
                || f.contains(".partial")),
            "{files:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
