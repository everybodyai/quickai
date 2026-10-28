//! Runs of configurations on computers QuickAI sets up (WP4.33, docs/remote-runs.md): Start sends the selected
//! configuration to the computer chosen in the status bar with QuickAI's own headless program, the window follows it
//! until it is ready and brings its ports here, and Stop stops it there. Each computer has a run of its own, and one
//! here can go on at the same time: choosing a computer in the status bar shows its run, and the chat talks to it
//! (the user: "It should also be able to run locally or run multiple remotes ... the chat attaches automatically to
//! the connected (running cfg)"). Another run there is said, and the person chooses: override it, stop it, or leave
//! it (section 3.2). A run lives until it is stopped, from here or there; the window finds the ones that go on when
//! it starts (`find`). The steps are `remote::job`'s, on threads of their own; this is what the window keeps of them.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossbeam_channel::Receiver;
use eframe::egui;

use crate::provision::EndpointInfo;
use crate::remote::alternatives::{self, Issue};
use crate::remote::job::{self, Job, Launching, Update};
use crate::remote::{self, RemoteSpec, Tunnel, Way};
use crate::run_file::RunFile;

use super::theme::{self, Role};
use super::App;

/// What the window keeps of the run on a computer it set up.
#[derive(Default)]
pub struct SetupRun {
    /// The configuration, by name.
    pub configuration: String,
    /// What is being done now, in a few words (`Copying QuickAI to Studio`); empty when nothing is.
    pub doing: String,
    /// QuickAI is on the computer, of this release: the first step, done.
    pub reached: bool,
    /// The run there as it last said itself: its state, the steps of its plan, the computer, where it listens.
    pub there: Option<Box<RunFile>>,
    /// Another run goes on there, and the person is asked what to do with it (`answer_busy`).
    pub busy: Option<Option<Box<RunFile>>>,
    /// What the configuration lacks there, each with another way, while the person is asked about them.
    pub alternatives: Option<Vec<Issue>>,
    /// The person's answers to them, by the issue's id: accepted or rejected.
    pub answers: BTreeMap<String, bool>,
    /// The words of `quickai-run up` the accepted answers came to, for the window to keep for the computer.
    remember: Vec<String>,
    /// It runs there, and each thing it started is reached here at these addresses.
    pub endpoints: Vec<EndpointInfo>,
    /// Why it could not be done, or stopped.
    pub failed: Option<String>,
    tunnel: Option<Tunnel>,
    updates: Option<Receiver<Update>>,
    cancel: Arc<AtomicBool>,
    /// The window's context, to wake it when a step says something.
    wake: Option<egui::Context>,
}

impl SetupRun {
    /// It runs there, and its ports are here.
    pub fn running(&self) -> bool {
        self.tunnel.is_some()
    }

    /// A step is being done (connecting, copying, starting, following, stopping).
    pub fn working(&self) -> bool {
        self.updates.is_some()
    }

    /// The address here of what it started that has `role` (`api`, `page`).
    pub fn endpoint(&self, role: &str) -> Option<&EndpointInfo> {
        self.endpoints.iter().find(|e| e.role == role)
    }

    /// Where the chat asks for a reply: the port here of its API, and the model it runs.
    pub fn chat_target(&self) -> Option<(u16, String)> {
        let api = self.endpoint("api").filter(|_| self.running())?;
        let port = api
            .url
            .split_once("://")
            .map_or(api.url.as_str(), |(_, rest)| rest)
            .split('/')
            .next()?
            .rsplit_once(':')?
            .1
            .parse()
            .ok()?;
        let model = self
            .there
            .as_ref()
            .and_then(|t| t.model.clone())
            .unwrap_or_default();
        Some((port, model))
    }
}

/// What the person answers when another run goes on there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Stop it, then run the selected configuration.
    Override,
    /// Stop it, and run nothing.
    StopIt,
    /// Leave it as it is.
    Cancel,
}

impl App {
    /// The run on the computer chosen in the status bar, when it is one QuickAI sets up and has one (working,
    /// running, or asking).
    pub(super) fn setup_run(&self) -> Option<&SetupRun> {
        let spec = self.setup_in_use()?;
        self.remote.setup.get(&spec.id)
    }

    /// The run on the computer `id`, when there is one.
    pub(super) fn setup_run_of(&self, id: &str) -> Option<&SetupRun> {
        self.remote.setup.get(id)
    }

