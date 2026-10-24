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
        // the program that goes with the engine, if one does (Open WebUI beside Ollama): downloaded on request too
        let companion = self.companion_doc().map(|c| {
            (
                c.doc.engine.name.clone(),
                c.doc.engine.license.clone().unwrap_or_default(),
            )
        });
        // an engine that is downloaded on request, not shipped, says so (its own license too); the
        // text of a window for llama.cpp, which ships, is what it has always been
        let downloaded = self
            .engine_doc()
            .filter(|e| !e.doc.engine.bundled)
            .map(|e| {
                let name = e.doc.engine.name.clone();
                let license = e.doc.engine.license.clone().unwrap_or_default();
                let hosts = e.doc.engine.egress.join(", ");
                let contacts = if hosts.is_empty() {
                    String::new()
                } else {
                    format!(
                        " When you pull a model it is {name} that contacts {hosts}, and only \
                         when you ask."
                    )
                };
                (
                    format!(
                        "With {name} chosen, ⚙ Settings' \"Look up {name} builds\" contacts \
                         GitHub to list and download its releases, only when you click it.{contacts}"
                    ),
                    format!(
                        "• {name} is not part of this package: it is downloaded on request, \
                         under its own license ({license}), and is not bundled."
                    ),
                )
            });
        // what is read scrolls, and the window's title, margins and Close button are 112 px: with less than a window
        // of 592 to put it in, the text gets what is left (so that Close is in view)
        let scroll = (ctx.content_rect().height() - 112.0).clamp(160.0, 480.0);
        egui::Window::new("ℹ About QuickAI")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .max_height(600.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(scroll)
                    .show(ui, |ui| {
                        // paragraphs, close together: it is text, and the sections have the space between them
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(
                            egui::RichText::new(format!("QuickAI v{}", env!("CARGO_PKG_VERSION")))
                                .strong()
                                .size(16.0),
                        );
                        ui.label(
                        "A simple GUI for starting and stopping local GGUF language models via \
                         llama-server (llama.cpp), with GPU acceleration through Vulkan.",
                    );
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("Usage").strong());
                        ui.label(
                            "Pick a model, optionally adjust the port / context size / GPU layers \
                         (⚙ Settings has more, including advanced llama.cpp options), then click \
                         Start and wait for the status to turn green. \"Open Chat UI\" opens a \
                         browser-based chat; \"🔌 API\" shows how to connect an external app \
                         instead. Only one model runs at a time.",
                        );
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("Disclaimer").strong());
                        ui.label(
                            "Everything runs locally on this machine — your prompts and any files \
                         you feed the model never leave it, except when you explicitly use the \
                         \"Get\" box or \"Look up online\" (which contact the URL you gave or \
                         Hugging Face to fetch/search for a file) or ⚙ Settings' \"Look up \
                         llama.cpp builds\" (which contacts GitHub to list/download \
                         llama-server releases). Model output can be inaccurate, \
                         biased, or simply wrong; treat it as a draft, not a fact, and don't \
                         rely on it for medical, legal, financial, or other safety-critical \
                         decisions. This software is provided \"as is\", without warranty of \
                         any kind — see the license below.",
                        );
                        if let Some((contacts, _)) = &downloaded {
                            ui.add_space(4.0);
                            ui.label(contacts);
                        }
                        if let Some((name, _)) = &companion {
                            ui.add_space(4.0);
                            ui.label(format!(
                                "{name}, the chat page ⚙ Settings can add, contacts nothing by itself and is \
                                 started offline. ⚙ Settings' \"Look up {name} builds\" contacts the Python \
                                 Package Index (pypi.org), and installing it fetches Python packages from \
                                 there and PyTorch from download.pytorch.org, only when you click."
                            ));
                        }
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("License").strong());
                        ui.label(
                            "QuickAI's own source code is licensed under the PolyForm Noncommercial \
                         License 1.0.0: free for personal use, study and research, and for \
                         charities, schools and public institutions. Using it in or for a business \
                         needs a commercial license: see everybodyai.dev/licensing.",
                        );
                        ui.add_space(4.0);
                        ui.label(
                            "This package also bundles, unmodified and under their own licenses:",
                        );
                        ui.label(
                            "• llama-server, from llama.cpp — MIT License, © the ggml authors.\n\
                         • Bonsai 8B and Bonsai 1.7B model weights, from PrismML — Apache \
                         License 2.0.",
                        );
                        if let Some((_, license)) = &downloaded {
                            ui.add_space(4.0);
                            ui.label(license);
                        }
                        if let Some((name, license)) = &companion {
                            ui.add_space(4.0);
                            ui.label(format!(
                                "• {name} is not part of this package either: it is downloaded on request \
                                 from the Python Package Index, under its own license ({license}, which \
                                 asks that its name and logo be left as they are), and is not bundled."
                            ));
                        }
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Full license texts for all three are in LICENSE and \
                             THIRD_PARTY_LICENSES.md next to this app.",
                            )
                            .small()
                            .color(super::theme::palette().muted),
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
