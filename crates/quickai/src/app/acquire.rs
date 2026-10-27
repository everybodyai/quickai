//! Getting models into the models folder (panel `acquire`).

use std::path::Path;

use eframe::egui;

use super::workbench::Given;
use super::App;
use crate::downloader;

impl App {
    /// The "Get:" box (download, import or search), "Look up online", the running download's
    /// progress and the look-up results.
    pub(super) fn show_acquire(&mut self, ui: &mut egui::Ui, active: bool) {
        if self.lists_models() {
            self.show_named_acquire(ui, active);
            return;
        }
        let can_look_up = self.can_look_up();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!active && self.download.is_none(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Get:").on_hover_text(
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
                        // the page of a file of the hub is the address of the file, the page of a repository is its
                        // name to search for, and what is not a .gguf is said and not fetched: a page saved under the
                        // name of a model is not one
                        let given = match super::workbench::read_box_for(&trimmed, &[".gguf"]) {
                            // an address with no file name at its end (a server that names the file itself) is fetched
                            // as it always was, and kept as download.gguf; one whose name is not a .gguf is refused
                            Given::Refused(_) if is_url && !names_a_file(&trimmed) => {
                                Given::Link(trimmed.clone())
                            }
                            other => other,
                        };
                        let source = match given {
                            Given::Link(address) => Some(address),
                            Given::File(path) => Some(path.display().to_string()),
                            Given::Words(words) => {
                                self.start_lookup(ui.ctx(), &words);
                                None
                            }
                            Given::Refused(why) => {
                                self.catalog.note = Some(why);
                                None
                            }
                            Given::Nothing => None,
                        };
                        if let Some(source) = source {
                            self.model_input.clear();
                            self.catalog.note = None;
                            let ctx_clone = ui.ctx().clone();
                            self.download = Some(downloader::spawn_download(
                                source,
                                None,
                                self.models_dir.clone(),
                                super::calm_wake(ctx_clone),
                            ));
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
                if self.lookup.has_results() && super::named_models::close_button(ui).clicked() {
                    self.lookup.open = false;
                }
            });
        });

        if let Some(job) = &self.download {
            ui.add_space(4.0);
            let mut cancel = false;
            ui.horizontal(|ui| {
                ui.colored_label(super::theme::palette().warn, job.progress_line());
                cancel = ui
                    .small_button("Cancel")
                    .on_hover_text("Stop the download. What was fetched so far is thrown away.")
                    .clicked();
            });
            if let Some(fraction) = job.fraction() {
                super::widgets::progress_bar(ui, Some(fraction));
            }
            if cancel {
                self.cancel_download();
            }
        } else if let Some(note) = self.catalog.note.clone() {
            // a download that failed says so where it was asked for: the log has the same words, and it is not
            // on the page while the results of a look-up are (the next download clears this, or its end)
            ui.add_space(4.0);
            let mut dismissed = false;
            super::theme::alert(ui, super::theme::Tone::Bad, |ui| {
                ui.label(note);
                dismissed = super::theme::button_sized(
                    ui,
                    super::theme::Role::Quiet,
                    true,
                    "Dismiss",
                    egui::vec2(0.0, 22.0),
                )
                .clicked();
            });
            if dismissed {
                self.catalog.note = None;
            }
        }

        self.show_lookup_results(ui);
    }
}

/// Whether the last part of the path of `url` (without a query) is a file name with an ending (`x.gguf`, `page.html`).
fn names_a_file(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let path = path.split_once("://").map_or(path, |(_, rest)| rest);
    let last = path
        .split_once('/')
        .map_or("", |(_, p)| p)
        .rsplit('/')
        .next()
        .unwrap_or("");
    last.rsplit_once('.')
        .is_some_and(|(stem, ending)| !stem.is_empty() && !ending.is_empty())
}

#[cfg(test)]
mod tests {
    use super::names_a_file;

    #[test]
    fn an_address_names_a_file_when_its_last_part_has_an_ending() {
        assert!(names_a_file("https://x.co/a/model.gguf?download=1"));
        assert!(names_a_file("https://x.co/blob/main/page.html"));
        assert!(!names_a_file("https://x.co/"));
        assert!(!names_a_file("https://x.co"));
        assert!(!names_a_file("https://x.co/files/12345"));
        assert!(!names_a_file("http://127.0.0.1:8000/?file=x"));
    }
}
