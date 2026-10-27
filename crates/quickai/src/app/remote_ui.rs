//! Running the models on another computer (WP4.27): the computers the person added, which of them the models run on,
//! the tunnel of the run that is going, and the window that adds, tests and removes them. The tunnel is `remote.rs`'s;
//! here it is opened when Start is pressed (on a thread: a login takes seconds), the run attaches to the engine through
//! it as to one that is already running here, and it is closed when the run is over.
//!
//! An engine can run elsewhere when its document can attach to one that is already running (`[launch.attach]` and a
//! `mode` field): Ollama and ComfyUI. llama.cpp starts its own server for each model, and stays on this computer.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crossbeam_channel::Receiver;
use eframe::egui;
use quickai_core::value::Value;

use super::theme::{self, Role};
use super::App;
use crate::models::ModelEntry;
use crate::remote::{self, RemoteSpec, Tunnel};

/// The answer of a tunnel being opened: the tunnel, or why there is none.
type Opening = Receiver<Result<Tunnel, String>>;

/// What the window holds of the remote computers.
pub struct RemoteState {
    /// The computers the person added, as the settings keep them.
    pub remotes: Vec<RemoteSpec>,
    /// The one the models run on (by id), `None` for this computer.
    pub runner: Option<String>,
    /// The tunnel of the run that is going, to the computer it was opened to.
    pub tunnel: Option<(String, Tunnel)>,
    /// A tunnel being opened, and the model the run is to start with once it is.
    opening: Option<(Opening, Option<ModelEntry>)>,
    /// Why the last connection failed, or the tunnel closed.
    pub error: Option<String>,
    /// The program that makes tunnels (`ssh`; a test's stand-in).
    pub ssh: PathBuf,
    /// The window that keeps the computers is open.
    pub show_window: bool,
    /// The computer being added or changed in that window (`None`: none is).
    pub form: Option<RemoteForm>,
    /// A test of a computer that is going, and what the last one found, by the computer's id.
    checking: Option<(String, Receiver<Result<String, String>>)>,
    pub checked: BTreeMap<String, Result<String, String>>,
}

impl Default for RemoteState {
    fn default() -> Self {
        RemoteState {
            remotes: Vec::new(),
            runner: None,
            tunnel: None,
            opening: None,
            error: None,
            ssh: remote::ssh_program(),
            show_window: false,
            form: None,
            checking: None,
            checked: BTreeMap::new(),
        }
    }
}

/// The fields of a computer as they are typed, before they are a `RemoteSpec`.
#[derive(Clone, Default)]
pub struct RemoteForm {
    /// The id of the computer being changed; `None` for a new one.
    pub editing: Option<String>,
    pub name: String,
    pub host: String,
    pub ssh_port: String,
    pub engine_port: String,
    pub identity: String,
    pub problem: Option<String>,
}

impl RemoteForm {
    fn of(spec: &RemoteSpec) -> RemoteForm {
        RemoteForm {
            editing: Some(spec.id.clone()),
            name: spec.name.clone(),
            host: spec.host.clone(),
            ssh_port: spec.ssh_port.to_string(),
            engine_port: spec.engine_port.map(|p| p.to_string()).unwrap_or_default(),
            identity: spec.identity.clone().unwrap_or_default(),
            problem: None,
        }
    }

    /// The computer the fields describe, or what is wrong with them.
    fn spec(&self, taken: &[RemoteSpec]) -> Result<RemoteSpec, String> {
        let port = |text: &str, what: &str| -> Result<Option<u16>, String> {
            let text = text.trim();
            if text.is_empty() {
                return Ok(None);
            }
            text.parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .map(Some)
                .ok_or_else(|| format!("{what} is a number from 1 to 65535."))
        };
        let mut spec = RemoteSpec::new(&self.name, &self.host);
        spec.ssh_port = port(&self.ssh_port, "The SSH port")?.unwrap_or(22);
        spec.engine_port = port(&self.engine_port, "The port there")?;
        spec.identity = Some(self.identity.trim().to_string()).filter(|i| !i.is_empty());
        if let Some(problem) = spec.problem() {
            return Err(problem);
        }
        // an id of its own: the one it had, or one made from its name that no other has
        match &self.editing {
            Some(id) => spec.id = id.clone(),
            None => {
                let base = spec.id.clone();
                let mut n = 2;
                while taken.iter().any(|r| r.id == spec.id) {
                    spec.id = format!("{base}-{n}");
                    n += 1;
                }
            }
        }
        Ok(spec)
    }
}

