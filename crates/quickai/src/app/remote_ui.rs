//! Running the models on another computer (WP4.27, and its window as it is now, WP4.32): the computers the person
//! added, which of them the models run on, the tunnel of the run that is going, and the window that adds, tests and
//! removes them. The tunnel is `remote.rs`'s; here it is opened when Start is pressed (on a thread: a login takes
//! seconds), the run attaches to the engine through it as to one that is already running here, and it is closed when
//! the run is over.
//!
//! Where the models run is chosen at the right end of the status bar, where it is said (C183): this computer, a
//! computer that was added, or a new one. The Computers window is a window of its own, as Settings is: a list, a form
//! that suggests the hosts of `~/.ssh/config`, and a test that says step by step what passed and what to do about what
//! did not.
//!
//! An engine can run elsewhere when its document can attach to one that is already running (`[launch.attach]` and a
//! `mode` field): Ollama and ComfyUI. llama.cpp starts its own server for each model, and stays on this computer.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crossbeam_channel::Receiver;
use eframe::egui;
use quickai_core::registry::Engine;
use quickai_core::value::Value;

use super::icons::{self, Icon};
use super::theme::{self, Role, Tone};
use super::widgets::Picked;
use super::App;
use crate::models::ModelEntry;
use crate::remote::{self, Checked, RemoteSpec, SshHost, Tunnel, Way};

/// The answer of a tunnel being opened: the tunnel, or why there is none.
type Opening = Receiver<Result<Tunnel, String>>;

/// What the window calls itself to the window manager.
const COMPUTERS_TITLE: &str = "QuickAI Computers";

/// The least size of the window.
const LEAST: [f32; 2] = [420.0, 200.0];

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
    error: Option<RemoteError>,
    /// The program that makes tunnels (`ssh`; a test's stand-in).
    pub(super) ssh: PathBuf,
    /// The window that keeps the computers is open.
    show_window: bool,
    /// It was asked for again while open: it comes to the front.
    raise_window: bool,
    /// The computer being added or changed in that window (`None`: none is).
    form: Option<RemoteForm>,
    /// The computer whose test the window shows (`None`: the list).
    detail: Option<String>,
    /// The hosts of `~/.ssh/config` and the keys of `~/.ssh`, read when the form opens.
    hosts: Vec<SshHost>,
    keys: Vec<PathBuf>,
    /// A test of a computer that is going, and what the last one found, by the computer's id.
    checking: Option<(String, Receiver<Checked>)>,
    checked: BTreeMap<String, Checked>,
    /// The runs on computers QuickAI set up, by the computer's id (WP4.33, `remote_run`).
    pub setup: BTreeMap<String, super::remote_run::SetupRun>,
    /// The runs that go on there are to be looked for: when the window starts, and when a computer is chosen.
    pub find_wanted: bool,
}

/// Why the computer in use is not there: it could not be reached, or the connection to it closed during the run.
#[derive(Clone, Debug)]
enum RemoteError {
    Unreached(String),
    Lost(String),
}

impl RemoteError {
    /// What happened, in a sentence.
    fn why(&self) -> &str {
        match self {
            RemoteError::Unreached(why) | RemoteError::Lost(why) => why,
        }
    }
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
            raise_window: false,
            form: None,
            detail: None,
            hosts: Vec::new(),
            keys: Vec::new(),
            checking: None,
            checked: BTreeMap::new(),
            setup: BTreeMap::new(),
            find_wanted: true,
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
    /// The port here, the tunnel's end on this computer.
    pub local_port: String,
    pub identity: String,
    pub problem: Option<String>,
    /// The name was typed: it no longer follows the address.
    pub name_typed: bool,
    /// The port here was typed: it no longer follows the engine's port.
    pub here_typed: bool,
    /// How a run there is made.
    pub way: Way,
    /// More options (the ports and the key file) are open.
    pub more: bool,
}

impl RemoteForm {
    /// The form of a new computer, its ports the usual ones: `own` is the engine's.
    fn new(own: u16) -> RemoteForm {
        RemoteForm {
            ssh_port: "22".into(),
            engine_port: own.to_string(),
            // QuickAI sets a new computer up, and runs the selected configuration there (WP4.33): the port here is
            // the same as its run's there
            way: Way::SetUp,
            ..RemoteForm::default()
        }
    }

    /// The form of `spec`, where the engine's own port is `own`.
    fn of(spec: &RemoteSpec, own: u16) -> RemoteForm {
        let engine_port = spec.port_for(own);
        RemoteForm {
            editing: Some(spec.id.clone()),
            name: spec.name.clone(),
            host: spec.host.clone(),
            ssh_port: spec.ssh_port.to_string(),
            engine_port: engine_port.to_string(),
            local_port: match spec.way {
                Way::SetUp => spec.local_port.map(|p| p.to_string()).unwrap_or_default(),
                Way::Attach => spec.local_port.unwrap_or(engine_port).to_string(),
            },
            identity: spec.identity.clone().unwrap_or_default(),
            problem: None,
            name_typed: true,
            here_typed: spec.local_port.is_some(),
            way: spec.way,
            more: spec.ssh_port != 22
                || spec.engine_port.is_some()
                || spec.local_port.is_some()
                || spec.identity.is_some(),
        }
    }

    /// The computer the fields describe, or what is wrong with them. A port that is the usual one is kept as none
    /// given: the engine's (`own`) follows the engine in use, and the one here follows the engine's.
    fn spec(&self, taken: &[RemoteSpec], own: u16) -> Result<RemoteSpec, String> {
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
        // a computer nobody named is called after its address
        let name = if self.name.trim().is_empty() {
            remote::name_from_address(&self.host)
        } else {
            self.name.clone()
        };
        let mut spec = RemoteSpec::new(&name, &self.host);
        spec.ssh_port = port(&self.ssh_port, "The SSH port")?.unwrap_or(22);
        match self.way {
            // the ports are the configuration's: only the one here can be given
            Way::SetUp => spec.local_port = port(&self.local_port, "The port here")?,
            Way::Attach => {
                spec.engine_port =
                    port(&self.engine_port, "The engine's port")?.filter(|p| *p != own);
                let there = spec.port_for(own);
                spec.local_port = port(&self.local_port, "The port here")?.filter(|p| *p != there);
            }
        }
        spec.identity = Some(self.identity.trim().to_string()).filter(|i| !i.is_empty());
        spec.way = self.way;
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
        self.engine_doc().is_some_and(runs_elsewhere)
    }

