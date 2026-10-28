//! The program that goes with the engine in use: Open WebUI beside Ollama (docs/openwebui.md, decisions
//! C89 to C97). It is a document of an engine in every way but one (`[engine] companion_of`), so everything
//! the window does for an engine it does for it: the values of its fields, the program that was found, the
//! manager of its builds, a run of its own. What is here is what is *between* the two: which of them
//! is started when, what the window says of the pair, and where the settings of the companion are drawn.
//!
//! **A run of its own.** The companion has a `Session` beside the engine's. It is started, once in a run of the
//! engine's, as soon as the engine is Running (Ollama answers, whether QuickAI started it or found it running)
//! and the person wants it and it is installed: when the run reaches Running, or later in the run at the moment
//! it becomes both (the box is ticked, or its install ends). It is stopped when the engine's run ends, however
//! it ends, and when it stops unasked it is the only thing that has: the engine's run goes on, the window says
//! that it stopped, and it is not started again until the next run.
//!
//! **The manager of its builds is the engine's.** The runtime manager is written for "the engine in use": its
//! sentences, its builds, its install, its delete and its look-up read `engine`, `values`, `binary`,
//! `runtime`, `host_facts` and the like of the window. Rather than write a second one, the window holds
//! those things for the companion in a `Slot` and, for the length of one call (`as_companion`), puts them
//! where the manager reads them and takes them back. Two things are not the slot's and are handled where
//! they are: a settings file is written once, whole, when the call is over (`save_settings` only says that it
//! wants to be written while the slot is in), and a delete that was asked in the companion's section is
//! answered in it (`delete_of_companion`).

use std::path::PathBuf;

use eframe::egui;
use quickai_core::form::{Edit, FormField};
use quickai_core::plan::{HostFacts, Parent, RunPlan, Selection};

use super::runtime_manager::RuntimeManager;
use super::values::FieldValues;
use super::widgets::{self, Reply};
use super::App;
use crate::events::ServerState;
use crate::launch;
use crate::runtime::Session;

/// What the window holds for one engine: the part of its state that depends on which engine it is for.
pub(super) struct Slot {
    pub engine: String,
    pub values: FieldValues,
    pub binary: PathBuf,
    pub runtime: RuntimeManager,
    pub host_facts: HostFacts,
    pub missing_binary_warned: bool,
    /// The run reached Running (a stop after it is not a failure to start).
    pub run_reached_running: bool,
}

/// The companion of the engine in use.
pub struct Companion {
    /// The id of its document (`openwebui`).
    pub id: String,
    /// What the window holds for it; `None` only while `as_companion` has it in the window.
    slot: Option<Slot>,
    /// Its own run.
    pub session: Session,
    /// It was started (or refused) for the run of the engine that is going: it is not started again until that
    /// run is over.
    started_for_run: bool,
    /// That it is not installed was said in the log for the run that is going (once: the person may install it
    /// while the engine runs, and then it is started).
    said_not_installed: bool,
}

impl Companion {
    /// The values the window holds for it, when they are not the ones in the window's own state.
    pub(super) fn slot_values(&self) -> Option<&FieldValues> {
        self.slot.as_ref().map(|s| &s.values)
    }
}

/// What the chat button of the run bar is, for an engine whose chat page is a companion's.
pub(super) struct ChatButton {
    pub label: String,
    pub enabled: bool,
    pub hover: String,
    /// What a click does.
    pub action: ChatAction,
}

pub(super) enum ChatAction {
    /// Open the page of the companion in the browser.
    Open(String),
    /// Open the Settings, where the companion is set up.
    SetUp,
}

impl App {
    /// The companion of the engine in use, as the settings hold it: `None` for an engine that has none.
    pub(super) fn build_companion(&self) -> Option<Companion> {
        let engine = self.registry.companions_of(&self.engine).next()?;
        let id = engine.id.clone();
        let values = FieldValues::from_settings(&self.stored, Some(engine));
        let binary = launch::locate_binary_in(
            &self.registry,
            &id,
            values.text("bin_override"),
            self.data_dir_override.as_deref(),
        );
        let mut host_facts = launch::host_facts(&binary, self.machine.clone(), None);
        if let Some(dir) = &self.data_dir_override {
            host_facts.data_dir = dir.clone();
        }
        let runtime = RuntimeManager {
            input: values
                .text("bin_override")
                .map(str::to_string)
                .unwrap_or_default(),
            ..Default::default()
        };
        Some(Companion {
            id: id.clone(),
            slot: Some(Slot {
                engine: id,
                values,
                binary,
                runtime,
                host_facts,
                missing_binary_warned: false,
                run_reached_running: false,
            }),
            session: Session::new(),
            started_for_run: false,
            said_not_installed: false,
        })
    }

    fn swap_slot(&mut self, slot: &mut Slot) {
        std::mem::swap(&mut self.engine, &mut slot.engine);
        std::mem::swap(&mut self.values, &mut slot.values);
        std::mem::swap(&mut self.binary, &mut slot.binary);
        std::mem::swap(&mut self.runtime, &mut slot.runtime);
        std::mem::swap(&mut self.host_facts, &mut slot.host_facts);
        std::mem::swap(
            &mut self.missing_binary_warned,
            &mut slot.missing_binary_warned,
        );
        std::mem::swap(&mut self.run_reached_running, &mut slot.run_reached_running);
    }

    /// Runs `f` with the window's state for an engine being the companion's, as the head of this file says.
    /// `None` when the engine in use has no companion (or the call is already inside one).
    pub(super) fn as_companion<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> Option<R> {
        if self.in_companion {
            return None;
        }
        let mut slot = self.companion.as_mut()?.slot.take()?;
        self.swap_slot(&mut slot);
        self.in_companion = true;
        let result = f(self);
        self.in_companion = false;
        self.swap_slot(&mut slot);
        if let Some(companion) = self.companion.as_mut() {
            companion.slot = Some(slot);
        }
        // what was changed while the slot was in is written once, with both engines' values in it
        if self.save_wanted.take() {
            self.save_settings();
        }
        Some(result)
    }

    /// What the chat button does for a companion that is not set up: the Settings, with its section open.
    pub(super) fn set_up_companion(&mut self) {
        self.companion_section_wanted = true;
        self.open_settings();
    }

