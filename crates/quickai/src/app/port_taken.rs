//! A run on this computer whose port another program holds (docs/remote-runs.md section 3.2, as it is for a computer
//! QuickAI sets up; the user: "if an engine is already running, inform the user and confirm over-ride - provide an
//! option to stop it also"). Before Start, the port the engine is to listen on is looked at; when another program
//! holds it, the person is told what it is and chooses:
//!
//! - an engine that already runs there, which QuickAI did not start (an Ollama installed as a service): **Use it**
//!   (what the window did by itself before: the run attaches to it), and *Always use the one that runs* sets the
//!   engine's mode to attach so that it is not asked again; **Override** (stop it, then start QuickAI's); **Stop it**;
//!   **Cancel**;
//! - another program: **Override**, **Use port N** (the next free one), **Stop it**, **Cancel**.
//!
//! An engine set to attach (its mode) is not asked about: the person said to use the one that runs.

use crossbeam_channel::Receiver;
use eframe::egui;

use crate::ports::{self, Freed};

use super::theme::{self, Role};
use super::App;

/// The question about a port that is taken, while it is asked.
pub(super) struct PortTaken {
    pub port: u16,
    /// What answers there, when it answers as the engine does (`Ollama 0.12.3`): the engine already runs.
    pub engine: Option<String>,
    /// The program that holds it, when it can be told (`ollama (process 1234)`).
    pub holder: Option<String>,
    /// The engine that runs can be used as it is: its document can attach to it, and its mode is not `spawn`.
    pub usable: bool,
    /// *Always use the one that runs*.
    pub always: bool,
    /// The program is being stopped on a thread; then what was done, and whether to start after.
    freeing: Option<(Receiver<Freed>, bool)>,
    /// Why it could not be stopped.
    pub failed: Option<String>,
}

/// What the person chose.
enum Chosen {
    UseIt,
    UsePort(u16),
    Override,
    StopIt,
    Cancel,
}

impl App {
    /// Whether the port the engine is to listen on here is taken now, and by what; `None` when it is free, or when the
    /// engine is set to attach to the one that runs.
    pub(super) fn port_taken_now(&self) -> Option<PortTaken> {
        let port = self.port();
        if !ports::taken(port) {
            return None;
        }
        let doc = self.engine_doc()?;
        let mode = doc
            .doc
            .launch
            .strategy_field
            .as_deref()
            .and_then(|field| self.values.text(field))
            .unwrap_or("auto");
        if mode == "attach" {
            return None;
        }
        let engine = doc.doc.launch.attach.as_ref().and_then(|attach| {
            crate::remote::identity(port, &attach.identity.path, &attach.identity.pointer)
                .map(|version| format!("{} {version}", doc.doc.engine.name))
        });
        let usable = engine.is_some() && mode != "spawn";
        Some(PortTaken {
            port,
            engine,
            holder: ports::holder(port),
            usable,
            always: false,
            freeing: None,
            failed: None,
        })
    }

