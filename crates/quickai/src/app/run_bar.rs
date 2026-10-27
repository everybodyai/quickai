//! Starting, loading and changing the model that runs, and what the status bar says of it. The buttons are in the
//! head of the page (`model_bar.rs`).

use eframe::egui;

use super::App;
use crate::events::ServerState;
use crate::launch;
use crate::models::ModelEntry;
use crate::runtime::process::ephemeral_port;

impl App {
    /// Plans a run of `model` (or of the engine alone, when there is none) from the settings as they
    /// stand and hands the plan to the session. What the command line looks like is the engine
    /// document's business (see `launch.rs`); a run that cannot even be planned is reported the way a
    /// failed start is.
    pub(super) fn start_run(
        &mut self,
        model: Option<&ModelEntry>,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        let display = model.map(|m| m.display.clone()).unwrap_or_default();
        let port = self.port();
        // With the proxy on, the engine listens on an internal-only port and the proxy binds the
        // public one; the plan needs that port, so it is found first.
        let internal_port = if self.traffic_proxy_on() {
            match ephemeral_port() {
                Ok(port) => Some(port),
                Err(e) => {
                    self.session.report_start_failure(
                        port,
                        &display,
                        format!(
                            "Could not allocate an internal port for the live-traffic proxy: {e}"
                        ),
                        wake,
                    );
                    return;
                }
            }
        } else {
            None
        };
        let selection = self.run_selection(model);
        match self.plan_run(&selection, internal_port) {
            Ok(plan) => {
                // a program the application did not find is the person's to allow, once
                if let Some(foreign) = self.foreign_program(&plan) {
                    self.pending_foreign = Some(foreign);
                    return;
                }
                self.session.start(&plan, &display, wake)
            }
            Err(problems) => {
                let message = problems
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                self.session
                    .report_start_failure(port, &display, message, wake);
            }
        }
    }

    /// The plan for a run of `selection` on this machine, with the program the window has found. The
    /// data directory is the one the window works in (a test's own, never the real home's).
    fn plan_run(
        &self,
        selection: &quickai_core::plan::Selection,
        internal_port: Option<u16>,
    ) -> Result<quickai_core::plan::RunPlan, Vec<quickai_core::diag::Diagnostic>> {
        let mut facts = launch::host_facts(&self.binary, self.machine.clone(), internal_port);
        if let Some(dir) = &self.data_dir_override {
            facts.data_dir = dir.clone();
        }
        quickai_core::plan::plan(&self.registry, selection, &facts)
    }

    /// What a run is asked of the planner: the engine, the model, the values the window holds, and the
    /// files that were fetched with the model, which go on its command line. One that has gone is said
    /// in the log and the model starts without it.
    pub(super) fn run_selection(
        &mut self,
        model: Option<&ModelEntry>,
    ) -> quickai_core::plan::Selection {
        let mut selection = launch::selection_from_values(
            &self.engine,
            model.map(ModelEntry::info),
            &self.run_values(),
            self.traffic_proxy_on(),
        );
        if let (Some(model), Some(engine)) = (model, self.engine_doc()) {
            let (attached, notes) = launch::attach_record(selection, model, engine);
            selection = attached;
            for note in notes {
                self.push_log(note);
            }
        }
        selection
    }

    /// What the Start button does: start the engine with the model the picker has selected.
    pub(super) fn start_selected(&mut self, ctx: &egui::Context) {
        // an engine that runs on its own is started with no model: the work it is sent names them
        let model = if self.runs_alone() {
            None
        } else {
            self.models.get(self.selected).cloned()
        };
        let remote = self.remote_in_use().is_some();
        if !remote && !self.binary.is_file() {
            self.missing_binary_warned = true;
            return;
        }
        self.save_settings();
        self.run_reached_running = false;
        self.log_lines.clear();
        self.traffic.clear();
        self.selected_session = None;
        // on another computer, the tunnel first: the run starts when it is open
        if remote {
            self.start_remote(model, ctx);
            return;
        }
        let ctx = ctx.clone();
        self.start_run(model.as_ref(), move || ctx.request_repaint());
    }

