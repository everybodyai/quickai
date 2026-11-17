//! What the window does with a computer QuickAI sets up besides running a configuration there (C228): what that
//! computer is (its memory and its graphics cards, as `quickai-headless computer` says them), which the panel fits the
//! model to and draws at its foot; the models it downloads itself (`quickai-headless get`, from Recommended and Add a
//! model), which go on when the window is closed; the models deleted there; and this computer's models copied there
//! without a run. The steps are `remote::job`'s, on threads of their own; this is what the window keeps of them.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use crossbeam_channel::{Receiver, TryRecvError};
use eframe::egui;
use quickai_core::gguf::ModelFacts;
use quickai_core::lookup::present::{safe_file_name, Download, Fetch};

use crate::devices::Device;
use crate::facts::ComputerFacts;
use crate::hardware::{self, SpeedTier};
use crate::remote::job::{self, Update};
use crate::remote::{LastThere, RemoteSpec, Way};
use crate::run_file::RunFile;

use super::graphics::{self, Takes};
use super::icons::{self, Icon};
use super::remote_models::PickThere;
use super::theme::{self, Role};
use super::App;

/// What a computer QuickAI sets up said it is, and when.
pub struct Known {
    pub facts: ComputerFacts,
    pub at: SystemTime,
}

/// A download on a computer QuickAI sets up, as the window follows it.
#[derive(Default)]
pub struct GetThere {
    /// What is downloaded, as the download names itself (`Qwen3.5-4B-Q4_K_M.gguf and 1 more file`).
    pub label: String,
    /// What the download there last said of itself: its files, and how far each has gone.
    pub there: Option<Box<RunFile>>,
    /// It has ended there (done, cancelled or failed); what the job says after it is still taken.
    pub ended: bool,
    updates: Option<Receiver<Update>>,
    cancel: Arc<AtomicBool>,
}

impl GetThere {
    /// It goes on there.
    pub fn going(&self) -> bool {
        !self.ended
    }

    /// The bytes done, and of how many when every file says.
    pub fn progress(&self) -> (u64, Option<u64>) {
        let steps = self
            .there
            .as_ref()
            .map(|t| t.steps.as_slice())
            .unwrap_or_default();
        let done = steps
            .iter()
            .map(|s| {
                if s.state == "done" {
                    s.total.unwrap_or(s.done)
                } else {
                    s.done
                }
            })
            .sum();
        let total = steps.iter().map(|s| s.total).sum::<Option<u64>>();
        (done, total)
    }

    /// How far it has gone, in words and as a share (0 to 1) when the sizes are known.
    pub fn line(&self, computer: &str) -> (String, Option<f32>) {
        let (done, total) = self.progress();
        match total.filter(|t| *t > 0) {
            Some(total) => (
                format!(
                    "Downloading on {computer}: {} of {}",
                    hardware::format_bytes(done),
                    hardware::format_bytes(total)
                ),
                Some((done as f32 / total as f32).clamp(0.0, 1.0)),
            ),
            None if done > 0 => (
                format!(
                    "Downloading on {computer}: {}",
                    hardware::format_bytes(done)
                ),
                None,
            ),
            None => (format!("Starting the download on {computer}…"), None),
        }
    }
}

/// What the window keeps of the computers QuickAI sets up besides their runs (C228).
#[derive(Default)]
pub struct ThereState {
    /// What each said it is, by the computer's id.
    pub known: BTreeMap<String, Known>,
    /// The asking of what one is, going on, by the computer's id.
    asking: BTreeMap<String, Receiver<Update>>,
    /// The download on each, by the computer's id.
    pub gets: BTreeMap<String, GetThere>,
    /// Deletions and copies there going on: the computer's id, what is done, and what it says.
    chores: Vec<(String, String, Receiver<Update>)>,
    /// The model the person asked to delete there, waiting for the answer: the computer's id, its file, its name.
    delete_asked: Option<(String, String, String)>,
    /// A model being downloaded here to be copied to a computer once it is in: the computer's id, and the files of the
    /// download as they are kept here.
    copy_after: Option<(String, Vec<String>)>,
    /// The computers asked what they are by themselves once (`ask_facts_there_once`).
    asked_once: std::collections::BTreeSet<String>,
    /// The shell of the Terminal tab on each, while the window is open (`remote_terminal`), by the computer's id.
    pub terminals: BTreeMap<String, super::remote_terminal::Terminal>,
}

impl App {
    /// What the computer QuickAI sets up that is shown said it is.
    pub(super) fn facts_there(&self) -> Option<&ComputerFacts> {
        let spec = self.setup_in_use()?;
        self.remote.there.known.get(&spec.id).map(|k| &k.facts)
    }

