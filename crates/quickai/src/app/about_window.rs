//! The ℹ About window: usage, disclaimer and licences, and on a tab of its own the companies EverybodyAI works with.

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

impl App {
    /// The partners' icons as pictures, made when the tab is first shown.
    fn partner_logos(&mut self, ctx: &egui::Context) -> Vec<egui::TextureId> {
        if self.partner_logos.len() != PARTNERS.len() {
            self.partner_logos = PARTNERS
                .iter()
                .map(|partner| {
                    let icon = eframe::icon_data::from_png_bytes(partner.icon).expect("a partner's icon is a PNG");
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
        self.partner_logos.iter().map(|texture| texture.id()).collect()
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
                        "With {name} chosen, Settings' \"Look up {name} builds\" contacts \
                         GitHub to list and download its releases, only when you click it.{contacts}"
                    ),
                    format!(
                        "• {name} is not part of this package: it is downloaded on request, \
                         under its own license ({license}), and is not bundled."
                    ),
                )
            });
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
                        ui.label(
                        "A simple GUI for starting and stopping local GGUF language models via \
                         llama-server (llama.cpp), with GPU acceleration through Vulkan.",
                    );
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("Usage").strong());
                        ui.label(
                            "Pick a model in the list on the left. The panel at the right says how it \
                         runs (Settings has more, including advanced llama.cpp options). Press Start in \
                         the bar over the page and wait for it to say Running, then chat in the page. \
                         While it runs, the panel shows how to connect an external app, and Open Chat \
                         UI opens a chat in the browser. Only one model runs at a time.",
                        );
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("Disclaimer").strong());
                        ui.label(
                            "Everything runs locally on this machine — your prompts and any files \
                         you feed the model never leave it, except when you explicitly use the \
                         \"Get\" box or \"Look up online\" (which contact the URL you gave or \
                         Hugging Face to fetch/search for a file) or Settings' \"Look up \
                         llama.cpp builds\" (which contacts GitHub to list/download \
                         llama-server releases). QuickAI sends no reports by itself: when \
                         something breaks it shows one, and you choose whether to post it. Model output can be inaccurate, \
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
                                "{name}, the chat page Settings can add, contacts nothing by itself and is \
                                 started offline. Settings' \"Look up {name} builds\" contacts the Python \
                                 Package Index (pypi.org), and installing it fetches Python packages from \
                                 there and PyTorch from download.pytorch.org, only when you click."
                            ));
                        }
                        ui.add_space(6.0);

                        ui.label(egui::RichText::new("License").strong());
                        ui.label(
                            "QuickAI's own source code is licensed under the MIT License: free to \
                         use for any purpose, at home or at work, and free to change and share.",
                        );
                        ui.add_space(4.0);
                        ui.label(
                            "This package also bundles, unmodified and under their own licenses:",
                        );
                        ui.label(
                            "• llama-server, from llama.cpp — MIT License, © the ggml authors.\n\
                         • Bonsai 2 4B model weights, created using Bonsai by Prism ML from \
                         Qwen3.5-4B (© Alibaba Cloud) — Apache License 2.0.",
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
                                "Full license texts for these, and for the Rust libraries this app is \
                             built from, are in LICENSE and THIRD_PARTY_LICENSES.md next to this app.",
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
