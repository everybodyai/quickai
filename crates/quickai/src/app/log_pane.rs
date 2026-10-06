//! The server log at the bottom of the main window (panel `log`).

use eframe::egui;

use super::App;

impl App {
    /// The rule above the log and the scrolling log itself.
    pub(super) fn show_log_pane(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.separator();
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.style_mut().override_font_id = Some(egui::FontId::monospace(11.0));
                for line in &self.log_lines {
                    ui.label(line);
                }
            });
    }
}
