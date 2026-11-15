//! Capabilities, in How it runs (C223): a switch for each thing the chat can do besides asking for replies, named in a
//! word or two and said in full on hover. Auto-compact is off until the person turns it on; Thinking is there for a
//! model that can answer without thinking first; Show thinking for one that thinks. Closed at first, as Tools is, with
//! how many are on. In the panel at the right, while the model is stopped and while it runs, and on the page of a
//! computer QuickAI sets up; what is switched is the person's, for every model and computer.

use eframe::egui;

use super::compact::{tokens_words, Capability, ROOM_CHOICES};
use super::icons::Icon;
use super::theme;
use super::App;

const HOVER: &str =
    "Switches for how the chat works with the model. Rest the pointer on one to see what it does.";
const AUTO_COMPACT: &str = "When the conversation would take more of the context than the share chosen beside the \
                            switch, room is made before the next request, between the steps of a long run of tool calls \
                            as well: first, what older tool calls gave is no longer sent (the last three stay); if that \
                            is not enough, the model summarizes the earlier messages, and the summary is sent in their \
                            place, so that the chat can go on. You still see every message, and a line in the chat \
                            shows where the summary begins. Off: when the context is full, the chat stops and says so.";
const AUTO_COMPACT_AT: &str = "Auto-compact makes room once the conversation would take more than this share of the \
                               context. A higher share keeps more of the conversation whole, but leaves less room for \
                               the answer (and for the thinking of a model that thinks).";
const THINKING: &str = "On: the model works through a question before it answers, which helps with logic, maths and \
                        planning. Off: it answers at once, sooner, and its answers take less of the context. From the \
                        next message on.";
const SHOW_THINKING: &str =
    "Shows what the model thought before each answer, folded above it: click to open it. What it \
     thought is never sent back to it.";

/// A switch of Capabilities: what it is, its name, what it does, and whether it is on.
struct Row {
    which: Capability,
    name: &'static str,
    hover: &'static str,
    on: bool,
}

impl App {
    /// The switches there are for the model the chat talks to (or, while it is stopped, the one selected).
    fn capability_rows(&self) -> Vec<Row> {
        let (thinks, switch) = self.chat_thinking();
        let thought = self.chat.turns.iter().any(|t| !t.thought.is_empty());
        let mut rows = vec![Row {
            which: Capability::AutoCompact,
            name: "Auto-compact",
            hover: AUTO_COMPACT,
            on: self.stored.auto_compact(),
        }];
        if switch {
            rows.push(Row {
                which: Capability::Thinking,
                name: "Thinking",
                hover: THINKING,
                on: self.thinking_on(),
            });
        }
        if thinks || switch || thought {
            rows.push(Row {
                which: Capability::ShowThinking,
                name: "Show thinking",
                hover: SHOW_THINKING,
                on: self.stored.show_thinking(),
            });
        }
        rows
    }

    /// Capabilities: its head with how many are on, which opens a switch for each (closed at first, as Tools is).
    pub(super) fn show_capabilities(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let rows = self.capability_rows();
        let on = rows.iter().filter(|r| r.on).count();
        let open_id = egui::Id::new("capabilities_open");
        let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        let mut toggle = false;
        super::inspector::section_head(ui, Icon::Bolt, "Capabilities", Some(HOVER), |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(16.0, 24.0), egui::Sense::click());
            let words = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(format!("{on} of {} on", rows.len()))
                            .small()
                            .color(if open { p.text } else { p.muted }),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_text(if open {
                    "Hide the switches."
                } else {
                    "Show a switch for each."
                });
            let hovered = response.hovered() || words.hovered();
            super::tools::chevron(
                ui.painter(),
                rect.center(),
                open,
                if hovered { p.text } else { p.muted },
            );
            toggle = response.clicked() || words.clicked();
        });
        if toggle {
            ui.data_mut(|d| d.insert_temp(open_id, !open));
        }
        if !open {
            return;
        }
        let mut switched: Option<(Capability, bool)> = None;
        let mut share = self.stored.auto_compact_at();
        let mut share_chosen = false;
        for row in &rows {
            // the switch from the right, the name in what is left; what it does on hover of either
            ui.horizontal(|ui| {
                ui.set_min_height(28.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut ticked = row.on;
                    if theme::switch(ui, &mut ticked, true)
                        .on_hover_text(row.hover)
                        .changed()
                    {
                        switched = Some((row.which, ticked));
                    }
                    // the share of the context from which Auto-compact makes room, beside its switch while it is on
                    if row.which == Capability::AutoCompact && row.on {
                        egui::ComboBox::from_id_salt("auto_compact_at")
                            .selected_text(egui::RichText::new(format!("at {share}%")).small())
                            .width(64.0)
                            .show_ui(ui, |ui| {
                                for choice in ROOM_CHOICES {
                                    if ui
                                        .selectable_label(share == choice, format!("{choice}%"))
                                        .clicked()
                                    {
                                        share = choice;
                                        share_chosen = true;
                                    }
                                }
                            })
                            .response
                            .on_hover_text(AUTO_COMPACT_AT);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add_space(22.0);
                        ui.add(
                            egui::Label::new(egui::RichText::new(row.name).color(p.text))
                                .truncate(),
                        )
                        .on_hover_text(row.hover);
                    });
                });
            });
        }
        if let Some((which, on)) = switched {
            self.set_capability(which, on);
        }
        if share_chosen {
            self.set_auto_compact_at(share);
        }
    }

    /// How much of the context the conversation takes, for the context's chip while the chat talks to a run: the words
    /// (`12K of 64K`), what they say in full, and the share; `None` until the engine counted a request of it.
    pub(super) fn context_in_use(&self) -> Option<(String, String, f32)> {
        if !self.chat_running() || self.chat.turns.is_empty() {
            return None;
        }
        let used = self.chat.used?.tokens;
        let context = self.chat_context()?;
        let share = (used as f64 / context as f64).clamp(0.0, 1.0) as f32;
        Some((
            format!(
                "{} of {}",
                tokens_words(used),
                super::chat::context_words(i64::try_from(context).unwrap_or(i64::MAX))
            ),
            format!("This chat takes about {used} of the {context} tokens of the context."),
            share,
        ))
    }
}
