//! Getting models into the models folder (panel `acquire`).

use std::path::Path;

use eframe::egui;

use super::App;
use crate::downloader;
use crate::hardware;
use crate::remote;

impl App {
    /// The "Model:" box (download, import or search), "Look up online", the running download's
    /// progress and the look-up results.
    pub(super) fn show_acquire(&mut self, ui: &mut egui::Ui, active: bool) {
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
                                self.show_lookup = true;
                                self.lookup_results.clear();
                                self.lookup_status =
                                    format!("Searching Hugging Face for \"{trimmed}\"...");
                                let ctx_clone = ui.ctx().clone();
                                self.lookup_rx =
                                    Some(remote::spawn_keyword_search(trimmed, move || {
                                        ctx_clone.request_repaint()
                                    }));
                            }
                        }
                        ui.add_space(8.0);
                        if ui
                            .button("Look up online")
                            .on_hover_text(
                                "Search Hugging Face for currently popular GGUF models you \
                                 can download with one click, without knowing a direct URL \
                                 or name.",
                            )
                            .clicked()
                        {
                            self.show_lookup = true;
                            self.lookup_results.clear();
                            self.lookup_status = "Searching Hugging Face for GGUF models...".into();
                            let ctx_clone = ui.ctx().clone();
                            self.lookup_rx =
                                Some(remote::spawn_lookup(move || ctx_clone.request_repaint()));
                        }
                    });
                });
                // Only shown once the lookup section actually has models
                // on screen to close — hidden entirely while idle or
                // still searching, rather than shown greyed out with
                // nothing for it to do.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let has_results = self.show_lookup && !self.lookup_results.is_empty();
                    if has_results
                        && ui
                            .add(egui::Button::new("X").small())
                            .on_hover_text("Close lookup results")
                            .clicked()
                    {
                        self.show_lookup = false;
                    }
                });
            });

            if let Some(job) = &self.download {
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(212, 160, 23), job.progress_line());
                if let Some(fraction) = job.fraction() {
                    ui.add(egui::ProgressBar::new(fraction));
                }
            }

            if self.show_lookup {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(&self.lookup_status)
                        .small()
                        .color(egui::Color32::GRAY),
                );
                if !self.lookup_results.is_empty() {
                    let mut to_download: Option<usize> = None;
                    const GB: u64 = 1024 * 1024 * 1024;
                    // First section is the plain top-10 (already
                    // downloads-sorted by the search). The rest slice
                    // the same pool by file size, so someone on
                    // constrained hardware has a browsable "Small"
                    // list instead of only ever seeing the biggest,
                    // most-downloaded repos.
                    let sections: [(Option<&str>, u64, u64); 4] = [
                        (None, 0, u64::MAX),
                        (Some("High (15 GB+)"), 15 * GB, u64::MAX),
                        (Some("Medium (4-15 GB)"), 4 * GB, 15 * GB),
                        (Some("Small (< 4 GB)"), 0, 4 * GB),
                    ];

                    // A fixed-height scroll area with its own collapsing
                    // header: the result list (up to 4 sections * 10
                    // entries) can get long, so it needs a hard size cap
                    // with scrollbars rather than pushing the rest of
                    // the window down, and a way to tuck it away once
                    // you've found what you wanted without losing the
                    // results if you reopen it.
                    egui::CollapsingHeader::new(format!("Results ({})", self.lookup_results.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            // `both()` (not just vertical) so a result
                            // line too wide for the panel gets its own
                            // bottom scrollbar instead of being clipped
                            // or forcing the whole panel wider. Default
                            // auto_shrink would instead shrink the box
                            // down to the widest row whenever that's
                            // narrower than the panel — so turn it off
                            // for the horizontal axis: the box always
                            // fills the panel's actual width (adapting
                            // live as the window is resized), and the
                            // horizontal scrollbar only appears once a
                            // row is wider than that. Vertical
                            // auto_shrink stays on, so a short result
                            // list doesn't reserve the full max_height.
                            egui::ScrollArea::both()
                                .auto_shrink([false, true])
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    for (heading, min, max) in sections {
                                        let indices: Vec<usize> = self
                                            .lookup_results
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, m)| {
                                                heading.is_none()
                                                    || m.size_bytes
                                                        .map(|s| s >= min && s < max)
                                                        .unwrap_or(false)
                                            })
                                            .map(|(i, _)| i)
                                            .take(10)
                                            .collect();
                                        if indices.is_empty() {
                                            continue;
                                        }
                                        if let Some(h) = heading {
                                            ui.add_space(6.0);
                                            ui.label(egui::RichText::new(h).strong().small());
                                        }
                                        for i in indices {
                                            let m = &self.lookup_results[i];
                                            ui.horizontal(|ui| {
                                                if ui
                                                    .add_enabled(
                                                        self.download.is_none(),
                                                        egui::Button::new("Get"),
                                                    )
                                                    .clicked()
                                                {
                                                    to_download = Some(i);
                                                }
                                                let size = m
                                                    .size_bytes
                                                    .map(hardware::format_bytes)
                                                    .unwrap_or_else(|| "?".into());
                                                ui.label(format!(
                                                    "{} — {} ({size}, {} downloads)",
                                                    m.repo_id, m.filename, m.downloads
                                                ));
                                            });
                                        }
                                    }
                                });
                        });
                    if let Some(i) = to_download {
                        let m = &self.lookup_results[i];
                        let ctx_clone = ui.ctx().clone();
                        self.download = Some(downloader::spawn_download(
                            m.url.clone(),
                            Some(m.filename.clone()),
                            self.models_dir.clone(),
                            move || ctx_clone.request_repaint(),
                        ));
                    }
                }
            }
        });
    }
}
