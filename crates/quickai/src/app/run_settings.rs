//! How the model runs, in the panel at the right (C206, A2 of `the mockups`, the user's pick of 2026-11-09):
//! how much of the chat it remembers as one row of three choices, **Light**, **Balanced** and **Long**, with a line of
//! what the chosen one means, and *Set the numbers yourself…* for the fields (Custom). **Balanced** is the context the
//! engine's document recommends for this model and this computer (Ollama's own, which QuickAI cannot better), **Long**
//! four times that (at least 16K, at most 32K), **Light** 2K (half of Balanced when that is 2K or less).
//! A choice sets the context size and nothing else. It is kept as the choice (`app.run_choice`), not only as the number:
//! another model picked runs the same way with its own number, and the next start works it out again. A first start's
//! 4K, which nobody chose, is Balanced.

use eframe::egui;
use quickai_core::form;
use quickai_core::form::FormField;

use super::theme;
use super::App;

/// The id of the field that the presets set.
const CONTEXT: &str = "ctx_size";

/// The context of a first start (the settings' own), which the person did not choose.
const FIRST_RUN_CONTEXT: i64 = 4096;

/// One of the choices of how the model runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Preset {
    Balanced,
    Long,
    Light,
    Custom,
}

impl Preset {
    pub(super) const ALL: [Preset; 4] = [
        Preset::Balanced,
        Preset::Long,
        Preset::Light,
        Preset::Custom,
    ];

    /// The three that set the number, as the row of the panel has them: the least memory first.
    const ROW: [Preset; 3] = [Preset::Light, Preset::Balanced, Preset::Long];

