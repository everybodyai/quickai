//! What is known of a model, in a window of its own (WP4.28): its name and file, how big it is on the disk, the
//! quantization its name says, what it can do, what goes with it and where it came from; and, at the head of the list
//! of models, how much room they all take. Opened from Details... beside the model's name, or from its menu in the
//! list.

use eframe::egui;

use super::theme;
use super::App;
use crate::models::ModelEntry;

/// The quantization a model's name says (`Q4_K_M`, `IQ3_XXS`, `F16`, `BF16`), when it says one. The name is split at its
/// separators, so `Q4_K_M` comes as `Q4`, `K`, `M`, and is put back together.
pub fn quantization(name: &str) -> Option<String> {
    let stem = name.trim_end_matches(".gguf");
    let words: Vec<String> = stem
        .split(['-', '.', '_', ':'])
        .map(str::to_ascii_uppercase)
        .collect();
    let starts_one = |w: &str| {
        let digits = w.strip_prefix("IQ").or_else(|| w.strip_prefix('Q'));
        digits.is_some_and(|d| d.chars().next().is_some_and(|c| c.is_ascii_digit()))
    };
    if let Some(at) = words.iter().position(|w| starts_one(w)) {
        let mut quant = words[at].clone();
        for part in &words[at + 1..] {
            if ["K", "M", "S", "L", "XS", "XXS", "NL", "0", "1"].contains(&part.as_str()) {
                quant.push('_');
                quant.push_str(part);
            } else {
                break;
            }
        }
        return Some(quant);
    }
    words
        .into_iter()
        .rev()
        .find(|w| ["F16", "BF16", "F32", "FP16"].contains(&w.as_str()))
}

/// The room all the models take, by what the list says of each.
pub fn total_bytes(models: &[ModelEntry]) -> u64 {
    models.iter().map(|m| m.size_bytes).sum()
}

impl App {
    /// The window of the details of the model `self.details_of` names, while it is open.
    pub(super) fn show_model_details(&mut self, ctx: &egui::Context) {
        let Some(name) = self.details_of.clone() else {
            return;
        };
        let Some(model) = self.models.iter().find(|m| m.filename == name).cloned() else {
            self.details_of = None;
            return;
        };
        let mut open = true;
        let lists = self.lists_models();
        let where_it_runs = self.remote_in_use().map(|r| r.name.clone());
        egui::Window::new(format!("Details: {}", model.display))
            .id(egui::Id::new("model_details"))
            .open(&mut open)
            .collapsible(false)
            .resizable([true, false])
            .default_width(460.0)
            .show(ctx, |ui| {
                egui::Grid::new("model_details_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        let row = |ui: &mut egui::Ui, label: &str, value: String| {
                            ui.label(theme::muted(label));
                            ui.add(egui::Label::new(value).selectable(true).wrap());
                            ui.end_row();
                        };
                        row(ui, "Name", model.display.clone());
                        if lists {
                            row(ui, "Engine", self.engine_name());
                            row(
                                ui,
                                "Kept on",
                                where_it_runs
                                    .clone()
                                    .unwrap_or_else(|| "this computer".into()),
                            );
                        } else {
                            row(ui, "File", model.filename.clone());
                            row(
                                ui,
                                "Folder",
                                model
                                    .path
                                    .parent()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_default(),
                            );
                        }
                        row(ui, "Size", super::shell::size_text(model.size_bytes));
                        if let Some(q) = quantization(&model.filename) {
                            row(ui, "Quantization", q);
                        }
                        let can: Vec<&str> = model
                            .note
                            .split(", ")
                            .filter(|w| !w.is_empty() && *w != "completion")
                            .collect();
                        if lists && !can.is_empty() {
                            row(ui, "Can", can.join(", "));
                        } else if !lists && !model.note.is_empty() {
                            row(ui, "About", model.note.clone());
                        }
                        if !model.companions.is_empty() {
                            let with: Vec<String> = model
                                .companions
                                .iter()
                                .map(|c| {
                                    let there = if c.exists() { "" } else { " (gone)" };
                                    format!("{}: {}{there}", c.label, c.name)
                                })
                                .collect();
                            row(ui, "Goes with", with.join("; "));
                        }
                        if model.bundled {
                            row(ui, "Comes with", "QuickAI (it is not deleted)".into());
                        }
                        if let Ok(meta) = std::fs::metadata(&model.path) {
                            if let Ok(when) = meta.modified() {
                                let ago = std::time::SystemTime::now()
                                    .duration_since(when)
                                    .map(|d| format!("{} ago", crate::hardware::format_duration(d)))
                                    .unwrap_or_default();
                                row(ui, "Added", ago);
                            }
                        }
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if !lists {
                        if let Some(folder) = model.path.parent() {
                            if ui.button("Show in folder").clicked() {
                                super::workbench::open_folder(folder);
                            }
                        }
                    }
                    if ui.button("Copy name").clicked() {
                        ui.ctx().copy_text(model.filename.clone());
                    }
                });
            });
        if !open {
            self.details_of = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quantization_is_read_from_the_name_as_files_and_tags_write_it() {
        assert_eq!(
            quantization("Qwen3-4B-Q4_K_M.gguf").as_deref(),
            Some("Q4_K_M")
        );
        assert_eq!(
            quantization("gemma-3-12b-it-IQ3_XXS.gguf").as_deref(),
            Some("IQ3_XXS")
        );
        assert_eq!(quantization("Bonsai-8B-Q1_0.gguf").as_deref(), Some("Q1_0"));
        assert_eq!(
            quantization("phi-4-mini-Q8_0.gguf").as_deref(),
            Some("Q8_0")
        );
        assert_eq!(
            quantization("mmproj-model-f16.gguf").as_deref(),
            Some("F16")
        );
        assert_eq!(quantization("qwen3:4b-q4_K_M").as_deref(), Some("Q4_K_M"));
        assert_eq!(quantization("qwen3:4b"), None);
        assert_eq!(quantization("model.gguf"), None);
    }
}
