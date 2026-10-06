//! The Start/Stop row and the status label (panel `run_bar`).

use eframe::egui;

use super::App;
use crate::events::ServerState;
use crate::launch;
use crate::models::ModelEntry;
use crate::runtime::process::ephemeral_port;

impl App {
    /// Plans a run of `model` from the settings as they stand and hands the plan to the session.
    /// What the command line looks like is the engine document's business (see `launch.rs`); a run
    /// that cannot even be planned is reported the way a failed start is.
    fn start_run(&mut self, model: &ModelEntry, wake: impl Fn() + Send + Sync + 'static) {
        let settings = self.run_settings();
        // With the proxy on, the engine listens on an internal-only port and the proxy binds the
        // public one; the plan needs that port, so it is found first.
        let internal_port = if settings.traffic_proxy {
            match ephemeral_port() {
                Ok(port) => Some(port),
                Err(e) => {
                    self.session.report_start_failure(
                        settings.port,
                        &model.display,
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
        match launch::plan_run(
            &self.registry,
            model,
            &settings,
            &self.llama_server_bin,
            self.machine.clone(),
            internal_port,
        ) {
            Ok(plan) => self.session.start(&plan, &model.display, wake),
            Err(problems) => {
                let message = problems
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                self.session
                    .report_start_failure(settings.port, &model.display, message, wake);
            }
        }
    }

    /// Start, Stop, Open Chat UI, API and the status label.
    pub(super) fn show_run_bar(&mut self, ui: &mut egui::Ui, state: ServerState, active: bool) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !active && !self.models.is_empty(),
                    egui::Button::new("Start"),
                )
                .on_hover_text(
                    "Launch llama-server with the selected model and the settings above.",
                )
                .clicked()
            {
                if let Some(model) = self.models.get(self.selected).cloned() {
                    if !self.llama_server_bin.is_file() {
                        self.missing_binary_warned = true;
                    } else {
                        self.save_settings();
                        self.log_lines.clear();
                        self.traffic.clear();
                        self.selected_session = None;
                        let ctx_clone = ui.ctx().clone();
                        self.start_run(&model, move || ctx_clone.request_repaint());
                    }
                }
            }
            if ui
                .add_enabled(
                    matches!(state, ServerState::Starting | ServerState::Running),
                    egui::Button::new("Stop"),
                )
                .on_hover_text("Stop the running llama-server process.")
                .clicked()
            {
                self.session.stop();
            }
            if ui
                .add_enabled(
                    state == ServerState::Running,
                    egui::Button::new("Open Chat UI"),
                )
                .on_hover_text(
                    "Open llama-server's built-in web chat for the running model in your browser.",
                )
                .clicked()
            {
                if let Some(port) = self.session.port() {
                    let _ = webbrowser::open(&format!("http://127.0.0.1:{port}"));
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
                _ => state.label().to_string(),
            };
            ui.colored_label(Self::state_color(state), label);
        });
    }
}
