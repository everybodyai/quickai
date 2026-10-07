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
use crate::models::{discover_all, ModelEntry};
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
            || self.named.pull.is_some()
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
        self.binary = launch::locate_binary(
            &self.registry,
            &self.engine,
            self.values.text("bin_override"),
        );
        self.host_facts = launch::host_facts(&self.binary, self.machine.clone(), None);
        if let Some(dir) = &self.data_dir_override {
            self.host_facts.data_dir = dir.clone();
        }
        self.missing_binary_warned = false;
        self.show_lookup = false;
        self.lookup_results.clear();
        self.lookup_status.clear();
        self.lookup_rx = None;
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
                .unwrap_or_else(paths::user_models_dir);
            let _ = std::fs::create_dir_all(&self.models_dir);
            self.models_dir_input = self.models_dir.display().to_string();
            self.models = discover_all(&self.bundled_models_dir, &self.models_dir);
        }
        self.selected = self.remembered_model().unwrap_or(0);
        self.refresh_recommendation();
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
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let action = engine.classify_input(&self.model_input, &|path| Path::new(path).is_file());
        match action {
            InputAction::Nothing => {}
            InputAction::Reject { message } => self.named.notice = Some(message),
            InputAction::Pull { reference } => {
                let Some(endpoint) = self.endpoint() else {
                    self.named.notice = Some(format!(
                        "Start {} first: the running {} pulls the model.",
                        self.engine_name(),
                        self.server_name()
                    ));
                    return;
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
                    return;
                };
                self.named.notice = None;
                self.model_input.clear();
                let ctx = ctx.clone();
                self.named.pull = Some(spawn_pull(
                    endpoint,
                    PullSpec::from_call(&call),
                    reference,
                    move || ctx.request_repaint(),
                ));
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
            egui::CentralPanel::default().show(ui, |ui| {
                app.show_header(ui);
                app.show_model_bar(ui, false);
                app.show_acquire(ui, false);
                app.show_controls(ui);
                app.show_run_bar(ui, state, false);
                app.show_api_info_panel(ui, state);
                app.show_diagnostics(ui, state);
                app.show_log_pane(ui);
            });
            app.show_settings_window(ui.ctx());
            app.show_confirm_delete_modal(ui.ctx());
        });
        output.textures_delta.clear();
    }

    #[test]
    fn p3_e26b_every_panel_draws_for_both_engines_in_the_states_a_window_can_be_in() {
        let ctx = egui::Context::default();
        for engine in ["llamacpp", "ollama"] {
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
}