    /// The document of the companion.
    pub(super) fn companion_doc(&self) -> Option<&quickai_core::registry::Engine> {
        let id = &self.companion.as_ref()?.id;
        self.registry.engine(id)
    }

    /// What the window calls it (`Open WebUI`).
    pub(super) fn companion_name(&self) -> Option<String> {
        self.companion_doc().map(|e| e.doc.engine.name.clone())
    }

    pub(super) fn companion_slot(&self) -> Option<&Slot> {
        self.companion.as_ref()?.slot.as_ref()
    }

    /// Its state, as its own run has it.
    pub(super) fn companion_state(&self) -> Option<ServerState> {
        self.companion.as_ref().map(|c| c.session.state())
    }

    /// Whether the person has it start with the engine (the `enabled` field).
    pub(super) fn companion_enabled(&self) -> bool {
        self.companion_slot()
            .and_then(|s| s.values.get("enabled"))
            .is_some_and(quickai_core::value::Value::truthy)
    }

    /// Whether its program is there to be started.
    pub(super) fn companion_installed(&self) -> bool {
        self.companion_slot().is_some_and(|s| s.binary.is_file())
    }

    /// Something of the companion is being fetched or removed: it is not the time to change the engine.
    pub(super) fn companion_busy(&self) -> bool {
        self.companion.as_ref().is_some_and(|c| {
            c.session.is_active()
                || c.slot
                    .as_ref()
                    .is_some_and(|s| s.runtime.install.is_some() || s.runtime.delete.is_some())
        })
    }

