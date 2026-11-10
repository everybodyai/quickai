//! The list of models of a computer QuickAI sets up (C199).
//! The computer is chosen at the top of the list (`show_computer_switch`), and the list under it is that
//! computer's: the model files it keeps, as `quickai-run models` said them when the window looked for its run, after a
//! run came up there, or at Refresh. The model picked there is the one its next run starts with, and the one that runs
//! has its green dot. This computer's models are under them, folded: one picked there is copied over when its run
//! starts, as before (the person is asked first).

use crossbeam_channel::TryRecvError;
use eframe::egui;

use crate::models::{KeptModel, ModelEntry};
use crate::remote::job::{self, Update};

use super::icons::{self, Icon};
use super::shell::{model_row, size_text};
use super::theme;
use super::App;

/// A model file a computer keeps, with the name the list gives it (this computer's name for one of the recommended
/// list, else what the file says).
#[derive(Clone)]
pub struct There {
    pub kept: KeptModel,
    pub name: String,
}

/// The model picked in a computer's list for its next run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickThere {
    /// A file that computer keeps, by its name.
    There(String),
    /// One of this computer's, by its file name: copied there when the run starts.
    Here(String),
}

impl There {
    /// Whether the run's model, as the run names it (its file, or the name the file gives itself), is this one.
    fn is(&self, named: &str) -> bool {
        self.kept.file == named || self.kept.display == named
    }
}

impl App {
    /// The models the computer `id` keeps, named as the list names them, as it last said them; `None` before it has.
    pub(super) fn models_there(&self, id: &str) -> Option<&[There]> {
        self.remote.kept.get(id).map(Vec::as_slice)
    }

    /// Keeps what the computer `id` said it keeps, named as this computer's models are (the recommended list's names).
    pub(super) fn keep_models_there(&mut self, id: &str, kept: Vec<KeptModel>) {
        let mut entries: Vec<ModelEntry> = kept.iter().map(entry_of).collect();
        if let Some(engine) = self.engine_doc() {
            let list = quickai_core::models::recommended(&self.registry, engine);
            crate::models::name_from_list(&mut entries, &crate::models::names_from_list(&list));
        }
        let there = kept
            .into_iter()
            .zip(entries)
            .map(|(kept, entry)| {
                // what the file does not name otherwise is named by the file, without its ending
                let name = if entry.display == kept.file {
                    kept.file.trim_end_matches(".gguf").to_string()
                } else {
                    entry.display
                };
                There { kept, name }
            })
            .collect();
        self.remote.kept.insert(id.to_string(), there);
    }

    /// The model of the run on the computer shown, as the run names it, while one runs there.
    fn run_model_there(&self) -> Option<&str> {
        self.setup_run()
            .filter(|r| r.running())
            .and_then(|r| r.there.as_ref())
            .and_then(|t| t.model.as_deref())
    }

    /// The name the list gives `named`, a model of the computer shown as a run names it: the list's name, else the
    /// file's name without `.gguf`.
    pub(super) fn model_there_name(&self, named: &str) -> String {
        self.setup_in_use()
            .and_then(|spec| self.models_there(&spec.id))
            .and_then(|there| there.iter().find(|t| t.is(named)))
            .map(|t| t.name.clone())
            .unwrap_or_else(|| named.trim_end_matches(".gguf").to_string())
    }

    /// Whether the computer `id` said what it keeps since the window started: it answered.
    pub(super) fn listed_there(&self, id: &str) -> bool {
        self.remote.kept.contains_key(id)
    }

    /// What the menu of computers says of the computer `id` QuickAI sets up: how many models it keeps and what runs
    /// there; `None` before it has said what it keeps.
    pub(super) fn there_summary(&self, id: &str) -> Option<String> {
        let there = self.models_there(id)?;
        let count = match there.len() {
            1 => "1 model".to_string(),
            n => format!("{n} models"),
        };
        let runs = self
            .setup_run_of(id)
            .filter(|r| r.running())
            .and_then(|r| r.there.as_ref())
            .and_then(|t| t.model.as_deref())
            .map(|named| {
                there.iter().find(|t| t.is(named)).map_or_else(
                    || named.trim_end_matches(".gguf").to_string(),
                    |t| t.name.clone(),
                )
            });
        Some(match runs {
            Some(model) => format!("{count} · runs {model}"),
            None => format!("{count} · nothing runs"),
        })
    }

    /// The model the next run on the computer shown starts with: the one picked in its list, else the one that runs
    /// there, else the first it keeps. `None`: it keeps none (or has not said), and this computer's selected model is
    /// sent, as before the list.
    pub(super) fn pick_there(&self) -> Option<PickThere> {
        let spec = self.setup_in_use()?;
        if let Some(pick) = self.remote.picked.get(&spec.id) {
            return Some(pick.clone());
        }
        let there = self.models_there(&spec.id).unwrap_or_default();
        let running = self
            .run_model_there()
            .and_then(|named| there.iter().find(|t| t.is(named)));
        running
            .or(there.first())
            .map(|t| PickThere::There(t.kept.file.clone()))
    }

