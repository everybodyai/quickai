//! What stands in for runs on computers QuickAI sets up in a window built without them (the feature `remote-setup`
//! off, docs/remote-runs.md section 5): there is no such computer to choose (`setup_in_use` is always `None`), so
//! nothing runs there and nothing of it is drawn. It has the names `remote_run` and `remote_page` give the rest of
//! the window, and does nothing with them.

use eframe::egui;

use crate::events::ServerState;
use crate::provision::EndpointInfo;

use super::App;

/// A run on a computer QuickAI sets up: never one here.
#[derive(Default)]
pub struct SetupRun {
    pub configuration: String,
    pub failed: Option<String>,
}

impl SetupRun {
    pub fn running(&self) -> bool {
        false
    }

    pub fn working(&self) -> bool {
        false
    }

    pub fn endpoint(&self, _role: &str) -> Option<&EndpointInfo> {
        None
    }

    pub fn chat_target(&self) -> Option<(u16, String)> {
        None
    }
}

impl App {
    pub(super) fn setup_run(&self) -> Option<&SetupRun> {
        None
    }

    pub(super) fn setup_run_of(&self, _id: &str) -> Option<&SetupRun> {
        None
    }

    pub(super) fn start_setup_run(&mut self, _ctx: &egui::Context, _override_first: bool) {}

    pub(super) fn stop_setup_run(&mut self) {}

    pub(super) fn poll_setup_run(&mut self, _ctx: &egui::Context) {}

    pub(super) fn show_busy_dialog(&mut self, _ctx: &egui::Context) {}

    pub(super) fn show_alternatives_dialog(&mut self, _ctx: &egui::Context) {}

    pub(super) fn show_remote_page(&mut self, _ui: &mut egui::Ui) {}

    pub(super) fn remote_run_status(&self) -> Option<(ServerState, String)> {
        None
    }

    /// The name of the configuration selected in the window.
    pub(super) fn selected_configuration_name(&self) -> String {
        self.registry
            .profile(&self.configs.active)
            .map(|e| e.configuration.name.clone())
            .unwrap_or_else(|| "Default".to_string())
    }
}
