//! The engine the window is for: which one is in use, how it is switched, and how the models of an
//! engine that keeps its own are listed, pulled and removed (docs/phase3-test-plan.md, groups E and
//! M). llama.cpp's window is drawn by the panels as it always was, from files in a folder; what is
//! here is for the other kind, whose models are named references a daemon lists (Ollama).
//!
//! Nothing here asks an engine anything on its own account: the list is fetched when the person
//! presses Refresh or when the engine has just come up, a pull is started by a button, and a stopped
//! engine is shown what it listed last time.

use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use eframe::egui;
use quickai_core::models::{find_listed, pull_call, remove_call, same_model};
use quickai_core::plan::Endpoint;
use quickai_core::registry::{Engine, InputAction};

use super::runtime_manager::RuntimeManager;
use super::values::FieldValues;
use super::App;
use crate::events::ServerState;
use crate::jobs::Job;
use crate::launch;
use crate::models::ModelEntry;
use crate::paths;
use crate::prims::engine_models::{
    ask_to_remove, list_models, models_to_show, spawn_pull, ListSpec, Listing, PullHandle, PullSpec,
};

/// What the window holds for an engine whose models the daemon lists.
#[derive(Default)]
pub struct NamedModels {
    /// A request for the list that has not been answered.
    fetch: Option<Receiver<Result<Listing, String>>>,
    /// What is shown is what the engine said last time, not what it says now.
    pub stale: bool,
    /// The pull that is running, and the way to stop it.
    pub pull: Option<(Job<()>, PullHandle)>,
    /// What to say under the box: why an input was refused, what a failed pull said.
    pub notice: Option<String>,
    /// The state the session was in at the last frame, to see the engine come up.
    last_state: Option<ServerState>,
}

impl App {
    /// The document of the engine in use.
    pub(super) fn engine_doc(&self) -> Option<&Engine> {
        self.registry.engine(&self.engine)
    }

    /// Whether the engine starts on its own, with no model chosen (ComfyUI): its models are used by
    /// the work a client sends it, and the window has no model to pick before Start.
    pub(super) fn runs_alone(&self) -> bool {
        self.engine_doc()
            .is_some_and(|e| !e.doc.provides.needs_model)
    }

    /// Whether the engine serves any API whose traffic the Live tab can read (a conversation).
    pub(super) fn reads_traffic(&self) -> bool {
        launch::engine_reads_traffic(&self.registry, &self.engine)
    }

    /// Whether the live-traffic proxy goes in front of the engine on the next start: the person asked
    /// for it, and there is a conversation for it to show. A proxy in front of an engine whose traffic
    /// is not read would only be a hop that its web page's connections have to survive.
    pub(super) fn traffic_proxy_on(&self) -> bool {
        self.enable_traffic_proxy && self.reads_traffic()
    }

    /// What the window calls the engine (`llama.cpp`, `Ollama`).
    pub(super) fn engine_name(&self) -> String {
        self.engine_doc()
            .map_or_else(|| self.engine.clone(), |e| e.doc.engine.name.clone())
    }

    /// What it calls the engine's program in a sentence (`llama-server`, `Ollama`).
    pub(super) fn server_name(&self) -> String {
        self.engine_doc().map_or_else(
            || self.engine.clone(),
            |e| {
                e.doc
                    .engine
                    .server_name
                    .clone()
                    .unwrap_or_else(|| e.doc.engine.name.clone())
            },
        )
    }

    /// Whether the engine's models are named references it lists (Ollama), not files in a folder.
    pub(super) fn lists_models(&self) -> bool {
        self.engine_doc()
            .and_then(|e| e.models.as_ref())
            .is_some_and(|m| m.list.is_some())
    }

    /// The engine as a client reaches it, while it is running.
    pub(super) fn endpoint(&self) -> Option<Endpoint> {
        let port = self.session.port()?;
        (self.session.state() == ServerState::Running).then(|| Endpoint {
            host: "127.0.0.1".to_string(),
            port: Some(port),
        })
    }

    /// The request that lists the engine's models, as its document describes it.
    fn list_spec(&self) -> Option<ListSpec> {
        let call = quickai_core::models::list_call(self.engine_doc()?, &self.host_facts)?.ok()?;
        Some(ListSpec::from_call(&call))
    }

    /// Whether the engine can be changed now: not while something runs or is being fetched with the
    /// current one.
    pub(super) fn engine_locked(&self) -> bool {
        self.session.is_active()
            || self.download.is_some()
            || self.runtime.install.is_some()
            || self.runtime.delete.is_some()
            || self.named.pull.is_some()
            || self.companion_busy()
    }

    /// Everything that depends on the engine: its executable, its models and the state of the
    /// panels that were about it. Called when the application starts and when the engine changes.
    pub(super) fn load_engine_state(&mut self) {
        self.runtime = RuntimeManager {
            input: self
                .values
                .text("bin_override")
                .map(str::to_string)
                .unwrap_or_default(),
            ..Default::default()
        };
        self.binary = launch::locate_binary_in(
            &self.registry,
            &self.engine,
            self.values.text("bin_override"),
            self.data_dir_override.as_deref(),
        );
        self.host_facts = launch::host_facts(&self.binary, self.machine.clone(), None);
        if let Some(dir) = &self.data_dir_override {
            self.host_facts.data_dir = dir.clone();
        }
        self.look_for_stopped_installs();
        self.missing_binary_warned = false;
        // the program that goes with this engine, if one does: its own settings, builds and run
        self.companion = self.build_companion();
        self.as_companion(|app| app.look_for_stopped_installs());
        self.lookup = super::lookup_view::LookupView::default();
        self.catalog = super::catalog_view::CatalogView::default();
        self.bench = super::workbench::Bench::default();
        self.model_input.clear();
        self.show_api_info = false;
        self.named = NamedModels::default();
        self.models = Vec::new();
        if self.lists_models() {
            self.models_dir_override = None;
            self.load_cached_models();
        } else {
            self.models_dir_override = self.values.text("models_dir").map(PathBuf::from);
            self.models_dir = self
                .models_dir_override
                .clone()
                .unwrap_or_else(|| self.default_models_dir());
            let _ = std::fs::create_dir_all(&self.models_dir);
            self.models_dir_input = self.models_dir.display().to_string();
            self.models = self.scan_models();
        }
        self.selected = self.remembered_model().unwrap_or(0);
        self.refresh_recommendation();
    }

    /// The models folder when the person has not chosen one: the engine's own, if its document says
    /// where its models are kept (ComfyUI's, under its own folder), else the one folder every
    /// engine of files has always shared.
    pub(super) fn default_models_dir(&self) -> PathBuf {
        if let Some(engine) = self.engine_doc() {
            let mut selection = self.form_selection();
            selection
                .values
                .remove(&format!("{}.models_dir", self.engine));
            if let Some(dir) =
                quickai_core::models::files_dir(engine, &selection, &self.form_facts())
            {
                return PathBuf::from(dir);
            }
        }
        paths::user_models_dir()
    }

    /// The places the engine's models are found, when its document names them (`[[models.sources]]`),
    /// with the models folder as the window holds it now.
    pub(super) fn model_sources(&self) -> Vec<quickai_core::models::FileSource> {
        let Some(engine) = self.engine_doc() else {
            return Vec::new();
        };
        let mut selection = self.form_selection();
        selection.values.insert(
            format!("{}.models_dir", self.engine),
            quickai_core::value::Value::from(self.models_dir.display().to_string()),
        );
        quickai_core::models::file_sources(engine, &selection, &self.form_facts())
    }

    /// Where the model the engine had selected last time is in the list now.
    fn remembered_model(&self) -> Option<usize> {
        let name = self.values.text("last_model")?;
        find_listed(self.models.iter().map(|m| m.filename.as_str()), name)
            .or_else(|| self.models.iter().position(|m| m.filename == name))
    }

    /// Changes the engine: what the one that is left holds is kept, and what the other held is taken
    /// up. Settings are written at once.
    pub(super) fn switch_engine(&mut self, id: &str) {
        if id == self.engine || self.engine_locked() || self.registry.engine(id).is_none() {
            return;
        }
        let mut stored = self.settings_now();
        stored.set_chosen_engine(id);
        self.write_settings(&stored);
        self.stored = stored;
        self.engine = id.to_string();
        self.values = FieldValues::from_settings(&self.stored, self.registry.engine(id));
        self.load_engine_state();
    }

    // ---- the models of an engine that lists them ---------------------------------------------------

    /// Shows what the engine said last time, if it said anything: no request is made.
    fn load_cached_models(&mut self) {
        let Some(spec) = self.list_spec() else { return };
        if let Ok(listing) = models_to_show(None, &spec) {
            self.set_listing(listing);
        }
    }