    /// The engines that can run on another computer, by name, as a sentence lists them (`ComfyUI or Ollama`, with `or`).
    fn remote_engine_names(&self, or: &str) -> String {
        let names: Vec<&str> = self
            .registry
            .selectable()
            .filter(|e| runs_elsewhere(e))
            .map(|e| e.doc.engine.name.as_str())
            .collect();
        match names.split_last() {
            Some((last, [])) => last.to_string(),
            Some((last, rest)) => format!("{} {or} {last}", rest.join(", ")),
            None => String::new(),
        }
    }

    /// The computer the engine's models run on, when it is not this one and the window attaches to the engine there
    /// (WP4.27).
    pub(super) fn remote_in_use(&self) -> Option<&RemoteSpec> {
        self.runner_spec().filter(|r| r.way == Way::Attach)
    }

    /// The computer QuickAI sets up and runs the selected configuration on, when that is where it runs (WP4.33).
    pub(super) fn setup_in_use(&self) -> Option<&RemoteSpec> {
        self.runner_spec().filter(|r| r.way == Way::SetUp)
    }

    /// The name of the computer chosen in the status bar, when it is not this one.
    pub(super) fn runner_name(&self) -> Option<String> {
        self.runner_spec().map(|r| r.name.clone())
    }

    /// The computer chosen in the status bar, when it can be used with the engine in use.
    pub(super) fn runner_spec(&self) -> Option<&RemoteSpec> {
        let id = self.remote.runner.as_deref()?;
        self.remote
            .remotes
            .iter()
            .find(|r| r.id == id)
            .filter(|r| self.usable_with_engine(r))
    }

    /// Whether `spec` can be used with the engine in use: any engine on a computer QuickAI sets up, and on one where
    /// the window attaches, an engine that can attach (Ollama, ComfyUI).
    fn usable_with_engine(&self, spec: &RemoteSpec) -> bool {
        spec.way == Way::SetUp || self.can_run_remote()
    }

    /// The engine's port on `spec`: the one the person gave, else the engine's own.
    fn remote_port(&self, spec: &RemoteSpec) -> u16 {
        spec.port_for(self.own_remote_port())
    }

    /// The port the engine listens on of itself, on another computer: the engine in use's own, or Ollama's while it is
    /// one that cannot run elsewhere.
    fn own_remote_port(&self) -> u16 {
        self.engine_doc()
            .filter(|e| runs_elsewhere(e))
            .and_then(|e| e.doc.provides.default_port)
            .unwrap_or(remote::DEFAULT_ENGINE_PORT)
    }

    /// The port of this computer the tunnel of the run listens on, while there is one.
    pub(super) fn tunnel_port(&self) -> Option<u16> {
        self.remote.tunnel.as_ref().map(|(_, t)| t.local_port)
    }

    /// A tunnel is being opened for a run.
    pub(super) fn remote_opening(&self) -> bool {
        self.remote.opening.is_some()
    }

    /// The models run on the computer `id`.
    fn in_use(&self, id: &str) -> bool {
        self.runner_spec().is_some_and(|r| r.id == id)
    }

    /// Whether the computer shown cannot be changed to `to` now. Between this computer and the ones QuickAI sets up it
    /// always can: each has a run of its own, which goes on while another is shown. A computer the window attaches to
    /// shares the window's one session with this computer, which must be free for it (`engine_locked`).
    pub(super) fn runner_change_locked(&self, to: Option<&str>) -> bool {
        let attaches = |id: Option<&str>| {
            id.and_then(|id| self.remote.remotes.iter().find(|r| r.id == id))
                .is_some_and(|r| r.way == Way::Attach)
        };
        (attaches(self.remote.runner.as_deref()) || attaches(to)) && self.engine_locked()
    }

