//! The ℹ About window: usage, disclaimer and licences.

use eframe::egui;

use super::App;

impl App {
    pub(super) fn show_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        let mut close_clicked = false;
        egui::Window::new("ℹ About QuickAI")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .max_height(600.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("QuickAI v{}", env!("CARGO_PKG_VERSION")))
                                .strong()
                                .size(16.0),
                        );
                        ui.label(
                        "A simple GUI for starting and stopping local GGUF language models via \
                         llama-server (llama.cpp), with GPU acceleration through Vulkan.",
                    );
                        ui.add_space(10.0);

                        ui.label(egui::RichText::new("Usage").strong());
                        ui.label(
                            "Pick a model, optionally adjust the port / context size / GPU layers \
                         (⚙ Settings has more, including advanced llama.cpp options), then click \
                         Start and wait for the status to turn green. \"Open Chat UI\" opens a \
                         browser-based chat; \"🔌 API\" shows how to connect an external app \
                         instead. Only one model runs at a time.",
                        );
                        ui.add_space(10.0);

                        ui.label(egui::RichText::new("Disclaimer").strong());
                        ui.label(
                            "Everything runs locally on this machine — your prompts and any files \
                         you feed the model never leave it, except when you explicitly use the \
                         \"Model\" box or \"Look up online\" (which contact the URL you gave or \
                         Hugging Face to fetch/search for a file) or ⚙ Settings' \"Look up \
                         llama.cpp builds\" (which contacts GitHub to list/download \
                         llama-server releases). Model output can be inaccurate, \
                         biased, or simply wrong; treat it as a draft, not a fact, and don't \
                         rely on it for medical, legal, financial, or other safety-critical \
                         decisions. This software is provided \"as is\", without warranty of \
                         any kind — see the license below.",
                        );
                        ui.add_space(10.0);

                        ui.label(egui::RichText::new("License").strong());
                        ui.label("QuickAI's own source code is licensed under the PolyForm Noncommercial License 1.0.0.");
                        ui.add_space(4.0);
                        ui.label(
                            "This package also bundles, unmodified and under their own licenses:",
                        );
                        ui.label(
                            "• llama-server, from llama.cpp — MIT License, © the ggml authors.\n\
                         • Bonsai 8B and Bonsai 1.7B model weights, from PrismML — Apache \
                         License 2.0.",
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Full license texts for all three are in LICENSE and \
                             THIRD_PARTY_LICENSES.md next to this app.",
                            )
                            .small()
                            .color(egui::Color32::GRAY),
                        );
                    });

                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            });
        self.show_about = open && !close_clicked;
    }
}