    /// The question, while a Start waits for it; and the stopping of the program, when it was asked for.
    pub(super) fn show_port_taken_dialog(&mut self, ctx: &egui::Context) {
        let Some(question) = self.port_taken.as_mut() else {
            return;
        };
        // a program being stopped: what was done, once it is
        if let Some((rx, start_after)) = &question.freeing {
            match rx.try_recv() {
                Ok(freed) if freed.free => {
                    let start_after = *start_after;
                    let port = question.port;
                    self.port_taken = None;
                    self.push_log(format!("[quickai] Port {port} is free now."));
                    if start_after {
                        self.port_check_skip = true;
                        self.start_selected(ctx);
                    }
                    return;
                }
                Ok(freed) => {
                    question.failed = Some(
                        freed
                            .hint
                            .unwrap_or_else(|| "it could not be stopped from here.".into()),
                    );
                    question.freeing = None;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => question.freeing = None,
            }
        }
        let server = self.server_name();
        let Some(question) = self.port_taken.as_mut() else {
            return;
        };
        let p = theme::palette();
        let next = (!question.usable)
            .then(|| super::diagnostics::next_free_port(question.port))
            .flatten();
        let (title, said) = match &question.engine {
            Some(engine) => (
                format!("{engine} already runs on this computer"),
                format!(
                    "{engine} answers on port {}, and QuickAI did not start it.",
                    question.port
                ),
            ),
            None => (
                format!("Port {} is taken", question.port),
                format!(
                    "{} listens on port {}, which {server} is set to use.",
                    question.holder.as_deref().unwrap_or("Another program"),
                    question.port
                ),
            ),
        };
        let freeing = question.freeing.is_some();
        let mut chosen = None;
        let modal = egui::Modal::new(egui::Id::new("port_taken_modal")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                egui::RichText::new(title)
                    .size(16.0)
                    .family(theme::semibold(ui)),
            );
            ui.add_space(6.0);
            ui.label(said);
            if let (Some(_), Some(holder)) = (&question.engine, &question.holder) {
                ui.label(theme::muted(format!("It is {holder}.")));
            }
            if question.usable {
                ui.add_space(4.0);
                ui.checkbox(&mut question.always, "Always use the one that runs")
                    .on_hover_text(format!(
                        "{server} is then used as it runs, when one does, and this is not asked again (Settings has its \
                         mode)."
                    ));
            }
            if let Some(why) = &question.failed {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("It was not stopped: {why}")).color(p.bad));
            }
            ui.add_space(12.0);
            ui.add_enabled_ui(!freeing, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::button_sized(ui, Role::Primary, true, "Override", egui::vec2(0.0, 30.0))
                        .on_hover_text(format!("Stop it, then start {server}."))
                        .clicked()
                    {
                        chosen = Some(Chosen::Override);
                    }
                    if question.usable {
                        if ui
                            .button("Use it")
                            .on_hover_text("Use the one that runs, as it is.")
                            .clicked()
                        {
                            chosen = Some(Chosen::UseIt);
                        }
                    } else if let Some(next) = next {
                        if ui
                            .button(format!("Use port {next}"))
                            .on_hover_text(format!("Start {server} on port {next}, which is free."))
                            .clicked()
                        {
                            chosen = Some(Chosen::UsePort(next));
                        }
                    }
                    if ui
                        .button("Stop it")
                        .on_hover_text("Stop it, and start nothing.")
                        .clicked()
                    {
                        chosen = Some(Chosen::StopIt);
                    }
                    if ui.button("Cancel").clicked() {
                        chosen = Some(Chosen::Cancel);
                    }
                    if freeing {
                        ui.spinner();
                        ui.label(theme::muted("Stopping it…"));
                    }
                });
            });
        });
        if modal.should_close() && chosen.is_none() && !freeing {
            chosen = Some(Chosen::Cancel);
        }
        let Some(chosen) = chosen else {
            return;
        };
        match chosen {
            Chosen::Cancel => self.port_taken = None,
            Chosen::UseIt => {
                let always = self.port_taken.as_ref().is_some_and(|q| q.always);
                self.port_taken = None;
                if always {
                    if let Some(field) = self
                        .engine_doc()
                        .and_then(|e| e.doc.launch.strategy_field.clone())
                    {
                        self.values.set(&field, "attach");
                        self.save_settings();
                    }
                }
                self.port_check_skip = true;
                self.start_selected(ctx);
            }
            Chosen::UsePort(port) => {
                self.port_taken = None;
                self.values.set("port", i64::from(port));
                self.save_settings();
                self.push_log(format!("[quickai] Port {port} is free: starting on it."));
                self.start_selected(ctx);
            }
            Chosen::Override | Chosen::StopIt => {
                let start_after = matches!(chosen, Chosen::Override);
                if let Some(question) = self.port_taken.as_mut() {
                    let port = question.port;
                    let (tx, rx) = crossbeam_channel::bounded(1);
                    let wake = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(ports::free_port(port));
                        wake.request_repaint();
                    });
                    question.failed = None;
                    question.freeing = Some((rx, start_after));
                }
            }
        }
    }
}