    /// Why where the models run cannot be changed now (`engine_locked`).
    fn where_lock_reason(&self) -> &'static str {
        if self.session.is_active() || self.remote_opening() {
            "Stop the server to change where it runs."
        } else {
            "Wait for what is being fetched to finish to change where it runs."
        }
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
        let here = remote::port_here(&spec, port);
        let ssh = self.remote.ssh.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let opened = here
                .and_then(|here| remote::open_with(&ssh, &spec, here, port, remote::OPEN_TIMEOUT));
            let _ = tx.send(opened);
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
                    self.remote.error = Some(RemoteError::Unreached(why));
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
                let why = format!("The connection closed: {}", tunnel.why_closed());
                self.push_log(format!("[remote] {why}"));
                self.remote.error = Some(RemoteError::Lost(why));
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
        if self.remote.runner == id || self.runner_change_locked(id.as_deref()) {
            return;
        }
        self.remote.runner = id;
        self.remote.error = None;
        self.remote.find_wanted = true;
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
        if spec.way == Way::SetUp {
            // reached, and QuickAI can run there: the engine is the configuration's, and is not asked for
            let ssh = self.remote.ssh.clone();
            let (tx, rx) = crossbeam_channel::bounded(1);
            let wake = ctx.clone();
            let id = spec.id.clone();
            std::thread::spawn(move || {
                let found = remote::probe::check_setup(&ssh, &spec, env!("CARGO_PKG_VERSION"));
                let _ = tx.send(found);
                wake.request_repaint();
            });
            self.remote.checked.remove(&id);
            self.remote.checking = Some((id, rx));
            return;
        }
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
                remote::check_steps(&ssh, &spec, port, &name, &identity.path, &identity.pointer);
            let _ = tx.send(found);
            wake.request_repaint();
        });
        self.remote.checked.remove(&id);
        self.remote.checking = Some((id, rx));
    }

    /// Opens the Computers window (or brings it to the front): on its form when there is no computer yet (there is
    /// nothing else to do in it then), else on its list.
    pub(super) fn open_computers(&mut self) {
        self.remote.show_window = true;
        self.remote.raise_window = true;
        self.remote.detail = None;
        if self.remote.remotes.is_empty() {
            self.new_remote_form();
        }
    }

    /// An empty form, with the hosts of `~/.ssh/config` and the keys of `~/.ssh` to suggest (read now: they may have
    /// changed since).
    fn new_remote_form(&mut self) {
        self.read_ssh_files();
        self.remote.form = Some(RemoteForm::new(self.own_remote_port()));
    }

    /// What the form suggests: the hosts of `~/.ssh/config` and the keys of `~/.ssh`.
    fn read_ssh_files(&mut self) {
        self.remote.hosts = remote::read_ssh_config_hosts();
        self.remote.keys = remote::read_ssh_keys();
    }

    /// The form's computer is added, or changes the one it was; its id when it was kept.
    pub(super) fn save_remote_form(&mut self) -> Option<String> {
        let own = self.own_remote_port();
        let form = self.remote.form.as_mut()?;
        let others: Vec<RemoteSpec> = self
            .remote
            .remotes
            .iter()
            .filter(|r| Some(&r.id) != form.editing.as_ref())
            .cloned()
            .collect();
        match form.spec(&others, own) {
            Ok(spec) => {
                let id = spec.id.clone();
                match self.remote.remotes.iter_mut().find(|r| r.id == spec.id) {
                    Some(existing) => *existing = spec,
                    None => self.remote.remotes.push(spec),
                }
                // what was found of it before is of the address it had
                self.remote.checked.remove(&id);
                self.remote.form = None;
                self.store_remotes();
                Some(id)
            }
            Err(problem) => {
                form.problem = Some(problem);
                None
            }
        }
    }

    /// Add and test: the form's computer is kept, and the window shows its test, which starts at once (for an engine
    /// that can run elsewhere: the test asks for the engine in use).
    fn add_and_test(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.save_remote_form() {
            self.act(Action::Check(id), ctx);
        }
    }

    /// Tests the computer `id`, when the engine in use can run on it.
    fn test_computer(&mut self, id: &str, ctx: &egui::Context) {
        if self.remote.checking.is_some() {
            return;
        }
        let spec = self.remote.remotes.iter().find(|r| r.id == id).cloned();
        if let Some(spec) = spec.filter(|s| self.usable_with_engine(s)) {
            self.check_remote(spec, ctx);
        }
    }

    /// Forgets a computer; the models run here again when it was the one in use (not while a run uses it).
    fn remove_remote(&mut self, id: &str) {
        if self.remote.runner.as_deref() == Some(id) {
            if self.engine_locked() {
                return;
            }
            self.remote.runner = None;
        }
        self.remote.remotes.retain(|r| r.id != id);
        self.remote.checked.remove(id);
        if self.remote.detail.as_deref() == Some(id) {
            self.remote.detail = None;
        }
        self.store_remotes();
    }

    /// What is known of whether the computer `id` answers: a test that passed, one that did not, or none yet (the
    /// connection of the run in use counts as a test).
    fn answers(&self, id: &str) -> Option<bool> {
        // a computer QuickAI set up whose run goes on, or did not come up
        match self.setup_run_of(id) {
            Some(run) if run.running() => return Some(true),
            Some(run) if run.failed.is_some() && !run.working() => return Some(false),
            _ => {}
        }
        if self.in_use(id) {
            if self.remote.error.is_some() {
                return Some(false);
            }
            if self.tunnel_port().is_some() {
                return Some(true);
            }
        }
        self.remote.checked.get(id).map(Checked::passed)
    }

    /// The line under a computer's name: where it is and what its last test found.
    fn computer_line(&self, spec: &RemoteSpec) -> String {
        match self.remote.checked.get(&spec.id) {
            Some(Checked {
                engine: Some(Ok(version)),
                ..
            }) => format!("{} · {} {version}", spec.host, self.engine_name()),
            Some(Checked {
                reached: Err(_), ..
            }) => format!("{} · could not be reached", spec.host),
            Some(_) => format!(
                "{} · {} is not answering there",
                spec.host,
                self.engine_name()
            ),
            None => spec.host.clone(),
        }
    }

    /// What this computer has, in a few words (`16 GB memory · AMD`).
    fn this_computer_line(&self) -> String {
        let gb = self.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let mut line = format!("{} GB memory", gb.round());
        for vendor in &self.gpu_vendors {
            line.push_str(" · ");
            line.push_str(vendor.label());
        }
        line
    }

    // ---- what is drawn -------------------------------------------------------------------------------------

    /// The right end of the status bar: where the models run, and the menu that changes it (option D of the samples,
    /// C183). It opens upwards, with this computer, the computers added (each with whether it answered) and the ways to
    /// add and manage them. Under an engine that cannot run elsewhere the computers are there, greyed, with why.
    pub(super) fn show_where_switch(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let (label, dot) = match self.runner_spec() {
            Some(r) => (r.name.clone(), self.answers(&r.id)),
            None => ("This computer".to_string(), None),
        };
        let font = egui::FontId::proportional(13.0);
        let galley = ui.painter().layout_no_wrap(label, font, p.muted);
        let width =
            8.0 + 15.0 + 6.0 + galley.size().x + if dot.is_some() { 14.0 } else { 0.0 } + 18.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::click());
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if ui.is_rect_visible(rect) {
            let lit = open || response.hovered();
            if lit {
                ui.painter().rect(
                    rect,
                    4.0,
                    p.raised_hover,
                    egui::Stroke::new(1.0, p.outline),
                    egui::StrokeKind::Inside,
                );
            }
            let color = if lit { p.text } else { p.muted };
            let mut x = rect.left() + 8.0;
            let icon = egui::Rect::from_min_size(
                egui::pos2(x, rect.center().y - 7.5),
                egui::vec2(15.0, 15.0),
            );
            icons::paint(ui.painter(), icon, Icon::Computer, color);
            x += 15.0 + 6.0;
            let text_at = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
            x += galley.size().x;
            ui.painter().galley(text_at, galley, color);
            if let Some(answers) = dot {
                x += 8.0;
                ui.painter().circle_filled(
                    egui::pos2(x, rect.center().y),
                    3.5,
                    if answers { p.ok } else { p.bad },
                );
                x += 6.0;
            }
            let chevron = egui::Rect::from_min_size(
                egui::pos2(x + 4.0, rect.center().y - 6.0),
                egui::vec2(12.0, 12.0),
            );
            icons::paint(ui.painter(), chevron, Icon::Chevron, color);
        }
        let response = response.on_hover_text(
            "Where the models run: this computer, or a more powerful one you added (reached over SSH). The chat, the \
             models and Connect an app work the same either way.",
        );
        egui::Popup::menu(&response)
            .align(egui::RectAlign::TOP_END)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .width(320.0)
            .show(|ui| {
                ui.set_width(320.0);
                self.show_where_menu(ui);
            });
    }

    /// The menu of the status bar's place: the choices, and the ways to the window.
    fn show_where_menu(&mut self, ui: &mut egui::Ui) {
        let lock_why = self.where_lock_reason();
        let mut chosen: Option<Option<String>> = None;
        let here = self.runner_spec().is_none();
        let locked = self.runner_change_locked(None);
        let this = where_row(
            ui,
            here,
            !locked,
            "This computer",
            &self.this_computer_line(),
            None,
        );
        if this.clicked() {
            chosen = Some(None);
        }
        this.on_hover_text(if locked {
            lock_why
        } else {
            "Run the models here."
        });
        for spec in &self.remote.remotes {
            let can = self.usable_with_engine(spec);
            let locked = self.runner_change_locked(Some(&spec.id));
            let line = if can {
                self.computer_line(spec)
            } else {
                spec.host.clone()
            };
            let dot = if can { self.answers(&spec.id) } else { None };
            let on = self.in_use(&spec.id);
            let item = where_row(ui, on, can && !locked, &spec.name, &line, dot);
            if item.clicked() {
                chosen = Some(Some(spec.id.clone()));
            }
            // a greyed computer says why when the pointer rests on it, and nothing more is said in the menu
            if !can {
                item.on_hover_text(format!(
                    "{} runs its models on this computer. {} can use {}.",
                    self.engine_name(),
                    self.remote_engine_names("and"),
                    spec.name
                ));
            } else if locked {
                item.on_hover_text(lock_why);
            }
        }
        ui.separator();
        if menu_item(ui, Icon::Gear, "Manage computers…").clicked() {
            self.open_computers();
        }
        if let Some(id) = chosen {
            self.choose_runner(id);
        }
    }

    /// The alert on the page when the computer in use could not be reached, or the connection to it closed: what
    /// happened, and the two ways on (again, or here).
    pub(super) fn show_remote_alert(&mut self, ui: &mut egui::Ui) {
        let Some(error) = self.remote.error.clone() else {
            return;
        };
        let Some(spec) = self.remote_in_use().cloned() else {
            return;
        };
        let why = error.why();
        let lost = matches!(error, RemoteError::Lost(_));
        let mut again = false;
        let mut here = false;
        let mut test = false;
        ui.add_space(8.0);
        theme::alert(ui, Tone::Bad, |ui| {
            ui.label(
                egui::RichText::new(if lost {
                    format!("Lost the connection to {}", spec.name)
                } else {
                    format!("Couldn't reach {}", spec.name)
                })
                .strong(),
            );
            ui.label(egui::RichText::new(why).small());
            ui.horizontal_wrapped(|ui| {
                let idle = !self.session.is_active() && !self.remote_opening();
                if theme::button_sized(ui, Role::Primary, idle, "Try again", egui::vec2(92.0, 26.0))
                    .on_disabled_hover_text("Stop the server first.")
                    .clicked()
                {
                    again = true;
                }
                if ui
                    .add_enabled(idle, egui::Button::new("Use this computer"))
                    .on_hover_text("Run the models here instead.")
                    .clicked()
                {
                    here = true;
                }
                if ui
                    .button("Test it…")
                    .on_hover_text(
                        "Open the computer in the Computers window and test it step by step.",
                    )
                    .clicked()
                {
                    test = true;
                }
            });
        });
        if again {
            self.remote.error = None;
            self.start_selected(ui.ctx());
        } else if here {
            self.choose_runner(None);
        } else if test {
            self.open_computers();
            self.act(Action::Check(spec.id.clone()), ui.ctx());
        }
    }

    /// The Computers window, if it is open: a window of its own, as Settings is (an immediate viewport: the frame that
    /// draws the main window draws it too).
    pub(super) fn show_remotes_window(&mut self, ctx: &egui::Context) {
        if !self.remote.show_window {
            return;
        }
        let id = egui::ViewportId::from_hash_of("quickai_computers");
        if std::mem::take(&mut self.remote.raise_window) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let mut close = false;
        let builder = crate::with_app_icon(
            egui::ViewportBuilder::default()
                .with_title(COMPUTERS_TITLE)
                .with_app_id(crate::APP_ID)
                .with_inner_size([540.0, 360.0])
                .with_min_inner_size(LEAST),
        );
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                // a context that cannot open a window (a test draws on one with no display) shows it inside the main
                // window, with its Close button under it
                self.show_computers_body(ui);
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            } else {
                if ui.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
                // as tall as what it shows: the form, a test, or the list
                theme::fitted_page(
                    ui,
                    "computers_scroll",
                    theme::page_frame(),
                    0.0,
                    LEAST[1],
                    |ui| self.show_computers_body(ui),
                );
            }
        });
        if close {
            self.remote.show_window = false;
            self.remote.form = None;
            self.remote.detail = None;
        }
    }

    /// What the window shows: the form when a computer is being added or changed, a computer's test when one was
    /// asked for, else the list.
    fn show_computers_body(&mut self, ui: &mut egui::Ui) {
        if self.remote.form.is_some() {
            self.show_computer_form(ui);
        } else if let Some(id) = self.remote.detail.clone() {
            self.show_computer_test(ui, &id);
        } else {
            self.show_computer_list(ui);
        }
    }

    /// The list: each computer with what it is and whether it answered, Use, and the rest in its menu.
    fn show_computer_list(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let can = self.can_run_remote();
        let locked = self.engine_locked();
        let lock_why = self.where_lock_reason();
        let mut action: Option<Action> = None;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Computers")
                    .size(18.0)
                    .family(theme::semibold(ui))
                    .color(p.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::labelled_button(
                    ui,
                    Role::Normal,
                    true,
                    Icon::Plus,
                    "Add a computer",
                    28.0,
                )
                .clicked()
                {
                    action = Some(Action::New);
                }
            });
        });
        ui.label(theme::muted(
            "Use the models of a more powerful computer: a desktop with a big graphics card, or a rented server.",
        ));
        // a computer the window attaches to has an engine there of its own: with another, it is greyed
        if !can && self.remote.remotes.iter().any(|r| r.way == Way::Attach) {
            ui.label(
                egui::RichText::new(format!(
                    "With {} the models run on this computer, or on one QuickAI sets up; {} can use the ones \
                     you run yourself.",
                    self.engine_name(),
                    self.remote_engine_names("and")
                ))
                .small()
                .color(p.faint),
            );
        }
        ui.add_space(8.0);
        let here = ComputerRow {
            spec: None,
            line: self.this_computer_line(),
            answers: None,
            on: self.runner_spec().is_none(),
            removable: false,
        };
        let rows = std::iter::once(here).chain(self.remote.remotes.iter().map(|spec| {
            ComputerRow {
                spec: Some(spec),
                line: self.computer_line(spec),
                answers: self.answers(&spec.id),
                on: self.in_use(&spec.id),
                // not the one chosen, while a run or a fetch keeps it, nor one a run goes on on
                removable: !(locked && self.remote.runner.as_deref() == Some(spec.id.as_str()))
                    && !self
                        .setup_run_of(&spec.id)
                        .is_some_and(|r| r.running() || r.working()),
            }
        }));
        for row in rows {
            let can = row.spec.is_none_or(|spec| self.usable_with_engine(spec));
            let locked = self.runner_change_locked(row.spec.map(|s| s.id.as_str()));
            let usable = !locked && can;
            if let Some(asked) = computer_row(ui, &row, can, usable, locked, lock_why) {
                action = Some(asked);
            }
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(
                "QuickAI connects with your own ssh: its keys, your agent and ~/.ssh/config, and no port is opened to \
                 the network on either side. The engine there keeps running when QuickAI closes.",
            )
            .small()
            .color(p.faint),
        );
        if let Some(action) = action {
            self.act(action, ui.ctx());
        }
    }

    /// Does what a click in the window asked for.
    fn act(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::New => self.new_remote_form(),
            Action::Back => self.remote.detail = None,
            Action::Edit(spec) => {
                self.read_ssh_files();
                self.remote.form = Some(RemoteForm::of(&spec, self.own_remote_port()));
            }
            Action::Remove(id) => self.remove_remote(&id),
            Action::Check(id) => {
                self.remote.detail = Some(id.clone());
                self.test_computer(&id, ctx);
            }
            Action::Use(id) => self.choose_runner(id),
        }
    }

    /// The form that adds or changes a computer: the address (with the hosts of `~/.ssh/config` offered), its name (made
    /// from the address until it is typed), and under More options the ports and the key file (with the keys of `~/.ssh`
    /// offered).
    fn show_computer_form(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let can = self.can_run_remote();
        let mut save = false;
        let mut cancel = false;
        let RemoteState {
            form, hosts, keys, ..
        } = &mut self.remote;
        let Some(form) = form.as_mut() else {
            return;
        };
        let title = match &form.editing {
            Some(_) => format!("Edit {}", form.name),
            None => "Add a computer".to_string(),
        };
        ui.label(
            egui::RichText::new(title)
                .size(18.0)
                .family(theme::semibold(ui))
                .color(p.text),
        );
        ui.label(theme::muted(
            "Use the models of a more powerful computer, reached over SSH with your own keys. Nothing is opened to \
             the network.",
        ));
        ui.add_space(10.0);
        let was = form.way;
        way_choice(ui, &mut form.way);
        if form.way != was && !form.here_typed {
            // the port here follows what is there: the engine's port, or for a run QuickAI sets up, its own
            form.local_port = match form.way {
                Way::SetUp => String::new(),
                Way::Attach => form.engine_port.clone(),
            };
        }
        ui.add_space(8.0);
        egui::Grid::new("computer_form")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| address_rows(ui, form, hosts));
        ui.add_space(4.0);
        egui::CollapsingHeader::new("More options: ports, key file")
            .id_salt("computer_more")
            .open(Some(form.more))
            .show(ui, |ui| {
                egui::Grid::new("computer_more_grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| more_rows(ui, form, keys));
            })
            .header_response
            .clicked()
            .then(|| form.more = !form.more);
        if let Some(problem) = &form.problem {
            ui.add_space(4.0);
            ui.colored_label(p.bad, problem);
        }
        ui.add_space(12.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = match (form.editing.is_some(), can) {
                    (false, true) => "Add and test",
                    (false, false) => "Add",
                    (true, true) => "Save and test",
                    (true, false) => "Save",
                };
                if theme::button_sized(ui, Role::Primary, true, label, egui::vec2(120.0, 30.0))
                    .clicked()
                {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if save {
            self.add_and_test(ui.ctx());
        } else if cancel {
            self.remote.form = None;
        }
    }

    /// A computer's test, step by step: reached over SSH, then the engine answering on its port there, each with what to
    /// do when it did not; and Use it, once it all passed.
    fn show_computer_test(&mut self, ui: &mut egui::Ui, id: &str) {
        let p = theme::palette();
        let Some(spec) = self.remote.remotes.iter().find(|r| r.id == id).cloned() else {
            self.remote.detail = None;
            return;
        };
        let can = self.usable_with_engine(&spec);
        let engine = self.engine_name();
        let port = self.remote_port(&spec);
        let testing = self
            .remote
            .checking
            .as_ref()
            .is_some_and(|(checking, _)| checking == id);
        let found = self.remote.checked.get(id).cloned();
        let in_use = self.in_use(id);
        let locked = self.runner_change_locked(Some(id));
        let lock_why = self.where_lock_reason();
        let mut action: Option<Action> = None;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&spec.name)
                    .size(18.0)
                    .family(theme::semibold(ui))
                    .color(p.text),
            );
            ui.label(theme::muted(spec.host.clone()));
        });
        ui.add_space(8.0);
        if !can {
            note(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "It is kept. {engine} runs on this computer only: choose {} to test it and use it.",
                        self.remote_engine_names("or")
                    ))
                    .small(),
                );
            });
        } else if testing {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::muted(format!("Testing {}…", spec.name)));
            });
        } else if let Some(found) = &found {
            match &found.reached {
                Ok(()) => step(ui, true, "Reached over SSH", |ui| {
                    ui.label(
                        theme::muted(format!("Logged in to {} with your key.", spec.host)).small(),
                    );
                }),
                Err(why) => step(
                    ui,
                    false,
                    &format!("Couldn't log in to {}", spec.host),
                    |ui| {
                        ui.label(egui::RichText::new(why).small());
                        if let Some(fix) = remote::login_fix(&spec, why) {
                            command_box(
                                ui,
                                &fix,
                                "Run this once on this computer, then test again:",
                            );
                        }
                    },
                ),
            }
            match &found.engine {
                // a computer QuickAI sets up: whether QuickAI can run there, in a sentence
                Some(Ok(said)) if spec.way == Way::SetUp => {
                    step(ui, true, "QuickAI can run there", |ui| {
                        ui.label(theme::muted(said.clone()).small());
                    })
                }
                Some(Err(why)) if spec.way == Way::SetUp => {
                    step(ui, false, "QuickAI can't run there", |ui| {
                        ui.label(egui::RichText::new(why).small());
                    })
                }
                Some(Ok(version)) => step(
                    ui,
                    true,
                    &format!("{engine} answers on port {port}"),
                    |ui| {
                        ui.label(theme::muted(format!("{engine} {version}")).small());
                    },
                ),
                Some(Err(_)) => step(
                    ui,
                    false,
                    &format!("{engine} isn't answering on port {port}"),
                    |ui| start_engine_help(ui, &self.engine, &engine, &spec.name),
                ),
                None => {}
            }
        } else {
            ui.label(theme::muted("Not tested yet."));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.link("All computers").clicked() {
                action = Some(Action::Back);
            }
            if ui.link("Edit…").clicked() {
                action = Some(Action::Edit(spec.clone()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let passed = found.as_ref().is_some_and(Checked::passed);
                if in_use {
                    theme::chip(ui, "in use", Some(Tone::Good));
                } else if theme::button_sized(
                    ui,
                    Role::Primary,
                    can && passed && !locked,
                    "Use it",
                    egui::vec2(84.0, 30.0),
                )
                .on_disabled_hover_text(if locked {
                    lock_why
                } else {
                    "Once every step passed."
                })
                .clicked()
                {
                    action = Some(Action::Use(Some(spec.id.clone())));
                }
                if ui
                    .add_enabled(can && !testing, egui::Button::new("Test again"))
                    .clicked()
                {
                    action = Some(Action::Check(spec.id.clone()));
                }
            });
        });
        if let Some(action) = action {
            self.act(action, ui.ctx());
        }
    }
}

