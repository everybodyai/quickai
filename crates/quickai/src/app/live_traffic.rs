//! The 📡 Live tab: the conversations list and the picked conversation.

use eframe::egui;

use super::App;

impl App {
    /// Every distinct client (see `TrafficTurn::session`) that has shown up
    /// in the 📡 Live Traffic feed so far, in first-seen order.
    fn session_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for t in &self.traffic {
            if !keys.iter().any(|k| k == &t.session) {
                keys.push(t.session.clone());
            }
        }
        keys
    }

    /// Content of the 📡 Live tab (see the tab strip in `ui()`): a session
    /// list down the left (like an MS Teams chat list — one row per client,
    /// click to switch, or "All" to see everything mixed together) with the
    /// picked session's conversation filling the rest of the tab on the
    /// right.
    pub(super) fn show_live_traffic_tab(&mut self, ui: &mut egui::Ui) {
        // Owned, not borrowed from `self.traffic` — so nothing here keeps
        // `self` borrowed once we get to the mutable `selected_session`
        // writes below.
        let session_keys = self.session_keys();
        if let Some(sel) = &self.selected_session {
            if !session_keys.contains(sel) {
                self.selected_session = None;
            }
        }

        egui::Panel::left("live_traffic_sessions_panel")
            .resizable(true)
            .default_size(220.0)
            .size_range(160.0..=360.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Conversations").strong())
                    .on_hover_text(
                        "Click a client below to view just its conversation, or \"All\" for \
                         everything mixed together.",
                    );
                ui.add_space(6.0);
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let all_selected = self.selected_session.is_none();
                        let all_label = format!(
                            "💬 All\n{} message{}",
                            self.traffic.len(),
                            if self.traffic.len() == 1 { "" } else { "s" }
                        );
                        if ui.selectable_label(all_selected, all_label).clicked() {
                            self.selected_session = None;
                        }
                        ui.add_space(2.0);

                        for (i, key) in session_keys.iter().enumerate() {
                            let mut count = 0usize;
                            let mut live = false;
                            let mut preview = String::new();
                            for t in self.traffic.iter().filter(|t| &t.session == key) {
                                count += 1;
                                live |= !t.done;
                                preview = t.prompt.clone().unwrap_or_else(|| {
                                    if t.attachments.is_empty() {
                                        t.path.clone()
                                    } else {
                                        t.attachments.join(", ")
                                    }
                                });
                            }
                            let dot = if live { "\u{25cf} " } else { "" };
                            let label = format!(
                                "{dot}Session {}\n{} \u{b7} {count} message{}",
                                i + 1,
                                truncate_preview(&preview, 34),
                                if count == 1 { "" } else { "s" }
                            );
                            let selected = self.selected_session.as_deref() == Some(key.as_str());
                            if ui
                                .selectable_label(selected, label)
                                .on_hover_text(format!("Client at {key}"))
                                .clicked()
                            {
                                self.selected_session = Some(key.clone());
                            }
                            ui.add_space(2.0);
                        }

                        if session_keys.is_empty() {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "No traffic yet — waiting for the first request.",
                                )
                                .color(super::theme::palette().muted),
                            );
                        }
                    });

                ui.add_space(6.0);
                if ui.button("Clear all").clicked() {
                    self.traffic.clear();
                    self.selected_session = None;
                }
            });

        // Matches the left panel's frame margin (`Frame::side_top_panel`'s
        // `Margin::symmetric(8, 2)`) instead of `CentralPanel`'s wider
        // default top margin, so the two headers line up on the same row
        // rather than the right one sitting a few pixels lower.
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Live chat/completion traffic").strong())
                .on_hover_text(
                "Every chat/completion request hitting this model, from any client, shown as \
                 it happens. Only chat/completion endpoints are shown here — other traffic \
                 (the chat UI's own page assets, health checks, etc.) is relayed to \
                 llama-server as normal but not logged.",
            );
                ui.add_space(6.0);
                ui.separator();

                let selected = self.selected_session.clone();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for turn in self
                            .traffic
                            .iter()
                            .filter(|t| selected.as_deref().is_none_or(|s| t.session == s))
                        {
                            super::theme::card(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} {}",
                                            turn.method, turn.path
                                        ))
                                        .strong()
                                        .monospace(),
                                    );
                                    if !turn.done {
                                        ui.label(
                                            egui::RichText::new("● live")
                                                .color(super::theme::palette().ok),
                                        );
                                    }
                                });
                                if let Some(prompt) = &turn.prompt {
                                    ui.label(
                                        egui::RichText::new(format!("↳ {prompt}"))
                                            .color(super::theme::palette().muted),
                                    );
                                }
                                for attachment in &turn.attachments {
                                    ui.label(
                                        egui::RichText::new(format!("↳ {attachment}"))
                                            .color(super::theme::palette().muted),
                                    );
                                }
                                if !turn.response.is_empty() {
                                    ui.label(&turn.response);
                                } else if turn.error.is_none() && !turn.done {
                                    ui.label(
                                        egui::RichText::new("…")
                                            .color(super::theme::palette().muted),
                                    );
                                }
                                if let Some(err) = &turn.error {
                                    ui.colored_label(
                                        super::theme::palette().bad,
                                        format!("error: {err}"),
                                    );
                                }
                            });
                            ui.add_space(4.0);
                        }
                    });
            });
    }
}

/// Shortens `s` to at most `max_chars` characters (UTF-8-safe), replacing
/// the empty case with a placeholder so a session's chat-list row never
/// shows a blank preview line.
fn truncate_preview(s: &str, max_chars: usize) -> String {
    if s.is_empty() {
        return "(no prompt)".to_string();
    }
    let mut truncated: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        truncated.push('\u{2026}');
    }
    truncated
}
