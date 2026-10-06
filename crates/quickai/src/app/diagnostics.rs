//! What to tell the user when the server cannot start or has crashed (panel `diagnostics`).

use eframe::egui;

use super::App;
use crate::events::ServerState;
use crate::gpu;
use crate::launch::ENGINE_ID;

impl App {
    /// The missing-binary line and the crash panel with its advice.
    pub(super) fn show_diagnostics(&mut self, ui: &mut egui::Ui, state: ServerState) {
        if self.missing_binary_warned {
            ui.colored_label(
                egui::Color32::from_rgb(192, 57, 43),
                format!(
                    "llama-server binary not found at {}",
                    self.llama_server_bin.display()
                ),
            );
        }

        if state == ServerState::Crashed {
            ui.add_space(6.0);
            ui.group(|ui| {
                ui.colored_label(
                    Self::state_color(state),
                    "The server exited before it finished starting — see the log below \
                     for the exact reason (a common one: \"unknown model architecture\", \
                     meaning this llama-server build predates support for this model).",
                );
                let path = self.llama_server_bin.display().to_string();
                let backend = self
                    .registry
                    .engine(ENGINE_ID)
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
    }
}