/// The rows of the form for the address and the name: the hosts of `~/.ssh/config` that fit what is typed are offered
/// under the address while it is being typed in, and the name follows the address until it is typed.
fn address_rows(ui: &mut egui::Ui, form: &mut RemoteForm, hosts: &[SshHost]) {
    let p = theme::palette();
    ui.label("Address").on_hover_text(
        "A host name or IP address, user@host, or the name of a Host in your ~/.ssh/config.",
    );
    let before = form.host.clone();
    let address = ui.add(
        egui::TextEdit::singleline(&mut form.host)
            .hint_text("studio.local, me@192.168.1.40, or a Host of ~/.ssh/config")
            .desired_width(320.0),
    );
    if form.host != before && !form.name_typed {
        form.name = remote::name_from_address(&form.host);
    }
    ui.end_row();
    let typed = form.host.trim().to_lowercase();
    let fits: Vec<&SshHost> = hosts
        .iter()
        .filter(|h| h.alias.to_lowercase().contains(&typed) && h.alias != form.host.trim())
        .take(6)
        .collect();
    if !fits.is_empty() && (address.has_focus() || typed.is_empty()) {
        ui.label("");
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("From ~/.ssh/config")
                    .small()
                    .color(p.faint),
            );
            for host in fits {
                let text = egui::RichText::new(format!("{}   {}", host.alias, host.goes_to));
                if ui.selectable_label(false, text).clicked() {
                    form.host = host.alias.clone();
                    if !form.name_typed {
                        form.name = remote::name_from_address(&host.alias);
                    }
                }
            }
        });
        ui.end_row();
    }
    ui.label("Name");
    if ui
        .add(
            egui::TextEdit::singleline(&mut form.name)
                .hint_text("Studio PC")
                .desired_width(320.0),
        )
        .changed()
    {
        form.name_typed = !form.name.trim().is_empty();
    }
    ui.end_row();
}