    /// Whether Start would load the selected model into the running engine: one running with none
    /// (management mode, where the daemon came up to list and pull and the person has now chosen), or one that
    /// has another loaded, which makes way for it (the daemon unloads it first).
    pub(super) fn can_load_selected(&self) -> bool {
        self.session.state() == ServerState::Running
            && self.session.loading().is_none()
            && self.lists_models()
            && self
                .models
                .get(self.selected)
                .is_some_and(|m| !self.is_loaded_model(&m.filename))
    }

    /// Whether the green button would start the server again with the selected model: an engine that serves one
    /// model file for its run (llama.cpp) is running another.
    pub(super) fn can_switch_selected(&self) -> bool {
        !self.lists_models()
            && !self.runs_alone()
            && !self.restart_with_selected
            && self.session.state() == ServerState::Running
            && self
                .models
                .get(self.selected)
                .is_some_and(|m| self.session.model_name() != Some(m.display.as_str()))
    }

    /// What the green button does for `can_switch_selected`: stops the server, and `poll_restart` starts it again
    /// with the selected model once it has stopped.
    pub(super) fn switch_to_selected(&mut self) {
        self.restart_with_selected = true;
        self.stop_companion();
        self.session.stop();
    }

    /// Each frame: a server stopped to be started with another model is started once it has stopped; one that
    /// crashed on the way is left as it is.
    pub(super) fn poll_restart(&mut self, ctx: &egui::Context) {
        if !self.restart_with_selected {
            return;
        }
        match self.session.state() {
            ServerState::Idle => {
                self.restart_with_selected = false;
                self.start_selected(ctx);
            }
            ServerState::Crashed => self.restart_with_selected = false,
            ServerState::Starting | ServerState::Running | ServerState::Stopping => {}
        }
    }

    /// What Start does for an engine that is running with no model: loads the one the picker has
    /// selected into it. Only the warm-up calls of the plan are used (the engine and its gateway are
    /// there already), so the plan is made without the gateway.
    pub(super) fn load_selected(&mut self) {
        if !self.can_load_selected() {
            return;
        }
        let Some(model) = self.models.get(self.selected).cloned() else {
            return;
        };
        let selection = launch::selection_from_values(
            &self.engine,
            Some(model.info()),
            &self.run_values(),
            false,
        );
        match self.plan_run(&selection, None) {
            Ok(plan) => {
                self.save_settings();
                self.session.load(&plan, &model.display, || {});
            }
            Err(problems) => {
                for problem in problems {
                    self.push_log(problem.to_string());
                }
            }
        }
    }

    /// What the line of the run bar says of the engine: the model and port while it serves one, what a manager
    /// engine or one that runs on its own is doing, or the state's own word.
    pub(super) fn run_status_text(&self, state: ServerState) -> String {
        if self.remote_opening() {
            return format!("Connecting to {}...", self.remote_label());
        }
        let said = self.local_status_text(state);
        // a run on another computer says where
        match self.remote.tunnel.as_ref() {
            Some(_) if state == ServerState::Running => {
                format!("{said} on {}", self.remote_label())
            }
            _ => said,
        }
    }

    fn local_status_text(&self, state: ServerState) -> String {
        match (state, self.session.model_name(), self.session.port()) {
            (ServerState::Running, Some(name), Some(port)) => {
                format!("Running: {name} (port {port})")
            }
            (ServerState::Running, None, Some(port)) if self.lists_models() => {
                match self.session.loading() {
                    Some(model) => {
                        format!(
                            "{} ready, loading {model} (port {port})",
                            self.engine_name()
                        )
                    }
                    None => format!(
                        "{} ready, no model loaded (port {port})",
                        self.engine_name()
                    ),
                }
            }
            (ServerState::Running, None, Some(port)) if self.runs_alone() => {
                format!("{} running (port {port})", self.engine_name())
            }
            _ => state.label().to_string(),
        }
    }
}
