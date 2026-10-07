//! The Start/Stop row and the status label (panel `run_bar`).

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
        let internal_port = if self.enable_traffic_proxy {
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
        match launch::plan_selection(
            &self.registry,
            &selection,
            &self.binary,
            self.machine.clone(),
            internal_port,
        ) {
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
            self.values.map(),
            self.enable_traffic_proxy,
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
        let model = self.models.get(self.selected).cloned();
        if !self.binary.is_file() {
            self.missing_binary_warned = true;
            return;
        }
        self.save_settings();
        self.run_reached_running = false;
        self.log_lines.clear();
        self.traffic.clear();
        self.selected_session = None;
        let ctx = ctx.clone();
        self.start_run(model.as_ref(), move || ctx.request_repaint());
    }

    /// Whether Start would load the selected model into an engine that is running with none:
    /// management mode, where the daemon came up to list and pull and the person has now chosen.
    pub(super) fn can_load_selected(&self) -> bool {
        self.session.state() == ServerState::Running
            && self.session.model_name().is_none()
            && self.session.loading().is_none()
            && self.lists_models()
            && self.models.get(self.selected).is_some()
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
            self.values.map(),
            false,
        );
        match launch::plan_selection(
            &self.registry,
            &selection,
            &self.binary,
            self.machine.clone(),
            None,
        ) {
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

    /// Start, Stop, Open Chat UI, API and the status label.
    pub(super) fn show_run_bar(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        let server = self.server_name();
        let loadable = self.can_load_selected();
        // an engine that lists its own models can start with none: the daemon runs, empty, and the
        // person can pull one
        let missing = self.engine_missing();
        let can_start = (!self.models.is_empty() || self.lists_models()) && !missing;
        let start_hint = if loadable {
            format!("Load the selected model into the running {server}.")
        } else {
            format!("Launch {server} with the selected model and the settings above.")
        };
        let web_ui = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.clone());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    (!active && can_start) || loadable,
                    egui::Button::new("Start"),
                )
                .on_hover_text(start_hint)
                .clicked()
            {
                if loadable {
                    self.load_selected();
                } else {
                    self.start_selected(ui.ctx());
                }
            }
            if ui
                .add_enabled(
                    matches!(state, ServerState::Starting | ServerState::Running),
                    egui::Button::new("Stop"),
                )
                .on_hover_text(format!("Stop the running {server} process."))
                .clicked()
            {
                self.session.stop();
            }
            if let Some(web_ui) = &web_ui {
                if ui
                    .add_enabled(
                        state == ServerState::Running,
                        egui::Button::new(web_ui.label.as_str()),
                    )
                    .on_hover_text(format!(
                        "Open {server}'s built-in web chat for the running model in your browser."
                    ))
                    .clicked()
                {
                    if let Some(port) = self.session.port() {
                        // the page at the root is the address without a path, as it always was
                        let path = if web_ui.path == "/" { "" } else { &web_ui.path };
                        let _ = webbrowser::open(&format!("http://127.0.0.1:{port}{path}"));
                    }
                }
            }
            if ui
                .add_enabled(state == ServerState::Running, egui::Button::new("🔌 API"))
                .on_hover_text("Show how to connect an external app to this running model")
                .clicked()
            {
                self.show_api_info = !self.show_api_info;
            }

            ui.add_space(12.0);
            let label = match (state, self.session.model_name(), self.session.port()) {
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
                _ => state.label().to_string(),
            };
            ui.colored_label(Self::state_color(state), label);
        });
    }
}