    pub(super) fn title(self) -> &'static str {
        match self {
            Preset::Balanced => "Balanced",
            Preset::Long => "Long chats",
            Preset::Light => "Light",
            Preset::Custom => "Custom",
        }
    }

    /// Its word in the row of the panel.
    fn short(self) -> &'static str {
        match self {
            Preset::Long => "Long",
            other => other.title(),
        }
    }

    fn description(self) -> &'static str {
        match self {
            Preset::Balanced => "Suits this model and this computer.",
            Preset::Long => "Remembers more of a chat or a document, and needs more memory.",
            Preset::Light => "Uses the least memory, and remembers less of the chat.",
            Preset::Custom => "Set the numbers yourself.",
        }
    }

    /// How it is kept in the settings (`app.run_choice`).
    pub(super) fn key(self) -> &'static str {
        match self {
            Preset::Balanced => "balanced",
            Preset::Long => "long",
            Preset::Light => "light",
            Preset::Custom => "custom",
        }
    }

    pub(super) fn from_key(key: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// A number of tokens of context as a person reads it: `4K`, `16K`, or the number when it is not a round one.
pub(super) fn context_words(tokens: i64) -> String {
    if tokens >= 1024 && tokens % 1024 == 0 {
        format!("{}K", tokens / 1024)
    } else {
        tokens.to_string()
    }
}

impl App {
    /// The engine's context-size field, when it has one that is shown: only then are there presets.
    fn context_field(&self) -> Option<FormField> {
        let form = self.form()?;
        form.section("primary")?
            .fields
            .iter()
            .find(|f| f.id == CONTEXT && f.visible)
            .cloned()
    }

    /// The context size the engine's document recommends for the chosen model on this computer; `None` when it has no
    /// recommendation of its own (Ollama's is the daemon's, and is left to it).
    fn recommended_context(&self) -> Option<i64> {
        let edits = form::recompute(
            &self.registry,
            &self.form_selection(),
            &self.form_facts(),
            "model",
        )
        .ok()?;
        let key = self.values.key(CONTEXT);
        edits
            .into_iter()
            .find(|e| e.key == key)
            .and_then(|e| e.value.as_i64())
    }

    /// The context size of a preset: `Some(None)` leaves it to the engine; `None` for Custom.
    pub(super) fn preset_context(&self, preset: Preset) -> Option<Option<i64>> {
        let recommended = self.recommended_context();
        let range = self
            .context_field()
            .and_then(|f| f.range)
            .map_or((256, 131072), |(low, high)| (low as i64, high as i64));
        let fit = |n: i64| n.clamp(range.0, range.1);
        match preset {
            Preset::Balanced => Some(recommended),
            Preset::Long => Some(Some(fit(
                recommended.map_or(16384, |r| (r * 4).clamp(16384, 32768))
            ))),
            // less than Balanced, always: half of it when it is 2K or less
            Preset::Light => Some(Some(fit(
                recommended.map_or(2048, |r| 2048.min(r / 2).max(512))
            ))),
            Preset::Custom => None,
        }
    }

    /// The preset that the context size in force is; Custom when it is none of them.
    pub(super) fn current_preset(&self) -> Preset {
        let Some(field) = self.context_field() else {
            return Preset::Custom;
        };
        let now = field.value.as_i64();
        [Preset::Balanced, Preset::Long, Preset::Light]
            .into_iter()
            .find(|p| self.preset_context(*p) == Some(now))
            .unwrap_or(Preset::Custom)
    }

    /// Sets the context size of `preset`.
    pub(super) fn apply_preset(&mut self, preset: Preset) {
        match self.preset_context(preset) {
            Some(Some(tokens)) => self.values.set(CONTEXT, tokens),
            Some(None) => self.values.clear(CONTEXT),
            None => {}
        }
    }

    /// The choice the panel shows: Custom when the person asked for their own numbers, else the preset the numbers are.
    pub(super) fn shown_preset(&self) -> Preset {
        if self.change_custom || self.run_choice == Some(Preset::Custom) {
            Preset::Custom
        } else {
            self.current_preset()
        }
    }

    /// Makes `preset` the way the model runs: its number, and the choice kept for the next model and the next start.
    pub(super) fn choose_preset(&mut self, preset: Preset) {
        self.change_custom = preset == Preset::Custom;
        self.apply_preset(preset);
        self.run_choice = Some(preset);
    }

    /// After another model was picked and its defaults were set: the choice kept is applied to it, so that it runs the
    /// same way with its own number (Long is Long for every model); the person's own numbers stay (`kept`, the context
    /// before the model was picked).
    pub(super) fn reapply_run_choice(&mut self, kept: Option<quickai_core::value::Value>) {
        if self.context_field().is_none() || self.remote_in_use().is_some() {
            return;
        }
        match self.run_choice {
            Some(Preset::Custom) => match kept {
                Some(value) => self.values.set(CONTEXT, value),
                None => self.values.clear(CONTEXT),
            },
            Some(preset) => self.apply_preset(preset),
            None => {}
        }
    }

    /// The context as the window holds it now, for `reapply_run_choice`.
    pub(super) fn context_now(&self) -> Option<quickai_core::value::Value> {
        self.values.get(CONTEXT).cloned()
    }

    /// At the start: the choice kept is taken up, its number worked out again for this model and this computer. With
    /// none kept (an earlier version, or a first start), the numbers say which it is; the context a first start has
    /// (4K, which nobody chose) is Balanced, and other numbers that are no preset are Custom.
    pub(super) fn take_up_run_choice(&mut self) {
        let kept = self.stored.run_choice().and_then(|k| Preset::from_key(&k));
        if self.context_field().is_none() || self.remote_in_use().is_some() {
            self.run_choice = kept;
            return;
        }
        let choice = kept.unwrap_or_else(|| match self.current_preset() {
            Preset::Custom if self.values.int(CONTEXT) == Some(FIRST_RUN_CONTEXT) => {
                Preset::Balanced
            }
            now => now,
        });
        self.choose_preset(choice);
    }

    /// How the model will run, in words.
    pub(super) fn run_summary(&self) -> String {
        if let Some(remote) = self.remote_in_use() {
            return format!("Runs on {}", remote.name);
        }
        let mut words = Vec::new();
        if let Some(field) = self.context_field() {
            // a preset is named when the numbers are one; other numbers are said as they are (a first run's 4K is
            // not something the person chose as "Custom")
            let preset = self.current_preset();
            if preset != Preset::Custom {
                words.push(preset.title().to_string());
            }
            words.push(match field.value.as_i64() {
                Some(tokens) => format!("{} context", context_words(tokens)),
                None => format!("{}'s own context", self.engine_name()),
            });
            words.push(if self.gpu_offload() {
                "uses the graphics card".to_string()
            } else {
                "CPU only".to_string()
            });
        } else if let Some(form) = self.form() {
            // an engine with no context (ComfyUI): its fields, as they are
            if let Some(section) = form.section("primary") {
                for field in section.fields.iter().filter(|f| f.visible) {
                    let value = match &field.value {
                        v if v.is_null() => continue,
                        v => v
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| v.as_i64().map(|n| n.to_string()))
                            .unwrap_or_else(|| v.to_string()),
                    };
                    words.push(format!("{} {value}", field.label));
                }
            }
        }
        words.join(" · ")
    }

    /// What is wrong with the fields of Change (the rules of the engine's document that a value breaks), a line each.
    pub(super) fn field_problems(&self) -> Vec<String> {
        let Some(form) = self.form() else {
            return Vec::new();
        };
        let Some(section) = form.section("primary") else {
            return Vec::new();
        };
        section
            .fields
            .iter()
            .filter(|f| f.visible)
            .flat_map(|f| super::widgets::problem_lines(f.error.as_deref()))
            .map(str::to_string)
            .collect()
    }

    /// How the model runs (A2 of `the mockups`, the user's pick of 2026-11-09): the configuration in one line,
    /// then how much of the chat it remembers as one row of three (Light, Balanced, Long) with a line of what the chosen
    /// one means, and *Set the numbers yourself…* for the fields (Custom, with the way back). An engine with no context
    /// (ComfyUI), or one on another computer, has its fields.
    pub(super) fn show_change_panel(&mut self, ui: &mut egui::Ui) {
        let running = self.session.is_active();
        let presets = self.context_field().is_some() && self.remote_in_use().is_none();
        // the configuration first: it is what the rest belongs to
        self.show_configuration_section(ui);
        ui.add_space(14.0);
        if !presets {
            self.show_controls(ui);
        } else {
            let p = theme::palette();
            let current = self.shown_preset();
            let custom = current == Preset::Custom;
            ui.horizontal(|ui| {
                ui.label(theme::muted("How much of the chat it remembers").small());
                if custom {
                    pill(ui, "Custom", p.warn);
                }
            });
            ui.add_space(2.0);
            let cells: Vec<(Preset, String)> = Preset::ROW
                .into_iter()
                .map(|preset| {
                    let numbers = match self.preset_context(preset) {
                        Some(Some(tokens)) => format!(" ({} context)", context_words(tokens)),
                        _ => String::new(),
                    };
                    (
                        preset,
                        format!("{}: {}{numbers}", preset.title(), preset.description()),
                    )
                })
                .collect();
            let mut chosen: Option<Preset> = None;
            ui.add_enabled_ui(!running, |ui| {
                chosen = choice_row(ui, &cells, (!custom).then_some(current));
            });
            if custom {
                ui.add_space(6.0);
                self.show_controls(ui);
                ui.add_space(4.0);
                if ui
                    .add_enabled(!running, egui::Link::new("Back to Balanced"))
                    .on_hover_text(Preset::Balanced.description())
                    .clicked()
                {
                    chosen = Some(Preset::Balanced);
                }
            } else {
                ui.add_space(7.0);
                let numbers = match self.context_field().and_then(|f| f.value.as_i64()) {
                    Some(tokens) => format!("{} context", context_words(tokens)),
                    None => format!("{}'s own context", self.engine_name()),
                };
                let device = if self.gpu_offload() {
                    "uses the graphics card"
                } else {
                    "runs on the processor only"
                };
                said_line(
                    ui,
                    &numbers,
                    &format!(", {device}. {}", current.description()),
                );
                ui.add_space(8.0);
                if ui
                    .add_enabled(!running, egui::Link::new("Set the numbers yourself…"))
                    .on_hover_text(
                        "The context, the port and the graphics card, as numbers of your own.",
                    )
                    .clicked()
                {
                    chosen = Some(Preset::Custom);
                }
            }
            if let Some(preset) = chosen {
                self.choose_preset(preset);
            }
        }
        if running {
            ui.add_space(4.0);
            ui.label(theme::muted("Stop the model to change these.").small());
        }
    }
}