impl App {
    /// Whether the engine in use can run on another computer: its document can attach to one that is already running,
    /// and says which way to start in a field (`mode`).
    pub(super) fn can_run_remote(&self) -> bool {
        self.engine_doc()
            .is_some_and(|e| e.doc.launch.attach.is_some() && e.doc.launch.strategy_field.is_some())
    }

    /// The computer the engine's models run on, when it is not this one.
    pub(super) fn remote_in_use(&self) -> Option<&RemoteSpec> {
        if !self.can_run_remote() {
            return None;
        }
        let id = self.remote.runner.as_deref()?;
        self.remote.remotes.iter().find(|r| r.id == id)
    }

    /// The engine's port on `spec`: the one the person gave, else the engine's own.
    fn remote_port(&self, spec: &RemoteSpec) -> u16 {
        let own = self
            .engine_doc()
            .and_then(|e| e.doc.provides.default_port)
            .unwrap_or(remote::DEFAULT_ENGINE_PORT);
        spec.port_for(own)
    }

    /// The port of this computer the tunnel of the run listens on, while there is one.
    pub(super) fn tunnel_port(&self) -> Option<u16> {
        self.remote.tunnel.as_ref().map(|(_, t)| t.local_port)
    }

    /// A tunnel is being opened for a run.
    pub(super) fn remote_opening(&self) -> bool {
        self.remote.opening.is_some()
    }

    /// The values a run is planned with: the window's, and, through a tunnel, its port and the way of an engine that is
    /// already running (nothing is started here: the engine is the remote computer's).
    pub(super) fn run_values(&self) -> BTreeMap<String, Value> {
        let mut values = self.values.map().clone();
        if let Some(port) = self.tunnel_port() {
            values.insert(format!("{}.port", self.engine), Value::Int(i64::from(port)));
            if let Some(field) = self
                .engine_doc()
                .and_then(|e| e.doc.launch.strategy_field.clone())
            {
                values.insert(
                    format!("{}.{field}", self.engine),
                    Value::Str("attach".into()),
                );
            }
        }
        values
    }