/// How a run there is made: QuickAI sets it up (the default), or the window attaches to an engine the person runs
/// there.
fn way_choice(ui: &mut egui::Ui, way: &mut Way) {
    let p = theme::palette();
    ui.label(egui::RichText::new("How it runs there").color(p.text));
    for (choice, title, said) in [
        (
            Way::SetUp,
            "QuickAI sets it up",
            "It copies QuickAI's headless program there and runs the configuration you select. Nothing to install \
             there but SSH.",
        ),
        (
            Way::Attach,
            "Connect to an engine already running there",
            "Ollama or ComfyUI that you run there yourself.",
        ),
    ] {
        ui.horizontal_top(|ui| {
            if ui.radio(*way == choice, "").clicked() {
                *way = choice;
            }
            ui.vertical(|ui| {
                if ui
                    .add(egui::Label::new(egui::RichText::new(title).color(p.text)).sense(egui::Sense::click()))
                    .clicked()
                {
                    *way = choice;
                }
                ui.label(egui::RichText::new(said).small().color(p.muted));
            });
        });
    }
}

/// The rows of the form under More options: the SSH port, the engine's port there (for an engine the person runs
/// there), the port here (the same as the engine's until it is typed) and the key file, with the keys of `~/.ssh`
/// (`keys`) offered.
fn more_rows(ui: &mut egui::Ui, form: &mut RemoteForm, keys: &[PathBuf]) {
    ui.label("SSH port");
    ui.add(
        egui::TextEdit::singleline(&mut form.ssh_port)
            .hint_text("22")
            .desired_width(80.0),
    );
    ui.end_row();
    if form.way == Way::Attach {
        engine_port_row(ui, form);
    }
    port_here_row(ui, form);
    key_row(ui, form, keys);
}

