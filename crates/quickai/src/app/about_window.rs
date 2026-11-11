//! The ℹ About window: usage, privacy, disclaimer and licences, and on a tab of its own the companies EverybodyAI works with.

use std::cell::Cell;

use eframe::egui;

use super::theme;
use super::App;

/// The title of the window, which is one of its own (C194).
const ABOUT_TITLE: &str = "About QuickAI";

/// Which window About is: the id the display knows it by.
pub(super) fn about_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_about")
}

/// The tabs of About: QuickAI itself, and its partners.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AboutTab {
    #[default]
    About,
    Partners,
}

/// A company EverybodyAI works with: its name, its site, what it does, and its icon (a PNG of assets/partners).
struct Partner {
    name: &'static str,
    site: &'static str,
    about: &'static str,
    icon: &'static [u8],
}

const PARTNERS: &[Partner] = &[Partner {
    name: "Tiqnova",
    site: "https://tiqnova.io",
    about: "Software and AI, built around how your business works. One team, from strategy to long-term support.",
    icon: include_bytes!("../../../../assets/partners/tiqnova.png"),
}];

/// The row of About's tabs, as the dock under the page has its own.
fn tab_row(ui: &mut egui::Ui, tab: &Cell<AboutTab>) {
    ui.horizontal(|ui| {
        for (which, label) in [(AboutTab::About, "About"), (AboutTab::Partners, "Partners")] {
            if super::shell::dock_tab(ui, label, tab.get() == which).clicked() {
                tab.set(which);
            }
        }
    });
}

/// What QuickAI is, whichever package it came in.
const INTRO: &str = "QuickAI is a desktop application for running AI models locally. It recommends models suited \
     to your hardware, downloads and runs them, and lets you chat with them in its own window or connect other \
     applications through a local API. Models run on llama.cpp, Ollama or ComfyUI (for images and video), and \
     QuickAI can also use models hosted on your other computers or on a server you specify. Optional voice \
     features let you speak to the model and have its answers read aloud.";

/// The packages whose names end in `-with-llamacpp-bonsai2-4b`.
const BUNDLED: &str =
    "This edition includes llama.cpp, the Bonsai 2 4B model and a speech recognition model, \
     so you can start your first chat without downloading anything.";

/// The app alone.
const ALONE: &str =
    "This edition contains the application only. llama.cpp, models and other components are \
     downloaded when you request them.";

const USAGE: &str = "Select a model from the list on the left, or choose one under Recommended. Press Start in \
     the toolbar above the page, wait until the status shows Running, and then chat on the page. The panel on the \
     right shows how the model runs; further options are available in Settings. While a model is running, the \
     panel also explains how to connect external applications, and Open Chat UI opens a chat interface in your \
     browser. One model runs at a time.";

/// What QuickAI is designed to keep on this computer, and when it connects.
const PRIVACY: &str = "QuickAI is designed to keep your prompts, the model's responses, your voice and your files \
     on this computer. It connects to the internet only at your request: to look up and download an engine's builds \
     (from GitHub); to search for and download models (from Hugging Face, or from an address you provide); to \
     download a voice; or to use a model on another computer or server that you have specified, which then \
     receives the data you send to it. QuickAI does not send reports automatically: when a problem occurs, it \
     displays a report that you may choose to submit.";

/// That the above is a design, not a guarantee.
const NO_GUARANTEE: &str = "While we have taken care to ensure that no other data leaves this computer, we cannot \
     guarantee it. Software may contain defects, and an error or malfunction in QuickAI, or in an engine, model, \
     tool (MCP server) or web page it runs, could cause data to be transmitted or retained unexpectedly.";

const NO_WARRANTY: &str = "AI models can produce inaccurate, biased or incomplete output. Treat their responses \
     as drafts rather than established facts, and do not rely on them for medical, legal, financial or other \
     safety-critical decisions. QuickAI is provided \"as is\", without warranty of any kind, express or implied, \
     and its authors accept no liability for any damages arising from its use, as set out in the license below.";

/// What the bundled packages carry, the others download on request too.
const ON_REQUEST: &str = "Other components are downloaded only at your request, each under its own license: \
     Ollama (MIT), ComfyUI (GPL-3.0), Open WebUI (Open WebUI License, which requires that its name and logo be \
     retained), the voice program quickai-voice (GPL-3.0) and its voices, and each model, under the license set \
     by its publisher.";