    /// Start, for a run on another computer: the tunnel is opened on a thread, and the run starts (with `model`) once it
    /// is (`poll_remote`).
    pub(super) fn start_remote(&mut self, model: Option<ModelEntry>, ctx: &egui::Context) {
        if self.remote.opening.is_some() {
            return;
        }
        let Some(spec) = self.remote_in_use().cloned() else {
            return;
        };
        let port = self.remote_port(&spec);
        let ssh = self.remote.ssh.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(remote::open_with(&ssh, &spec, port, remote::OPEN_TIMEOUT));
            wake.request_repaint();
        });
        self.remote.error = None;
        self.remote.opening = Some((rx, model));
        self.push_log(format!("[remote] Connecting to {}...", self.remote_label()));
    }

    /// Each frame: a tunnel that has opened starts its run, one that could not says why; a run that is over closes its
    /// tunnel, and a tunnel that closed by itself says why.
    pub(super) fn poll_remote(&mut self, ctx: &egui::Context) {
        if let Some((rx, model)) = self.remote.opening.take() {
            match rx.try_recv() {
                Ok(Ok(tunnel)) => {
                    let id = self.remote.runner.clone().unwrap_or_default();
                    self.push_log(format!(
                        "[remote] Connected to {}: its engine is on port {} here.",
                        self.remote_label(),
                        tunnel.local_port
                    ));
                    self.remote.tunnel = Some((id, tunnel));
                    self.run_reached_running = false;
                    let wake = ctx.clone();
                    self.start_run(model.as_ref(), move || wake.request_repaint());
                }
                Ok(Err(why)) => {
                    self.push_log(format!("[remote] {why}"));
                    self.remote.error = Some(why);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    self.remote.opening = Some((rx, model));
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {}
            }
        }
        if let Some((id, mut tunnel)) = self.remote.tunnel.take() {
            if !self.session.is_active() {
                // the run is over (stopped, or it never got going): the tunnel goes with it
                tunnel.close();
            } else if !tunnel.alive() {
                let why = tunnel.why_closed();
                self.push_log(format!("[remote] The connection closed: {why}"));
                self.remote.error = Some(format!("The connection closed: {why}"));
                self.remote.tunnel = Some((id, tunnel));
            } else {
                self.remote.tunnel = Some((id, tunnel));
            }
        }
        if let Some((id, rx)) = self.remote.checking.take() {
            match rx.try_recv() {
                Ok(found) => {
                    self.remote.checked.insert(id, found);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    self.remote.checking = Some((id, rx))
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {}
            }
        }
    }

    /// The computer the models run on, as the window names it (`Studio PC`).
    pub(super) fn remote_label(&self) -> String {
        self.remote_in_use()
            .map_or_else(|| "this computer".to_string(), |r| r.name.clone())
    }

    /// Chooses where the models run (a computer by id, or this one) and writes it down.
    pub(super) fn choose_runner(&mut self, id: Option<String>) {
        if self.engine_locked() || self.remote.runner == id {
            return;
        }
        self.remote.runner = id;
        self.remote.error = None;
        self.store_remotes();
        // what is listed is what that computer has: its own list, when it gave one
        self.named = Default::default();
        self.models.clear();
        if self.lists_models() {
            self.load_cached_models();
        }
    }

    /// Writes the computers and the choice into the settings.
    fn store_remotes(&mut self) {
        let mut stored = self.settings_now();
        stored.set_remotes(&self.remote.remotes);
        stored.set_runner(self.remote.runner.as_deref());
        self.write_settings(&stored);
        self.stored = stored;
    }

    /// Tests `spec`: the tunnel, and the engine in use answering through it, on a thread.
    fn check_remote(&mut self, spec: RemoteSpec, ctx: &egui::Context) {
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let (name, identity) = match &engine.doc.launch.attach {
            Some(attach) => (engine.doc.engine.name.clone(), attach.identity.clone()),
            None => return,
        };
        let port = self.remote_port(&spec);
        let ssh = self.remote.ssh.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        let id = spec.id.clone();
        std::thread::spawn(move || {
            let found =
                remote::check_with(&ssh, &spec, port, &name, &identity.path, &identity.pointer);
            let _ = tx.send(found);
            wake.request_repaint();
        });
        self.remote.checked.remove(&id);
        self.remote.checking = Some((id, rx));
    }

    // ---- what is drawn -------------------------------------------------------------------------------------

    /// The row that says where the models run, on the page of an engine that can run elsewhere, once there is somewhere
    /// else: the choice, and what the connection is doing.
    pub(super) fn show_runs_on(&mut self, ui: &mut egui::Ui) {
        if !self.can_run_remote() || self.remote.remotes.is_empty() {
            return;
        }
        let locked = self.engine_locked() || self.remote_opening();
        let current = self.remote_label();
        let mut chosen: Option<Option<String>> = None;
        let mut manage = false;
        ui.add_space(10.0);
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Runs on:").on_hover_text(
                    "Where the models run: this computer, or another one you added (reached over SSH). The chat, the \
                     models and Connect an app work the same either way.",
                );
                ui.add_enabled_ui(!locked, |ui| {
                    egui::ComboBox::from_id_salt("runs_on")
                        .selected_text(if self.remote_in_use().is_some() {
                            current.clone()
                        } else {
                            "This computer".to_string()
                        })
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(self.remote_in_use().is_none(), "This computer")
                                .clicked()
                            {
                                chosen = Some(None);
                            }
                            for r in &self.remote.remotes {
                                let on = self.remote.runner.as_deref() == Some(r.id.as_str());
                                if ui
                                    .selectable_label(on, format!("{} ({})", r.name, r.host))
                                    .clicked()
                                {
                                    chosen = Some(Some(r.id.clone()));
                                }
                            }
                            ui.separator();
                            if ui.button("Manage computers...").clicked() {
                                manage = true;
                            }
                        });
                });
                if self.remote_opening() {
                    ui.spinner();
                    ui.label(theme::muted(format!("Connecting to {current}...")));
                } else if self.tunnel_port().is_some() {
                    ui.label(
                        egui::RichText::new(format!("Connected to {current}"))
                            .small()
                            .color(theme::palette().ok),
                    );
                } else if self.remote_in_use().is_some() {
                    ui.label(theme::muted(format!(
                        "Start connects to {current}; the models run there."
                    )));
                }
            });
            if let Some(why) = &self.remote.error {
                ui.colored_label(theme::palette().bad, why);
            }
        });
        if let Some(id) = chosen {
            self.choose_runner(id);
        }
        if manage {
            self.remote.show_window = true;
        }
    }

    /// The window of the computers: what they are for, the list (Use, Test, Edit, Remove) and the form that adds or
    /// changes one.
    pub(super) fn show_remotes_window(&mut self, ctx: &egui::Context) {
        if !self.remote.show_window {
            return;
        }
        let mut open = true;
        let mut action: Option<Action> = None;
        egui::Window::new("Remote computers")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 420.0])
            .show(ctx, |ui| {
                ui.label(
                    "Run the models on a more powerful computer (a desktop with a big graphics card, a rented server) \
                     and use them from here: the chat, the models and Connect an app work as they do on this one.",
                );
                ui.label(theme::muted(
                    "QuickAI connects with your own ssh: its keys, your agent and ~/.ssh/config are used as they are, and \
                     no port is opened to the network on either side. On that computer, run the engine as you would \
                     there (Ollama's service, or `quickai-run up` with a configuration) and leave it running.",
                ));
                ui.add_space(6.0);
                if self.remote.remotes.is_empty() && self.remote.form.is_none() {
                    ui.label(theme::muted("No computers yet."));
                }
                let checking = self.remote.checking.as_ref().map(|(id, _)| id.clone());
                for r in self.remote.remotes.clone() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&r.name).strong());
                        ui.label(theme::muted(r.host.clone()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                action = Some(Action::Remove(r.id.clone()));
                            }
                            if ui.small_button("Edit").clicked() {
                                action = Some(Action::Edit(r.clone()));
                            }
                            if ui
                                .add_enabled(checking.is_none(), egui::Button::new("Test").small())
                                .on_hover_text("Connect, and ask the engine there what it is. Nothing is started.")
                                .clicked()
                            {
                                action = Some(Action::Check(r.clone()));
                            }
                            let in_use = self.remote.runner.as_deref() == Some(r.id.as_str());
                            if ui
                                .add_enabled(!in_use && self.can_run_remote(), egui::Button::new("Use").small())
                                .on_disabled_hover_text(if in_use {
                                    "The models run on it now."
                                } else {
                                    "This engine runs on this computer only."
                                })
                                .clicked()
                            {
                                action = Some(Action::Use(r.id.clone()));
                            }
                        });
                    });
                    if checking.as_deref() == Some(r.id.as_str()) {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(theme::muted("Testing..."));
                        });
                    } else if let Some(found) = self.remote.checked.get(&r.id) {
                        match found {
                            Ok(version) => ui.colored_label(
                                theme::palette().ok,
                                format!("Connected: {} {version} answers there.", self.engine_name()),
                            ),
                            Err(why) => ui.colored_label(theme::palette().bad, why),
                        };
                    }
                }
                ui.separator();
                match &mut self.remote.form {
                    None => {
                        if ui.button("Add a computer...").clicked() {
                            action = Some(Action::New);
                        }
                    }
                    Some(form) => {
                        let title = if form.editing.is_some() { "Change the computer" } else { "Add a computer" };
                        ui.label(egui::RichText::new(title).strong());
                        egui::Grid::new("remote_form").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label("Name:");
                            ui.add(egui::TextEdit::singleline(&mut form.name).hint_text("Studio PC").desired_width(260.0));
                            ui.end_row();
                            ui.label("Address:").on_hover_text(
                                "A host name or IP address, user@host, or the name of a Host in your ~/.ssh/config.",
                            );
                            ui.add(egui::TextEdit::singleline(&mut form.host).hint_text("me@studio.local").desired_width(260.0));
                            ui.end_row();
                            ui.label("SSH port:");
                            ui.add(egui::TextEdit::singleline(&mut form.ssh_port).hint_text("22").desired_width(80.0));
                            ui.end_row();
                            ui.label("Port there:").on_hover_text(
                                "The port the engine listens on, on that computer. Empty is the engine's own: 11434 for Ollama, 8188 for ComfyUI.",
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut form.engine_port)
                                    .hint_text("the engine's own")
                                    .desired_width(120.0),
                            );
                            ui.end_row();
                            ui.label("Key file:").on_hover_text(
                                "Only when ssh would not find the key by itself (your agent, ~/.ssh/id_*, ~/.ssh/config).",
                            );
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut form.identity)
                                        .hint_text("optional")
                                        .desired_width(220.0),
                                );
                                if ui.small_button("...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                                        form.identity = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();
                        });
                        if let Some(problem) = &form.problem {
                            ui.colored_label(theme::palette().bad, problem);
                        }
                        ui.horizontal(|ui| {
                            if theme::button_sized(ui, Role::Normal, true, "Save", egui::vec2(64.0, 24.0)).clicked() {
                                action = Some(Action::Save);
                            }
                            if ui.button("Cancel").clicked() {
                                action = Some(Action::Cancel);
                            }
                        });
                    }
                }
            });
        if !open {
            self.remote.show_window = false;
            self.remote.form = None;
        }
        match action {
            Some(Action::New) => self.remote.form = Some(RemoteForm::default()),
            Some(Action::Edit(spec)) => self.remote.form = Some(RemoteForm::of(&spec)),
            Some(Action::Cancel) => self.remote.form = None,
            Some(Action::Save) => self.save_remote_form(),
            Some(Action::Remove(id)) => self.remove_remote(&id),
            Some(Action::Check(spec)) => self.check_remote(spec, ctx),
            Some(Action::Use(id)) => self.choose_runner(Some(id)),
            None => {}
        }
    }

    /// The form's computer is added, or changes the one it was.
    pub(super) fn save_remote_form(&mut self) {
        let Some(form) = self.remote.form.as_mut() else {
            return;
        };
        let others: Vec<RemoteSpec> = self
            .remote
            .remotes
            .iter()
            .filter(|r| Some(&r.id) != form.editing.as_ref())
            .cloned()
            .collect();
        match form.spec(&others) {
            Ok(spec) => {
                match self.remote.remotes.iter_mut().find(|r| r.id == spec.id) {
                    Some(existing) => *existing = spec,
                    None => self.remote.remotes.push(spec),
                }
                self.remote.form = None;
                self.store_remotes();
            }
            Err(problem) => form.problem = Some(problem),
        }
    }

    /// Forgets a computer; the models run here again when it was the one in use (not while a run uses it).
    fn remove_remote(&mut self, id: &str) {
        if self.remote.runner.as_deref() == Some(id) {
            if self.engine_locked() || self.remote_opening() {
                return;
            }
            self.remote.runner = None;
        }
        self.remote.remotes.retain(|r| r.id != id);
        self.remote.checked.remove(id);
        self.store_remotes();
    }
}