/// The engine's port there.
fn engine_port_row(ui: &mut egui::Ui, form: &mut RemoteForm) {
    ui.label("Engine port").on_hover_text(
        "The port the engine listens on, on that computer: 11434 is Ollama's own, 8188 ComfyUI's.",
    );
    let before = form.engine_port.clone();
    ui.add(egui::TextEdit::singleline(&mut form.engine_port).desired_width(80.0));
    if form.engine_port != before && !form.here_typed {
        form.local_port = form.engine_port.clone();
    }
    ui.end_row();
}

/// The port here: where the tunnel brings the engine's port (for a computer QuickAI sets up, the first port its run
/// listens on), the same as there unless another is given.
fn port_here_row(ui: &mut egui::Ui, form: &mut RemoteForm) {
    let (there, hint) = match form.way {
        Way::SetUp => (
            "the run's first port there (its API)",
            "the same as there".to_string(),
        ),
        Way::Attach => ("the engine port", form.engine_port.trim().to_string()),
    };
    ui.label("Port here").on_hover_text(format!(
        "The port of this computer the engine is reached on, through the SSH tunnel: the address an app is given \
         (Connect an app). The same as {there} unless you give another; when that one is taken here, by an engine \
         of this computer, a free one is used."
    ));
    if ui
        .add(
            egui::TextEdit::singleline(&mut form.local_port)
                .hint_text(hint)
                .desired_width(80.0),
        )
        .changed()
    {
        form.here_typed = !form.local_port.trim().is_empty();
    }
    ui.end_row();
}