    /// The name of the model the next run on the computer shown starts with, as its list names it.
    pub(super) fn picked_there_name(&self) -> Option<String> {
        match self.pick_there()? {
            PickThere::There(file) => Some(
                self.kept_pick()
                    .map_or_else(|| file.trim_end_matches(".gguf").to_string(), |t| t.name),
            ),
            PickThere::Here(file) => self
                .models
                .iter()
                .find(|m| m.filename == file)
                .map(|m| m.display.clone()),
        }
    }

    /// The file of the computer shown that its next run starts with, when it is one that computer keeps.
    pub(super) fn kept_pick(&self) -> Option<There> {
        let PickThere::There(file) = self.pick_there()? else {
            return None;
        };
        let spec = self.setup_in_use()?;
        self.models_there(&spec.id)?
            .iter()
            .find(|t| t.kept.file == file)
            .cloned()
    }

    /// Asks the computer shown which models it keeps again (the Refresh of its list).
    fn refresh_models_there(&mut self, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        if self.remote.listing.is_some() {
            return;
        }
        let wake = ctx.clone();
        let updates = job::models(self.job_for(&spec), move || wake.request_repaint());
        self.remote.listing = Some((spec.id, updates));
    }

    /// What a Refresh of a computer's list said since the last frame.
    pub(super) fn poll_models_there(&mut self) {
        let Some((id, updates)) = &self.remote.listing else {
            return;
        };
        let id = id.clone();
        let mut said = None;
        let mut failed = None;
        let mut done = false;
        loop {
            match updates.try_recv() {
                Ok(Update::Models(models)) => said = Some(models),
                Ok(Update::Failed(why)) => failed = Some(why),
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }
        if let Some(models) = said {
            self.keep_models_there(&id, models);
        }
        if let Some(why) = failed {
            self.push_log(format!("[remote] {why}"));
        }
        if done {
            self.remote.listing = None;
        }
    }

    /// The list of the computer shown, under Add a model: its heading (with the room its models take and Refresh), the
    /// models it keeps, and this computer's under them, folded.
    pub(super) fn show_models_there(&mut self, ui: &mut egui::Ui) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let p = theme::palette();
        let known = self.models_there(&spec.id).is_some();
        let there: Vec<There> = self
            .models_there(&spec.id)
            .map(<[There]>::to_vec)
            .unwrap_or_default();
        let looking = self
            .remote
            .listing
            .as_ref()
            .is_some_and(|(id, _)| *id == spec.id)
            || self.setup_run().is_some_and(|r| r.working());
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let family = theme::semibold(ui);
            ui.label(
                egui::RichText::new("MODELS")
                    .size(11.5)
                    .family(family)
                    .color(p.muted),
            );
            if !there.is_empty() {
                let total: u64 = there.iter().map(|t| t.kept.size).sum();
                ui.label(
                    egui::RichText::new(size_text(total))
                        .size(11.0)
                        .color(p.faint),
                )
                .on_hover_text(format!("The room the models listed take on {}.", spec.name));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if icons::icon_button(ui, Icon::Refresh, egui::vec2(24.0, 20.0), !looking, false)
                    .on_hover_text(format!("Ask {} which models it keeps.", spec.name))
                    .clicked()
                {
                    self.refresh_models_there(ui.ctx());
                }
            });
        });
        ui.add_space(2.0);
        let pick = self.pick_there();
        let run = self.run_model_there().map(str::to_string);
        let here_open = self.remote.here_open || (known && there.is_empty());
        let mut picked: Option<(PickThere, Option<usize>)> = None;
        let mut fold = false;
        egui::ScrollArea::vertical()
            .id_salt("models_there")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if there.is_empty() {
                    let words = if !known && looking {
                        format!("Asking {} which models it keeps…", spec.name)
                    } else if !known {
                        format!("What {} keeps shows here once it is reached.", spec.name)
                    } else {
                        format!(
                            "No models on {} yet. Pick one of this computer's: it is copied there when you start.",
                            spec.name
                        )
                    };
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 8,
                            top: 4,
                            bottom: 6,
                        })
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(words).color(p.faint));
                        });
                }
                for t in &there {
                    let running = run.as_deref().is_some_and(|named| t.is(named));
                    let mut detail = vec![(size_text(t.kept.size), None)];
                    if running {
                        detail.push(("running".to_string(), Some(p.accent_text)));
                    }
                    let chosen = pick.as_ref() == Some(&PickThere::There(t.kept.file.clone()));
                    let (row, more) = model_row(ui, &t.name, &detail, chosen, running);
                    let row = row.on_hover_text(format!(
                        "{}, on {}: {}",
                        t.kept.file, spec.name, t.kept.path
                    ));
                    if row.clicked() {
                        picked = Some((PickThere::There(t.kept.file.clone()), None));
                    }
                    let menu = |ui: &mut egui::Ui| {
                        if ui.button("Copy name").clicked() {
                            ui.ctx().copy_text(t.kept.file.clone());
                            ui.close();
                        }
                    };
                    row.context_menu(menu);
                    if let Some(more) = more {
                        egui::Popup::menu(&more)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                            .show(menu);
                    }
                }
                // this computer's models that are not there: one picked is copied there when the run starts
                let here: Vec<(usize, &ModelEntry)> = self
                    .models
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.path.is_file())
                    .filter(|(_, m)| !there.iter().any(|t| t.kept.file == m.filename))
                    .collect();
                if here.is_empty() {
                    return;
                }
                ui.add_space(10.0);
                let heading = ui
                    .horizontal(|ui| {
                        ui.add_space(8.0);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        // the chevron points down when it is open, right when it is folded
                        let chevron = if here_open {
                            rect
                        } else {
                            rect.shrink(1.0)
                        };
                        if here_open {
                            icons::paint(ui.painter(), chevron, Icon::Chevron, p.faint);
                        } else {
                            paint_right(ui.painter(), chevron, p.faint);
                        }
                        let family = theme::semibold(ui);
                        ui.label(
                            egui::RichText::new("ON THIS COMPUTER")
                                .size(11.5)
                                .family(family)
                                .color(p.muted),
                        );
                        ui.label(
                            egui::RichText::new(here.len().to_string())
                                .size(11.0)
                                .color(p.faint),
                        );
                        ui.allocate_space(egui::vec2(ui.available_width(), 1.0));
                    })
                    .response;
                let heading = ui
                    .interact(heading.rect, heading.id.with("fold"), egui::Sense::click())
                    .on_hover_text(format!(
                        "This computer's models. One you pick is copied to {} when you start it there.",
                        spec.name
                    ));
                if heading.clicked() {
                    fold = true;
                }
                ui.add_space(4.0);
                if !here_open {
                    return;
                }
                for (i, m) in here {
                    let detail = vec![
                        (size_text(m.size_bytes), None),
                        ("copied there when run".to_string(), None),
                    ];
                    let chosen = pick.as_ref() == Some(&PickThere::Here(m.filename.clone()));
                    let (row, more) = model_row(ui, &m.display, &detail, chosen, false);
                    let row = row.on_hover_text(format!(
                        "{} is on this computer. Started on {}, it is copied there first.",
                        m.filename, spec.name
                    ));
                    if row.clicked() {
                        picked = Some((PickThere::Here(m.filename.clone()), Some(i)));
                    }
                    let menu = |ui: &mut egui::Ui| {
                        if ui.button("Copy name").clicked() {
                            ui.ctx().copy_text(m.filename.clone());
                            ui.close();
                        }
                    };
                    row.context_menu(menu);
                    if let Some(more) = more {
                        egui::Popup::menu(&more)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                            .show(menu);
                    }
                }
            });
        if fold {
            self.remote.here_open = !here_open;
        }
        if let Some((pick, index)) = picked {
            // one of this computer's is the window's selected model, which is what is sent and copied there
            if let Some(i) = index.filter(|i| *i != self.selected) {
                self.selected = i;
                self.apply_model_defaults();
                self.leave_configuration_for_model();
            }
            self.remote.picked.insert(spec.id, pick);
        }
    }
}

/// A model a computer keeps as a model entry of the list, for the names the window gives them.
fn entry_of(kept: &KeptModel) -> ModelEntry {
    ModelEntry {
        filename: kept.file.clone(),
        path: kept.path.clone().into(),
        display: kept.display.clone(),
        note: String::new(),
        bundled: false,
        size_bytes: kept.size,
        companions: Vec::new(),
        mode: None,
    }
}

/// A chevron that points right (a folded group).
fn paint_right(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let h = rect.height() * 0.22;
    painter.add(egui::Shape::line(
        vec![
            egui::pos2(c.x - h * 0.6, c.y - h * 1.2),
            egui::pos2(c.x + h * 0.6, c.y),
            egui::pos2(c.x - h * 0.6, c.y + h * 1.2),
        ],
        egui::Stroke::new(1.5, color),
    ));
}

/// The folder of a path of another computer, whichever separator it uses.
pub(super) fn folder_of(path: &str) -> Option<String> {
    let cut = path.rfind(['/', '\\'])?;
    Some(path[..cut].to_string()).filter(|f| !f.is_empty())
}
