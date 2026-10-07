//! The "are you sure?" modal every delete button goes through.

use eframe::egui;

use super::{App, PendingDelete};
use crate::launch;
use crate::prims::install;

impl App {
    /// Shows a blocking "are you sure?" modal for `self.pending_delete`, if
    /// any — the confirmation step every delete button (model or installed
    /// llama-server build) goes through before anything actually touches
    /// disk. A no-op when nothing is pending.
    pub(super) fn show_confirm_delete_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_delete else {
            return;
        };
        let (title, body) = match pending {
            PendingDelete::Model { filename, .. } => (
                "Delete model?".to_string(),
                format!("Delete \"{filename}\" from disk? This can't be undone."),
            ),
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
                    match std::fs::remove_file(&path) {
                        Ok(()) => self.push_log(format!("Deleted model: {filename}")),
                        Err(e) => self.push_log(format!("Could not delete {filename}: {e}")),
                    }
                    self.rescan_models();
                }
                Some(PendingDelete::Build { label, install_dir }) => {
                    let was_active = self
                        .bin_override()
                        .is_some_and(|p| p.starts_with(&install_dir));
                    if let Some(texts) = self.runtime_texts() {
                        match install::delete_installed(&install_dir) {
                            Ok(()) => self.push_log(texts.deleted_log(&label)),
                            Err(e) => self.push_log(texts.delete_failed_log(&label, &e)),
                        }
                    }
                    if was_active {
                        self.runtime.input.clear();
                        self.binary = launch::default_binary(&self.registry, &self.engine);
                        self.set_bin_override(None);
                        self.missing_binary_warned = false;
                        self.save_settings();
                    }
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
