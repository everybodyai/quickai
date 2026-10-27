//! The Start/Stop row and the status label (panel `run_bar`).

use eframe::egui;

use super::companion::ChatAction;
use super::theme::{self, Role};
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

    /// The buttons of the toolbar: Start, Stop, the engine's page or its chat, and Connect an app. What the server is
    /// doing is said by the status bar, not here.
    pub(super) fn show_run_buttons(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        let server = self.server_name();
        let loadable = self.can_load_selected();
        let switchable = self.can_switch_selected();
        // an engine that lists its own models can start with none: the daemon runs, empty, and the
        // person can pull one
        let missing = self.engine_missing();
        let alone = self.runs_alone();
        let can_start = (!self.models.is_empty() || self.lists_models() || alone) && !missing;
        let start_hint = if loadable {
            format!("Load the selected model into the running {server}.")
        } else if switchable {
            format!("Start {server} again with the selected model.")
        } else if alone {
            format!("Launch {server} with the settings above.")
        } else {
            format!("Launch {server} with the selected model and the settings above.")
        };
        let web_ui = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.clone());
        let height = 28.0;
        ui.horizontal(|ui| {
            let can_press_start = (!active && can_start) || loadable || switchable;
            // green while it is the thing to do: Start, or loading into an engine that has nothing loaded. With a model
            // running, the chat is the green button, and the one that changes the model is not
            let first_model = !active || self.session.model_name().is_none();
            let start = theme::button_sized(
                ui,
                if can_press_start && first_model {
                    Role::Primary
                } else {
                    Role::Normal
                },
                can_press_start,
                if loadable {
                    "Load model"
                } else if switchable {
                    "Use this model"
                } else {
                    "Start"
                },
                egui::vec2(72.0, height),
            )
            .on_hover_text(start_hint);
            if start.clicked() {
                if loadable {
                    self.load_selected();
                } else if switchable {
                    self.switch_to_selected();
                } else {
                    self.start_selected(ui.ctx());
                }
            }
            let can_stop = matches!(state, ServerState::Starting | ServerState::Running);
            if theme::button_sized(
                ui,
                if can_stop { Role::Danger } else { Role::Normal },
                can_stop,
                "Stop",
                egui::vec2(60.0, height),
            )
            .on_hover_text(format!("Stop the running {server} process."))
            .clicked()
            {
                // the page goes with what it talks to
                self.stop_companion();
                self.session.stop();
            }
            if let Some(web_ui) = &web_ui {
                let ready = state == ServerState::Running;
                if theme::button_sized(
                    ui,
                    if ready { Role::Primary } else { Role::Normal },
                    ready,
                    web_ui.label.as_str(),
                    egui::vec2(0.0, height),
                )
                .on_hover_text(if alone {
                    format!("Open the running {server} in your browser.")
                } else {
                    format!(
                        "Open {server}'s built-in web chat for the running model in your browser."
                    )
                })
                .clicked()
                {
                    if let Some(port) = self.session.port() {
                        // the page at the root is the address without a path, as it always was
                        let path = if web_ui.path == "/" { "" } else { &web_ui.path };
                        let _ = webbrowser::open(&format!("http://127.0.0.1:{port}{path}"));
                    }
                }
            }
            if web_ui.is_none() {
                if let Some(chat) = self.chat_button() {
                    // the one that matters is the chat page itself, once it can be opened: setting it up is
                    // not what the person is here for, and Start is the green button until it is pressed
                    let opens = matches!(chat.action, ChatAction::Open(_));
                    if theme::button_sized(
                        ui,
                        if chat.enabled && opens {
                            Role::Primary
                        } else {
                            Role::Normal
                        },
                        chat.enabled,
                        chat.label.as_str(),
                        egui::vec2(0.0, height),
                    )
                    .on_hover_text(chat.hover.as_str())
                    .clicked()
                    {
                        match chat.action {
                            ChatAction::Open(url) => {
                                let _ = webbrowser::open(&url);
                            }
                            ChatAction::SetUp => self.set_up_companion(),
                        }
                    }
                }
            }
            if theme::button_sized(
                ui,
                Role::Normal,
                state == ServerState::Running,
                "Connect an app",
                egui::vec2(0.0, height),
            )
            .on_hover_text(if alone {
                format!("Show how to connect an external app to the running {server}")
            } else {
                "Show how to connect an external app to this running model".to_string()
            })
            .clicked()
            {
                self.show_api_info = !self.show_api_info;
            }
        });
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