    /// What is going on in the companion's manager that the window's watcher should know about.
    pub(super) fn companion_running_now(&self) -> Option<&'static str> {
        let slot = self.companion_slot()?;
        if slot.runtime.install.is_some() {
            Some("an install")
        } else if slot.runtime.delete.is_some() {
            Some("a delete")
        } else if slot.runtime.lookup_rx.is_some() {
            Some("a look-up")
        } else {
            None
        }
    }

    /// The port its page is on: the one its settings give.
    fn companion_port(&self) -> Option<u16> {
        let engine = self.companion_doc()?;
        self.companion_slot()?
            .values
            .int("port")
            .and_then(|n| u16::try_from(n).ok())
            .or(engine.doc.provides.default_port)
    }

    /// The address of its page while it runs.
    pub(super) fn companion_url(&self) -> Option<String> {
        let companion = self.companion.as_ref()?;
        let web_ui = &self.companion_doc()?.doc.provides.web_ui.as_ref()?.path;
        let port = companion.session.port()?;
        (companion.session.state() == ServerState::Running).then(|| {
            // the page at the root is the address without a path, as it always was
            let path = if web_ui == "/" { "" } else { web_ui.as_str() };
            format!("http://127.0.0.1:{port}{path}")
        })
    }

    /// What the chat button of the run bar is for an engine that has no page of its own and whose companion
    /// has: the page when the companion runs, and the way to set it up before.
    pub(super) fn chat_button(&self) -> Option<ChatButton> {
        let web_ui = self.companion_doc()?.doc.provides.web_ui.as_ref()?;
        let name = self.companion_name()?;
        let server = self.server_name();
        if !self.companion_enabled() || !self.companion_installed() {
            return Some(ChatButton {
                label: "Set up chat UI...".to_string(),
                enabled: true,
                hover: format!(
                    "{name} is a chat page for {server}'s models. Set it up in Settings; nothing is downloaded until you ask."
                ),
                action: ChatAction::SetUp,
            });
        }
        let url = self.companion_url();
        Some(ChatButton {
            label: web_ui.label.clone(),
            enabled: url.is_some(),
            hover: if url.is_some() {
                format!(
                    "{name} is the chat page for {server}'s models: this opens it in your browser."
                )
            } else if self.companion_state() == Some(ServerState::Crashed) {
                format!("{name} stopped unexpectedly. It starts again the next time {server} is started.")
            } else {
                format!("{name} starts with {server}; this opens its page when it is running.")
            },
            action: ChatAction::Open(url.unwrap_or_default()),
        })
    }

    /// What the status line adds about the companion while the engine is up: where its page is, that it is
    /// starting, or that it stopped. `None` when there is nothing to say (it is not used, or the run is over).
    pub(super) fn companion_status(&self) -> Option<(String, ServerState)> {
        let companion = self.companion.as_ref()?;
        let name = self.companion_name()?;
        let state = companion.session.state();
        let text = match (state, companion.session.port()) {
            (ServerState::Starting, _) => format!("{name} starting..."),
            (ServerState::Running, Some(port)) => format!("{name} on port {port}"),
            (ServerState::Running, None) => format!("{name} running"),
            (ServerState::Crashed, _) => format!("{name} stopped unexpectedly"),
            (ServerState::Stopping, _) => format!("{name} stopping..."),
            (ServerState::Idle, _) => return None,
        };
        Some((text, state))
    }

    /// Each frame: what the companion said, its builds, and whether it is time to start or stop it.
    pub(super) fn poll_companion(&mut self, ctx: &egui::Context) {
        if self.companion.is_none() {
            return;
        }
        let name = self.companion_name().unwrap_or_default();
        let mut said = Vec::new();
        if let Some(companion) = self.companion.as_mut() {
            companion
                .session
                .poll(&mut |line| said.push(line), &mut |_| {});
        }
        for line in said {
            self.push_log(format!("[{name}] {line}"));
        }
        self.as_companion(|app| app.poll_runtime_manager());
        self.steer_companion(ctx);
    }

    /// Starts the companion once the engine runs and the person wants it and it is there (once in a run), and
    /// stops it when that run is over or going.
    fn steer_companion(&mut self, ctx: &egui::Context) {
        let main = self.session.state();
        let Some(companion) = self.companion.as_mut() else {
            return;
        };
        if companion.session.state() == ServerState::Running {
            if let Some(slot) = companion.slot.as_mut() {
                slot.run_reached_running = true;
            }
        }
        match main {
            ServerState::Running => {
                // not while it is up, and not again in a run in which it was started and has ended
                let waiting = !companion.started_for_run && !companion.session.is_active();
                if waiting && self.companion_enabled() {
                    self.start_companion(ctx);
                }
            }
            ServerState::Starting => {}
            ServerState::Stopping | ServerState::Idle | ServerState::Crashed => {
                // once, not at every frame of a stop that takes a while
                if matches!(
                    companion.session.state(),
                    ServerState::Starting | ServerState::Running
                ) {
                    companion.session.stop();
                }
                if main != ServerState::Stopping {
                    companion.started_for_run = false;
                    companion.said_not_installed = false;
                }
            }
        }
    }

    /// The plan for the companion's run: its own document and values, beside the engine that is running (its
    /// port is the one the companion's templates call `${parent.port}`).
    fn companion_plan(&self) -> Result<RunPlan, Vec<quickai_core::diag::Diagnostic>> {
        let slot = self.companion_slot().ok_or_else(Vec::new)?;
        let selection: Selection =
            launch::selection_from_values(&slot.engine, None, slot.values.map(), false);
        let mut facts = launch::host_facts(&slot.binary, self.machine.clone(), None);
        if let Some(dir) = &self.data_dir_override {
            facts.data_dir = dir.clone();
        }
        facts.parent = Some(Parent {
            id: self.engine.clone(),
            port: self.port(),
        });
        quickai_core::plan::plan(&self.registry, &selection, &facts)
    }

    /// Starts the companion for the run that is going. A companion that is not installed, or whose plan
    /// is wrong, is said in the log and the engine's run goes on without it. One that is not installed is
    /// said once and waited for (the person may install it while the engine runs); one that is refused or
    /// whose plan is wrong is the run's last word on it.
    fn start_companion(&mut self, ctx: &egui::Context) {
        let Some(name) = self.companion_name() else {
            return;
        };
        let port = self.companion_port().unwrap_or(0);
        let wake = {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        if !self.companion_installed() {
            let said = self
                .companion
                .as_ref()
                .is_some_and(|c| c.said_not_installed);
            if !said {
                self.push_log(format!(
                    "[{name}] not started: it is not installed. Open Settings and install it; nothing is downloaded until you do."
                ));
                if let Some(companion) = self.companion.as_mut() {
                    companion.said_not_installed = true;
                }
            }
            return;
        }
        if let Some(companion) = self.companion.as_mut() {
            companion.started_for_run = true;
        }
        match self.companion_plan() {
            Ok(plan) => {
                // a program that a document of the person's names, and the application did not find, is
                // theirs to allow: the question is asked for an engine's start, and a companion that would
                // need it is not started
                if self
                    .as_companion(|app| app.foreign_program(&plan))
                    .flatten()
                    .is_some()
                {
                    self.push_log(format!(
                        "[{name}] not started: a document of your own names a program QuickAI did not install and did not find. Start {} from the Settings it was set up in, or remove that document.",
                        self.server_name()
                    ));
                    return;
                }
                if let Some(slot) = self.companion.as_mut().and_then(|c| c.slot.as_mut()) {
                    slot.run_reached_running = false;
                }
                self.keep_the_first_address(&name);
                if let Some(companion) = self.companion.as_mut() {
                    companion.session.start(&plan, "", wake);
                }
            }
            Err(problems) => {
                let message = problems
                    .iter()
                    .map(|d| format!("[{name}] {d}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(companion) = self.companion.as_mut() {
                    companion
                        .session
                        .report_start_failure(port, "", message, wake);
                }
            }
        }
    }

    /// The address of the engine the companion was first started with, when it was. It keeps that one in a
    /// database of its own and does not read the environment for it again, so that is the address it calls
    /// until the person changes it in its own pages.
    pub(super) fn first_address(&self) -> Option<String> {
        let field = self.first_address_field()?;
        self.companion_slot()?
            .values
            .text(&field)
            .map(str::to_string)
            .filter(|a| !a.is_empty())
    }

    /// The field its document keeps that address in, when the document says (`first_address_field`).
    fn first_address_field(&self) -> Option<String> {
        let companion = self.companion.as_ref()?;
        self.registry
            .engine(&companion.id)?
            .doc
            .engine
            .first_address_field
            .clone()
    }

    /// Remembers the address the companion is started with the first time, and says in the log, every time
    /// it is started, when the engine is somewhere else now (the companion goes on calling the first).
    fn keep_the_first_address(&mut self, name: &str) {
        let Some(field) = self.first_address_field() else {
            return;
        };
        let now = quickai_core::plan::parent_url(self.port());
        match self.first_address() {
            None => {
                if let Some(slot) = self.companion.as_mut().and_then(|c| c.slot.as_mut()) {
                    slot.values.set(&field, now);
                }
                self.save_settings();
            }
            Some(first) if first != now => {
                let note = quickai_core::profile::notes::Note::new(
                    "companion_address",
                    &[
                        ("companion", name.to_string()),
                        ("engine", self.engine_name()),
                        ("first", first),
                        ("now", now),
                    ],
                );
                self.push_log(format!(
                    "[{name}] {}",
                    note.text(self.registry.vocabulary())
                ));
            }
            Some(_) => {}
        }
    }

    /// Stops the companion, when the person stops the engine.
    pub(super) fn stop_companion(&mut self) {
        if let Some(companion) = self.companion.as_mut() {
            companion.session.stop();
        }
    }

    /// Stops it and waits, for the application's exit.
    pub(super) fn stop_companion_blocking(&mut self) {
        if let Some(companion) = self.companion.as_mut() {
            companion.session.stop_blocking();
        }
    }

    /// The companion's section of the Settings: whether it starts with the engine, its port and network, its
    /// builds, and what is less used.
    pub(super) fn show_companion_settings(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.companion_name() else {
            return;
        };
        let (summary, license, homepage) = {
            let info = &self.companion_doc().map(|e| &e.doc.engine);
            (
                info.and_then(|i| i.summary.clone()).unwrap_or_default(),
                info.and_then(|i| i.license.clone()),
                info.and_then(|i| i.homepage.clone()),
            )
        };
        let installed = self.companion_installed();
        let enabled = self.companion_enabled();
        ui.add_space(6.0);
        // open while it is the thing to do: it is wanted and not there yet, or the person pressed the button that
        // sets it up
        let header = egui::CollapsingHeader::new(format!("Chat UI: {name}"))
            .id_salt("companion_settings")
            .default_open(enabled && !installed);
        let header = if std::mem::take(&mut self.companion_section_wanted) {
            header.open(Some(true))
        } else {
            header
        };
        super::theme::card(ui, |ui| {
            header.show(ui, |ui| {
                if !summary.is_empty() {
                    ui.label(
                        egui::RichText::new(summary)
                            .small()
                            .color(super::theme::palette().muted),
                    );
                }
                let line = match (license, homepage) {
                    (Some(license), Some(home)) => Some(format!("License: {license} · {home}")),
                    (Some(license), None) => Some(format!("License: {license}")),
                    (None, Some(home)) => Some(home),
                    (None, None) => None,
                };
                if let Some(line) = line {
                    ui.label(
                        egui::RichText::new(line)
                            .small()
                            .color(super::theme::palette().muted),
                    );
                }
                ui.add_space(4.0);
                self.as_companion(|app| {
                    let form = app.form();
                    let section = |id: &str| -> Vec<FormField> {
                        form.as_ref()
                            .and_then(|f| f.section(id))
                            .map(|s| s.fields.iter().filter(|f| f.visible).cloned().collect())
                            .unwrap_or_default()
                    };
                    app.show_fields(ui, &section("primary"), None);
                    ui.add_space(8.0);
                    // the manager of its builds is the field that is the program: where it is, and how to get it
                    let advanced = section("advanced");
                    for field in advanced
                        .iter()
                        .filter(|f| f.widget.as_deref() == Some("runtime_manager"))
                    {
                        app.show_runtime_manager(ui, field);
                    }
                    let others: Vec<FormField> = advanced
                        .iter()
                        .filter(|f| f.widget.as_deref() != Some("runtime_manager"))
                        .cloned()
                        .collect();
                    if !others.is_empty() {
                        ui.add_space(8.0);
                        egui::CollapsingHeader::new("More")
                            .id_salt("companion_more")
                            .default_open(false)
                            .show(ui, |ui| app.show_fields(ui, &others, None));
                    }
                });
            });
        });
    }

    /// Draws fields of the engine whose values the window holds, one after the other, and applies what the
    /// person did to them. `owner` is the name to say of a field that only a spawn can set while the program
    /// that was running was not started here (`None`: there is nothing to say).
    pub(super) fn show_fields(
        &mut self,
        ui: &mut egui::Ui,
        fields: &[FormField],
        owner: Option<&str>,
    ) {
        let mut edits: Vec<Edit> = Vec::new();
        let mut actions = Vec::new();
        for (i, field) in fields.iter().enumerate() {
            if i > 0 {
                ui.add_space(8.0);
            }
            if field.widget.as_deref() == Some("runtime_manager") {
                self.show_runtime_manager(ui, field);
                continue;
            }
            // a checkbox and a button carry their own label
            let own_label = field.ty == quickai_core::doc::engine::FieldType::Bool
                || field.widget.as_deref() == Some("button");
            // a choice is a short box: it is beside its label, where a path or a text, which want the width of the window,
            // are under theirs
            let beside = field.ty == quickai_core::doc::engine::FieldType::Choice && !own_label;
            let draw_label = |ui: &mut egui::Ui| {
                let label = ui.label(format!("{}:", field.label));
                if let Some(help) = &field.help {
                    label.on_hover_text(help);
                }
            };
            let reply = if beside {
                ui.horizontal(|ui| {
                    draw_label(ui);
                    ui.add_enabled_ui(field.enabled, |ui| widgets::show(ui, field))
                        .inner
                })
                .inner
            } else {
                if !own_label {
                    draw_label(ui);
                }
                ui.add_enabled_ui(field.enabled, |ui| widgets::show(ui, field))
                    .inner
            };
            match reply {
                Some(Reply::Set(value)) => edits.push(Edit {
                    key: field.key.clone(),
                    value,
                }),
                Some(Reply::Invoke(action)) => actions.push(action),
                None => {}
            }
            // what only a spawn can set belongs to whoever started an engine that was already running
            if let Some(owner) = owner {
                if self.session.attached()
                    && field.applies == quickai_core::doc::engine::Applies::SpawnOnly
                {
                    ui.label(
                        egui::RichText::new(format!("(Owned by the running {owner}.)"))
                            .small()
                            .color(super::theme::palette().muted),
                    );
                }
            }
        }
        self.values.apply(edits);
        for action in actions {
            self.invoke(&action);
        }
    }

    /// The panel for a companion that stopped: what happened, and the advice its document has for what the
    /// log says. The engine's run is going on, and the panel says so.
    pub(super) fn show_companion_crash(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.companion_name() else {
            return;
        };
        let server = self.companion_doc().map_or_else(
            || name.clone(),
            |e| {
                e.doc
                    .engine
                    .server_name
                    .clone()
                    .unwrap_or_else(|| name.clone())
            },
        );
        ui.add_space(6.0);
        self.as_companion(|app| app.show_crash_panel(ui, ServerState::Crashed, &server));
    }
}

#[cfg(test)]
mod tests {
    //! Group T of the test plan, the part that is the window's (work package 4.16): the companion beside
    //! a running Ollama, as the application starts it, shows it and stops it. Ollama is the fake daemon of
    //! the other window tests; Open WebUI is a script that writes down how it was started, with a page that
    //! answers `/health` where its settings say it is.

    use std::collections::BTreeMap;
    use std::path::Path;
    #[cfg(unix)]
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use quickai_core::value::Value;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::settings::Settings;
    use crate::testkit::fake_ollama::FakeOllama;
    #[cfg(unix)]
    use crate::testkit::files::FileServer;
    #[cfg(unix)]
    use crate::testkit::procs::{gone_within, pid_alive, read_pid, write_script};
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    fn put_values(settings: &mut Settings, engine: &str, values: &[(&str, Value)]) {
        let registry = build_registry(&ConfigDirs::default());
        let values: BTreeMap<String, Value> = values
            .iter()
            .map(|(k, v)| (format!("{engine}.{k}"), v.clone()))
            .collect();
        settings
            .set_engine_values(registry.engine(engine).unwrap(), &values)
            .unwrap();
    }

    /// An application with Ollama as its engine (attached to `ollama_port`), and the companion's values.
    fn app_over(dir: &Path, ollama_port: u16, companion: &[(&str, Value)]) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        put_values(
            &mut settings,
            "ollama",
            &[
                ("port", Value::Int(ollama_port as i64)),
                ("mode", Value::Str("attach".into())),
            ],
        );
        put_values(&mut settings, "openwebui", companion);
        let mut app = App::build(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
        );
        app.data_dir_override = Some(dir.join("data").display().to_string());
        app.load_engine_state();
        app
    }

    /// The program of the companion: it writes down what it was started with, and then waits.
    #[cfg(unix)]
    fn launcher(dir: &Path, then: &str) -> PathBuf {
        let record = dir.join("record");
        write_script(
            dir,
            "open-webui",
            &format!(
                "{{\n echo \"cwd=$(pwd)\"\n echo \"args=$*\"\n env | grep -E '^(DATA_DIR|STATIC_DIR|XDG_CACHE_HOME|HF_HOME|HOME|TMPDIR|OLLAMA_BASE_URL|ENABLE_OLLAMA_API|OFFLINE_MODE|CORS_ALLOW_ORIGIN|PYTHONPATH|PYTHONHOME|PYTHONNOUSERSITE)='\n}} > '{}'\necho $$ > '{}'\n{then}\n",
                record.display(),
                dir.join("pid").display()
            ),
        )
    }

    #[cfg(unix)]
    fn record(dir: &Path) -> BTreeMap<String, String> {
        wait_for(&dir.join("record"));
        std::fs::read_to_string(dir.join("record"))
            .unwrap()
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[cfg(unix)]
    fn wait_for(path: &Path) {
        crate::testkit::wait_until(
            Duration::from_secs(10),
            &format!("{}", path.display()),
            || path.is_file() && std::fs::metadata(path).is_ok_and(|m| m.len() > 0),
        );
    }

    /// The page of the companion: answers `/health` on a port of its own.
    #[cfg(unix)]
    fn page() -> (FileServer, u16) {
        let server = FileServer::start();
        server.add("/health", br#"{"status":true}"#.to_vec());
        let port: u16 = server.base().rsplit(':').next().unwrap().parse().unwrap();
        (server, port)
    }

    /// One frame's worth of what the application does between draws: what the sessions said, the builds, and
    /// the companion.
    #[cfg(unix)]
    fn frame(app: &mut App, ctx: &egui::Context) {
        let mut said = Vec::new();
        app.session.poll(&mut |line| said.push(line), &mut |_| {});
        for line in said {
            app.push_log(line);
        }
        app.poll_runtime_manager();
        app.poll_companion(ctx);
        app.poll_named_models(ctx);
    }

    #[cfg(unix)]
    fn until(app: &mut App, ctx: &egui::Context, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(app) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; log: {:#?}",
                app.log_lines
            );
            frame(app, ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Starts the engine as the Start button does, with no model: it attaches to the fake daemon.
    #[cfg(unix)]
    fn start_engine(app: &mut App) {
        let selection = launch::selection_from_values("ollama", None, app.values.map(), false);
        let plan = launch::plan_selection(
            &app.registry,
            &selection,
            Path::new("/nowhere/ollama"),
            app.machine.clone(),
            None,
        )
        .unwrap_or_else(|e| panic!("{e:#?}"));
        app.session.start(&plan, "", || {});
    }

    #[cfg(unix)]
    fn painted(ctx: &egui::Context, show: impl FnOnce(&mut egui::Ui)) -> String {
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => into.push(t.galley.text().to_string()),
                _ => {}
            }
        }
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let mut show = Some(show);
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(show) = show.take() {
                    show(ui);
                }
            });
        });
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut texts);
        }
        texts.join("\n")
    }

    #[test]
    #[cfg(unix)]
    fn p3_t30_the_companion_is_started_when_the_engine_answers_in_its_own_folder_and_stopped_with_it(
    ) {
        let dir = scratch("t30");
        let fake = FakeOllama::start();
        let (_page, port) = page();
        let program = launcher(&dir, "exec sleep 60");
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(true)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        let ctx = egui::Context::default();
        assert!(app.companion.is_some(), "Ollama has a companion");
        // before anything runs: nothing of it is started or said, and the button waits for it
        frame(&mut app, &ctx);
        assert_eq!(app.companion_state(), Some(ServerState::Idle));
        assert!(app.companion_status().is_none());
        let button = app.chat_button().expect("a chat button for Ollama");
        assert_eq!(button.label, "Open Chat UI");
        assert!(!button.enabled, "it opens a page that is not there yet");
        assert!(!dir.join("record").exists());

        // the engine answers, and then the companion is started
        start_engine(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        until(&mut app, &ctx, "the companion to run", |a| {
            a.companion_state() == Some(ServerState::Running)
        });

        // it was started in a folder of its own, on the port it was given, told where Ollama is, and
        // made to keep everything in its own root (the person's rule: engines are isolated completely)
        let own = format!("{}/data/openwebui", dir.display());
        let seen = record(&dir);
        // the folder as the shell sees it, through any link on the way (macOS's /var is /private/var)
        let real = |p: &str| std::fs::canonicalize(p).unwrap();
        assert_eq!(real(&seen["cwd"]), real(&format!("{own}/data")));
        assert_eq!(
            seen["args"],
            format!("serve --host 127.0.0.1 --port {port}")
        );
        assert_eq!(
            seen["OLLAMA_BASE_URL"],
            format!("http://127.0.0.1:{}", fake.port())
        );
        assert_eq!(seen["DATA_DIR"], format!("{own}/data"));
        assert_eq!(seen["STATIC_DIR"], format!("{own}/data/static"));
        assert_eq!(seen["XDG_CACHE_HOME"], format!("{own}/cache"));
        assert_eq!(seen["HF_HOME"], format!("{own}/cache/huggingface"));
        assert_eq!(seen["HOME"], format!("{own}/home"), "its home is its own");
        assert_eq!(seen["TMPDIR"], format!("{own}/cache/tmp"));
        assert_eq!(seen["OFFLINE_MODE"], "true");
        assert_eq!(seen["PYTHONPATH"], "");
        assert_eq!(seen["PYTHONNOUSERSITE"], "1");
        assert_eq!(
            seen["CORS_ALLOW_ORIGIN"],
            format!("http://127.0.0.1:{port};http://localhost:{port}")
        );
        assert!(
            Path::new(&format!("{own}/data")).is_dir(),
            "its folder was made"
        );
        assert!(Path::new(&format!("{own}/cache")).is_dir());

        // what the window says of it: its words are said with its name, and it is "ready", not "no model loaded"
        let said_by_it: Vec<&String> = app
            .log_lines
            .iter()
            .filter(|l| l.starts_with("[Open WebUI] "))
            .collect();
        assert!(
            said_by_it
                .iter()
                .any(|l| l.contains(&format!("serve --host 127.0.0.1 --port {port}"))),
            "{said_by_it:#?}"
        );
        assert!(
            said_by_it.iter().any(|l| l.ends_with("Open WebUI ready.")),
            "{said_by_it:#?}"
        );
        assert!(
            !app.log_lines
                .iter()
                .any(|l| l.contains("[Open WebUI]") && l.contains("no model loaded")),
            "a page has no model to load"
        );
        let status = app.companion_status().expect("something to say");
        assert_eq!(
            status,
            (format!("Open WebUI on port {port}"), ServerState::Running)
        );
        let url = app.companion_url().expect("its page");
        assert_eq!(url, format!("http://127.0.0.1:{port}"));
        let button = app.chat_button().unwrap();
        assert!(button.enabled && button.label == "Open Chat UI");
        assert!(matches!(button.action, ChatAction::Open(ref u) if *u == url));
        // the window draws the pair: the globe beside the running model opens the page (its words are said when the
        // pointer rests on it), and the status bar says what the page is doing
        assert!(app
            .browser_chat()
            .is_some_and(|chat| chat.label == "Open Chat UI" && chat.enabled));
        painted(&ctx, |ui| app.show_window(ui));
        let drawn = painted(&ctx, |ui| app.show_window(ui));
        assert!(
            drawn.contains(&format!("Open WebUI on port {port}")),
            "{drawn}"
        );

        // Stop, as the button does: the page goes with what it talks to, and its process is gone
        let pid = read_pid(&dir.join("pid"), Duration::from_secs(5));
        assert!(pid_alive(pid));
        app.stop_companion();
        app.session.stop();
        until(&mut app, &ctx, "both to end", |a| {
            a.session.state() == ServerState::Idle && a.companion_state() == Some(ServerState::Idle)
        });
        assert!(
            gone_within(pid, Duration::from_secs(5)),
            "Open WebUI is still running"
        );
        assert!(app.companion_status().is_none());
        assert!(app.chat_button().is_some_and(|b| !b.enabled));
        // and the daemon of the person's own is where it was
        assert!(!fake.requests_to("/api/version").is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn the_address_the_companion_was_first_started_with_is_remembered_and_said_when_the_engine_is_elsewhere_later(
    ) {
        // A66 of docs/phase3-test-plan.md: Open WebUI keeps the address of Ollama it is given at its first start
        // in a database of its own, so QuickAI remembers that address and says when it would give another
        let dir = scratch("first-address");
        let first = FakeOllama::start();
        let (_page, port) = page();
        let program = launcher(&dir, "exec sleep 60");
        let mut app = app_over(
            &dir,
            first.port(),
            &[
                ("enabled", Value::Bool(true)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        let ctx = egui::Context::default();
        assert_eq!(app.first_address(), None);
        start_engine(&mut app);
        until(&mut app, &ctx, "the companion to run", |a| {
            a.companion_state() == Some(ServerState::Running)
        });
        let first_url = format!("http://127.0.0.1:{}", first.port());
        assert_eq!(app.first_address().as_deref(), Some(first_url.as_str()));
        // remembered in the settings file, under the companion's own key, and in nothing a configuration takes
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(file["values"]["openwebui.first_ollama"], first_url.as_str());
        assert!(
            !app.log_lines
                .iter()
                .any(|l| l.contains("keeps the address")),
            "the first start has nothing to say: {:#?}",
            app.log_lines
        );
        app.stop_companion();
        app.session.stop();
        until(&mut app, &ctx, "both to end", |a| {
            a.session.state() == ServerState::Idle && a.companion_state() == Some(ServerState::Idle)
        });

        // the engine is somewhere else now: the companion is started again, and the log says what it will call
        let second = FakeOllama::start();
        app.values.set("port", second.port());
        start_engine(&mut app);
        until(&mut app, &ctx, "the companion to run again", |a| {
            a.companion_state() == Some(ServerState::Running)
        });
        let second_url = format!("http://127.0.0.1:{}", second.port());
        let said = app
            .log_lines
            .iter()
            .find(|l| l.contains("keeps the address"))
            .unwrap_or_else(|| panic!("{:#?}", app.log_lines));
        assert!(
            said.starts_with(
                "[Open WebUI] Open WebUI keeps the address of Ollama it was first started with "
            ) && said.contains(&format!("({first_url})"))
                && said.contains(&format!("it would be given {second_url} now")),
            "{said}"
        );
        // the first address is still the first one
        assert_eq!(app.first_address().as_deref(), Some(first_url.as_str()));
        app.stop_companion();
        app.session.stop();
    }

    #[test]
    #[cfg(unix)]
    fn p3_t31_when_the_engines_run_ends_the_companion_is_stopped_whatever_ended_it() {
        let dir = scratch("t31");
        let fake = FakeOllama::start();
        let (_page, port) = page();
        let program = launcher(&dir, "exec sleep 60");
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(true)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        let ctx = egui::Context::default();
        start_engine(&mut app);
        until(&mut app, &ctx, "both to run", |a| {
            a.companion_state() == Some(ServerState::Running)
        });
        let pid = read_pid(&dir.join("pid"), Duration::from_secs(5));
        // the engine's own run is stopped, with nothing said to the companion: it goes anyway
        app.session.stop();
        until(&mut app, &ctx, "both to end", |a| {
            a.session.state() == ServerState::Idle && a.companion_state() == Some(ServerState::Idle)
        });
        assert!(gone_within(pid, Duration::from_secs(5)));

        // and the next run starts it again, once
        std::fs::remove_file(dir.join("record")).unwrap();
        start_engine(&mut app);
        until(&mut app, &ctx, "both to run again", |a| {
            a.companion_state() == Some(ServerState::Running)
        });
        wait_for(&dir.join("record"));
        app.stop_companion_blocking();
        app.session.stop_blocking();
    }

    #[test]
    #[cfg(unix)]
    fn p3_t32_a_companion_that_stops_by_itself_leaves_the_engines_run_going_and_says_so() {
        let dir = scratch("t32");
        let fake = FakeOllama::start();
        let (_page, port) = page();
        let program = launcher(
            &dir,
            "sleep 0.5\necho 'ImportError: no module named torch' >&2\nexit 3",
        );
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(true)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        let ctx = egui::Context::default();
        start_engine(&mut app);
        until(&mut app, &ctx, "the companion to stop", |a| {
            a.companion_state() == Some(ServerState::Crashed)
        });
        // Ollama's run is the one it was: only the page has stopped
        assert_eq!(app.session.state(), ServerState::Running);
        assert_eq!(
            app.companion_status(),
            Some((
                "Open WebUI stopped unexpectedly".to_string(),
                ServerState::Crashed
            ))
        );
        assert!(
            app.log_lines.iter().any(|l| l.starts_with("[Open WebUI] ")
                && l.contains("Server process exited unexpectedly")
                && l.contains("exit status: 3")),
            "{:#?}",
            app.log_lines
        );
        assert!(app.companion_url().is_none());
        assert!(app.chat_button().is_some_and(|b| !b.enabled));
        // the panel says what happened and what its document advises for what the log said
        let drawn = painted(&ctx, |ui| app.show_diagnostics(ui, ServerState::Running));
        assert!(
            drawn.contains("Open WebUI stopped unexpectedly")
                && drawn.contains("Open WebUI is missing a Python package"),
            "{drawn}"
        );
        // Stop ends Ollama's run, and the crashed companion has nothing to stop; the next run starts it again
        app.session.stop();
        until(&mut app, &ctx, "the engine to stop", |a| {
            a.session.state() == ServerState::Idle
        });
        std::fs::remove_file(dir.join("record")).unwrap();
        start_engine(&mut app);
        until(&mut app, &ctx, "the companion to be started again", |_| {
            dir.join("record").exists()
        });
        app.stop_companion_blocking();
        app.session.stop_blocking();
    }

    #[test]
    #[cfg(unix)]
    fn p3_t33_it_is_not_started_when_it_is_not_asked_for_and_not_installed_it_is_said_and_the_engine_goes_on(
    ) {
        let (_page, port) = page();
        // not asked for: nothing of it starts or is said, and the button offers to set it up
        let dir = scratch("t33a");
        let fake = FakeOllama::start();
        let program = launcher(&dir, "exec sleep 60");
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(false)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        let ctx = egui::Context::default();
        start_engine(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        for _ in 0..30 {
            frame(&mut app, &ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.companion_state(), Some(ServerState::Idle));
        assert!(
            !dir.join("record").exists(),
            "it was started although it was not asked for"
        );
        assert!(
            !app.log_lines.iter().any(|l| l.contains("Open WebUI")),
            "{:#?}",
            app.log_lines
        );
        let button = app.chat_button().unwrap();
        assert_eq!(button.label, "Set up chat UI...");
        assert!(button.enabled, "a click opens the Settings");
        assert!(matches!(button.action, ChatAction::SetUp));
        app.session.stop_blocking();

        // asked for and not there: said once, the engine's run is not touched
        let dir = scratch("t33b");
        let fake = FakeOllama::start();
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(true)),
                ("port", Value::Int(port as i64)),
            ],
        );
        assert!(
            !app.companion_installed(),
            "nothing is installed under the scratch data folder"
        );
        start_engine(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        for _ in 0..30 {
            frame(&mut app, &ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
        let lines: Vec<&String> = app
            .log_lines
            .iter()
            .filter(|l| l.contains("[Open WebUI] not started: it is not installed"))
            .collect();
        assert_eq!(lines.len(), 1, "{:#?}", app.log_lines);
        assert_eq!(app.session.state(), ServerState::Running);
        assert_eq!(app.companion_state(), Some(ServerState::Idle));
        assert_eq!(app.chat_button().unwrap().label, "Set up chat UI...");
        app.session.stop_blocking();
    }

    #[test]
    #[cfg(unix)]
    fn p3_t33b_asked_for_or_installed_while_the_engine_runs_it_is_started_at_once_and_once() {
        let (_page, port) = page();
        let ctx = egui::Context::default();
        let nothing_for = |app: &mut App, frames: usize| {
            for _ in 0..frames {
                frame(app, &ctx);
                std::thread::sleep(Duration::from_millis(10));
            }
        };

        // not asked for when the run began, and the box is ticked in the middle of it: started then
        let dir = scratch("t33c");
        let fake = FakeOllama::start();
        let program = launcher(&dir, "exec sleep 60");
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(false)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        start_engine(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        nothing_for(&mut app, 30);
        assert!(
            !dir.join("record").exists(),
            "started although not asked for"
        );
        app.companion
            .as_mut()
            .and_then(|c| c.slot.as_mut())
            .expect("the companion's values")
            .values
            .set("enabled", true);
        until(
            &mut app,
            &ctx,
            "it to be started when the box is ticked",
            |a| a.companion_state() == Some(ServerState::Running),
        );
        assert!(dir.join("record").exists());
        assert_eq!(app.session.state(), ServerState::Running);
        app.stop_companion_blocking();
        app.session.stop_blocking();

        // asked for and not installed when the run began: said once; its program appears (what the end of an
        // install does) and it is started, and the log does not say it again
        let dir = scratch("t33d");
        let fake = FakeOllama::start();
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(true)),
                ("port", Value::Int(port as i64)),
            ],
        );
        start_engine(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        nothing_for(&mut app, 30);
        assert_eq!(app.companion_state(), Some(ServerState::Idle));
        let program = launcher(&dir, "exec sleep 60");
        app.companion
            .as_mut()
            .and_then(|c| c.slot.as_mut())
            .expect("the companion's values")
            .binary = program;
        until(
            &mut app,
            &ctx,
            "it to be started when it is installed",
            |a| a.companion_state() == Some(ServerState::Running),
        );
        let said = app
            .log_lines
            .iter()
            .filter(|l| l.contains("not started: it is not installed"))
            .count();
        assert_eq!(said, 1, "{:#?}", app.log_lines);
        app.stop_companion_blocking();
        app.session.stop_blocking();

        // one that ends by itself in a run is not started again in it
        let dir = scratch("t33e");
        let fake = FakeOllama::start();
        let program = write_script(
            &dir,
            "open-webui",
            &format!(
                "echo x >> '{}'\nsleep 0.3\nexit 3\n",
                dir.join("starts").display()
            ),
        );
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(true)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        start_engine(&mut app);
        until(&mut app, &ctx, "it to end by itself", |a| {
            a.companion_state() == Some(ServerState::Crashed)
        });
        nothing_for(&mut app, 80);
        let starts = std::fs::read_to_string(dir.join("starts")).unwrap();
        assert_eq!(
            starts.lines().count(),
            1,
            "it was started again in the same run"
        );
        assert_eq!(app.session.state(), ServerState::Running);
        app.session.stop_blocking();
    }

    #[test]
    #[cfg(unix)]
    fn p3_t38b_its_fields_are_locked_while_it_runs_and_not_while_only_the_engine_does() {
        let (_page, port) = page();
        let dir = scratch("t38b");
        let fake = FakeOllama::start();
        let program = launcher(&dir, "exec sleep 60");
        let mut app = app_over(
            &dir,
            fake.port(),
            &[
                ("enabled", Value::Bool(false)),
                ("port", Value::Int(port as i64)),
                ("bin_override", Value::Str(program.display().to_string())),
            ],
        );
        let ctx = egui::Context::default();
        let editable = |app: &mut App, id: &str| -> Option<bool> {
            app.as_companion(|a| a.form().and_then(|f| f.field(id).map(|f| f.enabled)))
                .flatten()
        };
        // Ollama is running, Open WebUI is not: its box and its port are the person's to set
        start_engine(&mut app);
        until(&mut app, &ctx, "the engine to run", |a| {
            a.session.state() == ServerState::Running
        });
        for id in ["enabled", "port", "network", "extra_env"] {
            assert_eq!(
                editable(&mut app, id),
                Some(true),
                "{id} while only Ollama runs"
            );
        }
        // the engine's own fields are locked, as they always were
        assert_eq!(
            app.form().and_then(|f| f.field("port").map(|f| f.enabled)),
            Some(false)
        );
        // Open WebUI is running: they are locked, until it is not
        app.companion
            .as_mut()
            .and_then(|c| c.slot.as_mut())
            .expect("the companion's values")
            .values
            .set("enabled", true);
        until(&mut app, &ctx, "it to run", |a| {
            a.companion_state() == Some(ServerState::Running)
        });
        for id in ["enabled", "port", "network", "extra_env"] {
            assert_eq!(editable(&mut app, id), Some(false), "{id} while it runs");
        }
        app.stop_companion_blocking();
        app.session.stop_blocking();
    }

    #[test]
    fn p3_t34_its_values_are_kept_under_its_own_keys_with_the_engines_and_forgotten_by_no_switch() {
        let dir = scratch("t34");
        let fake = FakeOllama::start();
        let mut app = app_over(&dir, fake.port(), &[]);
        let before = std::fs::read_to_string(dir.join("settings.json")).ok();
        assert!(before.is_none(), "nothing is written until something is");
        // what the person does in its section
        app.as_companion(|a| {
            a.values.set("enabled", true);
            a.values.set("port", 3999u16);
            a.values.set("network", "online");
            // a call that wants the settings written does not write them from the companion's state
            a.save_settings();
        });
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        let values = &saved["values"];
        assert_eq!(values["openwebui.enabled"], true);
        assert_eq!(values["openwebui.port"], 3999);
        assert_eq!(values["openwebui.network"], "online");
        assert_eq!(
            values["ollama.port"],
            fake.port(),
            "Ollama's own are what they were, and the engine is still Ollama's: {saved}"
        );
        assert_eq!(
            saved["engine"].as_str().or(values["app.engine"].as_str()),
            Some("ollama")
        );
        assert!(app.engine == "ollama" && app.values.int("port") == Some(fake.port() as i64));

        // another engine has no companion, and the file still holds what was kept
        app.switch_engine("llamacpp");
        assert!(app.companion.is_none() && app.chat_button().is_none());
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["values"]["openwebui.enabled"], true);
        // and back: they are the person's still
        app.switch_engine("ollama");
        assert!(app.companion_enabled());
        assert_eq!(app.companion_port(), Some(3999));
        // a settings file that names the companion as the engine is not an engine: llama.cpp is used
        let mut settings = Settings::default();
        settings.set_chosen_engine("openwebui");
        let (engine, note) =
            crate::config::engine_in_use(&build_registry(&ConfigDirs::default()), &settings);
        assert_eq!(engine, "llamacpp");
        assert!(note.is_some_and(|n| n.contains("openwebui")));
    }

    #[test]
    fn p3_t35_the_headless_plan_of_a_companion_is_for_the_engine_it_goes_with_on_its_default_port()
    {
        let registry = build_registry(&ConfigDirs::default());
        assert_eq!(
            launch::default_parent(&registry, "openwebui"),
            Some(Parent {
                id: "ollama".into(),
                port: 11434
            })
        );
        assert_eq!(launch::default_parent(&registry, "ollama"), None);
        assert_eq!(launch::default_parent(&registry, "nosuch"), None);
        let (code, out, err) = {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = crate::cli::run_with(
                &[
                    "plan".to_string(),
                    "--engine".to_string(),
                    "openwebui".to_string(),
                ],
                &ConfigDirs::default(),
                &mut out,
                &mut err,
            );
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("plan: (no model) on openwebui"), "{out}");
        assert!(
            out.contains("cwd ") && out.replace('\\', "/").contains("/.quickai/openwebui/data"),
            "{out}"
        );
        assert!(
            out.contains("env OLLAMA_BASE_URL=http://127.0.0.1:11434"),
            "{out}"
        );
        assert!(out.contains("env OFFLINE_MODE=true"), "{out}");
    }
}