/// The row of choices: one box cut in equal parts, the chosen one lit in the accent. Each part says on hover what it
/// does. What was clicked comes back.
fn choice_row(ui: &mut egui::Ui, cells: &[(Preset, String)], on: Option<Preset>) -> Option<Preset> {
    let p = theme::palette();
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        9.0,
        p.card,
        egui::Stroke::new(1.0, p.line),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(3.0);
    let gap = 3.0;
    let count = cells.len().max(1) as f32;
    let each = (inner.width() - gap * (count - 1.0)) / count;
    let mut chosen = None;
    for (i, (preset, hover)) in cells.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(inner.left() + i as f32 * (each + gap), inner.top()),
            egui::vec2(each, inner.height()),
        );
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let response = ui
            .interact(cell, ui.id().with(("run_choice", i)), sense)
            .on_hover_text(hover);
        let lit = on == Some(*preset);
        if lit {
            painter.rect(
                cell,
                6.0,
                p.accent_soft,
                egui::Stroke::new(1.0, p.accent_text.gamma_multiply(0.45)),
                egui::StrokeKind::Inside,
            );
        } else if enabled && response.hovered() {
            painter.rect_filled(cell, 6.0, p.raised_hover.gamma_multiply(0.6));
        }
        let color = match (lit, enabled) {
            (true, _) => p.accent_text,
            (false, true) => p.muted,
            (false, false) => p.faint,
        };
        let mut font = egui::TextStyle::Body.resolve(ui.style());
        if lit {
            font.family = theme::semibold(ui);
        }
        let galley = painter.layout_no_wrap(preset.short().to_string(), font, color);
        painter.galley(cell.center() - galley.size() / 2.0, galley, color);
        if response.clicked() {
            chosen = Some(*preset);
        }
    }
    chosen
}