/// The key file, with the keys of `~/.ssh` (`keys`) offered and a native dialog for another.
fn key_row(ui: &mut egui::Ui, form: &mut RemoteForm, keys: &[PathBuf]) {
    ui.label("Key file").on_hover_text(
        "Only when ssh would not find the key by itself (your agent, ~/.ssh/id_*, ~/.ssh/config).",
    );
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut form.identity)
                .hint_text("optional")
                .desired_width(240.0),
        );
        let mut browse = false;
        ui.menu_button("…", |ui| {
            for key in keys {
                let name = key.file_name().unwrap_or_default().to_string_lossy();
                if ui
                    .button(name)
                    .on_hover_text(key.display().to_string())
                    .clicked()
                {
                    form.identity = key.display().to_string();
                    form.problem = None;
                }
            }
            if keys.is_empty() {
                ui.label(theme::muted("No keys in ~/.ssh"));
            }
            ui.separator();
            browse = ui.button("Other file…").clicked();
        })
        .response
        .on_hover_text("A key of ~/.ssh, or another file");
        if browse {
            match browse_key(&form.identity) {
                Picked::File(path) => form.identity = path.display().to_string(),
                Picked::NoDialog => {
                    form.problem = Some(
                        "No file dialog could open: this computer has no desktop portal and no zenity to show one. \
                         Type the key's path in the box."
                            .into(),
                    )
                }
                Picked::Closed => {}
            }
        }
    });
    ui.end_row();
}

/// A native dialog for a key file, opened in the folder of the one given, else in `~/.ssh` (a hidden folder, which a
/// dialog does not show).
fn browse_key(given: &str) -> Picked {
    let mut dialog = rfd::FileDialog::new().set_title("Key file");
    let folder = std::path::Path::new(given.trim())
        .parent()
        .filter(|dir| dir.is_dir())
        .map(PathBuf::from)
        .or_else(|| remote::ssh_dir().filter(|dir| dir.is_dir()));
    if let Some(dir) = folder {
        dialog = dialog.set_directory(dir);
    }
    super::widgets::pick_file(dialog)
}

/// Whether `engine` can run on another computer: its document can attach to one that is already running, and says
/// which way to start in a field (`mode`).
fn runs_elsewhere(engine: &Engine) -> bool {
    engine.doc.launch.attach.is_some() && engine.doc.launch.strategy_field.is_some()
}

/// A row of the menu of the status bar's place: chosen or not, a computer's name and its line under it, and whether it
/// answered as a dot at its right end. It can be clicked only when `enabled`.
fn where_row(
    ui: &mut egui::Ui,
    on: bool,
    enabled: bool,
    name: &str,
    line: &str,
    dot: Option<bool>,
) -> egui::Response {
    let p = theme::palette();
    let group = ui
        .add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                let _ = ui.radio(on, "");
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(name).color(p.text));
                    ui.label(egui::RichText::new(line).small().color(p.faint));
                });
                if let Some(answers) = dot {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        theme::dot(ui, 8.0, 3.5, if answers { p.ok } else { p.bad });
                    });
                }
            })
            .response
        })
        .inner;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    ui.interact(group.rect, group.id.with("row"), sense)
}