    /// Asks the running engine for its models; with it stopped, shows the remembered list.
    pub(super) fn refresh_models(&mut self, ctx: &egui::Context) {
        let (Some(endpoint), Some(spec)) = (self.endpoint(), self.list_spec()) else {
            self.load_cached_models();
            return;
        };
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.named.fetch = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(list_models(&endpoint, &spec).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    fn set_listing(&mut self, listing: Listing) {
        let current = self.models.get(self.selected).map(|m| m.filename.clone());
        self.named.stale = listing.stale;
        for warning in listing.warnings {
            self.push_log(warning);
        }
        self.models = listing.rows.iter().map(ModelEntry::from_row).collect();
        let at = |name: &str| find_listed(self.models.iter().map(|m| m.filename.as_str()), name);
        self.selected = current
            .as_deref()
            .and_then(at)
            .or_else(|| self.remembered_model())
            .unwrap_or(0);
        self.refresh_recommendation();
    }

    /// Once a frame: the engine has come up (ask what it has), the answer has come, the pull has
    /// said something or finished.
    pub(super) fn poll_named_models(&mut self, ctx: &egui::Context) {
        if !self.lists_models() {
            return;
        }
        let state = self.session.state();
        if state == ServerState::Running && self.named.last_state != Some(ServerState::Running) {
            self.refresh_models(ctx);
        }
        self.named.last_state = Some(state);

        if let Some(rx) = &self.named.fetch {
            if let Ok(result) = rx.try_recv() {
                self.named.fetch = None;
                match result {
                    Ok(listing) => self.set_listing(listing),
                    Err(message) => {
                        self.push_log(format!("Could not list the models: {message}"));
                    }
                }
            }
        }

        let Some((mut job, handle)) = self.named.pull.take() else {
            return;
        };
        for line in job.take_statuses() {
            self.push_log(line);
        }
        match job.poll() {
            Some(Ok(())) => {
                self.push_log(format!("Pulled {}.", job.label));
                self.named.notice = None;
                let pulled = job.label.clone();
                self.refresh_models(ctx);
                self.values.set("last_model", pulled);
            }
            Some(Err(message)) => {
                self.push_log(format!("Could not pull {}: {message}", job.label));
                self.named.notice = Some(message);
            }
            None => self.named.pull = Some((job, handle)),
        }
    }

    /// What was typed into the model box: pulled, or refused with the sentence the document gives.
    pub(super) fn submit_model_input(&mut self, ctx: &egui::Context) {
        let text = self.model_input.clone();
        if self.pull_named(&text, ctx) {
            self.model_input.clear();
        }
    }

    /// `text` as the model box takes it: the engine's own rules say whether it is a name to pull or
    /// something to refuse, and a refusal is the sentence under the box. The box and a row of the
    /// look-up come through here, so nothing is pulled that the box would not take. `true` when a
    /// pull was started.
    pub(super) fn pull_named(&mut self, text: &str, ctx: &egui::Context) -> bool {
        let Some(engine) = self.engine_doc() else {
            return false;
        };
        let action = engine.classify_input(text, &|path| Path::new(path).is_file());
        match action {
            InputAction::Nothing => false,
            InputAction::Reject { message } => {
                self.named.notice = Some(message);
                false
            }
            InputAction::Pull { reference } => {
                let Some(endpoint) = self.endpoint() else {
                    self.named.notice = Some(format!(
                        "Start {} first: the running {} pulls the model.",
                        self.engine_name(),
                        self.server_name()
                    ));
                    return false;
                };
                let call = self
                    .engine_doc()
                    .and_then(|e| pull_call(e, &reference))
                    .and_then(Result::ok);
                let Some(call) = call else {
                    self.named.notice = Some(format!(
                        "{} has no way to pull a model.",
                        self.engine_name()
                    ));
                    return false;
                };
                self.named.notice = None;
                let ctx = ctx.clone();
                self.named.pull = Some(spawn_pull(
                    endpoint,
                    PullSpec::from_call(&call),
                    reference,
                    move || ctx.request_repaint(),
                ));
                true
            }
        }
    }

    /// The person said yes to removing `name`: the engine is asked, by the name it lists, and the
    /// list is asked for again. A refusal or a failure is shown under Settings.
    pub(super) fn remove_listed_model(&mut self, name: &str, ctx: &egui::Context) {
        let loaded = self.session.model_name().map(str::to_string);
        let pending = match ask_to_remove(name, loaded.as_deref()) {
            Ok(pending) => pending,
            Err(refusal) => {
                self.settings_error = Some(refusal);
                return;
            }
        };
        let Some(endpoint) = self.endpoint() else {
            self.settings_error = Some(format!(
                "Start {} to remove a model: the running {} does it.",
                self.engine_name(),
                self.server_name()
            ));
            return;
        };
        let call = self
            .engine_doc()
            .and_then(|e| remove_call(e, &pending.name))
            .and_then(Result::ok);
        let Some(call) = call else {
            self.settings_error = Some(format!(
                "{} has no way to remove a model.",
                self.engine_name()
            ));
            return;
        };
        match pending.confirm(&endpoint, &call) {
            Ok(()) => {
                self.push_log(format!("Removed model: {name}"));
                self.settings_error = None;
                self.refresh_models(ctx);
            }
            Err(sentence) => self.settings_error = Some(sentence),
        }
    }

    /// Whether `name` is the model the session has loaded (it cannot be removed).
    pub(super) fn is_loaded_model(&self, name: &str) -> bool {
        self.session
            .model_name()
            .is_some_and(|loaded| same_model(loaded, name))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use quickai_core::value::Value;
    use serde_json::json;

    use super::*;
    use crate::app::PendingDelete;
    use crate::config::{build_registry, ConfigDirs};
    use crate::launch;
    use crate::settings::Settings;
    use crate::testkit::fake_ollama::FakeOllama;
    use quickai_core::lookup::present::Fetch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// An application on settings of its own in `dir`, with no real home in sight: the settings file,
    /// the models folder and the data directory are all below `dir`.
    fn app_on(dir: &Path, mut settings: Settings) -> App {
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        let mut app = App::build(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
        );
        app.data_dir_override = Some(dir.join("data").display().to_string());
        app.load_engine_state();
        app
    }

    fn with_ollama_values(settings: &mut Settings, values: &[(&str, Value)]) {
        let registry = build_registry(&ConfigDirs::default());
        let values: BTreeMap<String, Value> = values
            .iter()
            .map(|(k, v)| (format!("ollama.{k}"), v.clone()))
            .collect();
        settings
            .set_engine_values(registry.engine("ollama").unwrap(), &values)
            .unwrap();
    }

    /// What Ollama listed the last time it ran, where the document says it is kept.
    fn remember_models(dir: &Path, names: &[&str]) {
        let rows: Vec<serde_json::Value> = names
            .iter()
            .map(|n| json!({"id": n, "display": n, "size_bytes": 1_000_000_000u64, "digest": "d", "capabilities": ["completion"]}))
            .collect();
        let cache = dir.join("data/ollama/catalog-cache.json");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, json!({"version": 1, "rows": rows}).to_string()).unwrap();
    }

    fn saved(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
    }

    #[test]
    fn p3_e23_changing_the_engine_keeps_what_each_holds_saves_the_choice_and_shows_the_others_models(
    ) {
        let dir = crate::testkit::scratch("e23");
        let mut settings = Settings {
            port: 8099,
            ..Settings::default()
        };
        with_ollama_values(
            &mut settings,
            &[
                ("port", Value::Int(12345)),
                ("last_model", Value::Str("bonsai-q1".into())),
            ],
        );
        remember_models(&dir, &["qwen3:4b", "bonsai-q1:latest"]);
        let mut app = app_on(&dir, settings);

        // a first run is llama.cpp: its values, its program, models that are files
        assert_eq!(app.engine, "llamacpp");
        assert_eq!(app.values.int("port"), Some(8099));
        assert!(
            app.binary.to_string_lossy().contains("llama-server"),
            "{:?}",
            app.binary
        );
        assert!(!app.lists_models());

        app.switch_engine("ollama");
        assert_eq!(app.engine, "ollama");
        assert_eq!(app.values.int("port"), Some(12345), "Ollama's own port");
        assert!(
            app.binary.to_string_lossy().contains("ollama"),
            "{:?}",
            app.binary
        );
        assert!(app.lists_models());
        // nothing is asked: the list is what Ollama said the last time, and the one remembered is
        // chosen by what it means (`bonsai-q1` is listed as `bonsai-q1:latest`)
        let listed: Vec<&str> = app.models.iter().map(|m| m.filename.as_str()).collect();
        assert_eq!(listed, ["qwen3:4b", "bonsai-q1:latest"]);
        assert!(app.named.stale);
        assert_eq!(app.models[app.selected].filename, "bonsai-q1:latest");

        // the choice is in the file at once, and llama.cpp's nine keys are where they were
        let file = saved(&dir);
        assert_eq!(file["values"]["app.engine"], "ollama");
        assert_eq!(file["port"], 8099);
        assert_eq!(file["values"]["ollama.port"], 12345);

        // back: the choice is taken away, both engines' values are still in the file and in use
        app.switch_engine("llamacpp");
        assert_eq!(app.values.int("port"), Some(8099));
        let file = saved(&dir);
        assert!(file["values"].get("app.engine").is_none(), "{file}");
        assert_eq!(file["values"]["ollama.port"], 12345);
        assert_eq!(file["port"], 8099);

        // while something is being fetched the engine stays as it is
        app.download = Some(crate::jobs::Job::spawn(
            "x".into(),
            "Downloading",
            || {},
            |_| std::thread::sleep(Duration::from_millis(300)),
        ));
        assert!(app.engine_locked());
        app.switch_engine("ollama");
        assert_eq!(app.engine, "llamacpp");
    }

    #[test]
    fn p3_e24_choosing_ollama_starts_nothing_and_asks_nothing() {
        let dir = crate::testkit::scratch("e24");
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 2_500_000_000);
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        with_ollama_values(&mut settings, &[("port", Value::Int(fake.port() as i64))]);
        let mut app = app_on(&dir, settings);
        assert_eq!(app.engine, "ollama");
        // drawn once, the frames that follow, the picker and Settings opened: no request to the daemon
        // that is running right there on the configured port
        let ctx = egui::Context::default();
        for _ in 0..3 {
            app.poll_named_models(&ctx);
        }
        app.refresh_models(&ctx);
        assert!(
            fake.requests().is_empty(),
            "the stopped engine was asked: {:?}",
            fake.requests().iter().map(|r| &r.path).collect::<Vec<_>>()
        );
        assert!(
            app.models.is_empty(),
            "nothing was listed, and nothing was remembered"
        );
    }

    #[test]
    fn p3_e25b_an_engine_that_is_not_installed_is_missing_and_one_that_ships_with_the_app_is_not() {
        let dir = crate::testkit::scratch("e25b");
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        // no override, none installed, none on this PATH under the name the document looks for
        with_ollama_values(
            &mut settings,
            &[("bin_override", Value::Str("/nowhere/ollama".into()))],
        );
        let app = app_on(&dir, settings);
        assert!(
            app.engine_missing(),
            "Ollama is downloaded on request: Start cannot work yet"
        );
        let llama = app_on(&crate::testkit::scratch("e25c"), Settings::default());
        assert!(
            !llama.engine_missing(),
            "llama.cpp is tried, and says what it could not find"
        );
    }

    #[test]
    fn p3_m07_the_model_box_of_ollama_refuses_what_is_not_a_name_and_asks_for_a_running_daemon_to_pull(
    ) {
        let dir = crate::testkit::scratch("m07");
        let fake = FakeOllama::start();
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        with_ollama_values(&mut settings, &[("port", Value::Int(fake.port() as i64))]);
        let mut app = app_on(&dir, settings);
        let ctx = egui::Context::default();
        let say = |app: &mut App, text: &str| {
            app.model_input = text.to_string();
            app.submit_model_input(&ctx);
            app.named.notice.clone()
        };
        assert!(say(&mut app, "two words")
            .unwrap()
            .contains("does not look like a model name"));
        assert!(say(&mut app, "https://example.com/model.gguf")
            .unwrap()
            .contains("pulls by name"));
        assert!(
            say(&mut app, "/etc/hostname")
                .unwrap()
                .contains("local file"),
            "an existing file is a file, not a name"
        );
        assert!(
            say(&mut app, "   ").is_some(),
            "the last notice stays when nothing was typed"
        );
        // a name is fine, but nothing is running to pull it
        let said = say(&mut app, "qwen3:4b").unwrap();
        assert!(said.starts_with("Start Ollama first"), "{said}");
        assert!(app.named.pull.is_none());
        assert!(fake.requests().is_empty(), "nothing was sent");
    }

    /// Draws every panel of the main window and the Settings window once, as a frame does.
    fn draw_everything(app: &mut App, ctx: &egui::Context, state: ServerState) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 560.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            app.show_window(ui);
            // what the window draws only in a state the session is not in here
            egui::Area::new(egui::Id::new("in-this-state")).show(ui.ctx(), |ui| {
                app.show_run_buttons(ui, state, false);
                app.show_api_info_panel(ui, state);
                app.show_diagnostics(ui, state);
            });
            // (the question of a delete is asked in the Settings window, where its button is)
            app.show_settings_window(ui.ctx());
            app.show_catalog_window(ui.ctx());
        });
        output.textures_delta.clear();
    }

    #[test]
    fn p3_e26b_every_panel_draws_for_every_engine_in_the_states_a_window_can_be_in() {
        let ctx = egui::Context::default();
        for engine in ["llamacpp", "ollama", "comfyui"] {
            let dir = crate::testkit::scratch(&format!("e26-{engine}"));
            let mut settings = Settings::default();
            settings.set_chosen_engine(engine);
            remember_models(&dir, &["qwen3:4b", "bonsai-q1:latest"]);
            let mut app = app_on(&dir, settings);
            for state in [
                ServerState::Idle,
                ServerState::Starting,
                ServerState::Running,
                ServerState::Stopping,
                ServerState::Crashed,
            ] {
                app.show_settings = state == ServerState::Idle;
                app.show_api_info = true;
                app.missing_binary_warned = state == ServerState::Starting;
                app.run_reached_running = state == ServerState::Crashed;
                app.named.notice = (state == ServerState::Idle).then(|| "a notice".to_string());
                draw_everything(&mut app, &ctx, state);
            }
            // and with the models gone, or a removal waiting for a yes
            app.models.clear();
            draw_everything(&mut app, &ctx, ServerState::Idle);
            app.show_settings = true;
            app.pending_delete = Some(PendingDelete::Listed {
                name: "qwen3:4b".into(),
            });
            draw_everything(&mut app, &ctx, ServerState::Idle);
        }
    }

    /// An application whose Ollama document is patched by the person's own configuration: the
    /// patch is `patch`, and the program the executable the settings name is a script that writes down
    /// that it ran.
    fn app_with_user_patch(dir: &Path, patch: &str) -> (App, PathBuf) {
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(cfg.join("engines")).unwrap();
        std::fs::write(
            cfg.join("engines/ollama.toml"),
            format!("schema = 1\nkind = \"engine\"\nid = \"ollama\"\n{patch}"),
        )
        .unwrap();
        let marker = dir.join("ran");
        let script = crate::testkit::procs::write_script(
            dir,
            "ollama",
            &format!("echo \"$0 $@\" > {}\nexec sleep 30", marker.display()),
        );
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        with_ollama_values(
            &mut settings,
            &[("bin_override", Value::Str(script.display().to_string()))],
        );
        (app_over(dir, settings), marker)
    }

    /// An application on `settings` and the person's configuration under `dir/cfg`.
    fn app_over(dir: &Path, settings: Settings) -> App {
        let registry = build_registry(&ConfigDirs {
            user: Some(dir.join("cfg")),
            ..ConfigDirs::default()
        });
        assert!(
            registry.diagnostics.iter().all(|d| !d.is_error()),
            "{:#?}",
            registry.diagnostics
        );
        let mut app = App::build(
            settings,
            registry,
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
        );
        app.data_dir_override = Some(dir.join("data").display().to_string());
        app.load_engine_state();
        app
    }

    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} never appeared",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn p3_g07_a_program_of_a_document_of_the_persons_is_asked_about_once_through_the_window() {
        let ctx = egui::Context::default();

        // the application's own document: nothing is asked, the program runs
        let dir = crate::testkit::scratch("g07-own");
        let (mut app, marker) = app_with_user_patch(&dir, "");
        app.start_selected(&ctx);
        assert!(app.pending_foreign.is_none());
        wait_for(&marker);
        app.session.stop_blocking();

        // a patch that changes only a default does not ask either (the program is the one found)
        let dir = crate::testkit::scratch("g07-default");
        let (mut app, marker) =
            app_with_user_patch(&dir, "[[field]]\nid = \"gpu\"\ndefault = \"off\"\n");
        app.start_selected(&ctx);
        assert!(app.pending_foreign.is_none());
        wait_for(&marker);
        app.session.stop_blocking();

        // a patch that names another program asks, and nothing starts until the answer
        let dir = crate::testkit::scratch("g07-foreign");
        let other = crate::testkit::procs::write_script(
            &dir,
            "other",
            &format!(
                "echo other > {}\nexec sleep 30",
                dir.join("other-ran").display()
            ),
        );
        let patch = format!("[launch.spawn]\nprogram = \"{}\"\n", other.display());
        let (mut app, marker) = app_with_user_patch(&dir, &patch);
        app.start_selected(&ctx);
        let foreign = app.pending_foreign.clone().expect("the person is asked");
        assert_eq!(foreign.program, other.display().to_string());
        assert_eq!(foreign.hash.len(), 64);
        assert!(
            foreign.files[0].ends_with("engines/ollama.toml"),
            "{:?}",
            foreign.files
        );
        assert_eq!(app.session.state(), ServerState::Idle, "nothing started");
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !marker.exists() && !dir.join("other-ran").exists(),
            "nothing ran"
        );

        // no: a line in the log, nothing remembered, nothing started
        app.answer_foreign_question(false, &ctx);
        assert!(app.pending_foreign.is_none());
        assert_eq!(app.session.state(), ServerState::Idle);
        assert!(app.stored.confirmed_engines().is_empty());
        assert!(
            app.log_lines
                .iter()
                .any(|l| l.contains("you did not allow")),
            "{:?}",
            app.log_lines
        );
        app.start_selected(&ctx);
        assert!(
            app.pending_foreign.is_some(),
            "a declined program is asked about again"
        );

        // yes: remembered by the hash, in the file, and the run goes ahead
        app.answer_foreign_question(true, &ctx);
        assert!(app.pending_foreign.is_none());
        assert_eq!(
            app.stored.confirmed_engines(),
            std::slice::from_ref(&foreign.hash)
        );
        assert_eq!(
            saved(&dir)["values"]["app.confirmed_engines"],
            json!([foreign.hash.clone()])
        );
        wait_for(&dir.join("other-ran"));
        app.session.stop_blocking();

        // after a restart (the settings read back from the file) the same document is not asked about
        let file: Settings =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            file.confirmed_engines(),
            std::slice::from_ref(&foreign.hash)
        );
        std::fs::remove_file(dir.join("other-ran")).unwrap();
        let mut again = app_over(&dir, file);
        again.start_selected(&ctx);
        assert!(again.pending_foreign.is_none());
        wait_for(&dir.join("other-ran"));
        again.session.stop_blocking();
    }

    /// Runs `step` until it says it is done, polling the application's own parts as a frame does.
    fn until(app: &mut App, ctx: &egui::Context, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(app) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            let mut said = Vec::new();
            app.session.poll(&mut |line| said.push(line), &mut |_| {});
            for line in said {
                app.push_log(line);
            }
            app.poll_named_models(ctx);
            app.poll_lookup();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn p3_m08_with_ollama_running_the_models_are_listed_a_name_is_pulled_and_a_model_is_removed() {
        let dir = crate::testkit::scratch("m08");
        let fake = FakeOllama::start();
        fake.add_model("bonsai-q1", 248_302_385);
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        with_ollama_values(
            &mut settings,
            &[
                ("port", Value::Int(fake.port() as i64)),
                ("mode", Value::Str("attach".into())),
            ],
        );
        let mut app = app_on(&dir, settings);
        let ctx = egui::Context::default();

        // Start with no model: the daemon that is already running is used, and the window asks it
        // what it has, once
        let selection = launch::selection_from_values("ollama", None, app.values.map(), false);
        let plan = launch::plan_selection(
            &app.registry,
            &selection,
            Path::new("/nowhere/ollama"),
            app.machine.clone(),
            None,
        )
        .unwrap_or_else(|e| panic!("{e:#?}"));
        app.session.start(&plan, "", || {});
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        assert!(
            app.session.attached(),
            "it was already running: QuickAI did not start it"
        );
        assert!(app.form_facts().session_attached);
        until(&mut app, &ctx, "the list", |a| !a.models.is_empty());
        let names: Vec<&str> = app.models.iter().map(|m| m.filename.as_str()).collect();
        assert_eq!(names, ["bonsai-q1:latest"]);
        assert!(!app.named.stale);
        assert_eq!(
            fake.requests_to("/api/tags").len(),
            1,
            "asked when it came up, and not again"
        );

        // a name typed into the box is pulled with progress, and the model appears in the list
        app.model_input = "qwen3:4b".into();
        app.submit_model_input(&ctx);
        assert!(app.named.pull.is_some(), "{:?}", app.named.notice);
        assert!(
            app.model_input.is_empty(),
            "the box is cleared once it is taken"
        );
        until(&mut app, &ctx, "the pull to end", |a| {
            a.named.pull.is_none()
        });
        assert_eq!(app.named.notice, None);
        until(&mut app, &ctx, "the new model in the list", |a| {
            a.models.len() == 2
        });
        assert!(app.models.iter().any(|m| m.filename == "qwen3:4b"));
        assert_eq!(
            fake.requests_to("/api/pull")
                .iter()
                .map(|r| r.body.clone())
                .collect::<Vec<_>>(),
            [Some(json!({"model": "qwen3:4b"}))]
        );

        // a pull the daemon cannot do says so in its words under the box
        fake.behave(|b| b.pull.fail = Some("pull model manifest: file does not exist".into()));
        app.model_input = "nosuchmodel:1b".into();
        app.submit_model_input(&ctx);
        until(&mut app, &ctx, "the pull to fail", |a| {
            a.named.pull.is_none()
        });
        assert!(
            app.named
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("file does not exist")),
            "{:?}",
            app.named.notice
        );
        fake.behave(|b| b.pull.fail = None);

        // removing: by the name the engine lists, and the list is asked for again
        app.remove_listed_model("bonsai-q1:latest", &ctx);
        assert_eq!(app.settings_error, None);
        let deletes = fake.requests_to("/api/delete");
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].body, Some(json!({"model": "bonsai-q1:latest"})));
        until(&mut app, &ctx, "the list without it", |a| {
            a.models.iter().all(|m| m.filename != "bonsai-q1:latest")
        });

        // stopping an engine QuickAI did not start leaves it running
        app.session.stop();
        until(&mut app, &ctx, "the session to end", |a| {
            a.session.state() == ServerState::Idle
        });
        assert!(app.models.iter().any(|m| m.filename == "qwen3:4b"));
        assert!(!fake.requests_to("/api/version").is_empty());
    }

    #[test]
    fn p3_m09_start_loads_the_chosen_model_into_a_daemon_that_runs_without_one() {
        let dir = crate::testkit::scratch("m09");
        let fake = FakeOllama::start();
        fake.add_model("bonsai-q1", 248_302_385)
            .add_model("qwen3:4b", 2_500_000_000);
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        with_ollama_values(
            &mut settings,
            &[
                ("port", Value::Int(fake.port() as i64)),
                ("mode", Value::Str("attach".into())),
            ],
        );
        let mut app = app_on(&dir, settings);
        let ctx = egui::Context::default();
        let run_with_no_model = |app: &mut App| {
            let selection = launch::selection_from_values("ollama", None, app.values.map(), false);
            let plan = launch::plan_selection(
                &app.registry,
                &selection,
                Path::new("/nowhere/ollama"),
                app.machine.clone(),
                None,
            )
            .unwrap_or_else(|e| panic!("{e:#?}"));
            app.session.start(&plan, "", || {});
        };

        // nothing is chosen to load while the daemon is not running, and nothing to load until it has listed
        assert!(!app.can_load_selected());
        run_with_no_model(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        until(&mut app, &ctx, "the list", |a| a.models.len() == 2);
        assert_eq!(app.session.model_name(), None);
        assert!(
            app.can_load_selected(),
            "running with no model and one chosen: Start loads it"
        );

        // the one the person chose, not the first: a load is a warm-up call that keeps it for ever and
        // sets no context size, and Start does nothing while it is under way
        app.selected = app.models.len() - 1;
        let chosen = app.models[app.selected].filename.clone();
        assert_ne!(app.selected, 0);
        fake.behave(|b| b.load_delay = Duration::from_millis(500));
        app.load_selected();
        assert_eq!(app.session.loading(), Some(chosen.as_str()));
        assert!(
            !app.can_load_selected(),
            "a second click must not queue a second load"
        );
        app.load_selected();
        until(&mut app, &ctx, "the model to load", |a| {
            a.session.model_name() == Some(chosen.as_str())
        });
        assert_eq!(app.session.loading(), None);
        assert_eq!(fake.loaded().as_deref(), Some(chosen.as_str()));
        let loads: Vec<_> = fake
            .requests_to("/api/generate")
            .iter()
            .map(|r| r.body.clone())
            .collect();
        assert_eq!(
            loads,
            [Some(json!({"model": chosen, "keep_alive": -1}))],
            "one load, with the name the engine lists"
        );
        assert!(
            !app.can_load_selected(),
            "with a model loaded Start is what it was: Stop first"
        );

        // stopping an engine QuickAI did not start unloads what it loaded and leaves it running
        app.session.stop();
        until(&mut app, &ctx, "the session to end", |a| {
            a.session.state() == ServerState::Idle
        });
        assert_eq!(fake.loaded(), None);
        assert!(
            !app.can_load_selected(),
            "a stopped engine has nothing to load into, whatever the picker still lists"
        );

        // a stop that comes while a model is loading ends the run, and the next run is not left
        // believing a load is still under way
        fake.behave(|b| b.load_delay = Duration::from_secs(3));
        run_with_no_model(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running && a.models.len() == 2
        });
        app.load_selected();
        assert!(app.session.loading().is_some());
        app.session.stop();
        until(&mut app, &ctx, "the session to end", |a| {
            a.session.state() == ServerState::Idle
        });
        assert_eq!(app.session.loading(), None);

        // a model that will not load: the daemon keeps running with nothing loaded, the sentence is in
        // the log, and Start can try again
        fake.behave(|b| {
            b.load_delay = Duration::ZERO;
            b.load_failure = Some((500, "model requires more system memory".into()));
        });
        run_with_no_model(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running && a.models.len() == 2
        });
        app.selected = 1;
        app.load_selected();
        until(&mut app, &ctx, "the failure", |a| {
            a.log_lines
                .iter()
                .any(|l| l.contains("requires more system memory"))
        });
        until(&mut app, &ctx, "loading to end", |a| {
            a.session.loading().is_none()
        });
        assert_eq!(app.session.state(), ServerState::Running);
        assert_eq!(app.session.model_name(), None);
        assert!(app.can_load_selected(), "and Start tries again");
        app.session.stop_blocking();
    }

    /// An Ollama window whose releases are looked up at `api` (what a person's patch of the engine
    /// document says), and what the look-up button says when it is pressed.
    fn lookup_status_against(dir: &Path, api: &str) -> String {
        let patch = format!(
            "[acquire]\nfeed = {{ use = \"github_release\", repo = \"ollama/ollama\", per_page = 10, show = 5, \
             prereleases = false, digest = true, api = \"{api}\" }}\n"
        );
        let (mut app, _) = app_with_user_patch(dir, &patch);
        let ctx = egui::Context::default();
        let spec = crate::prims::feed::LookupSpec::from_engine(
            app.registry.engine("ollama").unwrap(),
            &app.host_facts,
        )
        .expect("the document has a feed");
        assert_eq!(spec.api_base, api, "the person's patch is where it looks");
        let texts = app.runtime_texts().unwrap();
        app.start_build_lookup(spec, &texts, &ctx);
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.runtime.lookup_rx.is_some() {
            assert!(Instant::now() < deadline, "the look-up never ended");
            app.poll_runtime_manager();
            std::thread::sleep(Duration::from_millis(10));
        }
        app.runtime.lookup_status.clone()
    }

    #[test]
    fn p3_r12_a_lookup_that_fails_keeps_the_sentence_it_gave_and_one_that_finds_nothing_says_so() {
        use crate::testkit::fake_github::{FakeGithub, Mode};
        let dir = crate::testkit::scratch("r12");

        // found: what the feed offers for this machine
        let fake = FakeGithub::ollama();
        let found = lookup_status_against(&dir.join("a"), &fake.base());
        assert!(found.starts_with("Found "), "{found}");

        // the allowance used up: the sentence about the limit, not "nothing found"
        let reset = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 600;
        fake.mode(Mode::RateLimited { reset_epoch: reset });
        let limited = lookup_status_against(&dir.join("b"), &fake.base());
        assert!(
            limited.contains("rate limit") && limited.contains("resets in about"),
            "{limited}"
        );

        // anything else that goes wrong: said as a failure
        fake.mode(Mode::Status(500));
        let failed = lookup_status_against(&dir.join("c"), &fake.base());
        assert!(failed.starts_with("Lookup failed:"), "{failed}");
        fake.mode(Mode::Garbage);
        let garbled = lookup_status_against(&dir.join("d"), &fake.base());
        assert!(garbled.starts_with("Lookup failed:"), "{garbled}");

        // a feed that has nothing for this machine says that, and only that
        let empty = FakeGithub::with_releases(json!([]));
        let nothing = lookup_status_against(&dir.join("e"), &empty.base());
        assert_eq!(nothing, "No compatible builds found.");
    }

    // ---- looking models up online (work package 3.15, group U; 4.9) -----------------------------------

    /// An application on Ollama whose look-up is pointed at `site` by a document of the person's.
    fn app_with_library(dir: &Path, site: &str) -> App {
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(cfg.join("engines")).unwrap();
        std::fs::write(
            cfg.join("engines/ollama.toml"),
            format!(
                "schema = 1\nkind = \"engine\"\nid = \"ollama\"\n\n[[models.lookup]]\nid = \"library\"\nsite = \"{site}\"\n"
            ),
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        app_over(dir, settings)
    }

    /// Waits for the look-up that was started to say its last word.
    fn until_looked_up(app: &mut App, ctx: &egui::Context) {
        until(app, ctx, "the look-up to end", |a| !a.lookup.running());
    }

    /// The reference a button of the entry called `name` pulls, by the words on it.
    fn reference_of(
        app: &App,
        name: &str,
        label_starts: &str,
    ) -> quickai_core::lookup::present::Fetch {
        app.lookup
            .entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} is not among the results"))
            .offers
            .iter()
            .find(|o| o.label.starts_with(label_starts))
            .unwrap_or_else(|| panic!("{name} has no button that says {label_starts}"))
            .fetch
            .clone()
    }

    #[test]
    fn p3_u21_nothing_is_asked_of_the_site_until_the_button_is_pressed_and_then_once() {
        use crate::testkit::fake_library::FakeLibrary;
        let dir = crate::testkit::scratch("u21");
        let library = FakeLibrary::start();
        let mut app = app_with_library(&dir, &library.base());
        let ctx = egui::Context::default();
        assert!(app.can_look_up());
        assert_eq!(
            quickai_core::models::lookups(app.engine_doc().unwrap())[0]
                .kind
                .site(),
            library.base()
        );

        // the window drawn, the engine's panels polled, as often as a frame: the site hears nothing
        for state in [ServerState::Idle, ServerState::Running, ServerState::Idle] {
            draw_everything(&mut app, &ctx, state);
            app.poll_named_models(&ctx);
        }
        std::thread::sleep(Duration::from_millis(200));
        assert!(library.requests().is_empty(), "{:?}", library.paths());
        assert!(!app.lookup.open && app.lookup.entries.is_empty());

        // the button, with the box empty: the site's own list of what is popular, once
        app.start_lookup(&ctx, "");
        assert!(app.lookup.open);
        assert!(app.lookup.status.starts_with("Looking"));
        until_looked_up(&mut app, &ctx);
        assert_eq!(app.lookup.entries.len(), 20);
        assert!(!app.lookup.failed);
        assert!(
            app.lookup
                .status
                .starts_with("Found 20 models in 127.0.0.1:"),
            "{}",
            app.lookup.status
        );
        draw_everything(&mut app, &ctx, ServerState::Idle);
        assert_eq!(library.paths(), ["/search?c=local"], "one request, no more");

        // the words in the box, a tag among them not counting; the box itself is left as it was
        app.model_input = "gpt:20b".into();
        let words = app.model_input.clone();
        app.start_lookup(&ctx, &words);
        assert!(
            app.lookup.entries.is_empty(),
            "the old results are gone while it looks"
        );
        until_looked_up(&mut app, &ctx);
        assert_eq!(app.lookup.entries.len(), 19);
        assert_eq!(app.model_input, "gpt:20b");
        assert_eq!(
            library.paths(),
            ["/search?c=local", "/search?q=gpt&c=local"]
        );
        draw_everything(&mut app, &ctx, ServerState::Idle);

        // closed with the X, they are not shown and not asked for again
        app.lookup.open = false;
        draw_everything(&mut app, &ctx, ServerState::Idle);
        assert_eq!(library.requests().len(), 2);
    }

    #[test]
    fn p3_u22_a_look_up_that_fails_keeps_its_sentence_and_one_that_finds_nothing_says_so() {
        use crate::testkit::fake_library::{Behaviour, FakeLibrary};
        let dir = crate::testkit::scratch("u22");
        let library = FakeLibrary::start();
        let mut app = app_with_library(&dir, &library.base());
        let ctx = egui::Context::default();

        library.behave(Behaviour::Error(503));
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        assert!(app.lookup.entries.is_empty());
        assert!(app.lookup.failed, "a failure is marked as one");
        assert!(
            app.lookup
                .status
                .contains("answered with an error (HTTP 503)"),
            "the failure is the last word: {}",
            app.lookup.status
        );
        draw_everything(&mut app, &ctx, ServerState::Idle);

        library.behave(Behaviour::Garbage);
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        assert!(
            app.lookup
                .status
                .contains("not one this version of QuickAI can read"),
            "{}",
            app.lookup.status
        );

        library.behave(Behaviour::Normal);
        app.start_lookup(&ctx, "zzzz");
        until_looked_up(&mut app, &ctx);
        assert!(
            app.lookup.status.starts_with("No models found in"),
            "{}",
            app.lookup.status
        );
        assert!(!app.lookup.failed, "finding nothing is not a failure");
        draw_everything(&mut app, &ctx, ServerState::Idle);

        // a site that is not there: told, and the window goes on
        let mut gone = app_with_library(&crate::testkit::scratch("u22-gone"), "http://127.0.0.1:1");
        gone.start_lookup(&ctx, "");
        until_looked_up(&mut gone, &ctx);
        assert!(
            gone.lookup
                .status
                .starts_with("Could not reach 127.0.0.1:1:"),
            "{}",
            gone.lookup.status
        );
    }

    #[test]
    fn p3_u23_a_row_is_pulled_through_the_rules_of_the_box_and_waits_for_a_running_daemon() {
        use crate::testkit::fake_library::FakeLibrary;
        use quickai_core::lookup::present::Fetch;
        let dir = crate::testkit::scratch("u23");
        let library = FakeLibrary::start();
        let fake = FakeOllama::start();
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        with_ollama_values(
            &mut settings,
            &[
                ("port", Value::Int(fake.port() as i64)),
                ("mode", Value::Str("attach".into())),
            ],
        );
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(cfg.join("engines")).unwrap();
        std::fs::write(
            cfg.join("engines/ollama.toml"),
            format!(
                "schema = 1\nkind = \"engine\"\nid = \"ollama\"\n\n[[models.lookup]]\nid = \"library\"\nsite = \"{}\"\n",
                library.base()
            ),
        )
        .unwrap();
        let mut app = app_over(&dir, settings);
        let ctx = egui::Context::default();
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        let four = reference_of(&app, "tev1", "4b");
        assert_eq!(four, Fetch::Reference("tev1:4b".into()));
        let tev1 = app
            .lookup
            .entries
            .iter()
            .find(|e| e.name == "tev1")
            .unwrap();
        assert_eq!(
            tev1.offers
                .iter()
                .map(|o| o.fetch.clone())
                .collect::<Vec<_>>(),
            [Fetch::Reference("tev1:0.8b".into()), four.clone()]
        );
        let bare = reference_of(&app, "laya", "latest");
        assert_eq!(bare, Fetch::Reference("laya".into()));

        // nothing is running: the sentence of the box, and nothing sent
        app.model_input = "typed words".into();
        app.fetch_offer(&four, &ctx);
        let said = app.named.notice.clone().unwrap();
        assert!(said.starts_with("Start Ollama first"), "{said}");
        assert!(app.named.pull.is_none());
        assert!(fake.requests_to("/api/pull").is_empty());
        assert_eq!(
            app.model_input, "typed words",
            "a row does not take the box"
        );

        // the daemon that is already running is attached
        let selection = launch::selection_from_values("ollama", None, app.values.map(), false);
        let plan = launch::plan_selection(
            &app.registry,
            &selection,
            Path::new("/nowhere/ollama"),
            app.machine.clone(),
            None,
        )
        .unwrap_or_else(|e| panic!("{e:#?}"));
        app.session.start(&plan, "", || {});
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });

        // a size is pulled as name:size, and the model it makes is in the list afterwards
        app.fetch_offer(&four, &ctx);
        assert!(app.named.pull.is_some());
        assert_eq!(app.model_input, "typed words", "still the person's");
        until(&mut app, &ctx, "the pull to end", |a| {
            a.named.pull.is_none()
        });
        assert_eq!(app.named.notice, None);
        until(&mut app, &ctx, "the new model in the list", |a| {
            a.models.iter().any(|m| m.filename == "tev1:4b")
        });
        // a model with no size shown is pulled by its name
        app.fetch_offer(&bare, &ctx);
        assert!(app.named.pull.is_some());
        until(&mut app, &ctx, "the pull to end", |a| {
            a.named.pull.is_none()
        });
        assert_eq!(
            fake.requests_to("/api/pull")
                .iter()
                .map(|r| r.body.clone())
                .collect::<Vec<_>>(),
            [
                Some(json!({"model": "tev1:4b"})),
                Some(json!({"model": "laya"}))
            ]
        );
        // the box's own rules still apply to what a row says: nothing refused is sent
        assert!(!app.pull_named("Not A Name", &ctx));
        assert!(app
            .named
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("does not look like a model name")));
        assert_eq!(fake.requests_to("/api/pull").len(), 2);
        app.session.stop();
        until(&mut app, &ctx, "the session to end", |a| {
            a.session.state() == ServerState::Idle
        });
    }

    #[test]
    fn p3_u24_choosing_another_engine_clears_the_results() {
        use crate::testkit::fake_library::FakeLibrary;
        let dir = crate::testkit::scratch("u24");
        let library = FakeLibrary::start();
        let mut app = app_with_library(&dir, &library.base());
        let ctx = egui::Context::default();
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        assert_eq!(app.lookup.entries.len(), 20);

        // llama.cpp has a look-up of its own; what Ollama found is not shown under it
        app.switch_engine("llamacpp");
        assert_eq!(app.engine, "llamacpp");
        assert!(!app.lookup.open && app.lookup.entries.is_empty());
        assert!(!app.lookup.running());
        assert!(
            app.can_look_up(),
            "llama.cpp declares the Hugging Face look-up"
        );
        draw_everything(&mut app, &ctx, ServerState::Idle);
        assert_eq!(
            library.requests().len(),
            1,
            "and nothing more was asked of it"
        );

        // back to Ollama: nothing is shown until it is asked for again
        app.switch_engine("ollama");
        assert!(!app.lookup.open && app.lookup.entries.is_empty());
    }

    #[test]
    fn p3_u25_the_results_draw_whatever_the_look_up_said() {
        use quickai_core::lookup::present::{Entry, Fetch, Offer};
        let dir = crate::testkit::scratch("u25");
        let mut app = app_on(&dir, {
            let mut s = Settings::default();
            s.set_chosen_engine("ollama");
            s
        });
        let ctx = egui::Context::default();
        let offer = |label: &str, bundle: Option<&str>, class: Option<&str>| Offer {
            bundle: bundle.map(str::to_string),
            label: label.to_string(),
            hover: format!("Pull {label}"),
            bytes: Some(1),
            estimated: true,
            class: class.map(str::to_string),
            recommended: label.starts_with('4'),
            fetch: Fetch::Reference(label.to_string()),
        };
        let entry = |name: &str, kind: Option<&str>, offers: Vec<Offer>, description: &str| Entry {
            key: format!("k:{name}"),
            name: name.into(),
            description: description.into(),
            downloads: Some(3_301_841),
            likes: Some(12),
            tags: vec!["tools".into(), "thinking".into()],
            model_type: kind.map(str::to_string),
            offers,
            card: None,
        };
        app.lookup.open = true;
        app.lookup.status = "Found 6 models in ollama.com's library.".into();
        app.lookup.entries = vec![
            entry("plain", Some("chat"), vec![offer("latest", None, None)], ""),
            entry(
                "sized",
                Some("vision"),
                vec![
                    offer("1b", None, Some("small")),
                    offer("2b", None, Some("small")),
                ],
                "A sentence.",
            ),
            entry(
                "many",
                Some("code"),
                (1..60)
                    .map(|i| offer(&format!("{i}b"), None, Some("large")))
                    .collect(),
                &"long ".repeat(400),
            ),
            Entry {
                downloads: None,
                likes: None,
                tags: Vec::new(),
                ..entry("bare", None, vec![offer("7b", None, Some("medium"))], "d")
            },
            entry(
                "bundled",
                Some("vision"),
                vec![
                    offer("Q4", None, Some("small")),
                    offer("Q4", Some("vision"), Some("medium")),
                ],
                "",
            ),
            entry(
                "odd",
                Some("no-such-type"),
                vec![offer("x", None, None)],
                "",
            ),
        ];
        for state in [ServerState::Idle, ServerState::Running] {
            app.named.notice = (state == ServerState::Running).then(|| "a notice".to_string());
            draw_everything(&mut app, &ctx, state);
        }
        // each size filter, and one no entry has anything in
        for size in ["small", "medium", "large", "no-such-class"] {
            app.lookup.size = Some(size.to_string());
            draw_everything(&mut app, &ctx, ServerState::Idle);
        }
        app.lookup.size = None;
        // only a status, and nothing at all
        app.lookup.entries.clear();
        draw_everything(&mut app, &ctx, ServerState::Idle);
        app.lookup.status.clear();
        draw_everything(&mut app, &ctx, ServerState::Idle);
        app.lookup.open = false;
        draw_everything(&mut app, &ctx, ServerState::Idle);
    }

    // ---- the look-up of llama.cpp's window, and what a result is fetched as (work package 4.9) --------

    /// llama.cpp's window with its look-up pointed at `site` by a document of the person's, which patches
    /// the look-up of the application by its `id`: the rest of what that says stays.
    fn llama_with_hub(dir: &Path, site: &str) -> App {
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(cfg.join("engines")).unwrap();
        std::fs::write(
            cfg.join("engines/llamacpp.toml"),
            format!(
                "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\n[[models.lookup]]\nid = \"hub\"\nsite = \"{site}\"\n"
            ),
        )
        .unwrap();
        let settings = Settings {
            models_dir_override: Some(dir.join("models").display().to_string()),
            ..Settings::default()
        };
        let mut app = app_over(dir, settings);
        // the models that ship beside the executable of a checkout are not this test's business
        app.bundled_models_dir = dir.join("bundled");
        app.load_engine_state();
        app
    }

    /// Frames, until what is being downloaded has come.
    fn until_downloaded(app: &mut App, _ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.download.is_some() {
            assert!(Instant::now() < deadline, "the download did not end");
            app.poll_download();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    const RECORDED: [&str; 6] = [
        "mudler/locate-anything.cpp-gguf",
        "unsloth/Qwen3.8-27B-GGUF",
        "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
        "mixedbread-ai/mxbai-embed-large-v1",
        "mudler/Laguna-XS-2.1-APEX-GGUF",
        "ggml-org/gemma-3-4b-it-GGUF",
    ];

    #[test]
    fn p3_v16_the_window_of_llamacpp_looks_up_the_hub_when_asked_and_draws_what_it_finds() {
        use crate::testkit::fake_hub::{Behaviour, FakeHub};
        let dir = crate::testkit::scratch("v16");
        let hub = FakeHub::start();
        hub.only(&RECORDED);
        let mut app = llama_with_hub(&dir, &hub.base());
        let ctx = egui::Context::default();
        assert_eq!(app.engine, "llamacpp");
        assert!(app.can_look_up());
        // the look-up of the application, moved to the fake: the rest of what its document says stays
        let declared = quickai_core::models::lookups(app.engine_doc().unwrap())[0]
            .kind
            .clone();
        match &declared {
            quickai_core::registry::engine::LookupKind::HuggingFace {
                site,
                popular,
                read,
                ..
            } => {
                assert_eq!(site, &hub.base());
                assert_eq!(*popular, 30);
                assert_eq!(read.show_variants, 4);
            }
            other => panic!("{other:?}"),
        }

        // drawn as often as a frame: the site hears nothing
        for state in [ServerState::Idle, ServerState::Idle] {
            draw_everything(&mut app, &ctx, state);
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(hub.requests().is_empty());

        // Look up online: the most downloaded; each model is of a type, with its buttons
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        assert_eq!(app.lookup.entries.len(), 6);
        assert!(!app.lookup.failed);
        assert!(
            app.lookup
                .status
                .starts_with("Found 6 models on 127.0.0.1:"),
            "{}",
            app.lookup.status
        );
        let sections = quickai_core::lookup::view::arrange(
            &app.lookup.entries,
            app.registry.vocabulary().model_types(),
            None,
        );
        assert_eq!(
            sections
                .iter()
                .map(|s| (s.type_id.as_str(), s.entries.len()))
                .collect::<Vec<_>>(),
            [("code", 2), ("vision", 2), ("embedding", 1), ("other", 1)]
        );
        for size in [None, Some("small"), Some("medium"), Some("large")] {
            app.lookup.size = size.map(str::to_string);
            draw_everything(&mut app, &ctx, ServerState::Idle);
        }
        app.lookup.size = None;

        // Search with words, as the box does: the phrase, and then its words
        app.start_lookup(&ctx, "qwen coder");
        until_looked_up(&mut app, &ctx);
        assert!(hub
            .paths()
            .iter()
            .any(|p| p.ends_with("&search=qwen%20coder")));
        assert!(!app.lookup.entries.is_empty());

        // a site that fails: the sentence stays, red, and nothing is said to be missing
        hub.behave(Behaviour::Error(503));
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        assert!(app.lookup.failed && app.lookup.entries.is_empty());
        assert!(
            app.lookup
                .status
                .contains("answered with an error (HTTP 503)"),
            "{}",
            app.lookup.status
        );
        draw_everything(&mut app, &ctx, ServerState::Idle);
        // closed with the X, it is not shown and not asked for again
        let asked = hub.requests().len();
        app.lookup.open = false;
        draw_everything(&mut app, &ctx, ServerState::Idle);
        assert_eq!(hub.requests().len(), asked);
    }

    /// The fake hub with one small vision repository (two quantizations and a projector), and an
    /// application whose look-up reads it.
    fn tiny_world(
        name: &str,
    ) -> (
        crate::testkit::Scratch,
        crate::testkit::fake_hub::FakeHub,
        App,
    ) {
        let dir = crate::testkit::scratch(name);
        let hub = crate::testkit::fake_hub::FakeHub::start();
        hub.add_repo(
            "me/tiny-vl-GGUF",
            Some("image-text-to-text"),
            &["gguf"],
            &[
                ("tiny-Q4_K_M.gguf", 3000),
                ("tiny-Q8_0.gguf", 5000),
                ("mmproj-f16.gguf", 1000),
            ],
        );
        hub.only(&["me/tiny-vl-GGUF"]);
        let app = llama_with_hub(&dir, &hub.base());
        (dir, hub, app)
    }

    /// The button of the first result that says `label` and is in the bundle `bundle` (none: the weights).
    fn button(
        app: &App,
        label: &str,
        bundle: Option<&str>,
    ) -> quickai_core::lookup::present::Fetch {
        app.lookup.entries[0]
            .offers
            .iter()
            .find(|o| o.label.starts_with(label) && o.bundle.as_deref() == bundle)
            .unwrap_or_else(|| {
                panic!(
                    "no button `{label}` in {bundle:?}: {:?}",
                    app.lookup.entries[0]
                        .offers
                        .iter()
                        .map(|o| (&o.label, &o.bundle))
                        .collect::<Vec<_>>()
                )
            })
            .fetch
            .clone()
    }

    fn model_names(app: &App) -> Vec<&str> {
        app.models.iter().map(|m| m.filename.as_str()).collect()
    }

    #[test]
    fn p3_b17_a_button_of_a_bundle_downloads_both_files_and_the_picker_lists_the_weights_alone() {
        let (dir, hub, mut app) = tiny_world("b17");
        let ctx = egui::Context::default();
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        assert_eq!(app.lookup.entries.len(), 1);
        let entry = &app.lookup.entries[0];
        assert_eq!(entry.model_type.as_deref(), Some("vision"));
        assert_eq!(
            entry.offers.iter().filter(|o| o.bundle.is_none()).count(),
            2
        );
        assert_eq!(
            entry
                .offers
                .iter()
                .filter(|o| o.bundle.as_deref() == Some("vision"))
                .count(),
            2
        );

        // the quantization with its projector: one download, the weights selected afterwards
        let both = button(&app, "Q4_K_M", Some("vision"));
        app.fetch_offer(&both, &ctx);
        assert!(app.download.is_some());
        until_downloaded(&mut app, &ctx);
        let models = dir.join("models");
        assert!(models.join("tiny-Q4_K_M.gguf").is_file());
        assert!(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf").is_file());
        assert_eq!(
            model_names(&app),
            ["tiny-Q4_K_M.gguf"],
            "the projector is not a model"
        );
        assert_eq!(app.models[app.selected].filename, "tiny-Q4_K_M.gguf");
        let selected = &app.models[app.selected];
        assert_eq!(selected.companions.len(), 1);
        assert!(selected.companions[0].exists());
        assert!(
            selected
                .note
                .contains("With its projector: mmproj-f16.gguf."),
            "{}",
            selected.note
        );
        let log: Vec<&String> = app.log_lines.iter().collect();
        for said in [
            "Downloading tiny-Q4_K_M.gguf (1 of 2)",
            "Downloading mmproj-f16.gguf (2 of 2)",
            "Download complete: tiny-Q4_K_M.gguf",
        ] {
            assert!(
                log.iter().any(|l| l.as_str() == said),
                "`{said}` is not in the log: {log:?}"
            );
        }
        // the file is not fetched twice: the other quantization shares the projector
        let other = button(&app, "Q8_0", Some("vision"));
        app.fetch_offer(&other, &ctx);
        until_downloaded(&mut app, &ctx);
        assert_eq!(
            hub.downloads(),
            [
                "me/tiny-vl-GGUF:tiny-Q4_K_M.gguf",
                "me/tiny-vl-GGUF:mmproj-f16.gguf",
                "me/tiny-vl-GGUF:tiny-Q8_0.gguf",
            ]
        );
        assert_eq!(model_names(&app), ["tiny-Q4_K_M.gguf", "tiny-Q8_0.gguf"]);
        assert_eq!(app.models[app.selected].filename, "tiny-Q8_0.gguf");
        // only the weights: the same as ever, with no record and no companion
        std::fs::remove_dir_all(&models).unwrap();
        let alone = button(&app, "Q4_K_M", None);
        app.fetch_offer(&alone, &ctx);
        until_downloaded(&mut app, &ctx);
        assert_eq!(model_names(&app), ["tiny-Q4_K_M.gguf"]);
        assert!(app.models[0].companions.is_empty());
        assert!(!models.join("tiny-Q4_K_M.gguf.bundle.json").exists());
        // a second button is not taken while one download runs
        let again = button(&app, "Q8_0", None);
        app.fetch_offer(&again, &ctx);
        let asked = hub.downloads().len();
        app.fetch_offer(&again, &ctx);
        until_downloaded(&mut app, &ctx);
        assert_eq!(hub.downloads().len(), asked + 1, "one download, not two");
        // a name to pull is not something llama.cpp does
        app.fetch_offer(
            &quickai_core::lookup::present::Fetch::Reference("qwen3:4b".into()),
            &ctx,
        );
        assert!(
            app.log_lines
                .back()
                .unwrap()
                .contains("does not pull models by name"),
            "{:?}",
            app.log_lines.back()
        );
    }

    #[test]
    fn p3_b18_start_puts_the_projector_on_the_command_line_and_says_when_it_has_gone() {
        let (dir, _hub, mut app) = tiny_world("b18");
        let ctx = egui::Context::default();
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        let both = button(&app, "Q4_K_M", Some("vision"));
        app.fetch_offer(&both, &ctx);
        until_downloaded(&mut app, &ctx);
        let args_of = |app: &mut App, model: &crate::models::ModelEntry| -> Vec<String> {
            let selection = app.run_selection(Some(model));
            let plan = launch::plan_selection(
                &app.registry,
                &selection,
                Path::new("/opt/llama-server"),
                app.machine.clone(),
                None,
            )
            .unwrap_or_else(|e| panic!("{e:#?}"));
            match &plan.units[0].start[0] {
                quickai_core::plan::Start::Spawn(spawn) => spawn.arg_values(),
                other => panic!("{other:?}"),
            }
        };

        let model = app.models[app.selected].clone();
        let projector = dir
            .join("models/me--tiny-vl-GGUF--mmproj-f16.gguf")
            .display()
            .to_string();
        let args = args_of(&mut app, &model);
        let at = args
            .iter()
            .position(|a| a == "--mmproj")
            .expect("--mmproj is on the command line");
        assert_eq!(args[at + 1], projector);
        assert_eq!(
            args[0..2],
            [
                "--model".to_string(),
                dir.join("models/tiny-Q4_K_M.gguf").display().to_string()
            ]
        );

        // the file has gone: said in the log, and the model starts without it
        std::fs::remove_file(&projector).unwrap();
        app.rescan_models();
        let model = app.models[app.selected].clone();
        let args = args_of(&mut app, &model);
        assert!(!args.iter().any(|a| a == "--mmproj"), "{args:?}");
        assert!(
            app.log_lines.back().unwrap().contains(
                "mmproj-f16.gguf is not in the models folder any more: starting without it"
            ),
            "{:?}",
            app.log_lines.back()
        );
        // a model that has no companion is started as it always was
        let plain = crate::models::ModelEntry {
            filename: "plain.gguf".into(),
            path: dir.join("models/plain.gguf"),
            display: "plain.gguf".into(),
            note: String::new(),
            bundled: false,
            size_bytes: 1,
            companions: Vec::new(),
            mode: None,
        };
        let args = args_of(&mut app, &plain);
        assert!(!args.iter().any(|a| a.contains("mmproj")), "{args:?}");
    }

    #[test]
    fn p3_b19_deleting_a_model_deletes_its_record_and_the_projector_that_nothing_else_uses() {
        let (dir, _hub, mut app) = tiny_world("b19");
        let ctx = egui::Context::default();
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app, &ctx);
        for label in ["Q4_K_M", "Q8_0"] {
            let both = button(&app, label, Some("vision"));
            app.fetch_offer(&both, &ctx);
            until_downloaded(&mut app, &ctx);
        }
        let models = dir.join("models");
        assert_eq!(model_names(&app), ["tiny-Q4_K_M.gguf", "tiny-Q8_0.gguf"]);

        // the modal says the projector goes with it, unless another model uses it
        let first = app
            .models
            .iter()
            .find(|m| m.filename == "tiny-Q4_K_M.gguf")
            .unwrap()
            .path
            .clone();
        app.pending_delete = Some(PendingDelete::Model {
            filename: "tiny-Q4_K_M.gguf".into(),
            path: first.clone(),
        });
        draw_everything(&mut app, &ctx, ServerState::Idle);
        app.pending_delete = None;

        app.delete_model_files("tiny-Q4_K_M.gguf", &first);
        assert_eq!(model_names(&app), ["tiny-Q8_0.gguf"]);
        assert!(
            models.join("me--tiny-vl-GGUF--mmproj-f16.gguf").exists(),
            "the other quantization still uses it"
        );
        assert!(!models.join("tiny-Q4_K_M.gguf.bundle.json").exists());
        let log: Vec<&String> = app.log_lines.iter().rev().take(3).collect();
        assert!(
            log.iter()
                .any(|l| l.as_str()
                    == "Kept me--tiny-vl-GGUF--mmproj-f16.gguf: another model uses it."),
            "{log:?}"
        );
        assert!(
            log.iter()
                .any(|l| l.as_str() == "Deleted model: tiny-Q4_K_M.gguf"),
            "{log:?}"
        );

        let last = app.models[0].path.clone();
        app.delete_model_files("tiny-Q8_0.gguf", &last);
        assert!(app.models.is_empty());
        assert!(
            !models.join("me--tiny-vl-GGUF--mmproj-f16.gguf").exists(),
            "the last model that used it takes it"
        );
        let left: Vec<String> = std::fs::read_dir(&models)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(left.is_empty(), "{left:?}");
        assert!(
            app.log_lines
                .iter()
                .any(|l| l.starts_with("Also deleted: ") && l.contains("mmproj-f16.gguf")),
            "{:?}",
            app.log_lines
        );
    }

    #[test]
    fn p3_b20_a_bundle_that_comes_in_part_still_lists_the_weights_and_says_what_is_missing() {
        use quickai_core::lookup::huggingface::file_url;
        use quickai_core::lookup::present::{Download, Fetch, Piece};
        let (dir, hub, mut app) = tiny_world("b20");
        let ctx = egui::Context::default();
        let piece = |kind: Option<&str>, store_as: &str, name: &str, url: String| Piece {
            kind: kind.map(str::to_string),
            label: kind.map_or("model", |_| "projector").to_string(),
            url,
            store_as: store_as.to_string(),
            name: name.to_string(),
            size: Some(1000),
            sha256: None,
            place: None,
        };
        let download = Download {
            files: vec![
                piece(
                    None,
                    "half-Q4_K_M.gguf",
                    "half-Q4_K_M.gguf",
                    file_url(&hub.base(), "me/tiny-vl-GGUF", "main", "tiny-Q4_K_M.gguf"),
                ),
                piece(
                    Some("mmproj"),
                    "me--x--mmproj.gguf",
                    "mmproj.gguf",
                    "http://127.0.0.1:1/gone".to_string(),
                ),
            ],
            bundle: Some("vision".into()),
            source: "huggingface:me/tiny-vl-GGUF".into(),
            mode: None,
        };
        app.fetch_offer(&Fetch::Files(download), &ctx);
        until_downloaded(&mut app, &ctx);
        assert_eq!(
            model_names(&app),
            ["half-Q4_K_M.gguf"],
            "the weights run alone, so they are listed"
        );
        assert!(
            app.models[0].companions.is_empty(),
            "and nothing says they have a pair"
        );
        assert!(dir.join("models/half-Q4_K_M.gguf").is_file());
        let failed = app
            .log_lines
            .iter()
            .find(|l| l.starts_with("Download failed:"))
            .expect("the failure is in the log");
        assert!(
            failed.contains("could not fetch mmproj.gguf")
                && failed.contains("half-Q4_K_M.gguf was fetched and runs without it"),
            "{failed}"
        );
    }

    /// Run by hand: the real thing, end to end. A real bundle (`ggml-org/SmolVLM-256M-Instruct-GGUF`, a repository with
    /// no pipeline tag, 175 MB of weights and a projector of 190 MB) is looked up on the real hub, fetched by the button
    /// of its row, listed by the picker as the weights alone and started by a real `llama-server` with its projector:
    /// the server must say it takes pictures (`/props`, `modalities.vision`). When the look-up reads the hub wrongly,
    /// when the projector is not the file the server wants, or when `--mmproj` is not what it is told, this is the
    /// test that says so; it is the one thing the fakes cannot.
    #[test]
    #[ignore = "manual: needs the internet (about 365 MB) and a real llama-server (QUICKAI_REAL_LLAMA=<llama-server> cargo test -p quickai p3_b21 -- --ignored)"]
    fn p3_b21_a_real_bundle_is_fetched_and_the_real_server_loads_it_with_its_projector() {
        let binary = std::env::var("QUICKAI_REAL_LLAMA")
            .expect("QUICKAI_REAL_LLAMA is the llama-server to run");
        let dir = crate::testkit::scratch("b21");
        let port = crate::testkit::free_port();
        let settings = Settings {
            port,
            ctx_size: 2048,
            n_gpu_layers: 0,
            llama_bin_override: Some(binary),
            models_dir_override: Some(dir.join("models").display().to_string()),
            ..Settings::default()
        };
        // the documents of the application as they are: the real hub
        let mut app = app_over(&dir, settings);
        app.bundled_models_dir = dir.join("bundled");
        app.load_engine_state();
        let ctx = egui::Context::default();

        app.start_lookup(&ctx, "SmolVLM-256M");
        let deadline = Instant::now() + Duration::from_secs(180);
        while app.lookup.running() {
            assert!(Instant::now() < deadline, "the look-up did not end");
            app.poll_lookup();
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!app.lookup.failed, "{}", app.lookup.status);
        let entry = app
            .lookup
            .entries
            .iter()
            .find(|e| e.name == "ggml-org/SmolVLM-256M-Instruct-GGUF")
            .unwrap_or_else(|| {
                panic!(
                    "not found: {:?}",
                    app.lookup
                        .entries
                        .iter()
                        .map(|e| &e.name)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            entry.model_type.as_deref(),
            Some("vision"),
            "no pipeline tag, but a projector beside the weights"
        );
        let bundle = entry
            .offers
            .iter()
            .find(|o| o.bundle.as_deref() == Some("vision") && o.label.starts_with("Q8_0"))
            .unwrap_or_else(|| {
                panic!(
                    "no bundle of Q8_0: {:?}",
                    entry.offers.iter().map(|o| &o.label).collect::<Vec<_>>()
                )
            })
            .fetch
            .clone();

        app.fetch_offer(&bundle, &ctx);
        let deadline = Instant::now() + Duration::from_secs(1800);
        while app.download.is_some() {
            assert!(Instant::now() < deadline, "the download did not end");
            app.poll_download();
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(
            app.log_lines
                .iter()
                .any(|l| l.starts_with("Download complete:")),
            "{:?}",
            app.log_lines
        );
        assert_eq!(model_names(&app), ["SmolVLM-256M-Instruct-Q8_0.gguf"]);
        let model = &app.models[app.selected];
        assert_eq!(model.companions.len(), 1);
        assert_eq!(
            model.companions[0].name,
            "mmproj-SmolVLM-256M-Instruct-f16.gguf"
        );
        assert!(model.companions[0].exists());

        // started the way the Start button does it, by the real server
        app.start_selected(&ctx);
        until(&mut app, &ctx, "the real server to be running", |a| {
            a.session.state() == ServerState::Running
        });
        let log: Vec<&String> = app.log_lines.iter().collect();
        assert!(
            log.iter()
                .any(|l| l.starts_with("$ ") && l.contains("--mmproj ")),
            "the command line has no --mmproj: {log:?}"
        );
        let props: serde_json::Value = ureq::get(format!("http://127.0.0.1:{port}/props"))
            .call()
            .expect("/props")
            .into_body()
            .read_json()
            .expect("JSON");
        assert_eq!(
            props["modalities"]["vision"],
            json!(true),
            "the server was started with the projector and does not take pictures: {props}"
        );
        app.session.stop_blocking();
    }

    // ------------------------------------------------------------------ the recommended models (J)

    /// A world with the fake hub serving two repositories, and a catalog of the person's that lists
    /// them with the hashes their bytes really have, one that lies about its hash, one too big for any
    /// machine and one that is only a name.
    fn catalog_world(
        name: &str,
    ) -> (
        crate::testkit::Scratch,
        crate::testkit::fake_hub::FakeHub,
        App,
    ) {
        use crate::testkit::fake_hub::{content, sha256_hex, FakeHub};
        let dir = crate::testkit::scratch(name);
        let hub = FakeHub::start();
        for (repo, files) in [
            (
                "me/tiny-vl-GGUF",
                vec![
                    ("tiny-Q4_K_M.gguf", 3000),
                    ("tiny-Q8_0.gguf", 5000),
                    ("mmproj-f16.gguf", 1000),
                ],
            ),
            ("me/vec-GGUF", vec![("vec-Q8_0.gguf", 2000)]),
            ("me/liar-GGUF", vec![("liar-Q4_K_M.gguf", 1500)]),
        ] {
            hub.add_repo(repo, None, &["gguf"], &files);
        }
        hub.only(&[]);
        let hash = |repo: &str, file: &str, len: usize| sha256_hex(&content(repo, file, len));
        let document = format!(
            r#"schema = 1
kind = "catalog"
id = "t"
title = "Test"
verified = "2026-10-07"

[[model]]
id = "tiny-vl"
name = "Tiny VL"
by = "Acme"
type = "vision"
note = "Describes pictures."
license = "Apache-2.0"
params = "0.1B"
context = 8192
input = ["text", "image"]
features = ["fast"]
start = true
[model.run.llamacpp]
repo = "me/tiny-vl-GGUF"
rev = "0123456789abcdef0123456789abcdef01234567"
files = [
  {{ file = "tiny-Q4_K_M.gguf", size = 3000, sha256 = "{q4}", best = true }},
  {{ file = "tiny-Q8_0.gguf", size = 5000, sha256 = "{q8}" }},
]
[model.run.llamacpp.companions]
mmproj = {{ file = "mmproj-f16.gguf", size = 1000, sha256 = "{proj}" }}
[model.run.ollama]
ref = "tiny-vl:1b"
size = 900

[[model]]
id = "vec"
name = "Vec Maker"
by = "Acme"
type = "embedding"
note = "For search."
[model.run.llamacpp]
repo = "me/vec-GGUF"
mode = "embedding"
files = [ {{ file = "vec-Q8_0.gguf", size = 2000, sha256 = "{vec}" }} ]

[[model]]
id = "liar"
name = "Liar"
note = "Lists a hash its file does not have."
[model.run.llamacpp]
repo = "me/liar-GGUF"
files = [ {{ file = "liar-Q4_K_M.gguf", size = 1500, sha256 = "{zero}" }} ]

[[model]]
id = "huge"
name = "Huge"
note = "More than any computer here."
[model.run.llamacpp]
repo = "me/huge-GGUF"
files = [ {{ file = "huge-Q4_K_M.gguf", size = 600000000000, sha256 = "{zero}" }} ]
"#,
            q4 = hash("me/tiny-vl-GGUF", "tiny-Q4_K_M.gguf", 3000),
            q8 = hash("me/tiny-vl-GGUF", "tiny-Q8_0.gguf", 5000),
            proj = hash("me/tiny-vl-GGUF", "mmproj-f16.gguf", 1000),
            vec = hash("me/vec-GGUF", "vec-Q8_0.gguf", 2000),
            zero = "0".repeat(64),
        );
        std::fs::create_dir_all(dir.join("cfg/catalog")).unwrap();
        std::fs::write(dir.join("cfg/catalog/t.toml"), document).unwrap();
        let app = llama_with_hub(&dir, &hub.base());
        (dir, hub, app)
    }

    fn catalog_names(app: &App) -> Vec<&str> {
        app.catalog
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect()
    }

    /// The button of the entry called `name` that says `label` in the bundle `bundle`.
    fn catalog_button(app: &App, name: &str, label: &str, bundle: Option<&str>) -> Fetch {
        app.catalog
            .entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("no `{name}` in {:?}", catalog_names(app)))
            .offers
            .iter()
            .find(|o| o.label.starts_with(label) && o.bundle.as_deref() == bundle)
            .unwrap_or_else(|| panic!("no button `{label}` of {name}"))
            .fetch
            .clone()
    }

    /// A configuration for llama.cpp that names a model, and what a person is told about it here.
    fn llama_configuration(app: &App, model_toml: &str) -> quickai_core::profile::Configuration {
        let text = format!(
            "schema = 1\nkind = \"profile\"\nid = \"given\"\nname = \"Given\"\nengine = \"llamacpp\"\n{model_toml}"
        );
        quickai_core::profile::check::open_text(&app.registry, &text, env!("CARGO_PKG_VERSION"))
            .unwrap_or_else(|n| panic!("{}", n.text(app.registry.vocabulary())))
            .configuration
    }

    fn notes_of(
        app: &App,
        configuration: &quickai_core::profile::Configuration,
    ) -> Vec<quickai_core::profile::notes::Note> {
        let installed = app.installed_now();
        let here = app.configuration_inventory(Some(configuration), &installed);
        quickai_core::profile::check::review(&app.registry, configuration, &[], &here, false).notes
    }

    #[test]
    fn p3_a61_the_note_of_a_model_of_the_list_that_is_not_here_has_a_button_that_fetches_it_after_a_click(
    ) {
        use super::super::configurations_window::NoteAction;
        let (dir, hub, mut app) = catalog_world("a61");
        let ctx = egui::Context::default();
        let configuration = llama_configuration(
            &app,
            "[[model]]\ncatalog = \"tiny-vl\"\nfile = \"tiny-Q4_K_M.gguf\"\n",
        );
        // not here: the note says what it costs and where it can be had, and has a button
        let notes = notes_of(&app, &configuration);
        let note = notes
            .iter()
            .find(|n| n.id == "model_to_get")
            .expect("a note");
        let said = note.text(app.registry.vocabulary());
        assert!(
            said.starts_with("Not here: ") && said.contains("tiny-Q4_K_M.gguf"),
            "{said}"
        );
        let (action, label, disabled) = app.action_for(note, &configuration).expect("a button");
        assert_eq!((label, disabled), ("Get", None));
        assert_eq!(
            action,
            NoteAction::GetCatalog {
                id: "tiny-vl".into(),
                file: "tiny-Q4_K_M.gguf".into()
            }
        );
        // nothing was asked of the site, and nothing is fetched, until the click
        assert!(app.download.is_none());
        assert!(hub.downloads().is_empty(), "{:?}", hub.downloads());
        app.run_note_action(action, &ctx);
        assert!(app.download.is_some(), "the Recommended window's own job");
        until_downloaded(&mut app, &ctx);
        let models = dir.join("models");
        // the weights, and what the list pairs with them (a projector): the same as its window's button
        assert!(models.join("tiny-Q4_K_M.gguf").is_file());
        assert!(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf").is_file());
        assert!(models.join("tiny-Q4_K_M.gguf.bundle.json").is_file());
        // and the note is gone
        assert!(notes_of(&app, &configuration)
            .iter()
            .all(|n| n.id != "model_to_get"));
        // for another engine the button waits until the configuration is chosen
        let mut other = configuration.clone();
        other.engine = "ollama".into();
        assert_eq!(
            app.action_for(note, &other).unwrap().2.as_deref(),
            Some("Choose this configuration first: it is for another engine.")
        );
    }

    #[test]
    fn p3_a62_a_file_that_is_not_here_has_a_button_that_looks_it_up_after_a_click_and_a_path_is_refused(
    ) {
        use super::super::configurations_window::NoteAction;
        let (_dir, hub, mut app) = catalog_world("a62");
        let ctx = egui::Context::default();
        let configuration = llama_configuration(
            &app,
            "[[model]]\nfile = \"Qwen3-VL-8B-Q4_K_M.gguf\"\nsize = 2800000000\n",
        );
        let notes = notes_of(&app, &configuration);
        let note = notes
            .iter()
            .find(|n| n.id == "model_unknown")
            .expect("a note");
        assert!(
            note.text(app.registry.vocabulary())
                .contains("does not say where it came from"),
            "{}",
            note.text(app.registry.vocabulary())
        );
        let (action, label, disabled) = app.action_for(note, &configuration).expect("a button");
        assert_eq!((label, disabled), ("Look up online", None));
        // the name of the file without its quantization is what is looked up
        assert_eq!(
            action,
            NoteAction::LookUp {
                query: "Qwen3-VL-8B".into()
            }
        );
        // nothing is asked until the click; then the look-up is the person's, with the words in the box
        assert!(hub.requests().is_empty());
        app.run_note_action(action, &ctx);
        assert_eq!(app.model_input, "Qwen3-VL-8B");
        assert!(app.lookup.running() || !app.lookup.status.is_empty());
        // a file that remembers where it came from looks up that repository's name
        let known = llama_configuration(
            &app,
            "[[model]]\nfile = \"Mine-Q4_K_M.gguf\"\nsize = 100\nfrom = \"huggingface:acme/Mine-GGUF\"\n",
        );
        let notes = notes_of(&app, &known);
        let note = notes
            .iter()
            .find(|n| n.id == "model_to_get")
            .expect("a note");
        assert!(note
            .text(app.registry.vocabulary())
            .ends_with("It can be got from huggingface.co/acme/Mine-GGUF."));
        assert_eq!(
            app.action_for(note, &known).unwrap().0,
            NoteAction::LookUp {
                query: "Mine-GGUF".into()
            }
        );
        // a name that is a path is refused before anything is looked for
        for bad in ["../../etc/passwd", "sub/x.gguf", "C:x.gguf"] {
            let text = format!(
                "schema = 1\nkind = \"profile\"\nid = \"x\"\nengine = \"llamacpp\"\n[[model]]\nfile = \"{bad}\"\n"
            );
            assert!(
                quickai_core::profile::check::open_text(&app.registry, &text, "0.1.0").is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn p3_j19_the_window_opens_without_asking_anything_and_shows_what_the_engine_offers() {
        let (_dir, hub, mut app) = catalog_world("j19");
        let ctx = egui::Context::default();
        assert!(app.can_recommend());
        assert!(!app.catalog.open);
        // drawn as often as a frame, closed: the site hears nothing and nothing is built
        draw_everything(&mut app, &ctx, ServerState::Idle);
        assert!(app.catalog.entries.is_empty());

        app.open_catalog();
        assert!(app.catalog.open);
        // what the engine offers, smallest first; the shipped list is there too
        let names = catalog_names(&app);
        assert!(
            names.contains(&"Tiny VL") && names.contains(&"Vec Maker") && names.contains(&"Huge")
        );
        let at = |n: &str| names.iter().position(|x| *x == n).unwrap();
        assert!(
            at("Vec Maker") < at("Tiny VL") && at("Tiny VL") < at("Huge"),
            "{names:?}"
        );
        // it opens on what the list says to start with
        assert_eq!(
            app.catalog.browse.scope,
            quickai_core::lookup::view::Scope::Start
        );
        for state in [ServerState::Idle, ServerState::Running] {
            draw_everything(&mut app, &ctx, state);
        }
        // every choice of the window draws: a scope, words, a size
        use quickai_core::lookup::view::Scope;
        for scope in [
            Scope::All,
            Scope::Start,
            Scope::Type("vision".into()),
            Scope::Type("embedding".into()),
        ] {
            for (words, size) in [
                ("", None),
                ("tiny", None),
                ("zzzz", None),
                ("", Some("small")),
                ("", Some("large")),
            ] {
                app.catalog.browse.scope = scope.clone();
                app.catalog.browse.words = words.to_string();
                app.catalog.browse.size = size.map(str::to_string);
                draw_everything(&mut app, &ctx, ServerState::Idle);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            hub.requests().is_empty(),
            "opening and browsing ask nothing: {:?}",
            hub.paths()
        );

        // what is known of a model is on its entry
        let tiny = app
            .catalog
            .entries
            .iter()
            .find(|e| e.name == "Tiny VL")
            .unwrap();
        let card = tiny.card.as_ref().unwrap();
        assert_eq!(
            (
                card.by.as_str(),
                card.license.as_str(),
                card.params.as_str()
            ),
            ("Acme", "Apache-2.0", "0.1B")
        );
        assert!(card.start);
        let line = super::super::lookup_view::facts_line(tiny);
        assert_eq!(line, "Acme · 0.1B · Apache-2.0 · 8.2K context", "{line}");
        assert_eq!(
            super::super::lookup_view::card_chips(card),
            ["sees images", "fast"]
        );

        // closed with the window's own X, it is not drawn; another engine starts again from nothing
        app.catalog.open = false;
        draw_everything(&mut app, &ctx, ServerState::Idle);
        app.open_catalog();
        app.load_engine_state();
        assert!(!app.catalog.open && app.catalog.entries.is_empty());
    }

    /// One frame of `show` on a screen of `size`, without the textures a renderer would take.
    fn frame(
        ctx: &egui::Context,
        size: egui::Vec2,
        show: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        frame_with(ctx, size, Vec::new(), show)
    }

    /// The same frame with what a person did in it: the pointer moved, the wheel turned.
    fn frame_with(
        ctx: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
        show: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, show);
        output.textures_delta.clear();
        output
    }

    /// A text a frame painted: its words, how far it reaches, where the screen cuts it, and whether
    /// egui had to cut it short itself (the ellipsis of a label that is told to stay on one line).
    struct Painted {
        text: String,
        right: f32,
        clip_right: f32,
        elided: bool,
        at: egui::Pos2,
    }

    fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<Painted> {
        fn walk(shape: &egui::epaint::Shape, clip: egui::Rect, into: &mut Vec<Painted>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, clip, into)),
                egui::epaint::Shape::Text(t) => into.push(Painted {
                    text: t.galley.text().to_string(),
                    right: t.pos.x + t.galley.rect.max.x,
                    clip_right: clip.max.x,
                    elided: t.galley.elided,
                    at: t.pos,
                }),
                _ => {}
            }
        }
        let mut into = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, clipped.clip_rect, &mut into);
        }
        into
    }

    /// The texts of a frame that a person cannot read to the end: cut with an ellipsis, or reaching
    /// past the edge that clips them.
    fn cut_texts(painted: &[Painted]) -> Vec<String> {
        painted
            .iter()
            .filter(|p| p.elided || p.right > p.clip_right + 0.5)
            .map(|p| {
                format!(
                    "{:?} (reaches {:.0} of {:.0}, elided: {})",
                    p.text, p.right, p.clip_right, p.elided
                )
            })
            .collect()
    }

    #[test]
    fn p3_j24_nothing_a_card_says_is_cut_off_at_the_edge_of_the_list() {
        let ctx = egui::Context::default();
        for engine in ["llamacpp", "ollama"] {
            let dir = crate::testkit::scratch(&format!("j24-{engine}"));
            let mut settings = Settings::default();
            settings.set_chosen_engine(engine);
            let mut app = app_on(&dir, settings);
            app.open_catalog();
            let entries = std::mem::take(&mut app.catalog.entries);
            assert!(entries.len() > 40, "{engine}: {} models", entries.len());
            let labels = app
                .engine_doc()
                .map(super::super::lookup_view::bundle_words)
                .unwrap_or_default();

            // every card of the shipped list, one under the other, as wide as the list is
            let installed = |_: &quickai_core::lookup::present::Offer| false;
            let fit = |_: &quickai_core::lookup::present::Offer| crate::hardware::Fit::Comfortable;
            let decor = super::super::lookup_view::Decor {
                installed: &installed,
                fit: &fit,
                ram: 16 * GB,
            };
            let mut chosen = None;
            let output = frame(&ctx, egui::vec2(576.0, 30000.0), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    for entry in &entries {
                        let shown: Vec<usize> = (0..entry.offers.len()).collect();
                        super::super::lookup_view::show_entry(
                            ui,
                            entry,
                            &shown,
                            &labels,
                            false,
                            &mut chosen,
                            Some(&decor),
                        );
                    }
                });
            });
            let painted = painted_texts(&output.shapes);
            let cut = cut_texts(&painted);
            assert!(cut.is_empty(), "{engine}: cut short: {cut:#?}");
            // and what a model says of itself is all there, in one piece
            for entry in entries.iter().filter(|e| !e.description.is_empty()) {
                assert!(
                    painted.iter().any(|p| p.text == entry.description),
                    "{engine}: the note of {} is not on the screen: {:?}",
                    entry.name,
                    entry.description
                );
            }
        }
    }

    #[test]
    fn p3_j24b_the_window_as_a_person_first_sees_it_has_nothing_cut_off() {
        let ctx = egui::Context::default();
        let dir = crate::testkit::scratch("j24b");
        let mut app = app_on(&dir, Settings::default());
        app.open_catalog();
        let mut failed: Vec<String> = Vec::new();
        for scope in [
            quickai_core::lookup::view::Scope::Start,
            quickai_core::lookup::view::Scope::All,
            quickai_core::lookup::view::Scope::Type("chat".into()),
        ] {
            app.catalog.browse.scope = scope.clone();
            // the first frames of a window only measure it
            let size = egui::vec2(760.0, 560.0);
            for _ in 0..2 {
                frame(&ctx, size, |ui| app.show_catalog_window(ui.ctx()));
            }
            let output = frame(&ctx, size, |ui| app.show_catalog_window(ui.ctx()));
            let painted = painted_texts(&output.shapes);
            assert!(
                painted
                    .iter()
                    .any(|p| p.text.contains("Recommended models")),
                "{scope:?}: the window was not drawn"
            );
            for p in &painted {
                eprintln!(
                    "DBG {scope:?} at=({:.0},{:.0}) right={:.0} clip={:.0} {:?}",
                    p.at.x, p.at.y, p.right, p.clip_right, p.text
                );
            }
            let cut = cut_texts(&painted);
            if !cut.is_empty() {
                failed.push(format!("{scope:?}: cut short: {cut:#?}"));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }

    #[test]
    fn p3_j24c_another_page_of_the_list_starts_at_its_top() {
        use quickai_core::lookup::view::{browse, Scope};
        let ctx = egui::Context::default();
        let dir = crate::testkit::scratch("j24c");
        let mut app = app_on(&dir, Settings::default());
        app.open_catalog();
        app.catalog.browse.scope = Scope::All;
        let size = egui::vec2(760.0, 560.0);
        let draw = |app: &mut App, events: Vec<egui::Event>| {
            painted_texts(
                &frame_with(&ctx, size, events, |ui| app.show_catalog_window(ui.ctx())).shapes,
            )
        };
        // a window needs a few frames to be measured, and a list a few to be drawn from a new top
        let show = |app: &mut App| {
            let mut texts = Vec::new();
            for _ in 0..3 {
                texts = draw(app, Vec::new());
            }
            texts
        };
        // the wheel turned down over the list, and the toolkit left to finish the motion: it smooths a
        // turn over many frames, and what is left of it would move a list that was just put at its top
        let over_list = egui::pos2(450.0, 350.0);
        let scroll_down = |app: &mut App| {
            for _ in 0..10 {
                let turn = egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -300.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                };
                draw(app, vec![egui::Event::PointerMoved(over_list), turn]);
            }
            for _ in 0..120 {
                draw(app, Vec::new());
            }
        };
        let types = app.registry.vocabulary().model_types().to_vec();
        let first_of = |app: &App, what: &quickai_core::lookup::view::Browse| -> String {
            let shown = browse(&app.catalog.entries, &types, what);
            app.catalog.entries[shown.sections[0].entries[0].entry]
                .name
                .clone()
        };
        let on_screen = |texts: &[Painted], name: &str| texts.iter().any(|p| p.text == name);
        let page = |scope: Scope| quickai_core::lookup::view::Browse {
            scope,
            ..Default::default()
        };

        let first_all = first_of(&app, &page(Scope::All));
        assert!(
            on_screen(&show(&mut app), &first_all),
            "{first_all} is not at the top"
        );
        scroll_down(&mut app);
        assert!(
            !on_screen(&show(&mut app), &first_all),
            "the wheel did not move the list: {first_all} is still at its top"
        );

        // another page is shown from its top, not from where the last one was left
        let vision = page(Scope::Type("vision".into()));
        let first_vision = first_of(&app, &vision);
        app.catalog.browse = vision;
        assert!(
            on_screen(&show(&mut app), &first_vision),
            "the page of the vision models does not start at {first_vision}"
        );

        // and so does a search
        scroll_down(&mut app);
        let found = quickai_core::lookup::view::Browse {
            words: "qwen".into(),
            ..page(Scope::All)
        };
        let first_found = first_of(&app, &found);
        app.catalog.browse = found;
        assert!(
            on_screen(&show(&mut app), &first_found),
            "the search does not start at {first_found}"
        );

        // and a window that is opened again
        scroll_down(&mut app);
        app.open_catalog();
        let first_start = first_of(&app, &app.catalog.browse);
        assert!(
            on_screen(&show(&mut app), &first_start),
            "the window opened again does not start at {first_start}"
        );
    }

    #[test]
    fn p3_j20_a_button_of_the_window_fetches_what_it_says_and_the_model_is_then_installed_and_named(
    ) {
        let (dir, hub, mut app) = catalog_world("j20");
        let ctx = egui::Context::default();
        app.open_catalog();
        let installed = |app: &App, name: &str, label: &str, bundle: Option<&str>| -> bool {
            let o = app
                .catalog
                .entries
                .iter()
                .find(|e| e.name == name)
                .unwrap()
                .offers
                .iter()
                .find(|o| o.label.starts_with(label) && o.bundle.as_deref() == bundle)
                .unwrap();
            super::super::catalog_view::installed(o, &app.catalog.have, &[])
        };
        assert!(!installed(&app, "Tiny VL", "Q4_K_M", None));
        // a failure that an earlier click left in the window goes with the next click on a size
        app.catalog.note = Some("Download failed: an earlier one".into());

        // the weights with the projector: one job, both files, each checked against its hash
        let fetch = catalog_button(&app, "Tiny VL", "Q4_K_M", Some("vision"));
        assert!(matches!(&fetch, Fetch::Files(d) if d.files.iter().all(|f| f.sha256.is_some())));
        app.fetch_offer(&fetch, &ctx);
        until_downloaded(&mut app, &ctx);
        assert_eq!(
            app.catalog.note, None,
            "a download that ends well leaves no note"
        );
        assert!(
            app.log_lines
                .iter()
                .any(|l| l == "Download complete: tiny-Q4_K_M.gguf"),
            "{:?}",
            app.log_lines
        );
        let models = dir.join("models");
        assert!(models.join("tiny-Q4_K_M.gguf").is_file());
        assert!(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf").is_file());
        assert!(models.join("tiny-Q4_K_M.gguf.bundle.json").is_file());
        // where the weights came from is remembered, in the data directory and not in the models folder
        // (A63 of docs/phase3-test-plan.md), so that a configuration can say where to get the model
        let size = std::fs::metadata(models.join("tiny-Q4_K_M.gguf"))
            .unwrap()
            .len();
        assert_eq!(
            crate::sources::recall(&dir.join("data"), "tiny-Q4_K_M.gguf", size).as_deref(),
            Some("huggingface:me/tiny-vl-GGUF")
        );
        assert!(!models.join("model-sources.json").exists());
        // the window knows without being told: the weights alone are there, and so is the pair, and
        // the other quantization is not
        assert!(installed(&app, "Tiny VL", "Q4_K_M", None));
        assert!(installed(&app, "Tiny VL", "Q4_K_M", Some("vision")));
        assert!(!installed(&app, "Tiny VL", "Q8_0", None));
        // the picker names it as the list does, with what the list says of it and what goes with it
        let model = app
            .models
            .iter()
            .find(|m| m.filename == "tiny-Q4_K_M.gguf")
            .unwrap();
        assert_eq!(model.display, "Tiny VL (Q4_K_M)");
        assert!(
            model.note.starts_with("Describes pictures."),
            "{}",
            model.note
        );
        assert!(
            model.note.contains("With its projector: mmproj-f16.gguf."),
            "{}",
            model.note
        );
        draw_everything(&mut app, &ctx, ServerState::Idle);
        // asked for once each, from the commit the list names
        assert_eq!(
            hub.downloads(),
            [
                "me/tiny-vl-GGUF:tiny-Q4_K_M.gguf",
                "me/tiny-vl-GGUF:mmproj-f16.gguf"
            ]
        );
        assert!(
            hub.paths()
                .iter()
                .all(|p| p.contains("/resolve/0123456789abcdef0123456789abcdef01234567/")),
            "{:?}",
            hub.paths()
        );

        // an embedding model: its record says the mode, the picker says how it starts, and Start uses it
        let fetch = catalog_button(&app, "Vec Maker", "Q8_0", None);
        app.fetch_offer(&fetch, &ctx);
        until_downloaded(&mut app, &ctx);
        let vec = app
            .models
            .iter()
            .find(|m| m.filename == "vec-Q8_0.gguf")
            .unwrap()
            .clone();
        assert_eq!(vec.display, "Vec Maker (Q8_0)");
        assert_eq!(vec.mode.as_deref(), Some("embedding"));
        assert!(
            vec.note.ends_with("Starts as an embedding server."),
            "{}",
            vec.note
        );
        assert!(models.join("vec-Q8_0.gguf.bundle.json").is_file());
        let selection = app.run_selection(Some(&vec));
        assert_eq!(selection.mode.as_deref(), Some("embedding"));
        assert!(
            app.log_lines.iter().all(|l| !l.contains("mode `")),
            "no complaint: {:?}",
            app.log_lines
        );

        // a model with no mode and no record starts as it always did
        let tiny = app
            .models
            .iter()
            .find(|m| m.filename == "tiny-Q4_K_M.gguf")
            .unwrap()
            .clone();
        let selection = app.run_selection(Some(&tiny));
        assert_eq!(selection.mode, None);
        assert_eq!(selection.components["mmproj"].len(), 1);
    }

    #[test]
    fn p3_j20b_a_file_that_is_not_the_one_the_list_was_made_for_is_not_kept_and_the_window_says_so()
    {
        let (dir, _hub, mut app) = catalog_world("j20b");
        let ctx = egui::Context::default();
        app.open_catalog();
        let fetch = catalog_button(&app, "Liar", "Q4_K_M", None);
        app.fetch_offer(&fetch, &ctx);
        until_downloaded(&mut app, &ctx);
        let said = app.log_lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(
            said.contains("liar-Q4_K_M.gguf does not match the checksum the list was made with")
                && said.contains("it was not kept"),
            "{said}"
        );
        // the log is under the window: the window says it too
        let note = app.catalog.note.clone().unwrap_or_default();
        assert!(
            note.starts_with("Download failed: liar-Q4_K_M.gguf does not match the checksum"),
            "{note}"
        );
        let left: Vec<String> = std::fs::read_dir(dir.join("models"))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert!(left.is_empty(), "{left:?}");
        assert!(app.models.iter().all(|m| m.filename != "liar-Q4_K_M.gguf"));
        assert!(app.download.is_none(), "the window can fetch again");
    }

    #[test]
    fn p3_j21_in_ollamas_window_the_list_offers_names_and_marks_the_ones_it_has() {
        let dir = crate::testkit::scratch("j21");
        std::fs::create_dir_all(dir.join("cfg/catalog")).unwrap();
        std::fs::write(
            dir.join("cfg/catalog/t.toml"),
            r#"schema = 1
kind = "catalog"
id = "t"

[[model]]
id = "tiny-vl"
name = "Tiny VL"
type = "vision"
start = true
[model.run.llamacpp]
repo = "me/tiny-vl-GGUF"
files = [ { file = "tiny-Q4_K_M.gguf", size = 3000 } ]
[model.run.ollama]
ref = "tiny-vl:1b"
size = 900

[[model]]
id = "qwen"
name = "Qwen"
[model.run.ollama]
ref = "qwen3:4b"
size = 2500000000
"#,
        )
        .unwrap();
        remember_models(&dir, &["qwen3:4b", "bonsai-q1:latest"]);
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        let mut app = app_over(&dir, settings);
        let ctx = egui::Context::default();
        assert!(app.can_recommend());
        app.open_catalog();
        // only what Ollama pulls by name, as buttons that pull: the list of this test and the one the
        // application ships
        let names = catalog_names(&app);
        assert!(
            names.contains(&"Tiny VL") && names.contains(&"Qwen"),
            "{names:?}"
        );
        assert!(
            names.contains(&"Qwen3.5 4B"),
            "the shipped list is there too"
        );
        assert!(app.catalog.entries.iter().all(|e| e
            .offers
            .iter()
            .all(|o| matches!(o.fetch, Fetch::Reference(_)))));
        assert_eq!(
            catalog_button(&app, "Tiny VL", "1b", None),
            Fetch::Reference("tiny-vl:1b".into())
        );
        // the models Ollama lists are installed, whichever way their name is written
        let mut listed: Vec<String> = app.models.iter().map(|m| m.filename.clone()).collect();
        listed.sort();
        assert_eq!(listed, ["bonsai-q1:latest", "qwen3:4b"]);
        let installed = |name: &str| {
            let o = &app
                .catalog
                .entries
                .iter()
                .find(|e| e.name == name)
                .unwrap()
                .offers[0];
            super::super::catalog_view::installed(o, &app.catalog.have, &listed)
        };
        assert!(installed("Qwen"));
        assert!(!installed("Tiny VL"));
        draw_everything(&mut app, &ctx, ServerState::Idle);
        // llama.cpp's window does not offer the model it has no files for, and vice versa
        let mut settings = Settings::default();
        settings.set_chosen_engine("llamacpp");
        let mut llama = app_over(&dir, settings);
        llama.open_catalog();
        assert!(
            catalog_names(&llama).contains(&"Tiny VL") && !catalog_names(&llama).contains(&"Qwen")
        );
    }

    #[test]
    fn p3_j17_a_button_is_installed_when_every_file_it_fetches_is_there_at_its_size() {
        use quickai_core::lookup::present::{Download, Offer, Piece};
        use std::collections::BTreeMap;
        let piece = |name: &str, size: Option<u64>| Piece {
            kind: None,
            label: "model".into(),
            url: String::new(),
            store_as: name.into(),
            name: name.into(),
            size,
            sha256: None,
            place: None,
        };
        let offer = |files: Vec<Piece>| Offer {
            bundle: None,
            label: "x".into(),
            hover: String::new(),
            bytes: None,
            estimated: false,
            class: None,
            recommended: false,
            fetch: Fetch::Files(Download {
                files,
                bundle: None,
                source: String::new(),
                mode: None,
            }),
        };
        let have: BTreeMap<String, u64> =
            [("a.gguf".to_string(), 10), ("b.gguf".to_string(), 20)].into();
        let is =
            |files: Vec<Piece>| super::super::catalog_view::installed(&offer(files), &have, &[]);
        assert!(is(vec![piece("a.gguf", Some(10))]));
        assert!(
            is(vec![piece("a.gguf", Some(10)), piece("b.gguf", Some(20))]),
            "a bundle: all of it"
        );
        assert!(
            !is(vec![piece("a.gguf", Some(10)), piece("c.gguf", Some(5))]),
            "one is missing"
        );
        assert!(
            !is(vec![piece("a.gguf", Some(11))]),
            "a file of another size is another file"
        );
        assert!(
            is(vec![piece("a.gguf", None)]),
            "a size not said is not looked at"
        );
        assert!(!is(vec![]), "nothing to fetch is not a model that is there");
        // a name the engine lists, whichever way it is written
        let reference = |r: &str| Offer {
            fetch: Fetch::Reference(r.into()),
            ..offer(vec![])
        };
        let listed = vec!["qwen3:latest".to_string(), "library/gemma3:4b".to_string()];
        let there = |r: &str| super::super::catalog_view::installed(&reference(r), &have, &listed);
        assert!(there("qwen3") && there("qwen3:latest") && there("gemma3:4b"));
        assert!(!there("qwen3:4b") && !there("gemma3"));
    }

    // ----------------------------------------------------------------- the real thing, by kind of model

    /// A picture of one colour, as a PNG: a signature, a header, one block of uncompressed data, an end.
    fn plain_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for b in bytes {
                crc ^= u32::from(*b);
                for _ in 0..8 {
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
            out
        }
        // every line: a filter byte (none) and the pixels
        let mut line = vec![0u8];
        for _ in 0..width {
            line.extend_from_slice(&rgb);
        }
        let raw = line.repeat(height as usize);
        // zlib: a header, stored blocks of at most 65535 bytes, the Adler-32 of the data
        let mut zlib = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
        for (i, block) in blocks.iter().enumerate() {
            zlib.push(u8::from(i + 1 == blocks.len()));
            zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
            zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
            zlib.extend_from_slice(block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for byte in &raw {
            a = (a + u32::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend(chunk(b"IHDR", &header));
        png.extend(chunk(b"IDAT", &zlib));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    fn base64(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n =
                chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn p3_j20c_a_png_made_of_one_colour_is_what_it_says_for_the_manual_test_that_sends_it() {
        let png = plain_png(2, 2, [255, 0, 0]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
    }

    /// The smallest model of each kind in the list, fetched by the button of its row (the bundle, when there is
    /// one), started the way the Start button starts it by the real `llama-server`, and asked one thing of the
    /// right kind: a question, a picture of one colour, a recording of speech, a sentence to embed, passages to rank.
    /// What the fakes cannot say, this does: that the list's commit, hash and projector are a model that works, that
    /// the mode of an embedding model is what makes the server answer, and that a record carries it from the
    /// download to the command line.
    #[test]
    #[ignore = "manual: needs the internet (about 2.3 GB), a real llama-server and, for the audio model, a recording (QUICKAI_REAL_LLAMA=<llama-server> [QUICKAI_REAL_AUDIO=<speech.wav>] cargo test -p quickai p3_j23 -- --ignored)"]
    fn p3_j23_the_smallest_model_of_each_kind_is_fetched_from_the_list_and_answers_for_real() {
        let binary = std::env::var("QUICKAI_REAL_LLAMA")
            .expect("QUICKAI_REAL_LLAMA is the llama-server to run");
        let speech = std::env::var("QUICKAI_REAL_AUDIO").ok();
        let dir = crate::testkit::scratch("j23");
        let port = crate::testkit::free_port();
        let settings = Settings {
            port,
            ctx_size: 2048,
            n_gpu_layers: 0,
            llama_bin_override: Some(binary),
            models_dir_override: Some(dir.join("models").display().to_string()),
            ..Settings::default()
        };
        // the documents of the application as they are: the real list, the real hub
        let mut app = app_over(&dir, settings);
        app.bundled_models_dir = dir.join("bundled");
        app.load_engine_state();
        let ctx = egui::Context::default();

        let post = |path: &str, body: serde_json::Value| -> serde_json::Value {
            ureq::post(format!("http://127.0.0.1:{port}{path}"))
                .send_json(&body)
                .unwrap_or_else(|e| panic!("{path}: {e}"))
                .into_body()
                .read_json()
                .expect("JSON")
        };
        let red = base64(&plain_png(96, 96, [220, 30, 30]));
        let mut asked = Vec::new();
        for (id, what) in [
            ("lfm2.5-350m", "chat"),
            ("bge-small-en-v1.5", "embedding"),
            ("qwen3-reranker-0.6b", "rerank"),
            ("lfm2.5-vl-450m", "picture"),
            ("qwen3-asr-0.6b", "speech"),
        ] {
            // `QUICKAI_J23_ONLY=id,id` runs some of them
            if std::env::var("QUICKAI_J23_ONLY").is_ok_and(|only| !only.split(',').any(|o| o == id))
            {
                continue;
            }
            if what == "speech" && speech.is_none() {
                asked.push(format!("{id}: skipped, no recording (QUICKAI_REAL_AUDIO)"));
                continue;
            }
            app.open_catalog();
            let entry = app
                .catalog
                .entries
                .iter()
                .find(|e| e.key == format!("catalog:{id}"))
                .unwrap_or_else(|| panic!("{id} is not in the list"))
                .clone();
            // what a person is first offered: the weights with their projector when there is one
            let offer = entry.offers.first().expect("a button");
            let Fetch::Files(download) = &offer.fetch else {
                panic!("{id}: not files")
            };
            let weights = download.weights().expect("weights").store_as.clone();
            app.fetch_offer(&offer.fetch, &ctx);
            let deadline = Instant::now() + Duration::from_secs(1800);
            while app.download.is_some() {
                assert!(Instant::now() < deadline, "{id}: the download did not end");
                app.poll_download();
                std::thread::sleep(Duration::from_millis(200));
            }
            let said = app.log_lines.iter().cloned().collect::<Vec<_>>().join("\n");
            assert!(
                said.contains(&format!("Download complete: {weights}")),
                "{id}: {said}"
            );
            app.selected = app
                .models
                .iter()
                .position(|m| m.filename == weights)
                .unwrap_or_else(|| panic!("{id}: the picker does not list {weights}"));
            // started the way the Start button does it
            app.start_selected(&ctx);
            let deadline = Instant::now() + Duration::from_secs(300);
            let mut server_said: Vec<String> = Vec::new();
            while app.session.state() != ServerState::Running {
                assert!(
                    Instant::now() < deadline,
                    "{id}: the server did not run ({:?}): {:?} and the server said {:?}",
                    app.session.state(),
                    app.log_lines,
                    server_said
                );
                app.session
                    .poll(&mut |line| server_said.push(line), &mut |_| {});
                std::thread::sleep(Duration::from_millis(200));
            }
            // the command line of this start, which the server's log begins with
            let line = server_said
                .iter()
                .rev()
                .find(|l| l.starts_with("$ "))
                .cloned()
                .unwrap_or_default();
            let answer = match what {
                "chat" => {
                    let a = post(
                        "/v1/chat/completions",
                        json!({"messages": [{"role": "user", "content": "In one short sentence, what is the capital of France?"}], "max_tokens": 60, "temperature": 0}),
                    );
                    let text = a["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    assert!(text.to_lowercase().contains("paris"), "{id}: {text}");
                    text
                }
                "embedding" => {
                    assert!(
                        line.contains("--embeddings"),
                        "{id}: the command line has no --embeddings: {line}"
                    );
                    let a = post(
                        "/v1/embeddings",
                        json!({"input": ["hello world", "bonjour le monde"], "model": "x"}),
                    );
                    let n = a["data"][0]["embedding"].as_array().map_or(0, Vec::len);
                    assert!(
                        n > 10 && a["data"].as_array().map_or(0, Vec::len) == 2,
                        "{id}: {a}"
                    );
                    format!("vectors of {n}")
                }
                "rerank" => {
                    assert!(
                        line.contains("--reranking"),
                        "{id}: the command line has no --reranking: {line}"
                    );
                    let a = post(
                        "/v1/rerank",
                        json!({"query": "What is the capital of France?", "documents": ["Paris is the capital of France.", "Bananas are yellow.", "Berlin is in Germany."], "model": "x"}),
                    );
                    assert_eq!(a["results"][0]["index"], json!(0), "{id}: {a}");
                    format!("{}", a["results"][0]["relevance_score"])
                }
                "picture" => {
                    assert!(
                        line.contains("--mmproj "),
                        "{id}: the command line has no --mmproj: {line}"
                    );
                    let a = post(
                        "/v1/chat/completions",
                        json!({"messages": [{"role": "user", "content": [
                            {"type": "text", "text": "What colour is this picture? Answer in one word."},
                            {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{red}")}}]}],
                            "max_tokens": 30, "temperature": 0}),
                    );
                    let text = a["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    assert!(text.to_lowercase().contains("red"), "{id}: {text}");
                    text
                }
                _ => {
                    assert!(
                        line.contains("--mmproj "),
                        "{id}: the command line has no --mmproj: {line}"
                    );
                    let wav = std::fs::read(speech.as_deref().unwrap()).expect("the recording");
                    let a = post(
                        "/v1/chat/completions",
                        json!({"messages": [{"role": "user", "content": [
                            {"type": "text", "text": "Transcribe this audio."},
                            {"type": "input_audio", "input_audio": {"data": base64(&wav), "format": "wav"}}]}],
                            "max_tokens": 200, "temperature": 0}),
                    );
                    a["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string()
                }
            };
            asked.push(format!("{id}: {}", answer.replace('\n', " ")));
            // stopped the way the Stop button stops it, and waited for, so that the next model can start
            app.session.stop();
            let deadline = Instant::now() + Duration::from_secs(60);
            while app.session.state() != ServerState::Idle {
                assert!(Instant::now() < deadline, "{id}: the server did not stop");
                app.session.poll(&mut |_| {}, &mut |_| {});
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        eprintln!("J23:\n{}", asked.join("\n"));
    }
}