    /// Whether that computer is being asked what it is.
    pub(super) fn asking_facts_there(&self) -> bool {
        self.setup_in_use()
            .is_some_and(|s| self.remote.there.asking.contains_key(&s.id))
    }

    /// Asks the computer QuickAI sets up that is shown what it is (↻ at the foot of the panel). QuickAI of this release
    /// is copied there first when it is not there.
    pub(super) fn ask_facts_there(&mut self, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        if self.remote.there.asking.contains_key(&spec.id) {
            return;
        }
        let wake = ctx.clone();
        let updates = job::facts(self.job_for(&spec), move || wake.request_repaint());
        self.remote.there.asking.insert(spec.id, updates);
    }

    /// Asks the computer shown what it is, once while the window is open (Recommended opened for it before it was
    /// reached): a failure is not asked again by itself, ↻ asks again.
    pub(super) fn ask_facts_there_once(&mut self, ctx: &egui::Context) {
        let Some(id) = self.setup_in_use().map(|s| s.id.clone()) else {
            return;
        };
        if self.remote.there.asked_once.insert(id) {
            self.ask_facts_there(ctx);
        }
    }

    /// Keeps what the computer `id` said it is; the panel fits the model to it at once when it is the one shown.
    pub(super) fn keep_facts(&mut self, id: &str, facts: ComputerFacts) {
        self.remote.there.known.insert(
            id.to_string(),
            Known {
                facts,
                at: SystemTime::now(),
            },
        );
        if self.setup_in_use().is_some_and(|s| s.id == id) {
            self.settle_devices();
            self.auto_fit();
        }
    }

