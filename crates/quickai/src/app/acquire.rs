//! Getting models into the models folder: what the box of *Add a model* does with what is typed in it for an engine
//! that keeps files, and the download under way (`add_model.rs` draws them).

use eframe::egui;

use super::workbench::Given;
use super::App;
use crate::downloader;

impl App {
    /// What Enter in the box does for an engine that keeps files (llama.cpp): a link downloads that file, a path of
    /// this computer links the file into the models folder, and words are looked up online. The page of a file of the
    /// hub is the address of the file, the page of a repository is its name to search for, and what is not a .gguf is
    /// said and not fetched: a page saved under the name of a model is not one.
    pub(super) fn submit_file_box(&mut self, ctx: &egui::Context) {
        let trimmed = self.model_input.trim().to_string();
        if trimmed.is_empty() || self.download.is_some() {
            return;
        }
        let is_url = downloader::looks_like_url(&trimmed);
        let given = match super::workbench::read_box_for(&trimmed, &[".gguf"]) {
            // an address with no file name at its end (a server that names the file itself) is fetched as it always
            // was, and kept as download.gguf; one whose name is not a .gguf is refused
            Given::Refused(_) if is_url && !names_a_file(&trimmed) => Given::Link(trimmed.clone()),
            other => other,
        };
        let source = match given {
            Given::Link(address) => Some(address),
            Given::File(path) => Some(path.display().to_string()),
            Given::Words(words) => {
                self.start_lookup(ctx, &words);
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
            self.lookup.open = false;
            self.download = Some(downloader::spawn_download(
                source,
                None,
                self.models_dir.clone(),
                super::calm_wake(ctx.clone()),
            ));
        }
    }

    /// The download under way, with its Cancel, or why the last one failed until it is dismissed.
    pub(super) fn show_download_status(&mut self, ui: &mut egui::Ui) {
        if let Some(job) = &self.download {
            ui.add_space(6.0);
            let mut cancel = false;
            ui.horizontal(|ui| {
                ui.colored_label(super::theme::palette().warn, job.progress_line());
                cancel = ui
                    .small_button("Cancel")
                    .on_hover_text(
                        "Stop the download. Get it again to go on from where it stopped.",
                    )
                    .clicked();
            });
            if let Some(fraction) = job.fraction() {
                super::widgets::progress_bar(ui, Some(fraction));
            }
            if cancel {
                self.cancel_download();
            }
        } else if let Some(note) = self.catalog.note.clone() {
            // a download that failed says so where it was asked for: the log has the same words (the next download
            // clears this, or its end)
            ui.add_space(6.0);
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
