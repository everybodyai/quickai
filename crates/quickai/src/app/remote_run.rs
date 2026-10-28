//! A run of the selected configuration on a computer QuickAI sets up (WP4.33, docs/remote-runs.md): Start sends it
//! there with QuickAI's own headless program, the window follows it until it is ready and brings its ports here, and
//! Stop stops it there. Another run there is said, and the person chooses: override it, stop it, or leave it
//! (section 3.2). The run lives until it is stopped, from here or there; a window that starts again, or a computer
//! chosen again, finds it (`find`). The steps are `remote::job`'s, on threads of their own; this is what the window
//! keeps of them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossbeam_channel::Receiver;
use eframe::egui;

use crate::provision::EndpointInfo;
use crate::remote::job::{self, Job, Launching, Update};
use crate::remote::{self, RemoteSpec, Tunnel};
use crate::run_file::{RunFile, Status};

use super::theme::{self, Role};
use super::App;

/// What the window keeps of the run on a computer it set up.
#[derive(Default)]
pub struct SetupRun {
    /// The computer, by id.
    pub computer: String,
    /// The configuration, by name.
    pub configuration: String,
    /// What is being done now, in a few words (`Copying QuickAI to Studio`); empty once it runs.
    pub doing: String,
    /// What the run there says of itself while it is being prepared.
    pub status: Option<Box<Status>>,
    /// Another run goes on there, and the person is asked what to do with it (`answer`).
    pub busy: Option<Option<Box<RunFile>>>,
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
    /// The run on a computer QuickAI set up, when there is one (working, running, or asking).
    pub(super) fn setup_run(&self) -> Option<&SetupRun> {
        self.remote.setup.as_ref()
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
        let cancel = Arc::new(AtomicBool::new(false));
        let wake = ctx.clone();
        let updates = job::start(
            self.job_for(&spec),
            Launching {
                name: name.clone(),
                text,
                flags: Vec::new(),
                override_first,
            },
            move || wake.request_repaint(),
            cancel.clone(),
        );
        self.push_log(format!("[remote] Running \"{name}\" on {}...", spec.name));
        self.remote.setup = Some(SetupRun {
            computer: spec.id.clone(),
            configuration: name,
            doing: format!("Connecting to {}", spec.name),
            updates: Some(updates),
            cancel,
            wake: Some(ctx.clone()),
            ..SetupRun::default()
        });
    }

    /// Stop, with a computer QuickAI set up chosen: what runs there is stopped, and its ports are let go of here.
    pub(super) fn stop_setup_run(&mut self) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let wake = self.setup_run().and_then(|r| r.wake.clone());
        let updates = job::stop(self.job_for(&spec), move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        });
        let run = self.remote.setup.get_or_insert_with(SetupRun::default);
        run.cancel.store(true, Ordering::SeqCst);
        if let Some(mut tunnel) = run.tunnel.take() {
            tunnel.close();
        }
        run.computer = spec.id.clone();
        run.doing = format!("Stopping the run on {}", spec.name);
        run.busy = None;
        run.endpoints.clear();
        run.updates = Some(updates);
    }

    /// Finds the run that goes on on the computer QuickAI set up, if one does, and brings its ports here: when the
    /// window starts, and when that computer is chosen.
    pub(super) fn find_setup_run(&mut self, ctx: &egui::Context) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        if self.setup_run().is_some_and(|r| r.working() || r.running()) {
            return;
        }
        let wake = ctx.clone();
        let updates = job::find(self.job_for(&spec), move || wake.request_repaint());
        self.remote.setup = Some(SetupRun {
            computer: spec.id.clone(),
            doing: format!("Looking for a run on {}", spec.name),
            updates: Some(updates),
            wake: Some(ctx.clone()),
            ..SetupRun::default()
        });
    }

    /// What the person answered about another run there.
    pub(super) fn answer_busy(&mut self, answer: Answer, ctx: &egui::Context) {
        if let Some(run) = self.remote.setup.as_mut() {
            run.busy = None;
        }
        match answer {
            Answer::Override => self.start_setup_run(ctx, true),
            Answer::StopIt => self.stop_setup_run(),
            Answer::Cancel => self.remote.setup = None,
        }
    }

    /// Each frame: a run that is to be looked for is, what the steps said is kept, and a tunnel that closed by itself
    /// is said.
    pub(super) fn poll_setup_run(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.remote.find_wanted) {
            self.find_setup_run(ctx);
        }
        let Some(run) = self.remote.setup.as_mut() else {
            return;
        };
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
        let mut log = Vec::new();
        let mut gone = false;
        for update in said {
            match update {
                Update::Doing(what) => {
                    log.push(format!("[remote] {what}..."));
                    run.doing = what;
                }
                Update::Status(status) => run.status = Some(status),
                Update::Busy(other) => {
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
                            "[remote] {} ({}) is at {}",
                            endpoint.program, endpoint.role, endpoint.url
                        ));
                    }
                    run.doing.clear();
                    run.status = None;
                    run.endpoints = endpoints;
                    run.tunnel = Some(tunnel);
                }
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
                let why = format!("The connection closed: {}", tunnel.why_closed());
                log.push(format!("[remote] {why}"));
                run.tunnel = None;
                run.endpoints.clear();
                run.failed = Some(why);
            }
        }
        if gone {
            self.remote.setup = None;
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
            ui.add_space(12.0);
            ui.horizontal(|ui| {
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

    /// The name of the configuration selected in the window.
    fn selected_configuration_name(&self) -> String {
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