const ON_REQUEST_ALONE: &str = "Other components are downloaded only at your request, each under its own \
     license: llama.cpp (MIT), the speech recognition model (MIT), Ollama (MIT), ComfyUI (GPL-3.0), Open WebUI \
     (Open WebUI License, which requires that its name and logo be retained), the voice program quickai-voice \
     (GPL-3.0) and its voices, and each model, under the license set by its publisher.";

/// What this package carries besides QuickAI, seen beside its program: llama-server, the model and the speech model
/// of the packages whose names end in `-with-llamacpp-bonsai2-4b`, and quickai-whisper, which every package has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Carried {
    llama_server: bool,
    bonsai: bool,
    speech_model: bool,
    whisper: bool,
}

impl Carried {
    fn here() -> Self {
        let beside = |name: &str| {
            crate::paths::exe_dir().is_some_and(|dir| {
                dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
                    .is_file()
            })
        };
        let speech = crate::paths::bundled_speech_dir();
        Carried {
            llama_server: beside("llama-server"),
            bonsai: crate::paths::bundled_models_dir()
                .join("Ternary-Bonsai-2-4B-Q2_0.gguf")
                .is_file(),
            speech_model: std::fs::read_dir(&speech).is_ok_and(|entries| {
                entries
                    .flatten()
                    .any(|e| e.path().extension().is_some_and(|x| x == "bin"))
            }),
            whisper: beside("quickai-whisper"),
        }
    }

    /// The package with llama.cpp and a model in it.
    fn bundled(self) -> bool {
        self.llama_server && self.bonsai
    }

    /// The programs and models this package carries, with their licenses.
    fn list(self) -> String {
        let mut lines = Vec::new();
        if self.llama_server {
            lines.push("• llama-server, from llama.cpp — MIT License, © the ggml authors.");
        }
        if self.bonsai {
            lines.push(
                "• Bonsai 2 4B model weights, created using Bonsai by Prism ML from Qwen3.5-4B \
                 (© Alibaba Cloud) — Apache License 2.0.",
            );
        }
        if self.speech_model {
            lines.push(
                "• Whisper base speech recognition model, by OpenAI, in whisper.cpp format — MIT License.",
            );
        }
        if self.whisper {
            lines.push(
                "• quickai-whisper, QuickAI's speech-to-text program, built on whisper.cpp — MIT License.",
            );
        }
        if lines.is_empty() {
            return "This package includes no third-party programs or models.".into();
        }
        format!(
            "This package also includes the following components, each under its own license:\n{}",
            lines.join("\n")
        )
    }
}

impl App {
    /// The partners' icons as pictures, made when the tab is first shown.
    fn partner_logos(&mut self, ctx: &egui::Context) -> Vec<egui::TextureId> {
        if self.partner_logos.len() != PARTNERS.len() {
            self.partner_logos = PARTNERS
                .iter()
                .map(|partner| {
                    let icon = eframe::icon_data::from_png_bytes(partner.icon)
                        .expect("a partner's icon is a PNG");
                    ctx.load_texture(
                        format!("partner-{}", partner.name),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [icon.width as usize, icon.height as usize],
                            &icon.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                })
                .collect();
        }
        self.partner_logos
            .iter()
            .map(|texture| texture.id())
            .collect()
    }

    /// The Partners tab: each company with its icon, its name, a link to its site, and what it does.
    fn show_partners(&mut self, ui: &mut egui::Ui) {
        let logos = self.partner_logos(ui.ctx());
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label("Companies that work with EverybodyAI, the makers of QuickAI.");
        ui.add_space(12.0);
        for (partner, logo) in PARTNERS.iter().zip(logos) {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new((logo, egui::Vec2::splat(48.0))).corner_radius(10.0));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(partner.name).strong().size(16.0));
                    if ui
                        .link(partner.site.trim_start_matches("https://"))
                        .on_hover_text(format!("{} on the web", partner.name))
                        .clicked()
                    {
                        let _ = webbrowser::open(partner.site);
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(partner.about);
            ui.add_space(12.0);
        }
    }