/// A word in a thin rounded frame of `color` (Custom, beside the label of the row).
fn pill(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 14.0, galley.size().y + 2.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_stroke(
        rect,
        rect.height() / 2.0,
        egui::Stroke::new(1.0, color.gamma_multiply(0.55)),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// The line under the row: the number in the colour of the text, then what the choice means, wrapped to the panel.
fn said_line(ui: &mut egui::Ui, numbers: &str, rest: &str) {
    let p = theme::palette();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(numbers, 0.0, egui::TextFormat::simple(font.clone(), p.body));
    job.append(rest, 0.0, egui::TextFormat::simple(font, p.muted));
    job.wrap.max_width = ui.available_width();
    ui.label(job);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// An application of `engine` with one model of `bytes`, on a computer of 16 GB.
    fn app_with_model(dir: &Path, engine: &str, bytes: u64) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::File::create(dir.join("models/Model-Q4_K_M.gguf"))
            .unwrap()
            .set_len(bytes)
            .unwrap();
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        app.bundled_models_dir = dir.join("bundled");
        app.models = app.scan_models();
        app.selected = 0;
        app
    }

    #[test]
    fn a_context_is_said_in_thousands_when_it_is_a_round_number_of_them() {
        assert_eq!(context_words(4096), "4K");
        assert_eq!(context_words(16384), "16K");
        assert_eq!(context_words(131072), "128K");
        assert_eq!(context_words(3000), "3000");
        assert_eq!(context_words(512), "512");
    }

    #[test]
    fn the_presets_of_llamacpp_are_the_recommended_context_four_times_it_and_2k() {
        let dir = scratch("presets-llama");
        // a 1 GB model on 16 GB: the document recommends 16K
        let mut app = app_with_model(&dir, "llamacpp", GB);
        assert_eq!(app.preset_context(Preset::Balanced), Some(Some(16384)));
        assert_eq!(app.preset_context(Preset::Long), Some(Some(32768)));
        assert_eq!(app.preset_context(Preset::Light), Some(Some(2048)));
        assert_eq!(app.preset_context(Preset::Custom), None);

        // a first run's 4K is none of them, and the line does not call it Custom: it says the numbers
        assert_eq!(app.current_preset(), Preset::Custom);
        let said = app.run_summary();
        assert_eq!(said, "4K context · uses the graphics card");

        for (preset, said) in [
            (Preset::Balanced, "Balanced · 16K context"),
            (Preset::Long, "Long chats · 32K context"),
            (Preset::Light, "Light · 2K context"),
        ] {
            app.apply_preset(preset);
            assert_eq!(app.current_preset(), preset);
            assert!(app.run_summary().starts_with(said), "{}", app.run_summary());
        }
        // a preset sets the context and nothing else: no GPU is said as such
        app.values.set("n_gpu_layers", 0i64);
        assert!(
            app.run_summary().ends_with("CPU only"),
            "{}",
            app.run_summary()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bigger_model_has_a_smaller_balanced_context_and_light_is_always_less() {
        let dir = scratch("presets-big");
        // a 3 GB model on 16 GB: 8K is Balanced
        let app = app_with_model(&dir, "llamacpp", 3 * GB);
        assert_eq!(app.preset_context(Preset::Balanced), Some(Some(8192)));
        assert_eq!(app.preset_context(Preset::Long), Some(Some(32768)));
        assert_eq!(app.preset_context(Preset::Light), Some(Some(2048)));
        // a 6 GB model on 16 GB: 2K is Balanced, Long chats is at least 16K, and Light is half of Balanced
        let app = app_with_model(&dir, "llamacpp", 6 * GB);
        assert_eq!(app.preset_context(Preset::Balanced), Some(Some(2048)));
        assert_eq!(app.preset_context(Preset::Long), Some(Some(16384)));
        assert_eq!(app.preset_context(Preset::Light), Some(Some(1024)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ollamas_balanced_is_its_own_context_and_the_others_set_one() {
        let dir = scratch("presets-ollama");
        let mut app = app_with_model(&dir, "ollama", GB);
        assert_eq!(app.preset_context(Preset::Balanced), Some(None));
        assert_eq!(app.current_preset(), Preset::Balanced);
        assert!(
            app.run_summary()
                .starts_with("Balanced · Ollama's own context"),
            "{}",
            app.run_summary()
        );
        app.apply_preset(Preset::Long);
        assert_eq!(app.values.int("ctx_size"), Some(16384));
        app.apply_preset(Preset::Balanced);
        assert_eq!(app.values.get("ctx_size"), None, "left to Ollama again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_engine_with_no_context_says_its_fields_and_has_no_presets() {
        let dir = scratch("presets-comfy");
        let app = app_with_model(&dir, "comfyui", GB);
        assert_eq!(app.current_preset(), Preset::Custom);
        let said = app.run_summary();
        assert!(said.contains("Port"), "{said}");
        assert!(!said.contains("context"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
