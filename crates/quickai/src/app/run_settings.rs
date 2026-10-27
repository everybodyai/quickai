//! How the model will run, in one line under its name, and what *Change* opens there (UI v2, C176): the Server box of
//! the page is gone, and with it the Configuration box over it.
//!
//! The line says it in words ("Balanced · 4K context · uses the graphics card"). *Change* opens, under it, the choices
//! an engine with a context size has as presets (A3): **Balanced** is the context the engine's document recommends
//! for this model and this computer (Ollama's own, which QuickAI cannot better), **Long chats** four times that (at
//! least 16K, at most 32K), **Light** 2K (half of Balanced when that is 2K or less); **Custom** is the boxes of the fields as they always were (the port, the
//! context size, the GPU). Under them, where the models run (once another computer can), and the configuration in use.
//! A preset sets the context size and nothing else, so what is chosen is read back from it, and the line names a preset
//! only when the context size is one.

use eframe::egui;
use quickai_core::form;
use quickai_core::form::FormField;

use super::icons::{self, Icon};
use super::theme;
use super::App;

/// The id of the field that the presets set.
const CONTEXT: &str = "ctx_size";

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

    pub(super) fn title(self) -> &'static str {
        match self {
            Preset::Balanced => "Balanced",
            Preset::Long => "Long chats",
            Preset::Light => "Light",
            Preset::Custom => "Custom",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Preset::Balanced => "Good for most chats. Suits this model and this computer.",
            Preset::Long => "Remembers more of a chat or a document. Needs more memory.",
            Preset::Light => "Uses the least memory. Remembers less of the chat.",
            Preset::Custom => "Set the numbers yourself.",
        }
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

    /// The line under the model's name: how it will run, with *Change* (and the panel it opens), and the configuration
    /// when it is not the Default, with *Modified* when what the window holds is not what it saved.
    pub(super) fn show_run_line(&mut self, ui: &mut egui::Ui) {
        let summary = self.run_summary();
        let configuration = self
            .active_entry()
            .filter(|e| e.id != crate::configurations::DEFAULT_ID)
            .map(|e| e.name);
        // the Default changes with every model picked: only a configuration the person saved is said to be modified
        // here (Change has the Default's Save and Revert)
        let modified = configuration.is_some() && self.configuration_modified();
        let mut toggle = false;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let p = theme::palette();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Sliders, p.faint);
            ui.label(theme::muted(summary.as_str()));
            if let Some(name) = &configuration {
                ui.label(theme::muted(format!("· {name}")));
            }
            if modified {
                theme::chip(ui, "Modified", Some(theme::Tone::Warn)).on_hover_text(
                    "What is set here is no longer what the configuration saved. Change has Save and Revert.",
                );
            }
            toggle = ui
                .link(if self.change_open { "Done" } else { "Change" })
                .on_hover_text("How the model runs, where, and the configuration in use.")
                .clicked();
        });
        if toggle {
            self.change_open = !self.change_open;
        }
        if self.change_open {
            ui.add_space(6.0);
            theme::card(ui, |ui| self.show_change_panel(ui));
        }
    }

    /// What *Change* opens: the configuration in use, the presets (or the fields, for an engine with none), and where the
    /// models run.
    fn show_change_panel(&mut self, ui: &mut egui::Ui) {
        let running = self.session.is_active();
        let presets = self.context_field().is_some() && self.remote_in_use().is_none();
        // the configuration first: it is what the rest belongs to (and so it is in one place whatever is under it)
        self.show_configuration_section(ui);
        ui.add_space(8.0);
        if presets {
            let current = if self.change_custom {
                Preset::Custom
            } else {
                self.current_preset()
            };
            ui.label(
                egui::RichText::new("HOW SHOULD IT RUN?")
                    .size(11.0)
                    .family(theme::semibold(ui))
                    .color(theme::palette().faint),
            );
            let mut chosen: Option<Preset> = None;
            ui.add_enabled_ui(!running, |ui| {
                for preset in Preset::ALL {
                    let numbers = match self.preset_context(preset) {
                        Some(Some(tokens)) => format!(" ({} context)", context_words(tokens)),
                        _ => String::new(),
                    };
                    if preset_row(
                        ui,
                        current == preset,
                        preset.title(),
                        &format!("{}{numbers}", preset.description()),
                    )
                    .clicked()
                    {
                        chosen = Some(preset);
                    }
                }
            });
            if let Some(preset) = chosen {
                self.change_custom = preset == Preset::Custom;
                self.apply_preset(preset);
            }
            if current == Preset::Custom {
                ui.add_space(4.0);
                self.show_controls(ui);
            }
        } else {
            self.show_controls(ui);
        }
        if running {
            ui.label(theme::muted("Stop the model to change these.").small());
        }
        if self.can_run_remote() {
            ui.add_space(6.0);
            ui.separator();
            if self.remote.remotes.is_empty() {
                if ui
                    .link("Run on another computer...")
                    .on_hover_text(
                        "Use the engine of a more powerful computer, reached over SSH. The chat and Connect an app \
                         work the same.",
                    )
                    .clicked()
                {
                    self.remote.show_window = true;
                }
            } else {
                self.show_runs_on_row(ui);
            }
        }
    }
}

/// One choice of the presets: a round button, its name, and under it what it does.
fn preset_row(ui: &mut egui::Ui, on: bool, title: &str, said: &str) -> egui::Response {
    let p = theme::palette();
    let enabled = ui.is_enabled();
    let width = ui.available_width().min(560.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if on {
            ui.painter().rect_filled(rect, 6.0, p.accent_soft);
        } else if response.hovered() && enabled {
            ui.painter()
                .rect_filled(rect, 6.0, p.raised_hover.gamma_multiply(0.6));
        }
        let centre = egui::pos2(rect.left() + 16.0, rect.top() + 13.0);
        ui.painter().circle_stroke(
            centre,
            7.0,
            egui::Stroke::new(1.5, if on { p.accent_text } else { p.outline }),
        );
        if on {
            ui.painter().circle_filled(centre, 3.5, p.accent_text);
        }
        let ink = if enabled { p.text } else { p.faint };
        let title = ui.painter().layout_no_wrap(
            title.to_string(),
            egui::TextStyle::Button.resolve(ui.style()),
            ink,
        );
        let small = ui.painter().layout_no_wrap(
            said.to_string(),
            egui::TextStyle::Small.resolve(ui.style()),
            p.muted,
        );
        let left = rect.left() + 32.0;
        ui.painter()
            .galley(egui::pos2(left, rect.top() + 4.0), title, ink);
        ui.painter()
            .galley(egui::pos2(left, rect.top() + 22.0), small, p.muted);
    }
    response
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