/// What a click in the window of the computers asked for.
enum Action {
    New,
    Edit(RemoteSpec),
    Cancel,
    Save,
    Remove(String),
    Check(RemoteSpec),
    Use(String),
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use eframe::egui;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::events::ServerState;
    use crate::settings::Settings;
    use crate::testkit::fake_ollama::FakeOllama;
    use crate::testkit::{fake_ssh, scratch};

    const GB: u64 = 1024 * 1024 * 1024;

    /// An application on `engine` in `dir` with one remote computer, `host`, whose engine is on `port` of "its"
    /// loopback (this machine's, through the stand-in for ssh), chosen as where the models run. Nothing of the engine
    /// is installed here.
    fn app_on_remote(dir: &Path, engine: &str, host: &str, port: u16, ssh: &Path) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        let mut studio = RemoteSpec::new("Studio", host);
        studio.engine_port = Some(port);
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
        app.remote.ssh = ssh.to_path_buf();
        app.binary = PathBuf::from("/nowhere/ollama");
        app
    }

    /// Polls what a frame polls until `done`.
    fn until(app: &mut App, ctx: &egui::Context, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done(app) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {:?}, models {:?}, error {:?}, log {:?}",
                app.session.state(),
                app.models.iter().map(|m| &m.filename).collect::<Vec<_>>(),
                app.remote.error,
                app.log_lines
            );
            let mut said = Vec::new();
            app.session.poll(&mut |line| said.push(line), &mut |_| {});
            for line in said {
                app.push_log(line);
            }
            app.poll_remote(ctx);
            app.poll_named_models(ctx);
            app.poll_chat();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[cfg(unix)]
    fn the_models_run_on_the_other_computer_through_a_tunnel_and_everything_works_as_it_does_here()
    {
        let dir = scratch("remote-run");
        let Some(ssh) = fake_ssh::install(&dir) else {
            eprintln!("no python3: the stand-in for ssh cannot run");
            return;
        };
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 2_500_000_000);
        let mut app = app_on_remote(&dir, "ollama", "studio.local", fake.port(), &ssh);
        let ctx = egui::Context::default();
        assert!(app.can_run_remote());
        assert_eq!(app.remote_label(), "Studio");
        assert!(
            !app.engine_missing(),
            "Ollama is the other computer's to have"
        );

        // Start: the tunnel, then the run attaches to the engine there
        app.start_selected(&ctx);
        assert!(app.remote_opening());
        assert!(app.engine_locked());
        assert!(app
            .run_status_text(ServerState::Idle)
            .starts_with("Connecting to Studio"));
        until(&mut app, &ctx, "the run on the other computer", |a| {
            a.session.state() == ServerState::Running && !a.models.is_empty()
        });
        assert!(app.session.attached(), "nothing was started here");
        let port = app.tunnel_port().expect("a tunnel");
        assert_ne!(port, fake.port());
        assert_eq!(app.port(), port, "a client here uses the tunnel");
        assert!(app
            .run_status_text(ServerState::Running)
            .ends_with("on Studio"));
        assert!(fake_ssh::last_args(&ssh)
            .contains(&format!("127.0.0.1:{port}:127.0.0.1:{}", fake.port())));

        // its models are listed, a pull is made there, and the chat talks to it
        assert!(app.models.iter().any(|m| m.filename == "qwen3:4b"));
        app.model_input = "smollm2:135m".into();
        app.submit_model_input(&ctx);
        until(&mut app, &ctx, "the pull there", |a| {
            a.models.iter().any(|m| m.filename == "smollm2:135m")
        });
        app.selected = app
            .models
            .iter()
            .position(|m| m.filename == "qwen3:4b")
            .unwrap();
        app.load_selected();
        until(&mut app, &ctx, "the model to be loaded there", |a| {
            a.session.model_name() == Some("qwen3:4b")
        });
        app.chat.input = "Hello".into();
        app.send_chat(&ctx);
        until(&mut app, &ctx, "the reply", |a| !a.chat.replying());
        assert_eq!(app.chat.error, None);
        assert!(app
            .chat
            .turns
            .last()
            .is_some_and(|t| !t.from_person && !t.text.is_empty()));
        assert_eq!(fake.requests_to("/v1/chat/completions").len(), 1);

        // Stop: the run ends, the engine there goes on, and the tunnel is closed
        app.session.stop();
        until(
            &mut app,
            &ctx,
            "the run to end and the tunnel to close",
            |a| a.session.state() == ServerState::Idle && a.tunnel_port().is_none(),
        );
        assert!(crate::remote::identity(port, "/api/version", "/version").is_none());
        // and what it listed is remembered as that computer's, apart from this one's
        assert!(dir.join("data/ollama/catalog-cache-studio.json").is_file());
        assert!(!dir.join("data/ollama/catalog-cache.json").exists());
    }

    #[test]
    #[cfg(unix)]
    fn a_computer_that_cannot_be_reached_says_why_and_a_pull_that_waited_for_it_is_dropped() {
        let dir = scratch("remote-denied");
        let Some(ssh) = fake_ssh::install(&dir) else {
            return;
        };
        let mut app = app_on_remote(&dir, "ollama", "denied", 11434, &ssh);
        let ctx = egui::Context::default();
        assert!(
            app.pull_named("qwen3:4b", &ctx),
            "the pull waits for the connection"
        );
        until(&mut app, &ctx, "the connection to fail", |a| {
            !a.remote_opening()
        });
        until(&mut app, &ctx, "the pull to be dropped", |a| {
            a.named.queued.is_none()
        });
        let why = app.remote.error.clone().expect("why");
        assert!(why.contains("refused the login"), "{why}");
        assert_eq!(app.session.state(), ServerState::Idle);
        assert!(app
            .named
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("did not start")));
        assert!(!app.engine_locked());
    }

    #[test]
    fn llamacpp_runs_here_whatever_was_chosen_and_a_computer_is_added_changed_and_removed() {
        let dir = scratch("remote-form");
        let mut app = app_on_remote(&dir, "llamacpp", "studio.local", 11434, Path::new("ssh"));
        assert!(
            !app.can_run_remote(),
            "llama.cpp starts its own server for each model"
        );
        assert!(app.remote_in_use().is_none());

        // added: its id from its name, kept in the settings
        app.remote.form = Some(RemoteForm {
            name: "Studio".into(),
            host: "me@gpu-box".into(),
            ssh_port: "2222".into(),
            ..RemoteForm::default()
        });
        app.save_remote_form();
        assert!(app.remote.form.is_none());
        let ids: Vec<&str> = app.remote.remotes.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            ["studio", "studio-2"],
            "a name that is taken gets an id of its own"
        );
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(file["values"]["app.remotes"][1]["host"], "me@gpu-box");
        assert_eq!(file["values"]["app.remotes"][1]["ssh_port"], 2222);
        assert_eq!(file["values"]["app.runner"], "studio");

        // what is wrong with the fields is said, and nothing is kept
        app.remote.form = Some(RemoteForm {
            name: "Bad".into(),
            host: "x".into(),
            ssh_port: "99999".into(),
            ..RemoteForm::default()
        });
        app.save_remote_form();
        assert!(app
            .remote
            .form
            .as_ref()
            .and_then(|f| f.problem.clone())
            .is_some_and(|p| p.contains("65535")));
        assert_eq!(app.remote.remotes.len(), 2);

        // removing the one in use puts the models back here
        app.remove_remote("studio");
        assert_eq!(app.remote.runner, None);
        assert_eq!(app.remote.remotes.len(), 1);
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert!(file["values"].get("app.runner").is_none());
    }
}