    /// Start, with a computer QuickAI sets up chosen: the selected configuration is sent there and run.
    /// `override_first`: what runs there now is stopped first (the person said Override).
    pub(super) fn start_setup_run(&mut self, ctx: &egui::Context, override_first: bool) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        if self.setup_run().is_some_and(SetupRun::working) {
            return;
        }
        let (name, text) = self.selected_configuration_text();
        // what was answered about the other ways proposed is kept from one start to the next
        let answers = self
            .remote
            .setup
            .get(&spec.id)
            .map(|r| r.answers.clone())
            .unwrap_or_default();
        let cancel = Arc::new(AtomicBool::new(false));
        let wake = ctx.clone();
        let updates = job::start(
            self.job_for(&spec),
            Launching {
                name: name.clone(),
                text,
                flags: spec.accepted.clone(),
                override_first,
                answers: answers.clone(),
                local_models: self.local_model_files(),
            },
            move || wake.request_repaint(),
            cancel.clone(),
        );
        self.push_log(format!("[remote] Running \"{name}\" on {}...", spec.name));
        self.remote.setup.insert(
            spec.id.clone(),
            SetupRun {
                configuration: name,
                doing: format!("Connecting to {}", spec.name),
                answers,
                updates: Some(updates),
                cancel,
                wake: Some(ctx.clone()),
                ..SetupRun::default()
            },
        );
    }

    /// Stop, with a computer QuickAI set up chosen: what runs there is stopped, and its ports are let go of here.
    pub(super) fn stop_setup_run(&mut self) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let job = self.job_for(&spec);
        let run = self.remote.setup.entry(spec.id.clone()).or_default();
        let wake = run.wake.clone();
        run.cancel.store(true, Ordering::SeqCst);
        if let Some(mut tunnel) = run.tunnel.take() {
            tunnel.close();
        }
        run.doing = format!("Stopping the run on {}", spec.name);
        run.busy = None;
        run.failed = None;
        run.endpoints.clear();
        run.updates = Some(job::stop(job, move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        }));
    }

    /// Finds the run that goes on on the computer `spec` QuickAI set up, if one does, and brings its ports here.
    fn find_setup_run(&mut self, spec: &RemoteSpec, ctx: &egui::Context) {
        if self
            .remote
            .setup
            .get(&spec.id)
            .is_some_and(|r| r.working() || r.running())
        {
            return;
        }
        let wake = ctx.clone();
        let updates = job::find(self.job_for(spec), move || wake.request_repaint());
        self.remote.setup.insert(
            spec.id.clone(),
            SetupRun {
                doing: format!("Looking for a run on {}", spec.name),
                updates: Some(updates),
                wake: Some(ctx.clone()),
                ..SetupRun::default()
            },
        );
    }

    /// What the person answered about another run there.
    pub(super) fn answer_busy(&mut self, answer: Answer, ctx: &egui::Context) {
        let Some(id) = self.setup_in_use().map(|s| s.id.clone()) else {
            return;
        };
        if let Some(run) = self.remote.setup.get_mut(&id) {
            run.busy = None;
        }
        match answer {
            Answer::Override => self.start_setup_run(ctx, true),
            Answer::StopIt => self.stop_setup_run(),
            Answer::Cancel => {
                self.remote.setup.remove(&id);
            }
        }
    }

    /// Each frame: the runs that are to be looked for are (each computer QuickAI sets up, when the window starts, and
    /// one chosen again), what the steps said is kept, and a tunnel that closed by itself is said.
    pub(super) fn poll_setup_run(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.remote.find_wanted) {
            let specs: Vec<RemoteSpec> = self
                .remote
                .remotes
                .iter()
                .filter(|r| r.way == Way::SetUp)
                .cloned()
                .collect();
            for spec in specs {
                self.find_setup_run(&spec, ctx);
            }
        }
        let mut log = Vec::new();
        let mut gone = Vec::new();
        let mut remembered = Vec::new();
        for (id, run) in self.remote.setup.iter_mut() {
            let name = self
                .remote
                .remotes
                .iter()
                .find(|r| &r.id == id)
                .map_or_else(|| id.clone(), |r| r.name.clone());
            if take_updates(run, &name, &mut log) {
                gone.push(id.clone());
            }
            if !run.remember.is_empty() {
                remembered.push((id.clone(), std::mem::take(&mut run.remember)));
            }
        }
        for id in gone {
            self.remote.setup.remove(&id);
        }
        // what the person accepted for a computer is kept with it, so that the next run does not ask again
        if !remembered.is_empty() {
            for (id, words) in remembered {
                if let Some(spec) = self.remote.remotes.iter_mut().find(|r| r.id == id) {
                    alternatives::merge_accepted(&mut spec.accepted, &words);
                }
            }
            self.store_remotes();
        }
        for line in log {
            self.push_log(line);
        }
    }

    /// The question when another run goes on on the computer (section 3.2): what runs there, and Override (stop it
    /// and run the selected configuration), Stop it, or Cancel.
    pub(super) fn show_busy_dialog(&mut self, ctx: &egui::Context) {
        let Some(run) = self.setup_run() else {
            return;
        };
        let Some(other) = run.busy.clone() else {
            return;
        };
        let computer = self
            .setup_in_use()
            .map_or_else(|| "That computer".to_string(), |s| s.name.clone());
        let ours = self.selected_configuration_name();
        let what = match &other {
            Some(other) => format!(
                "QuickAI runs \"{}\" on {computer}, started {}.",
                other.name(),
                since(other.started)
            ),
            None => format!("Another run of QuickAI goes on on {computer}."),
        };
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::new("remote_busy_modal")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(
                egui::RichText::new(format!("{computer} already runs something"))
                    .size(16.0)
                    .family(theme::semibold(ui)),
            );
            ui.add_space(6.0);
            ui.label(what);
            ui.label(theme::muted(
                "One run at a time on a computer. Nothing is stopped until you choose.",
            ));
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::button_sized(ui, Role::Primary, true, "Override", egui::vec2(0.0, 30.0))
                    .on_hover_text(format!("Stop it, then run \"{ours}\" on {computer}."))
                    .clicked()
                {
                    answer = Some(Answer::Override);
                }
                if ui
                    .button("Stop it")
                    .on_hover_text("Stop it, and run nothing there.")
                    .clicked()
                {
                    answer = Some(Answer::StopIt);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(Answer::Cancel);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(Answer::Cancel);
        }
        if let Some(answer) = answer {
            self.answer_busy(answer, ctx);
        }
    }

    /// The model files this computer has, by name, with their path and size: what can be copied to a computer that
    /// lacks one.
    fn local_model_files(&self) -> BTreeMap<String, (PathBuf, u64)> {
        self.models
            .iter()
            .filter(|m| m.path.is_file())
            .map(|m| (m.filename.clone(), (m.path.clone(), m.size_bytes)))
            .collect()
    }

    /// The questions about the other ways for what the configuration lacks there (section 6): each with Accept and
    /// Reject, and Continue once each is answered, which starts again with the answers.
    pub(super) fn show_alternatives_dialog(&mut self, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let Some(run) = self.remote.setup.get_mut(&spec.id) else {
            return;
        };
        let Some(issues) = run.alternatives.clone() else {
            return;
        };
        let p = theme::palette();
        let configuration = run.configuration.clone();
        let mut go = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("remote_alternatives_modal")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.label(
                egui::RichText::new(format!(
                    "Before \"{configuration}\" runs on {}",
                    spec.name
                ))
                .size(16.0)
                .family(theme::semibold(ui)),
            );
            ui.label(theme::muted(format!(
                "What {} lacks for it, each with another way. Accept or reject each; nothing is installed before \
                 you continue.",
                spec.name
            )));
            ui.add_space(8.0);
            for issue in &issues {
                let answer = run.answers.get(&issue.id).copied();
                theme::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width(440.0);
                            ui.label(egui::RichText::new(format!("⚠ {}", issue.problem)).color(p.text));
                            let way = egui::RichText::new(format!("↳ {}", issue.alternative));
                            ui.label(match answer {
                                Some(false) => way.strikethrough().color(p.muted),
                                _ => way.color(p.ok),
                            });
                            if answer == Some(false) {
                                ui.label(egui::RichText::new(&issue.then).small().color(p.bad));
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.selectable_label(answer == Some(false), "Reject").clicked() {
                                run.answers.insert(issue.id.clone(), false);
                            }
                            if ui.selectable_label(answer == Some(true), "Accept").clicked() {
                                run.answers.insert(issue.id.clone(), true);
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
            ui.label(theme::muted(format!(
                "What you accept is kept for {}, so the next run does not ask again.",
                spec.name
            )));
            ui.separator();
            let left = issues
                .iter()
                .filter(|i| !run.answers.contains_key(&i.id))
                .count();
            ui.horizontal(|ui| {
                if left > 0 {
                    ui.label(theme::muted(format!("{left} left to answer")));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::button_sized(ui, Role::Primary, left == 0, "Continue", egui::vec2(0.0, 30.0))
                        .clicked()
                    {
                        go = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });
        if modal.should_close() && !go {
            cancel = true;
        }
        if go {
            run.alternatives = None;
            self.start_setup_run(ctx, false);
        } else if cancel {
            self.remote.setup.remove(&spec.id);
        }
    }

    /// The name of the configuration selected in the window.
    pub(super) fn selected_configuration_name(&self) -> String {
        self.registry
            .profile(&self.configs.active)
            .map(|e| e.configuration.name.clone())
            .unwrap_or_else(|| "Default".to_string())
    }

    /// The selected configuration as a file to run elsewhere: its name, and the text of what the window holds now.
    fn selected_configuration_text(&self) -> (String, String) {
        let id = self.configs.active.clone();
        let name = self.selected_configuration_name();
        let installed = self.installed_now();
        let captured = self.captured_now(&installed);
        let (text, _) = crate::configurations::export_captured(
            &self.registry,
            captured,
            &id,
            &name,
            &self.host(),
        );
        (name, text)
    }

    /// The job for `spec`: this window's ssh and scp, and the headless program of this release.
    fn job_for(&self, spec: &RemoteSpec) -> Job {
        Job {
            ssh: self.remote.ssh.clone(),
            scp: remote::ssh::scp_program(),
            spec: spec.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// What the steps of `run` (on the computer `name`) said since the last frame, kept in it; what is to be logged is
/// added to `log`. Whether the run is gone (nothing runs there, or it was stopped).
fn take_updates(run: &mut SetupRun, name: &str, log: &mut Vec<String>) -> bool {
    let mut said = Vec::new();
    if let Some(updates) = &run.updates {
        loop {
            match updates.try_recv() {
                Ok(update) => said.push(update),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    run.updates = None;
                    break;
                }
            }
        }
    }
    let mut gone = false;
    for update in said {
        match update {
            Update::Doing(what) => {
                log.push(format!("[remote] {what}..."));
                // past the connection and the copy, QuickAI is there
                if what.starts_with("Starting") || what.starts_with("Stopping \"") {
                    run.reached = true;
                }
                run.doing = what;
            }
            Update::Status(status) => {
                run.reached = true;
                if let Some(there) = status.running {
                    if run.configuration.is_empty() {
                        run.configuration = there.name();
                    }
                    run.there = Some(Box::new(there));
                }
            }
            Update::Busy(other) => {
                run.reached = true;
                run.doing.clear();
                run.busy = Some(other);
            }
            Update::Running {
                tunnel,
                endpoints,
                run: there,
            } => {
                if run.configuration.is_empty() {
                    run.configuration = there.name();
                }
                for endpoint in &endpoints {
                    log.push(format!(
                        "[remote] {} ({}) on {name} is at {}",
                        endpoint.program, endpoint.role, endpoint.url
                    ));
                }
                run.reached = true;
                run.doing.clear();
                run.there = Some(there);
                run.endpoints = endpoints;
                run.tunnel = Some(tunnel);
            }
            Update::Alternatives(issues) => {
                run.reached = true;
                run.doing.clear();
                run.alternatives = Some(issues);
            }
            Update::Remember(words) => run.remember.extend(words),
            Update::Nothing | Update::Stopped => gone = true,
            Update::Failed(why) => {
                log.push(format!("[remote] {why}"));
                run.doing.clear();
                run.failed = Some(why);
            }
        }
    }
    if let Some(tunnel) = run.tunnel.as_mut() {
        if !tunnel.alive() {
            let why = format!("The connection to {name} closed: {}", tunnel.why_closed());
            log.push(format!("[remote] {why}"));
            run.tunnel = None;
            run.endpoints.clear();
            run.failed = Some(why);
        }
    }
    gone
}

/// How long ago `started` (seconds since 1970) was, in words: `3 minutes ago`.
fn since(started: u64) -> String {
    let seconds = crate::run_file::now().saturating_sub(started);
    let (n, unit) = match seconds {
        0..=59 => return "a moment ago".into(),
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
