//! What stands in for runs on computers QuickAI sets up in a window built without them (the feature `remote-setup`
//! off, docs/remote-runs.md section 5): there is no such computer to choose (`setup_in_use` is always `None`), so
//! nothing runs there and nothing of it is drawn. It has the names `remote_run` and `remote_page` give the rest of
//! the window, and does nothing with them.

use eframe::egui;

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

    pub fn computer(&self) -> Option<String> {
        None
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

    pub(super) fn show_models_there(&mut self, _ui: &mut egui::Ui) {}

    pub(super) fn model_there_name(&self, named: &str) -> String {
        named.to_string()
    }

    pub(super) fn there_summary(&self, _id: &str) -> Option<String> {
        None
    }

    pub(super) fn listed_there(&self, _id: &str) -> bool {
        false
    }

    /// The name of the configuration selected in the window.
    pub(super) fn selected_configuration_name(&self) -> String {
        self.registry
            .profile(&self.configs.active)
            .map(|e| e.configuration.name.clone())
            .unwrap_or_else(|| "Default".to_string())
    }

    // what `remote_there` gives the rest of the window (C228): nothing, as no such computer is chosen

    pub(super) fn facts_there(&self) -> Option<&crate::facts::ComputerFacts> {
        None
    }

    pub(super) fn asking_facts_there(&self) -> bool {
        false
    }

    pub(super) fn ask_facts_there(&mut self, _ctx: &egui::Context) {}

    pub(super) fn ask_facts_there_once(&mut self, _ctx: &egui::Context) {}

    pub(super) fn cards_there(&self) -> Vec<crate::devices::Device> {
        Vec::new()
    }

    pub(super) fn ram_there(&self) -> Option<u64> {
        None
    }

    pub(super) fn subject_there(&self) -> Option<(u64, Option<&quickai_core::gguf::ModelFacts>)> {
        None
    }

    pub(super) fn there_words(&self) -> String {
        String::new()
    }

    pub(super) fn computers_to_recommend_for(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    pub(super) fn have_there(&self) -> Option<std::collections::BTreeMap<String, u64>> {
        None
    }

    pub(super) fn setup_bar_title(&self) -> String {
        String::new()
    }

    pub(super) fn setup_bar_state(&self) -> (String, egui::Color32, String) {
        (String::new(), egui::Color32::TRANSPARENT, String::new())
    }

    pub(super) fn setup_bar_actions(&mut self, _ui: &mut egui::Ui, _height: f32) -> (bool, bool) {
        (false, false)
    }

    pub(super) fn show_computer_there(&mut self, _ui: &mut egui::Ui, _meters: f32) {}

    pub(super) fn fetch_there(
        &mut self,
        _fetch: &quickai_core::lookup::present::Fetch,
        _ctx: &egui::Context,
    ) {
    }

    pub(super) fn fetch_here_then_copy(
        &mut self,
        _fetch: &quickai_core::lookup::present::Fetch,
        _ctx: &egui::Context,
    ) {
    }

    pub(super) fn getting_there(&self) -> Option<(String, String, Option<f32>)> {
        None
    }

    pub(super) fn cancel_get_there(&mut self, _ctx: &egui::Context) {}

    pub(super) fn note_last_there(&mut self) {}

    pub(super) fn terminal_shown(&self) -> bool {
        false
    }

    pub(super) fn terminal_hover(&self) -> String {
        String::new()
    }

    pub(super) fn show_terminal_tab(&mut self, _ui: &mut egui::Ui) {}
}
