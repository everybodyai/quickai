use std::collections::VecDeque;
use std::path::PathBuf;

use eframe::egui;

use crate::config;
use crate::events::{ServerState, TrafficEvent};
use crate::gpu::{self, GpuVendor};
use crate::hardware;
use crate::jobs::Job;
use crate::launch::MachineFacts;
use crate::models::{discover_all, ModelEntry};
use crate::paths;
use crate::runtime::Session;
use crate::settings::Settings;
use engine::NamedModels;
use lookup_view::LookupView;
use quickai_core::form::{self, FormModel};
use quickai_core::plan::{HostFacts, ModelInfo, Selection};
use quickai_core::registry::Registry;
use quickai_core::value::Value;
use runtime_manager::RuntimeManager;
use values::FieldValues;

mod about_window;
mod acquire;
mod api_info;
mod catalog_view;
mod confirm_delete;
mod controls;
mod diagnostics;
mod engine;
mod foreign_program;
mod header;
mod live_traffic;
mod log_pane;
mod lookup_view;
mod model_bar;
mod named_models;
mod run_bar;
mod runtime_manager;
mod settings_window;
mod values;
mod widgets;

/// The widgets a document may name, for the loader (`prims::known`).
pub(crate) use widgets::NAMED as NAMED_WIDGETS;

const MAX_LOG_LINES: usize = 2000;

