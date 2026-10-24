//! The server log, the first tab of the dock at the foot of the main window (panel `log`).

use eframe::egui;

use super::theme;
use super::App;

/// Room inside the log's frame, on each side.
const INSET: f32 = 8.0;

impl App {
    /// The scrolling log, in a well that takes the rest of the dock.
    pub(super) fn show_log_pane(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let room = ui.available_size();
        egui::Frame::new()
            .fill(p.log)
            .inner_margin(egui::Margin::same(INSET as i8))
            .show(ui, |ui| {
                // the frame fills what is left of the window (less than a line leaves it its own size)
                let inside = egui::vec2(2.0 * INSET, 2.0 * INSET);
                ui.set_min_size((room - inside).max(egui::Vec2::ZERO));
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.style_mut().override_font_id = Some(egui::FontId::monospace(12.0));
                        if self.log_lines.is_empty() {
                            ui.label(
                                egui::RichText::new("Messages from the server show up here.")
                                    .color(p.faint),
                            );
                        }
                        for line in &self.log_lines {
                            ui.label(line);
                        }
                    });
            });
    }
}
