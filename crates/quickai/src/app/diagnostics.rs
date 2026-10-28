//! What to tell the user when the server cannot start or has crashed (panel `diagnostics`).

use eframe::egui;
use quickai_core::advice::{advice, Advice};

use super::theme::{self, Tone};
use super::App;
use crate::events::ServerState;
use crate::gpu;
use crate::launch;

impl App {
    /// Whether the engine's program cannot be found and there is no use in starting: the page that installs it
    /// is shown instead. A bundled engine that is not beside the application (a build of QuickAI that came
    /// without it, a program of the person's own that was moved) is installed the same way when its document
    /// has builds to download; one with none is tried and says what it could not find.
    pub(super) fn engine_missing(&self) -> bool {
        // one that runs on another computer is that computer's to have
        if self.remote_in_use().is_some() || self.binary.is_file() {
            return false;
        }
        self.engine_doc()
            .is_none_or(|e| !e.doc.engine.bundled || e.doc.acquire.is_some())
    }

    /// Whether the page is the one that installs the engine: it is not here, and nothing of it runs (an Ollama the
    /// person runs themselves, attached to, is there to use whatever is installed).
    pub(super) fn needs_install(&self) -> bool {
        self.engine_missing() && !self.session.is_active()
    }

    /// The missing-binary line and the crash panel with its advice.
    pub(super) fn show_diagnostics(&mut self, ui: &mut egui::Ui, state: ServerState) {
        // the computer the models run on could not be reached, or the connection to it closed
        self.show_remote_alert(ui);
        let server = self.server_name();
        // an engine that is not installed has the section that installs it at the top of the page
        if !self.engine_missing() && self.missing_binary_warned {
            ui.add_space(8.0);
            theme::alert(ui, Tone::Bad, |ui| {
                ui.label(format!(
                    "{server} binary not found at {}",
                    self.binary.display()
                ));
            });
        }

        if state == ServerState::Crashed {
            ui.add_space(8.0);
            // llama.cpp's own panel is about a start that failed: a server that was running and stopped is said as
            // every engine says it, with its document's advice
            if self.engine == crate::settings::DEFAULT_ENGINE
                && !self.run_reached_running
                && self.crash_advice(&server).1.is_empty()
            {
                self.show_llamacpp_crash_panel(ui, state);
            } else {
                self.show_crash_panel(ui, state, &server);
            }
        }
        // the program that goes with the engine stopped, and the engine's run goes on
        if self.companion_state() == Some(ServerState::Crashed) {
            self.show_companion_crash(ui);
        }
    }

    /// llama.cpp's panel for a start that failed when its document has no advice for what the log says: the likeliest
    /// reason is a build older than the model, and the way to another build.
    fn show_llamacpp_crash_panel(&mut self, ui: &mut egui::Ui, _state: ServerState) {
        let mut builds = false;
        theme::alert(ui, Tone::Bad, |ui| {
            ui.label("llama-server stopped before it was ready. The Log tab says why.");
            let path = self.binary.display().to_string();
            let backend = self
                .registry
                .engine(&self.engine)
                .and_then(|e| quickai_core::acquire::backend(e, &path));
            if let Some(warning) =
                backend.and_then(|b| gpu::mismatch_warning(&self.gpu_vendors, b, &b.label))
            {
                ui.label(
                    egui::RichText::new(warning)
                        .small()
                        .color(super::theme::palette().warn),
                );
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(
                        "A common reason is a model newer than this build of llama.cpp (\"unknown model \
                         architecture\").",
                    )
                    .small(),
                );
                builds = ui
                    .small_button("Choose another build...")
                    .on_hover_text(
                        "Settings, Advanced: llama.cpp: the builds on offer for this computer, and a box for a \
                         llama-server of your own.",
                    )
                    .clicked();
            });
        });
        if builds {
            self.advanced_section_wanted = true;
            self.open_settings();
        }
    }

    /// What the crash panel says first, and the advice the engine's document has for what the log says.
    fn crash_advice(&self, server: &str) -> (String, Vec<Advice>) {
        let ended_in = if self.run_reached_running {
            "running"
        } else {
            "starting"
        };
        let what = if self.run_reached_running {
            format!("{server} stopped unexpectedly")
        } else {
            format!("{server} stopped before it was ready")
        };
        let log: Vec<String> = self.log_lines.iter().cloned().collect();
        let selection = launch::selection_from_values(
            &self.engine,
            self.selected_model_info(),
            self.values.map(),
            self.traffic_proxy_on(),
        );
        let hints = advice(
            &self.registry,
            &selection,
            &self.form_facts(),
            &log,
            Some(ended_in),
        );
        (what, hints)
    }

    /// Another engine's panel: what happened, and the advice its document has for what the log says.
    pub(super) fn show_crash_panel(
        &mut self,
        ui: &mut egui::Ui,
        _state: ServerState,
        server: &str,
    ) {
        let (what, hints) = self.crash_advice(server);
        let mut asked: Option<String> = None;
        let mut use_port: Option<(String, u16)> = None;
        theme::alert(ui, Tone::Bad, |ui| {
            ui.label(format!("{what}. The Log tab says why."));
            for Advice { text, action } in &hints {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(text).small());
                    if let Some(action) = action {
                        // advice that points at a port has the fix in one click: the next port nothing listens on
                        if let Some((field, port)) = self.free_port_for(action) {
                            if ui
                                .small_button(format!("Use port {port}"))
                                .on_hover_text(format!(
                                    "Nothing on this computer listens on port {port}: keep it and start again."
                                ))
                                .clicked()
                            {
                                use_port = Some((field, port));
                            }
                        }
                        if ui.small_button(action_label(action)).clicked() {
                            asked = Some(action.clone());
                        }
                    }
                });
            }
        });
        if let Some((field, port)) = use_port {
            self.values.set(&field, i64::from(port));
            self.save_settings();
            self.push_log(format!("[quickai] Port {port} is free: starting on it."));
            self.start_selected(ui.ctx());
        } else if let Some(action) = asked {
            self.invoke(&action);
        }
    }

    /// For advice that sends the person to a port field (`focus_field:port`), that field and the first port above
    /// the one it holds that nothing on this computer listens on.
    fn free_port_for(&self, action: &str) -> Option<(String, u16)> {
        let id = action.strip_prefix("focus_field:")?;
        let field = self.engine_doc()?.doc.fields.iter().find(|f| f.id == id)?;
        if field.ty != quickai_core::doc::engine::FieldType::Port {
            return None;
        }
        let now = self
            .values
            .int(id)
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(8080);
        next_free_port(now).map(|port| (id.to_string(), port))
    }
}

/// The first port after `taken`, within a hundred, that can be listened on at 127.0.0.1 now (where the engines
/// listen). Asked again each time the panel is drawn, which is only while it is shown.
fn next_free_port(taken: u16) -> Option<u16> {
    (taken.saturating_add(1)..=taken.saturating_add(100))
        .filter(|&p| p >= 1024)
        .find(|&p| std::net::TcpListener::bind(("127.0.0.1", p)).is_ok())
}

/// What the button of a piece of advice says.
fn action_label(action: &str) -> &'static str {
    match action.split_once(':').map_or(action, |(verb, _)| verb) {
        "focus_field" => "Go to the setting",
        _ => "Open Settings",
    }
}