/// A row of the list of the Computers window: this computer (no `spec`) or one that was added.
struct ComputerRow<'a> {
    spec: Option<&'a RemoteSpec>,
    /// Where it is and what its last test found.
    line: String,
    /// Whether it answered, when that is known.
    answers: Option<bool>,
    /// The models run on it.
    on: bool,
    /// It can be removed now.
    removable: bool,
}

/// A row of the list: a dot of whether it answered, its name (and `in use`) over its line, Use when it can be used
/// (`usable`) and is not, and a computer's Edit and its menu: Test (when the engine `can` run elsewhere) and Remove.
/// What was clicked comes back.
fn computer_row(
    ui: &mut egui::Ui,
    row: &ComputerRow,
    can: bool,
    usable: bool,
    locked: bool,
    lock_why: &str,
) -> Option<Action> {
    let p = theme::palette();
    let mut action = None;
    let fill = if row.on {
        p.accent_soft
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let color = match row.answers {
                    Some(true) => p.ok,
                    Some(false) => p.bad,
                    None => p.faint,
                };
                theme::dot(ui, 10.0, 4.0, color);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let name = row.spec.map_or("This computer", |s| s.name.as_str());
                        ui.label(egui::RichText::new(name).strong().color(p.text));
                        if row.on {
                            theme::chip(ui, "in use", Some(Tone::Good));
                        }
                    });
                    ui.label(egui::RichText::new(&row.line).small().color(p.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match row.spec {
                        Some(spec) => {
                            if let Some(asked) = computer_menu(ui, spec, can, row.removable) {
                                action = Some(asked);
                            }
                            if icons::icon_button(ui, Icon::Pencil, ROW_BUTTON, true, false)
                                .on_hover_text("Edit: its address, name, ports and key file")
                                .clicked()
                            {
                                action = Some(Action::Edit(spec.clone()));
                            }
                        }
                        // this computer's Use in line with the others'
                        None => ui.add_space(2.0 * (ROW_BUTTON.x + ui.spacing().item_spacing.x)),
                    }
                    if !row.on
                        && ui
                            .add_enabled(usable, egui::Button::new("Use"))
                            .on_disabled_hover_text(if locked {
                                lock_why
                            } else {
                                "This engine runs on this computer only."
                            })
                            .clicked()
                    {
                        action = Some(Action::Use(row.spec.map(|s| s.id.clone())));
                    }
                });
            });
        });
    action
}

/// The size of the picture buttons of a computer's row.
const ROW_BUTTON: egui::Vec2 = egui::vec2(28.0, 24.0);

/// The … of a computer of the list and its menu: Test and Remove. What was chosen comes back.
fn computer_menu(
    ui: &mut egui::Ui,
    spec: &RemoteSpec,
    can: bool,
    removable: bool,
) -> Option<Action> {
    let mut action = None;
    let more = icons::icon_button(ui, Icon::More, ROW_BUTTON, true, false)
        .on_hover_text("Test or remove it");
    egui::Popup::menu(&more)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            if ui.add_enabled(can, egui::Button::new("Test")).clicked() {
                action = Some(Action::Check(spec.id.clone()));
            }
            if ui
                .add_enabled(removable, egui::Button::new("Remove"))
                .on_disabled_hover_text("Stop the server to remove the computer it runs on.")
                .clicked()
            {
                action = Some(Action::Remove(spec.id.clone()));
            }
        });
    action
}

/// A step of a computer's test: a tick or a cross, what it is, and under it what `add` says of it.
fn step(ui: &mut egui::Ui, ok: bool, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette();
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        let icon = if ok { Icon::Check } else { Icon::Close };
        icons::paint(ui.painter(), rect, icon, if ok { p.ok } else { p.bad });
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).strong().color(p.text));
            add(ui);
        });
    });
    ui.add_space(4.0);
    ui.separator();
}

/// What to do when the engine (`id`, called `name`) is not answering on the computer `computer`: how to start it there.
fn start_engine_help(ui: &mut egui::Ui, id: &str, name: &str, computer: &str) {
    let p = theme::palette();
    ui.label(
        egui::RichText::new(format!(
            "It isn't running on {computer}, or it listens on another port (More options, under Edit)."
        ))
        .small(),
    );
    if id == "ollama" {
        command_box(
            ui,
            "ollama serve",
            &format!("On {computer}, start it with:"),
        );
        ui.label(
            egui::RichText::new(
                "or, to keep it running after a restart: sudo systemctl enable --now ollama",
            )
            .small()
            .color(p.faint),
        );
    } else {
        ui.label(
            egui::RichText::new(format!(
                "On {computer}, start it with QuickAI's headless program and a configuration of {name}: \
                 quickai-run up <configuration>"
            ))
            .small(),
        );
    }
}

/// An item of a menu: its picture and words at the left, the width of the menu, lit when the pointer is on it.
fn menu_item(ui: &mut egui::Ui, icon: Icon, text: &str) -> egui::Response {
    let p = theme::palette();
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter().rect_filled(rect, 4.0, p.raised_hover);
        }
        let color = if response.hovered() { p.text } else { p.body };
        let square = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 8.0, rect.center().y - 7.0),
            egui::vec2(14.0, 14.0),
        );
        icons::paint(ui.painter(), square, icon, color);
        ui.painter().text(
            egui::pos2(square.right() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::TextStyle::Button.resolve(ui.style()),
            color,
        );
    }
    response
}

/// A note that is neither good nor bad news: why something cannot be done here.
fn note(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette();
    egui::Frame::new()
        .fill(p.raised)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// A command to run, in a box with a copy button, under the sentence that says where.
fn command_box(ui: &mut egui::Ui, command: &str, said: &str) {
    let p = theme::palette();
    ui.label(egui::RichText::new(said).small());
    theme::well_frame(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(command).monospace().color(p.text));
            icons::copy_button(ui, command, command);
        });
    });
}

/// What a click in the window of the computers asked for.
enum Action {
    New,
    Back,
    Edit(RemoteSpec),
    Remove(String),
    Check(String),
    Use(Option<String>),
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
        assert!(why.why().contains("refused the login"), "{why:?}");
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