    /// The graphics of the computer shown that a model can go on, as it said them.
    pub(super) fn cards_there(&self) -> Vec<Device> {
        self.facts_there()
            .map(|f| {
                f.devices
                    .iter()
                    .map(|d| d.to_device())
                    .filter(Device::is_graphics)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The memory of the computer shown, once it said it.
    pub(super) fn ram_there(&self) -> Option<u64> {
        self.facts_there()
            .map(|f| f.memory_bytes)
            .filter(|bytes| *bytes > 0)
    }

    /// The model the next run on the computer shown starts with, as the panel counts it (its bytes with what goes with
    /// it, and what its file says), once that computer said what it is: one it keeps, one of this computer's that is
    /// copied there at the start, or this computer's selected model when nothing is picked there.
    pub(super) fn subject_there(&self) -> Option<(u64, Option<&ModelFacts>)> {
        self.facts_there()?;
        let local = |file: Option<&str>| {
            let model = match file {
                Some(file) => self.models.iter().find(|m| m.filename == file)?,
                None => self.models.get(self.selected)?,
            };
            let selected = self
                .models
                .get(self.selected)
                .is_some_and(|m| m.filename == model.filename);
            let projector = if selected { self.projector_bytes() } else { 0 };
            Some((
                model.size_bytes + projector,
                selected.then(|| self.model_facts()).flatten(),
            ))
        };
        match self.pick_there() {
            Some(PickThere::There(file)) => {
                let spec = self.setup_in_use()?;
                let kept = &self
                    .models_there(&spec.id)?
                    .iter()
                    .find(|t| t.kept.file == file)?
                    .kept;
                let with: u64 = kept.companions.iter().map(|c| c.size).sum();
                Some((kept.size + with, kept.facts.as_ref()))
            }
            Some(PickThere::Here(file)) => local(Some(&file)),
            None => local(None),
        }
    }

    /// What the computer shown has, in words, for the head of Recommended: its memory and its cards.
    pub(super) fn there_words(&self) -> String {
        let Some(facts) = self.facts_there() else {
            return if self.asking_facts_there() {
                "asking what it has…".to_string()
            } else {
                "not reached yet".to_string()
            };
        };
        let mut words = vec![format!(
            "{} of memory",
            hardware::format_bytes(facts.memory_bytes)
        )];
        let cards = self.cards_there();
        if cards.is_empty() {
            // before llama.cpp is there, the makers of its cards; after, none it can use is none
            if !facts.devices_known {
                words.extend(facts.vendors.iter().map(|v| format!("{v} graphics")));
            }
        } else {
            words.extend(
                cards
                    .iter()
                    .map(|c| format!("{}, {}", c.name, hardware::format_bytes(c.total_bytes))),
            );
        }
        words.join(" · ")
    }

    /// The computers QuickAI sets up, by id and name: what Recommended can be for besides this computer.
    pub(super) fn computers_to_recommend_for(&self) -> Vec<(String, String)> {
        if !self.remote_setup_on() {
            return Vec::new();
        }
        self.remote
            .remotes
            .iter()
            .filter(|r| r.way == Way::SetUp)
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect()
    }

    /// The model files the computer shown keeps, with what goes with them, as Recommended's "installed" reads them:
    /// by their names, with their sizes.
    pub(super) fn have_there(&self) -> Option<BTreeMap<String, u64>> {
        let spec = self.setup_in_use()?;
        let there = self.models_there(&spec.id)?;
        let mut have = BTreeMap::new();
        for t in there {
            have.insert(t.kept.file.clone(), t.kept.size);
            for companion in &t.kept.companions {
                have.insert(companion.file.clone(), companion.size);
            }
        }
        Some(have)
    }

    // ---- the bar ------------------------------------------------------------------------------------------

    /// What the bar names while a computer QuickAI sets up is shown: the model its run runs, else the one its next run
    /// starts with.
    pub(super) fn setup_bar_title(&self) -> String {
        let running = self
            .setup_run()
            .filter(|r| r.running())
            .and_then(|r| r.there.as_ref())
            .and_then(|t| t.model.clone());
        if let Some(named) = running {
            return self.model_there_name(&named);
        }
        self.picked_there_name()
            .or_else(|| self.models.get(self.selected).map(|m| m.display.clone()))
            .unwrap_or_else(|| self.selected_configuration_name())
    }

    /// What the bar says the run there is doing, the colour of its dot, and what its hover says.
    pub(super) fn setup_bar_state(&self) -> (String, egui::Color32, String) {
        let p = theme::palette();
        let Some(spec) = self.setup_in_use() else {
            return (String::new(), p.faint, String::new());
        };
        let name = &spec.name;
        match self.setup_run() {
            Some(r) if r.running() => (
                format!("Running on {name}"),
                p.ok,
                format!("It runs on {name} until you stop it, also when QuickAI is closed."),
            ),
            Some(r) if r.working() && r.doing.starts_with("Stopping") => {
                (format!("Stopping on {name}…"), p.warn, r.doing.clone())
            }
            Some(r) if r.working() && r.doing.starts_with("Looking") => {
                (format!("on {name}"), p.faint, r.doing.clone())
            }
            Some(r) if r.working() => (
                format!("Getting ready on {name}…"),
                p.warn,
                r.doing.clone(),
            ),
            Some(r) if r.failed.is_some() => (
                format!("Stopped on {name}"),
                p.bad,
                r.failed.clone().unwrap_or_default(),
            ),
            _ => (
                format!("on {name}"),
                p.faint,
                format!(
                    "{name} ({}). Start runs \"{}\" there: QuickAI copies itself there when it needs to, installs \
                     what is missing, and brings its ports here. It runs there until you stop it.",
                    spec.host,
                    self.selected_configuration_name()
                ),
            ),
        }
    }

    /// Start or Stop of the run on the computer shown, drawn from the right: what was pressed (start, stop).
    pub(super) fn setup_bar_actions(&mut self, ui: &mut egui::Ui, height: f32) -> (bool, bool) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return (false, false);
        };
        let run = self.setup_run();
        let looking = run.is_some_and(|r| r.working() && r.doing.starts_with("Looking"));
        if run.is_some_and(|r| r.running() || r.working()) && !looking {
            let stop = icons::labelled_button(ui, Role::Danger, true, Icon::Stop, "Stop", height)
                .on_hover_text(format!("Stop the run on {} (Ctrl+Enter).", spec.name))
                .clicked();
            return (false, stop);
        }
        let with = self
            .picked_there_name()
            .map(|model| format!(" with {model}"))
            .unwrap_or_default();
        let hint = format!(
            "Run \"{}\"{with} on {}: QuickAI copies itself there when it needs to, installs what is missing, and \
             brings its ports here (Ctrl+Enter).",
            self.selected_configuration_name(),
            spec.name
        );
        let start =
            icons::labelled_button(ui, Role::Primary, !looking, Icon::Play, "Start", height)
                .on_hover_text(&hint)
                .on_disabled_hover_text(format!("Looking for a run on {} first.", spec.name));
        super::help::mark_spot(ui, super::help::Spot::Play, start.rect);
        (start.clicked(), false)
    }

    // ---- the foot of the panel ------------------------------------------------------------------------------

    /// The foot of the panel for a computer QuickAI sets up: its name, how fast the model should be there, ↻, and a
    /// meter for each of its cards the model goes on and for its memory, as it said them.
    pub(super) fn show_computer_there(&mut self, ui: &mut egui::Ui, meters: f32) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let p = theme::palette();
        let speed = self.speed().filter(|_| self.facts_there().is_some());
        let asking = self.asking_facts_there();
        let asked_at = self.remote.there.known.get(&spec.id).map(|k| ago(k.at));
        let mut again = false;
        let about = format!(
            "What the model and its context would take of each graphics card of {} and of its memory, as its file \
             says, and how fast it should answer there.",
            spec.name
        );
        super::inspector::section_head(ui, Icon::Computer, &spec.name, Some(&about), |ui| {
            let hover = match &asked_at {
                Some(at) => format!(
                    "Asked of {} {at}: its graphics cards and its memory. ↻ asks again.",
                    spec.name
                ),
                None => format!("Ask {} what it has.", spec.name),
            };
            again = icons::icon_button(ui, Icon::Refresh, egui::vec2(22.0, 22.0), !asking, false)
                .on_hover_text(hover)
                .clicked();
            if asking {
                ui.spinner();
            } else if let Some(tier) = speed {
                let color = match tier {
                    SpeedTier::Fast => p.accent_text,
                    SpeedTier::Medium | SpeedTier::Slow => p.warn,
                };
                ui.label(egui::RichText::new(tier.label()).color(color));
            }
        });
        if again {
            self.ask_facts_there(ui.ctx());
        }
        let Some(facts) = self.facts_there().cloned() else {
            ui.label(
                theme::muted(format!(
                    "Not reached yet. What its graphics cards and memory hold shows here once QuickAI reaches {}: \
                     when you choose it, and with ↻.",
                    spec.name
                ))
                .small(),
            );
            return;
        };
        let takes = self.takes();
        egui::ScrollArea::vertical()
            .id_salt("computer_there_meters")
            .max_height(meters)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if !facts.devices_known && !facts.vendors.is_empty() {
                    // only the makers are known before llama.cpp is installed there
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{} card", facts.vendors.join(", ")))
                                .small()
                                .color(p.muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new("size known after the first start")
                                    .small()
                                    .color(p.faint),
                            );
                        });
                    });
                }
                match &takes {
                    Some((cards, memory)) => {
                        for (card, part) in cards {
                            graphics::meter_row(ui, &card.name, part, card.total_bytes);
                        }
                        graphics::meter_row(ui, "Memory", memory, facts.memory_bytes);
                    }
                    None => graphics::meter_row(
                        ui,
                        "Memory",
                        &Takes {
                            model: 0,
                            context: 0,
                            work: 0,
                        },
                        facts.memory_bytes,
                    ),
                }
            });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (word, color) in [
                ("model", p.accent_text),
                ("context", graphics::context_color()),
                ("other", graphics::work_color()),
            ] {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, color);
                ui.label(egui::RichText::new(word).size(11.0).color(p.faint));
                ui.add_space(4.0);
            }
        });
    }

    // ---- downloads there ------------------------------------------------------------------------------------

    /// What a Get pressed with a computer QuickAI sets up shown does: that computer downloads the files itself. An engine
    /// that pulls by name pulls there when the run starts.
    pub(super) fn fetch_there(&mut self, fetch: &Fetch, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        match fetch {
            Fetch::Files(download) => self.get_there(&spec, download.clone(), ctx),
            Fetch::Reference(reference) => {
                self.catalog.note = Some(format!(
                    "{} pulls `{reference}` on {} when the run there starts with it.",
                    self.engine_name(),
                    spec.name
                ));
            }
        }
    }

    /// Starts the download of `download` on `spec`, which goes on when the window is closed.
    fn get_there(&mut self, spec: &RemoteSpec, download: Download, ctx: &egui::Context) {
        if self
            .remote
            .there
            .gets
            .get(&spec.id)
            .is_some_and(GetThere::going)
        {
            return;
        }
        let label = download
            .primary()
            .map(|w| safe_file_name(&w.store_as))
            .unwrap_or_default();
        let cancel = Arc::new(AtomicBool::new(false));
        let wake = ctx.clone();
        let updates = job::get(
            self.job_for(spec),
            download,
            move || wake.request_repaint(),
            cancel.clone(),
        );
        self.push_log(format!("[remote] Downloading {label} on {}...", spec.name));
        self.remote.there.gets.insert(
            spec.id.clone(),
            GetThere {
                label,
                updates: Some(updates),
                cancel,
                ..GetThere::default()
            },
        );
    }

    /// Follows a download that goes on on the computer `id` (found when the window looked for its run).
    fn follow_get_there(&mut self, id: &str, ctx: &egui::Context) {
        if self.remote.there.gets.contains_key(id) {
            return;
        }
        let Some(spec) = self.remote.remotes.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let wake = ctx.clone();
        let updates = job::follow_get(
            self.job_for(&spec),
            move || wake.request_repaint(),
            cancel.clone(),
        );
        self.remote.there.gets.insert(
            spec.id,
            GetThere {
                updates: Some(updates),
                cancel,
                ..GetThere::default()
            },
        );
    }

    /// Cancels the download on the computer shown: it stops there.
    pub(super) fn cancel_get_there(&mut self, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let job = self.job_for(&spec);
        let Some(get) = self.remote.there.gets.get_mut(&spec.id) else {
            return;
        };
        get.cancel.store(true, Ordering::SeqCst);
        let wake = ctx.clone();
        get.updates = Some(job::cancel_get(job, move || wake.request_repaint()));
    }

    /// The download on the computer shown, while it goes on: what it is, and how far it has gone in words and as a
    /// share.
    pub(super) fn getting_there(&self) -> Option<(String, String, Option<f32>)> {
        let spec = self.setup_in_use()?;
        let get = self.remote.there.gets.get(&spec.id).filter(|g| g.going())?;
        let (line, share) = get.line(&spec.name);
        Some((get.label.clone(), line, share))
    }

    /// A Get's "Download here, then copy" (for a computer that cannot reach the internet): the files are downloaded
    /// here, then copied to the computer shown.
    pub(super) fn fetch_here_then_copy(&mut self, fetch: &Fetch, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let Fetch::Files(download) = fetch else {
            return;
        };
        if self.download.is_some() {
            return;
        }
        let files: Vec<String> = download
            .files
            .iter()
            .filter(|p| p.place.is_none())
            .map(|p| safe_file_name(&p.store_as))
            .collect();
        self.push_log(format!(
            "[remote] Downloading {} here, to copy it to {} once it is in.",
            files.first().cloned().unwrap_or_default(),
            spec.name
        ));
        self.remote.there.copy_after = Some((spec.id.clone(), files));
        self.fetch_files(download.clone(), ctx);
    }

    // ---- the models there: delete, copy -----------------------------------------------------------------------

    /// Asks whether to delete the model `file` (named `name`) on the computer shown.
    pub(super) fn ask_to_delete_there(&mut self, file: &str, name: &str) {
        if let Some(spec) = self.setup_in_use() {
            self.remote.there.delete_asked =
                Some((spec.id.clone(), file.to_string(), name.to_string()));
        }
    }

    /// The question before a model is deleted on a computer QuickAI sets up.
    pub(super) fn show_delete_there_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, file, name)) = self.remote.there.delete_asked.clone() else {
            return;
        };
        let Some(spec) = self.remote.remotes.iter().find(|r| r.id == id).cloned() else {
            self.remote.there.delete_asked = None;
            return;
        };
        let mut answer: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("delete_there_modal")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(
                egui::RichText::new(format!("Delete model on {}?", spec.name))
                    .size(16.0)
                    .family(theme::semibold(ui)),
            );
            ui.add_space(6.0);
            ui.label(format!(
                "Delete \"{name}\" from {}'s disk? This cannot be undone.",
                spec.name
            ));
            ui.label(theme::muted(format!(
                "{file}, and the files that go with it that no other model there uses."
            )));
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::button_sized(ui, Role::Danger, true, "Delete", egui::vec2(0.0, 30.0))
                    .clicked()
                {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        let Some(delete) = answer else {
            return;
        };
        self.remote.there.delete_asked = None;
        if !delete {
            return;
        }
        // a model deleted there is not the next run's any more
        if self.remote.picked.get(&id) == Some(&PickThere::There(file.clone())) {
            self.remote.picked.remove(&id);
        }
        let wake = ctx.clone();
        let updates = job::delete(self.job_for(&spec), file.clone(), move || {
            wake.request_repaint()
        });
        self.push_log(format!("[remote] Deleting {file} on {}...", spec.name));
        self.remote
            .there
            .chores
            .push((id, format!("Deleting {name}"), updates));
    }

    /// Copies this computer's model at `index` (and what goes with it) to the computer shown, without a run: there it
    /// is kept with the models copied for runs.
    pub(super) fn copy_model_there(&mut self, index: usize, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let Some(model) = self.models.get(index) else {
            return;
        };
        let files = files_of(&model.path);
        self.push_log(format!(
            "[remote] Copying {} to {}...",
            model.filename, spec.name
        ));
        self.send_files_there(&spec, files, ctx);
    }

    /// Whether something is being copied to or deleted on the computer shown, in words.
    pub(super) fn chore_there(&self) -> Option<String> {
        let spec = self.setup_in_use()?;
        self.remote
            .there
            .chores
            .iter()
            .find(|(id, _, _)| *id == spec.id)
            .map(|(_, what, _)| what.clone())
    }

    /// Starts copying `files` (a path here, and the name it is kept under) to `spec`, one job each.
    fn send_files_there(
        &mut self,
        spec: &RemoteSpec,
        files: Vec<(PathBuf, String)>,
        ctx: &egui::Context,
    ) {
        for (path, name) in files {
            let wake = ctx.clone();
            let updates = job::send_model(self.job_for(spec), path, name.clone(), move || {
                wake.request_repaint()
            });
            self.remote
                .there
                .chores
                .push((spec.id.clone(), format!("Copying {name}"), updates));
        }
    }

    // ---- what was last used there ----------------------------------------------------------------------------------

    /// Notes what is used on the computer shown now (the configuration, and the model picked there), so that the
    /// window comes back to it (C228).
    pub(super) fn note_last_there(&mut self) {
        let Some(id) = self.setup_in_use().map(|s| s.id.clone()) else {
            return;
        };
        let model = match self.pick_there() {
            Some(PickThere::There(file) | PickThere::Here(file)) => Some(file),
            None => None,
        };
        let last = LastThere {
            configuration: self.configs.active.clone(),
            model,
        };
        let Some(spec) = self.remote.remotes.iter_mut().find(|r| r.id == id) else {
            return;
        };
        if spec.last.as_ref() != Some(&last) {
            spec.last = Some(last);
            self.store_remotes();
        }
    }

    /// What the window shows when it comes back to the computer `id` (it opened, or the computer was chosen again): the
    /// configuration of the run that goes on there (`ran`, its name as a slug), else, when nothing runs there, the one
    /// last used there. Never over a configuration the person chose since, and never while the window cannot change it.
    pub(super) fn come_back_to(&mut self, id: &str, ran: Option<String>, nothing: bool) {
        if self.remote.person_chose
            || !self.setup_in_use().is_some_and(|s| s.id == id)
            || self.engine_locked()
        {
            return;
        }
        let entries = self.configuration_entries();
        let wanted = match ran {
            Some(slug) => entries
                .iter()
                .find(|e| crate::remote::slug(&e.configuration.name) == slug)
                .map(|e| e.id.clone()),
            None if nothing => self
                .setup_in_use()
                .and_then(|s| s.last.as_ref())
                .map(|l| l.configuration.clone())
                .filter(|c| entries.iter().any(|e| &e.id == c)),
            None => None,
        };
        if let Some(configuration) = wanted.filter(|c| *c != self.configs.active) {
            self.choose_configuration(&configuration);
        }
    }

    /// The model last used on the computer shown, as its next run would start with it: one it keeps, else one of this
    /// computer's (copied there at the start).
    pub(super) fn last_model_there(&self) -> Option<PickThere> {
        let spec = self.setup_in_use()?;
        let file = spec.last.as_ref()?.model.clone()?;
        if self
            .models_there(&spec.id)
            .is_some_and(|there| there.iter().any(|t| t.kept.file == file))
        {
            return Some(PickThere::There(file));
        }
        self.models
            .iter()
            .any(|m| m.filename == file)
            .then_some(PickThere::Here(file))
    }

    // ---- each frame ----------------------------------------------------------------------------------------------

    /// Each frame: what the asking, the downloads, the deletions and the copies said is kept and logged, a model
    /// downloaded here to be copied is copied once it is in, and the question before a deletion is drawn.
    pub(super) fn poll_there(&mut self, ctx: &egui::Context) {
        let mut said: Vec<(String, ComputerFacts)> = Vec::new();
        let mut listed = Vec::new();
        let mut log: Vec<String> = Vec::new();
        let mut note: Option<String> = None;
        let name_of = |remotes: &[RemoteSpec], id: &str| {
            remotes
                .iter()
                .find(|r| r.id == id)
                .map_or_else(|| id.to_string(), |r| r.name.clone())
        };
        // what the computers said they are
        let remotes = &self.remote.remotes;
        self.remote.there.asking.retain(|id, updates| loop {
            match updates.try_recv() {
                Ok(Update::Facts(facts)) => said.push((id.clone(), *facts)),
                Ok(Update::Failed(why)) => log.push(format!("[remote] {why}")),
                Ok(_) => {}
                Err(TryRecvError::Empty) => break true,
                Err(TryRecvError::Disconnected) => break false,
            }
        });
        // the downloads
        let mut gone = Vec::new();
        for (id, get) in self.remote.there.gets.iter_mut() {
            let Some(updates) = &get.updates else {
                gone.push(id.clone());
                continue;
            };
            let name = name_of(remotes, id);
            loop {
                match updates.try_recv() {
                    Ok(Update::Getting(run)) => {
                        if get.label.is_empty() {
                            get.label = run.configuration.clone();
                        }
                        get.ended = false;
                        get.there = Some(run);
                    }
                    Ok(Update::GetEnded {
                        label,
                        outcome,
                        message,
                    }) => {
                        let what = if label.is_empty() {
                            get.label.clone()
                        } else {
                            label
                        };
                        get.ended = true;
                        get.there = None;
                        log.push(match outcome.as_str() {
                            "done" => format!("[remote] Downloaded {what} on {name}"),
                            "stopped" => {
                                format!("[remote] The download of {what} on {name} was cancelled")
                            }
                            _ => {
                                let why = message.unwrap_or_default();
                                note = Some(format!("{name} could not download {what}: {why}"));
                                format!("[remote] {name} could not download {what}: {why}")
                            }
                        });
                    }
                    Ok(Update::Models(models)) => listed.push((id.clone(), models)),
                    Ok(Update::Facts(facts)) => said.push((id.clone(), *facts)),
                    Ok(Update::Doing(what)) => log.push(format!("[remote] {what}...")),
                    Ok(Update::Failed(why)) => {
                        get.ended = true;
                        note = Some(why.clone());
                        log.push(format!("[remote] {why}"));
                    }
                    Ok(_) => {}
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        gone.push(id.clone());
                        break;
                    }
                }
            }
        }
        for id in gone {
            self.remote.there.gets.remove(&id);
        }
        // the deletions and the copies
        let remotes = &self.remote.remotes;
        self.remote.there.chores.retain(|(id, _, updates)| loop {
            match updates.try_recv() {
                Ok(Update::Deleted(lines)) => {
                    log.extend(lines.into_iter().map(|l| format!("[remote] {l}")))
                }
                Ok(Update::Sent(file)) => log.push(format!(
                    "[remote] Copied {file} to {}",
                    name_of(remotes, id)
                )),
                Ok(Update::Doing(what)) => log.push(format!("[remote] {what}...")),
                Ok(Update::Models(models)) => listed.push((id.clone(), models)),
                Ok(Update::Facts(facts)) => said.push((id.clone(), *facts)),
                Ok(Update::Failed(why)) => log.push(format!("[remote] {why}")),
                Ok(_) => {}
                Err(TryRecvError::Empty) => break true,
                Err(TryRecvError::Disconnected) => break false,
            }
        });
        // a model downloaded here to be copied there, once the download here is over
        if self.download.is_none() {
            if let Some((id, files)) = self.remote.there.copy_after.take() {
                let spec = self.remote.remotes.iter().find(|r| r.id == id).cloned();
                let paths: Vec<(PathBuf, String)> = files
                    .iter()
                    .map(|f| (self.models_dir.join(f), f.clone()))
                    .collect();
                match spec {
                    Some(spec) if paths.first().is_some_and(|(p, _)| p.is_file()) => {
                        // the record that pairs them goes too, so that the projector is attached there
                        let mut paths: Vec<(PathBuf, String)> =
                            paths.into_iter().filter(|(p, _)| p.is_file()).collect();
                        let record = crate::bundles::record_path(&paths[0].0);
                        if record.is_file() {
                            let name = record
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            paths.push((record, name));
                        }
                        log.push(format!(
                            "[remote] Copying {} to {}...",
                            paths[0].1, spec.name
                        ));
                        self.send_files_there(&spec, paths, ctx);
                    }
                    Some(spec) => log.push(format!(
                        "[remote] {} was not downloaded here, so nothing was copied to {}.",
                        files.first().cloned().unwrap_or_default(),
                        spec.name
                    )),
                    None => {}
                }
            }
        }
        for (id, facts) in said {
            self.keep_facts(&id, facts);
        }
        for (id, models) in listed {
            self.keep_models_there(&id, models);
        }
        if let Some(note) = note {
            self.catalog.note = Some(note);
        }
        for line in log {
            self.push_log(line);
        }
    }

    /// What the steps of a run, a Refresh or a find said of the computer `id` besides the run: what it is, and a
    /// download that goes on there, which is then followed.
    pub(super) fn take_there_news(
        &mut self,
        id: &str,
        facts: Option<ComputerFacts>,
        getting: bool,
        ctx: &egui::Context,
    ) {
        if let Some(facts) = facts {
            self.keep_facts(id, facts);
        }
        if getting {
            self.follow_get_there(id, ctx);
        }
    }
}

