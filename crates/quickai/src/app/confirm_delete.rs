//! The "are you sure?" modal every delete button goes through.

use eframe::egui;

use super::{App, PendingDelete};
use crate::bundles;

impl App {
    /// Deletes a model: its weights, the record beside them and the files that went with them that no
    /// other model uses, and says in the log what went and what stayed.
    pub(super) fn delete_model_files(&mut self, filename: &str, path: &std::path::Path) {
        let report = bundles::remove_model(path);
        match &report.weights_error {
            None => self.push_log(format!("Deleted model: {filename}")),
            Some(e) => self.push_log(format!("Could not delete {filename}: {e}")),
        }
        let also: Vec<&str> = report
            .removed
            .iter()
            .map(String::as_str)
            .filter(|name| *name != filename)
            .collect();
        if !also.is_empty() {
            self.push_log(format!("Also deleted: {}.", also.join(", ")));
        }
        for kept in &report.kept {
            self.push_log(format!("Kept {kept}: another model uses it."));
        }
        for failed in &report.failed {
            self.push_log(format!("Could not delete {failed}"));
        }
        self.rescan_models();
    }

    /// Shows a blocking "are you sure?" modal for `self.pending_delete`, if
    /// any — the confirmation step every delete button (model or installed
    /// llama-server build) goes through before anything actually touches
    /// disk. A no-op when nothing is pending.
    pub(super) fn show_confirm_delete_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_delete else {
            return;
        };
        let (title, body) = match pending {
            PendingDelete::Model { filename, path } => ("Delete model?".to_string(), {
                let mut body = format!("Delete \"{filename}\" from disk? This can't be undone.");
                for companion in bundles::companions(path) {
                    body.push_str(&format!(
                        "\nIts {} ({}) goes with it, unless another model uses it.",
                        companion.label, companion.name
                    ));
                }
                body
            }),
            PendingDelete::Listed { name } => (
                "Remove model?".to_string(),
                format!(
                    "Remove \"{name}\" from {}?\nThis can't be undone.",
                    self.engine_name()
                ),
            ),
            PendingDelete::Build { label, .. } => match self.runtime_texts() {
                Some(texts) => (texts.confirm_title.clone(), texts.confirm_body(label)),
                None => ("Delete build?".to_string(), format!("Delete \"{label}\"?")),
            },
            PendingDelete::Unfinished { label, .. } => match self.runtime_texts() {
                Some(texts) => (
                    texts.confirm_unfinished_title.clone(),
                    texts.confirm_unfinished_body(label),
                ),
                None => (
                    "Delete unfinished install?".to_string(),
                    format!("Delete \"{label}\"?"),
                ),
            },
        };

        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::new("confirm_delete_modal")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(egui::RichText::new(title).strong());
            ui.add_space(6.0);
            ui.label(body);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
                if ui.button("Delete").clicked() {
                    confirmed = true;
                }
            });
        });
        if modal.should_close() {
            cancelled = true;
        }

        if confirmed {
            match self.pending_delete.take() {
                Some(PendingDelete::Model { filename, path }) => {
                    self.delete_model_files(&filename, &path);
                }
                // removing seventy thousand files takes seconds: a thread of its own does it (the log
                // says how it went, and the build in use is given up, when it is done)
                Some(PendingDelete::Build { label, install_dir }) => {
                    self.start_delete(label, install_dir, false, ctx);
                }
                Some(PendingDelete::Unfinished { label, dir }) => {
                    self.start_delete(label, dir, true, ctx);
                }
                Some(PendingDelete::Listed { name }) => {
                    self.remove_listed_model(&name, ctx);
                }
                None => {}
            }
        } else if cancelled {
            self.pending_delete = None;
        }
    }
}
