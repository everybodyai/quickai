//! What to tell the user when the server cannot start or has crashed (panel `diagnostics`).

use eframe::egui;
use quickai_core::advice::{advice, Advice};

use super::App;
use crate::events::ServerState;
use crate::gpu;
use crate::launch;

impl App {
    /// Whether the engine's program cannot be found and there is no use in starting: an engine that
    /// is downloaded on request (not bundled) says so up front. A bundled engine is tried and says
    /// what it could not find when the person starts it, as it always has.
    pub(super) fn engine_missing(&self) -> bool {
        let bundled = self.engine_doc().is_some_and(|e| e.doc.engine.bundled);
        !bundled && !self.binary.is_file()
    }

    /// The missing-binary line and the crash panel with its advice.
    pub(super) fn show_diagnostics(&mut self, ui: &mut egui::Ui, state: ServerState) {
        let server = self.server_name();
        if self.engine_missing() {
            let name = self.engine_name();
            ui.colored_label(
                egui::Color32::from_rgb(192, 57, 43),
                format!(
                    "{name} is not installed. Open ⚙ Settings → Advanced: {name} and click \
                     \"Look up {name} builds\" to download one; nothing is downloaded until you do."
                ),
            );
        } else if self.missing_binary_warned {
            ui.colored_label(
                egui::Color32::from_rgb(192, 57, 43),
                format!("{server} binary not found at {}", self.binary.display()),
            );
        }

        if state == ServerState::Crashed {
            ui.add_space(6.0);
            if self.engine == crate::settings::DEFAULT_ENGINE {
                self.show_llamacpp_crash_panel(ui, state);
            } else {
                self.show_crash_panel(ui, state, &server);
            }
        }
        // the program that goes with the engine stopped, and the engine's run goes on
        if self.companion_state() == Some(ServerState::Crashed) {
            self.show_companion_crash(ui);
        }
    }

    /// llama.cpp's panel, word for word as it has always been.
    fn show_llamacpp_crash_panel(&self, ui: &mut egui::Ui, state: ServerState) {
        ui.group(|ui| {
            ui.colored_label(
                Self::state_color(state),
                "The server exited before it finished starting — see the log below \
                 for the exact reason (a common one: \"unknown model architecture\", \
                 meaning this llama-server build predates support for this model).",
            );
            let path = self.binary.display().to_string();
            let backend = self
                .registry
                .engine(&self.engine)
                .and_then(|e| quickai_core::acquire::backend(e, &path));
            if let Some(warning) =
                backend.and_then(|b| gpu::mismatch_warning(&self.gpu_vendors, b, &b.label))
            {
                ui.label(
                    egui::RichText::new(warning)
                        .small()
                        .color(egui::Color32::from_rgb(212, 160, 23)),
                );
            }
            ui.label(
                egui::RichText::new(
                    "Worth trying a different llama-server build:\n\
                     1. Open ⚙ Settings → Advanced: llama.cpp.\n\
                     2. Click \"Look up llama.cpp builds\" and pick one for your \
                        platform/GPU from the list — it installs and applies \
                        automatically.\n\
                     3. Or, if you already have a llama-server build of your own \
                        (a different backend, a custom compile, etc.), type or browse \
                        to its path in the \"llama-server binary\" field above and \
                        click Apply instead.\n\
                     4. Click Start again.",
                )
                .small(),
            );
        });
    }

    /// Another engine's panel: what happened, and the advice its document has for what the log says.
    pub(super) fn show_crash_panel(&mut self, ui: &mut egui::Ui, state: ServerState, server: &str) {
        let ended_in = if self.run_reached_running {
            "running"
        } else {
            "starting"
        };
        let what = if self.run_reached_running {
            format!("{server} stopped unexpectedly")
        } else {
            format!("{server} exited before it finished starting")
        };
        let log: Vec<String> = self.log_lines.iter().cloned().collect();
        let selection = launch::selection_from_values(
            &self.engine,
            self.selected_model_info(),
            self.values.map(),
            self.traffic_proxy_on(),
        );
        let hints = advice(
            &self.registry,
            &selection,
            &self.form_facts(),
            &log,
            Some(ended_in),
        );
        let mut asked: Option<String> = None;
        ui.group(|ui| {
            ui.colored_label(
                Self::state_color(state),
                format!("{what} — see the log below for the exact reason."),
            );
            for Advice { text, action } in &hints {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(text).small());
                    if let Some(action) = action {
                        if ui.small_button(action_label(action)).clicked() {
                            asked = Some(action.clone());
                        }
                    }
                });
            }
        });
        if let Some(action) = asked {
            self.invoke(&action);
        }
    }
}

/// What the button of a piece of advice says.
fn action_label(action: &str) -> &'static str {
    match action.split_once(':').map_or(action, |(verb, _)| verb) {
        "focus_field" => "Go to the setting",
        _ => "Open Settings",
    }
}