/// The files of a model here to copy to another computer: its weights, what the record beside them pairs with them, and
/// the record, each with the name it is kept under.
fn files_of(weights: &std::path::Path) -> Vec<(PathBuf, String)> {
    let name = |p: &std::path::Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut files = vec![(weights.to_path_buf(), name(weights))];
    for companion in crate::bundles::companions(weights) {
        // one kept in a folder of its own (an engine's text encoder) is not copied beside the weights
        if companion.exists() && companion.path.parent() == weights.parent() {
            let kept = name(&companion.path);
            files.push((companion.path, kept));
        }
    }
    let record = crate::bundles::record_path(weights);
    if record.is_file() {
        let kept = name(&record);
        files.push((record, kept));
    }
    files
}

/// How long ago `at` was, in words: `a moment ago`, `3 minutes ago`.
fn ago(at: SystemTime) -> String {
    let seconds = SystemTime::now()
        .duration_since(at)
        .map_or(0, |d| d.as_secs());
    let (n, unit) = match seconds {
        0..=59 => return "a moment ago".to_string(),
        60..=3599 => (seconds / 60, "minute"),
        3600..=86_399 => (seconds / 3600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

#[cfg(test)]
mod tests {
    use super::GetThere;
    use crate::app::remote_models::PickThere;
    use crate::app::App;
    use crate::config::{build_registry, ConfigDirs};
    use crate::facts::{ComputerFacts, DeviceFacts};
    use crate::models::{KeptFile, KeptModel};
    use crate::remote::{RemoteSpec, Way};
    use crate::run_file::{RunFile, RunStep};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// A window that shows Studio, a computer QuickAI sets up, and has not looked for its run.
    fn on_studio(dir: &std::path::Path) -> App {
        let mut settings = Settings::default();
        let mut studio = RemoteSpec::new("Studio", "me@studio.local");
        studio.way = Way::SetUp;
        settings.set_remotes(&[studio]);
        settings.set_runner(Some("studio"));
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        app.remote.find_wanted = false;
        app
    }

    fn kept(file: &str, size: u64) -> KeptModel {
        KeptModel {
            file: file.into(),
            display: file.into(),
            size,
            path: format!("/home/me/.quickai/models/{file}"),
            own: true,
            facts: Some(quickai_core::gguf::ModelFacts {
                layers: Some(32),
                kv_bytes_per_token: Some(128 * 1024),
                context_length: Some(131_072),
                ..Default::default()
            }),
            projector_bytes: GB / 2,
            companions: vec![KeptFile {
                file: format!("mmproj-{file}"),
                size: GB / 2,
            }],
        }
    }

    #[test]
    fn the_panel_counts_the_computer_shown_once_it_said_what_it_has() {
        let dir = scratch("panel-counts-studio");
        let mut app = on_studio(&dir);
        app.keep_models_there("studio", vec![kept("Big-Q4_K_M.gguf", 5 * GB)]);
        app.remote
            .picked
            .insert("studio".into(), PickThere::There("Big-Q4_K_M.gguf".into()));
        // before Studio said what it has, nothing is fitted to this computer instead
        assert!(
            app.subject().is_none(),
            "nothing is counted before Studio said"
        );
        assert_eq!(app.ram_for_fit(), 0);
        assert!(app.need().is_none());

        app.keep_facts(
            "studio",
            ComputerFacts {
                system: "linux x86_64".into(),
                memory_bytes: 64 * GB,
                memory_free_bytes: 60 * GB,
                vendors: vec!["NVIDIA".into()],
                devices_known: true,
                devices: vec![DeviceFacts {
                    id: "CUDA0".into(),
                    name: "RTX 4090".into(),
                    total_bytes: 24 * GB,
                    free_bytes: 23 * GB,
                    kind: "discrete".into(),
                }],
                linked: false,
            },
        );
        assert_eq!(
            app.ram_for_fit(),
            64 * GB,
            "Studio's memory, not this computer's 16 GB"
        );
        assert!(app.cards_known());
        let cards = app.graphics_cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].name, "RTX 4090");
        let need = app.need().expect("the file there says what it needs");
        assert_eq!(
            need.weights,
            5 * GB + GB / 2,
            "the model with its projector"
        );
        assert_eq!(need.layers, 32);
        assert_eq!(
            app.subject_facts().and_then(|f| f.context_length),
            Some(131_072)
        );
        assert!(
            app.there_words().contains("RTX 4090"),
            "{}",
            app.there_words()
        );
        let have = app.have_there().expect("what Studio keeps");
        assert_eq!(have.get("Big-Q4_K_M.gguf"), Some(&(5 * GB)));
        assert_eq!(have.get("mmproj-Big-Q4_K_M.gguf"), Some(&(GB / 2)));
    }

    #[test]
    fn a_download_there_counts_the_bytes_of_all_its_files() {
        let step = |state: &str, done: u64, total: Option<u64>| RunStep {
            kind: "fetch".into(),
            text: "Download".into(),
            state: state.into(),
            done,
            total,
            began: None,
            ended: None,
        };
        let mut get = GetThere {
            label: "Big-Q4_K_M.gguf and 1 more file".into(),
            ..GetThere::default()
        };
        assert_eq!(get.line("Studio").0, "Starting the download on Studio…");
        get.there = Some(Box::new(RunFile {
            pid: 1,
            configuration: get.label.clone(),
            version: "0.2.11".into(),
            started: 0,
            detached: true,
            state: "preparing".into(),
            endpoints: Vec::new(),
            model: None,
            steps: vec![step("done", 100, Some(300)), step("doing", 50, Some(100))],
            computer: None,
        }));
        // a file that is done counts whole, whatever its last progress said
        assert_eq!(get.progress(), (350, Some(400)));
        let (line, share) = get.line("Studio");
        assert!(line.starts_with("Downloading on Studio: "), "{line}");
        assert!((share.unwrap() - 0.875).abs() < 1e-6);
        assert!(get.going());
    }
}