/// What the log pane starts with: the problems found in the configuration, if any (the notes that
/// need no action are left out). A clean start shows nothing, as it always has.
fn config_log_lines(registry: &Registry) -> VecDeque<String> {
    registry
        .diagnostics
        .iter()
        .filter(|d| d.severity != quickai_core::diag::Severity::Info)
        .flat_map(|d| {
            format!("config: {d}")
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

pub struct App {
    session: Session,
    /// The loaded documents: what engines exist and how they run.
    registry: Registry,
    /// The application's own documents, without the person's: what the engine's own strategies find is
    /// judged against these (see `foreign_program.rs`).
    embedded: Registry,
    /// A program the person has not yet said may run, which Start is waiting on.
    pending_foreign: Option<quickai_core::guard::Foreign>,
    /// The engine in use: the one the settings name, or llama.cpp when there is no document for it.
    engine: String,
    /// What the window holds for an engine whose models the daemon lists (Ollama).
    named: NamedModels,
    /// What is known about this machine, for planning a run.
    machine: MachineFacts,
    /// What the file held when it was loaded: the keys this version does not know are saved back.
    stored: Settings,
    /// Where settings are written; `None` is where `settings.json` is kept (a test names a file of
    /// its own).
    settings_path: Option<PathBuf>,
    /// The values of the engine's fields (`llamacpp.port`, ...): what the controls show and a run uses.
    values: FieldValues,
    /// The machine, as planning and the form see it; the parts that change are set at each use.
    host_facts: HostFacts,
    /// Where the application keeps its data (`~/.quickai`), when a test says another place.
    data_dir_override: Option<String>,
    models: Vec<ModelEntry>,
    selected: usize,
    log_lines: VecDeque<String>,
    bundled_models_dir: PathBuf,
    models_dir: PathBuf,
    /// The engine's executable, wherever its document says it is (or the user chose).
    binary: PathBuf,
    missing_binary_warned: bool,
    /// The run that is going or went reached Running (so a crash was not at the start).
    run_reached_running: bool,
    /// Scratch text-edit buffer for the combined URL/path/search box — not
    /// persisted. What it means depends on its shape: a `http(s)://` URL is
    /// downloaded, an existing local file path is imported, anything else
    /// is treated as a Hugging Face search term (see the "Go" button's
    /// dynamic label right where it's used).
    model_input: String,
    download: Option<Job<String>>,
    /// What the look-up of models online has said, for whichever engine is in use.
    lookup: LookupView,
    /// The window of the recommended models: what it shows and which of them are there already.
    catalog: catalog_view::CatalogView,
    total_ram_bytes: u64,
    recommended_filename: Option<String>,
    dark_mode: bool,
    show_api_info: bool,
    show_settings: bool,
    show_about: bool,
    /// `None` unless the user picked a custom folder in ⚙ Settings; see
    /// `Settings::models_dir_override` for why this isn't always Some.
    models_dir_override: Option<PathBuf>,
    /// Scratch text-edit buffer for the ⚙ Settings window, separate from the `PathBuf` above so a
    /// half-typed path doesn't take effect until "Apply" is clicked.
    models_dir_input: String,
    settings_error: Option<String>,
    /// Off by default — see `Settings::enable_traffic_proxy`.
    enable_traffic_proxy: bool,
    active_tab: Tab,
    traffic: VecDeque<TrafficTurn>,
    /// `None` shows every session at once; `Some(session_key)` filters the
    /// 📡 Live tab down to just that client — see `TrafficTurn::session`.
    selected_session: Option<String>,
    /// GPU vendor(s) this machine looks like it has — see `gpu::detect`.
    /// Best-effort and computed once; only used to warn about a vendor-
    /// locked llama.cpp build (CUDA/ROCm) that won't match the hardware.
    gpu_vendors: Vec<GpuVendor>,
    /// The binary box, the installed builds and the look-up of new ones (⚙ Settings → Advanced).
    runtime: RuntimeManager,
    /// A delete awaiting user confirmation — see `PendingDelete` and
    /// `show_confirm_delete_modal`.
    pending_delete: Option<PendingDelete>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Main,
    Live,
}

/// One request/response pair shown in the 📡 Live tab. `response` fills
/// in as it streams; `done`/`error` mark when there's nothing more coming.
struct TrafficTurn {
    id: u64,
    /// Which client this came from — see `TrafficEvent::Request`'s doc.
    session: String,
    method: String,
    path: String,
    prompt: Option<String>,
    /// Best-effort description of any documents/images sent alongside the
    /// prompt — see `TrafficEvent::Request::attachments`.
    attachments: Vec<String>,
    response: String,
    done: bool,
    error: Option<String>,
}

const MAX_TRAFFIC_TURNS: usize = 200;

/// A destructive delete waiting on user confirmation via a modal — see
/// `show_confirm_delete_modal`. Holding the info needed to both describe
/// and perform the delete means the modal doesn't need to re-derive it
/// (or risk acting on a stale index) once the user confirms.
enum PendingDelete {
    Model {
        filename: String,
        path: PathBuf,
    },
    Build {
        label: String,
        install_dir: PathBuf,
    },
    /// A model an engine keeps and lists by name, removed by asking the engine.
    Listed {
        name: String,
    },
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        let registry = config::build_registry(&config::ConfigDirs::from_environment());
        cc.egui_ctx.set_visuals(if settings.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        Self::build(
            settings,
            registry,
            hardware::total_ram_bytes(),
            gpu::detect(),
            None,
        )
    }

    /// The application's state for these settings and documents. `settings_path` is where they are
    /// written (`None`: where `settings.json` is kept); the machine is what the caller found out
    /// about it.
    pub(crate) fn build(
        settings: Settings,
        registry: Registry,
        total_ram_bytes: u64,
        gpu_vendors: Vec<GpuVendor>,
        settings_path: Option<PathBuf>,
    ) -> Self {
        let mut log_lines = config_log_lines(&registry);
        let (engine, note) = config::engine_in_use(&registry, &settings);
        log_lines.extend(note);
        let values = FieldValues::from_settings(&settings, registry.engine(&engine));
        let machine = MachineFacts::detect(total_ram_bytes, &gpu_vendors);

        let mut app = Self {
            session: Session::new(),
            embedded: config::build_registry(&config::ConfigDirs::default()),
            pending_foreign: None,
            registry,
            engine,
            named: NamedModels::default(),
            host_facts: HostFacts::default(),
            data_dir_override: None,
            machine,
            values,
            dark_mode: settings.dark_mode,
            enable_traffic_proxy: settings.enable_traffic_proxy,
            stored: settings,
            settings_path,
            models: Vec::new(),
            selected: 0,
            log_lines,
            bundled_models_dir: paths::bundled_models_dir(),
            models_dir: PathBuf::new(),
            binary: PathBuf::new(),
            missing_binary_warned: false,
            run_reached_running: false,
            model_input: String::new(),
            download: None,
            lookup: LookupView::default(),
            catalog: catalog_view::CatalogView::default(),
            total_ram_bytes,
            recommended_filename: None,
            show_api_info: false,
            show_settings: false,
            show_about: false,
            models_dir_override: None,
            models_dir_input: String::new(),
            settings_error: None,
            active_tab: Tab::Main,
            traffic: VecDeque::new(),
            selected_session: None,
            gpu_vendors,
            runtime: RuntimeManager::default(),
            pending_delete: None,
        };
        // the executable, the models and the panels' state of the engine in use
        app.load_engine_state();
        app
    }

    /// The model the picker has selected, as the form and the planner know a model.
    fn selected_model_info(&self) -> Option<ModelInfo> {
        self.models.get(self.selected).map(|m| m.info())
    }

    /// What the form and the planner are asked about: the engine, the model and the field values.
    fn form_selection(&self) -> Selection {
        let mut selection = Selection::new(&self.engine);
        selection.model = self.selected_model_info();
        selection.values = self.values.map().clone();
        selection
    }

    /// The machine as the form sees it: with the executable that is in use now and whether the
    /// server is running (starting, running or stopping).
    fn form_facts(&self) -> HostFacts {
        let mut facts = self.host_facts.clone();
        facts.binary = Some(self.binary.display().to_string());
        facts.session_running = self.session.is_active();
        facts.session_attached = self.session.attached();
        facts
    }

    /// The engine's settings, as they are right now. `None` only when the engine document is
    /// missing, which a test of the embedded documents rules out.
    fn form(&self) -> Option<FormModel> {
        FormModel::build(&self.registry, &self.form_selection(), &self.form_facts()).ok()
    }

    /// Whether the engine uses the GPU: its own `gpu_offload` fact, which feeds the speed estimate
    /// and the model recommendation. Offload counts as on when the form cannot say.
    fn gpu_offload(&self) -> bool {
        self.form()
            .and_then(|form| form.derived("gpu_offload").and_then(Value::as_bool))
            .unwrap_or(true)
    }

    /// Picking a model re-derives the fields that depend on it (the context size), as it always has.
    fn apply_model_defaults(&mut self) {
        if self.selected_model_info().is_none() {
            return;
        }
        if let Ok(edits) = form::recompute(
            &self.registry,
            &self.form_selection(),
            &self.form_facts(),
            "model",
        ) {
            self.values.apply(edits);
        }
    }

    fn refresh_recommendation(&mut self) {
        self.recommended_filename =
            hardware::recommend(&self.models, self.total_ram_bytes, self.gpu_offload());
    }

    /// The port the engine is on: the field the document marks as the public one, else the engine's
    /// own default.
    fn port(&self) -> u16 {
        self.values
            .int("port")
            .and_then(|n| u16::try_from(n).ok())
            .or_else(|| self.engine_doc().and_then(|e| e.doc.provides.default_port))
            .unwrap_or(8080)
    }

    /// The custom llama-server the user chose, if any.
    fn bin_override(&self) -> Option<PathBuf> {
        self.values.text("bin_override").map(PathBuf::from)
    }

    fn set_bin_override(&mut self, path: Option<PathBuf>) {
        match path {
            Some(path) => self.values.set("bin_override", path.display().to_string()),
            None => self.values.clear("bin_override"),
        }
    }

    /// Re-scans both models folders, keeping the current selection if it's
    /// still present. Shared by the Refresh button and the ⚙ Settings
    /// "Apply"/"Reset to default" actions on the downloaded-models folder.
    fn rescan_models(&mut self) {
        let current = self.models.get(self.selected).map(|m| m.filename.clone());
        self.models = self.scan_models();
        self.selected = current
            .and_then(|f| self.models.iter().position(|m| m.filename == f))
            .unwrap_or(0);
        self.refresh_recommendation();
    }

    /// The models both folders hold, named as the picker shows them: what the file says, else what the
    /// recommended list calls it, with a sentence about the mode it is started in when its record
    /// names one.
    fn scan_models(&self) -> Vec<ModelEntry> {
        let mut models = discover_all(
            &self.bundled_models_dir,
            &self.models_dir,
            &self.companion_patterns(),
        );
        if let Some(engine) = self.engine_doc() {
            let list = quickai_core::models::recommended(&self.registry, engine);
            crate::models::name_from_list(&mut models, &crate::models::names_from_list(&list));
            let labels: Vec<(String, String)> = quickai_core::models::modes(engine)
                .iter()
                .map(|m| (m.id.clone(), m.label.clone()))
                .collect();
            crate::models::say_modes(&mut models, &labels);
        }
        models
    }

    /// The patterns of the files that go with a model and are not models themselves (`mmproj*.gguf`):
    /// what the engine's bundles say, so the picker lists weights only.
    fn companion_patterns(&self) -> Vec<String> {
        self.engine_doc()
            .map(|e| {
                quickai_core::lookup::bundle::companion_patterns(quickai_core::models::bundles(e))
            })
            .unwrap_or_default()
    }

    fn push_log(&mut self, line: String) {
        self.log_lines.push_back(line);
        while self.log_lines.len() > MAX_LOG_LINES {
            self.log_lines.pop_front();
        }
    }

    /// What a button of a form asked for, by the id its document gave it. An id this version does
    /// not know is said in the log, and nothing is guessed.
    fn invoke(&mut self, action: &str) {
        match action {
            "open:settings" => self.show_settings = true,
            other if other.starts_with("open:settings.") => self.show_settings = true,
            other if other.starts_with("focus_field:") => {
                // the field is where its section puts it: Settings for an advanced one, the row of
                // controls above for a primary one, which is already on screen
                let id = &other["focus_field:".len()..];
                let section = self
                    .engine_doc()
                    .and_then(|e| e.doc.fields.iter().find(|f| f.id == id))
                    .and_then(|f| f.ui.section.clone());
                if section.as_deref().unwrap_or("advanced") != "primary" {
                    self.show_settings = true;
                }
                self.push_log(format!("[quickai] Look at the `{id}` setting."));
            }
            other => self.push_log(format!(
                "[quickai] A button asked for `{other}`, which this version of QuickAI does not do"
            )),
        }
    }

    /// The settings as they are now: the keys this version does not know come back from the file as it
    /// was loaded, the application's own two settings from here, and the fields of the engine in use
    /// through their `legacy_key`s (or under `values`). The settings of the engines that are not in
    /// use, and the choice of engine, are as they were.
    fn settings_now(&self) -> Settings {
        let mut settings = self.stored.clone();
        settings.dark_mode = self.dark_mode;
        settings.enable_traffic_proxy = self.enable_traffic_proxy;
        if let Some(engine) = self.registry.engine(&self.engine) {
            let mut values = self.values.clone();
            match self.models.get(self.selected) {
                Some(m) => values.set("last_model", m.filename.clone()),
                None => values.clear("last_model"),
            }
            // the models folder is the setting of an engine whose models are files
            if !self.lists_models() {
                match &self.models_dir_override {
                    Some(dir) => values.set("models_dir", dir.display().to_string()),
                    None => values.clear("models_dir"),
                }
            }
            // Every value the controls can produce fits its key; if one somehow does not, the
            // engine's keys keep what the file had rather than a half-written file.
            let _ = settings.set_engine_values(engine, values.map());
        }
        settings
    }

    /// Writes `settings.json`.
    fn save_settings(&self) {
        self.write_settings(&self.settings_now());
    }

    fn write_settings(&self, settings: &Settings) {
        match &self.settings_path {
            Some(path) => settings.save_to(path),
            None => settings.save(),
        }
    }

    /// Applies one proxy `TrafficEvent` to the 📡 Live tab's turn list.
    fn apply_traffic_event(&mut self, event: TrafficEvent) {
        match event {
            TrafficEvent::Request {
                id,
                session,
                method,
                path,
                prompt,
                attachments,
            } => {
                if self.traffic.len() >= MAX_TRAFFIC_TURNS {
                    self.traffic.pop_front();
                }
                self.traffic.push_back(TrafficTurn {
                    id,
                    session,
                    method,
                    path,
                    prompt,
                    attachments,
                    response: String::new(),
                    done: false,
                    error: None,
                });
            }
            TrafficEvent::ResponseDelta { id, text } => {
                if let Some(turn) = self.traffic.iter_mut().rev().find(|t| t.id == id) {
                    turn.response.push_str(&text);
                }
            }
            TrafficEvent::ResponseDone { id, full_text } => {
                if let Some(turn) = self.traffic.iter_mut().rev().find(|t| t.id == id) {
                    if let Some(text) = full_text {
                        turn.response = text;
                    }
                    turn.done = true;
                }
            }
            TrafficEvent::ResponseError { id, message } => {
                if let Some(turn) = self.traffic.iter_mut().rev().find(|t| t.id == id) {
                    turn.error = Some(message);
                    turn.done = true;
                }
            }
        }
    }

    /// Drains the in-flight download's channel, if any. On completion,
    /// re-scans models_dir (the new file is now sitting there) and selects
    /// it; on failure, logs, and scans all the same: the weights of a bundle
    /// are there when only a file after them failed.
    fn poll_download(&mut self) {
        let Some(mut job) = self.download.take() else {
            return;
        };
        // what the job said on the way, which polling is what gathers: the last of it comes with the end
        let outcome = job.poll();
        for line in job.take_statuses() {
            self.push_log(line);
        }
        match outcome {
            Some(Ok(filename)) => {
                self.push_log(format!("Download complete: {filename}"));
                self.catalog.note = None;
                let current = self.models.get(self.selected).map(|m| m.filename.clone());
                self.models = self.scan_models();
                if self.catalog.open {
                    self.refresh_catalog_have();
                }
                self.selected = self
                    .models
                    .iter()
                    .position(|m| m.filename == filename)
                    .or_else(|| {
                        current.and_then(|f| self.models.iter().position(|m| m.filename == f))
                    })
                    .unwrap_or(0);
                self.refresh_recommendation();
                self.save_settings();
            }
            Some(Err(msg)) => {
                self.push_log(format!("Download failed: {msg}"));
                self.catalog.note = Some(format!("Download failed: {msg}"));
                self.rescan_models();
            }
            None => self.download = Some(job),
        }
    }

    fn state_color(state: ServerState) -> egui::Color32 {
        match state {
            ServerState::Idle => egui::Color32::GRAY,
            ServerState::Starting | ServerState::Stopping => egui::Color32::from_rgb(212, 160, 23),
            ServerState::Running => egui::Color32::from_rgb(46, 139, 50),
            ServerState::Crashed => egui::Color32::from_rgb(192, 57, 43),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut new_lines = Vec::new();
        let mut new_traffic = Vec::new();
        self.session
            .poll(&mut |line| new_lines.push(line), &mut |evt| {
                new_traffic.push(evt)
            });
        for line in new_lines {
            self.push_log(line);
        }
        for evt in new_traffic {
            self.apply_traffic_event(evt);
        }
        self.poll_download();
        self.poll_lookup();
        self.poll_runtime_manager();
        self.poll_named_models(ui.ctx());

        let active = self.session.is_active();
        let state = self.session.state();
        if state == ServerState::Running {
            self.run_reached_running = true;
        }

        egui::CentralPanel::default().show(ui, |ui| {
            self.show_header(ui);

            if self.active_tab == Tab::Live {
                self.show_live_traffic_tab(ui);
                return;
            }

            self.show_model_bar(ui, active);

            ui.add_space(8.0);
            self.show_acquire(ui, active);

            ui.add_space(8.0);
            self.show_controls(ui);

            ui.add_space(8.0);
            self.show_run_bar(ui, state, active);

            self.show_api_info_panel(ui, state);
            self.show_diagnostics(ui, state);
            self.show_log_pane(ui);
        });

        self.show_settings_window(ui.ctx());
        self.show_about_window(ui.ctx());
        self.show_catalog_window(ui.ctx());
        self.show_confirm_delete_modal(ui.ctx());
        self.show_foreign_program_modal(ui.ctx());
    }

    fn on_exit(&mut self) {
        self.session.stop_blocking();
    }
}