    pub(super) fn show_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut close = false;
        let tab = Cell::new(self.about_tab);
        // the program that goes with the engine, if one does (Open WebUI beside Ollama): downloaded on request too
        let companion = self.companion_doc().map(|c| c.doc.engine.name.clone());
        // an engine that is downloaded on request, not shipped, says what it contacts
        let downloaded = self
            .engine_doc()
            .filter(|e| !e.doc.engine.bundled)
            .map(|e| {
                let name = e.doc.engine.name.clone();
                let hosts = e.doc.engine.egress.join(", ");
                let contacts = if hosts.is_empty() {
                    String::new()
                } else {
                    format!(
                        " When you download a model, {name} itself contacts {hosts}, only at \
                         your request."
                    )
                };
                format!(
                    "When {name} is selected, \"Look up {name} builds\" in Settings contacts \
                     GitHub to list and download its releases, only when you click it.{contacts}"
                )
            });
        let carried = Carried::here();
        let id = about_viewport();
        if std::mem::take(&mut self.raise_about) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let builder = theme::own_window(ABOUT_TITLE, [560.0, 600.0], [400.0, 300.0]);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            let text = |ui: &mut egui::Ui| {
                        if tab.get() == AboutTab::Partners {
                            self.show_partners(ui);
                            return;
                        }
                        // paragraphs, close together: it is text, and the sections have the space between them
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.horizontal(|ui| {
                            self.show_logo(ui, 48.0);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.heading(format!("QuickAI {}", env!("CARGO_PKG_VERSION")));
                                let site = super::shell::WEBSITE_URL;
                                if ui
                                    .link(site.trim_start_matches("https://"))
                                    .on_hover_text("QuickAI on the web")
                                    .clicked()
                                {
                                    let _ = webbrowser::open(site);
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.label(INTRO);
                        ui.add_space(4.0);
                        ui.label(if carried.bundled() { BUNDLED } else { ALONE });
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("Usage").strong());
                        ui.label(USAGE);
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("Privacy").strong());
                        ui.label(PRIVACY);
                        if let Some(contacts) = &downloaded {
                            ui.add_space(4.0);
                            ui.label(contacts);
                        }
                        if let Some(name) = &companion {
                            ui.add_space(4.0);
                            ui.label(format!(
                                "{name}, the chat interface available in Settings, makes no connections of \
                                 its own and is started in offline mode. \"Look up {name} builds\" in \
                                 Settings contacts the Python Package Index (pypi.org); installing it \
                                 downloads Python packages from there and PyTorch from \
                                 download.pytorch.org, only when you click."
                            ));
                        }
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("Disclaimer").strong());
                        ui.label(NO_GUARANTEE);
                        ui.add_space(4.0);
                        ui.label(NO_WARRANTY);
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("License").strong());
                        ui.label(
                            "QuickAI's source code is licensed under the MIT License. You may use it \
                             for any purpose, personal or commercial, and modify and redistribute it.",
                        );
                        ui.add_space(4.0);
                        ui.label(carried.list());
                        ui.add_space(4.0);
                        ui.label(if carried.bundled() { ON_REQUEST } else { ON_REQUEST_ALONE });
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "The full license texts for these components, and for the Rust libraries \
                                 QuickAI is built with, are provided in LICENSE and THIRD_PARTY_LICENSES.md \
                                 alongside the application.",
                            )
                            .small()
                            .color(theme::palette().muted),
                        );
            };
            if class == egui::ViewportClass::EmbeddedWindow {
                // a context that cannot open a window (a test's) shows it inside the main one: what is read scrolls, and
                // the window's title, margins and Close button are 112 px; with less than a window of 592 to put it in, the
                // text gets what is left (so that Close is in view)
                let scroll = (ui.ctx().content_rect().height() - 112.0).clamp(160.0, 480.0);
                tab_row(ui, &tab);
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(scroll).show(ui, text);
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            } else {
                if ui.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
                egui::Panel::top("about_tabs")
                    .frame(theme::page_frame())
                    .show(ui, |ui| tab_row(ui, &tab));
                egui::Panel::bottom("about_footer")
                    .frame(theme::page_frame())
                    .show(ui, |ui| {
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                egui::CentralPanel::default()
                    .frame(theme::page_frame())
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, text);
                    });
            }
        });
        self.about_tab = tab.get();
        if close {
            self.show_about = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Carried;

    #[test]
    fn about_says_what_this_package_carries() {
        let alone = Carried {
            whisper: true,
            ..Carried::default()
        };
        assert!(!alone.bundled());
        assert!(alone.list().contains("quickai-whisper") && !alone.list().contains("llama-server"));
        let bundled = Carried {
            llama_server: true,
            bonsai: true,
            speech_model: true,
            whisper: true,
        };
        assert!(bundled.bundled());
        assert_eq!(bundled.list().lines().count(), 5);
        assert!(Carried::default().list().contains("no third-party"));
    }
}
