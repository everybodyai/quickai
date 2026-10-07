//! Getting models into the models folder (panel `acquire`).

use std::path::Path;

use eframe::egui;

use super::App;
use crate::downloader;

impl App {
    /// The "Model:" box (download, import or search), "Look up online", the running download's
    /// progress and the look-up results.
    pub(super) fn show_acquire(&mut self, ui: &mut egui::Ui, active: bool) {
        if self.lists_models() {
            self.show_named_acquire(ui, active);
            return;
        }
        let can_look_up = self.can_look_up();
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!active && self.download.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Model:").on_hover_text(
                            "One box, three things it can mean: a direct https:// link \
                             downloads that file, an existing local path imports it, and \
                             anything else is treated as a Hugging Face search term. Use \
                             \"...\" to browse for a local file instead of typing a path.",
                        );
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.model_input)
                                .hint_text("URL, local path, or search by name...")
                                .desired_width(260.0),
                        );
                        if ui
                            .button("...")
                            .on_hover_text("Browse for a local .gguf file")
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("GGUF model", &["gguf"])
                                .pick_file()
                            {
                                self.model_input = path.display().to_string();
                            }
                        }
                        let trimmed = self.model_input.trim().to_string();
                        let is_url = downloader::looks_like_url(&trimmed);
                        let is_local_file = !is_url && Path::new(&trimmed).is_file();
                        let (button_label, button_hover) = if trimmed.is_empty() {
                            (
                                "Search",
                                "Type a URL, a local file path, or a model name to search \
                                 Hugging Face for.",
                            )
                        } else if is_url {
                            ("Download", "Download that URL into your models folder.")
                        } else if is_local_file {
                            (
                                "Import",
                                "Link that local file into your models folder — instant \
                                 and no extra disk space, falling back to a copy only if \
                                 linking isn't possible.",
                            )
                        } else {
                            (
                                "Search",
                                "Search Hugging Face by name, then automatically widen the \
                                 search to similar models too — all listed together below.",
                            )
                        };
                        let submitted =
                            field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let go = ui
                            .add_enabled(!trimmed.is_empty(), egui::Button::new(button_label))
                            .on_hover_text(button_hover)
                            .clicked();
                        if !trimmed.is_empty() && (go || submitted) {
                            if is_url || is_local_file {
                                self.model_input.clear();
                                let ctx_clone = ui.ctx().clone();
                                self.download = Some(downloader::spawn_download(
                                    trimmed,
                                    None,
                                    self.models_dir.clone(),
                                    move || ctx_clone.request_repaint(),
                                ));
                            } else {
                                self.start_lookup(ui.ctx(), &trimmed);
                            }
                        }
                        ui.add_space(8.0);
                        if ui
                            .add_enabled(can_look_up, egui::Button::new("Look up online"))
                            .on_hover_text(
                                "Search Hugging Face for currently popular GGUF models you \
                                 can download with one click, without knowing a direct URL \
                                 or name.",
                            )
                            .clicked()
                        {
                            self.start_lookup(ui.ctx(), "");
                        }
                        if ui
                            .add_enabled(self.can_recommend(), egui::Button::new("Recommended"))
                            .on_hover_text(
                                "Browse the models QuickAI recommends, from small ones that run \
                                 on any computer to bigger ones: chat, code, pictures, audio, \
                                 documents, translation and search. The list is part of the \
                                 application: nothing is asked of the internet to open it.",
                            )
                            .clicked()
                        {
                            self.open_catalog();
                        }
                    });
                });
                // Only shown once the lookup section actually has models
                // on screen to close — hidden entirely while idle or
                // still searching, rather than shown greyed out with
                // nothing for it to do.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.lookup.has_results()
                        && ui
                            .add(egui::Button::new("X").small())
                            .on_hover_text("Close lookup results")
                            .clicked()
                    {
                        self.lookup.open = false;
                    }
                });
            });

            if let Some(job) = &self.download {
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(212, 160, 23), job.progress_line());
                if let Some(fraction) = job.fraction() {
                    super::widgets::progress_bar(ui, Some(fraction));
                }
            }

            self.show_lookup_results(ui);
        });
    }
}
