//! How the model runs, in the panel at the right (C212, P1 of `the mockups`, the user's pick of
//! 2026-11-09): the context as a grid of lengths, **4K** to **256K** and **Other…**, each with a dot of where it would
//! fit (on the graphics card, partly in the computer's memory, or not at all) and the lengths past what the model was
//! trained for dashed; then Graphics (`graphics.rs`). It took the place of Light, Balanced and Long, which stopped at
//! 32K.
//!
//! The choice is kept (`app.run_choice`), not only the number: **the suggested length** (the one with the mark, the
//! longest of 4K to 32K that fits on the card, or comfortably in the memory) is worked out again for each model; a
//! length chosen stays for the next model as far as it takes it; **Other…** keeps its number. Ollama's suggestion is
//! its own context, which QuickAI leaves to it. A first start's 4K, which nobody chose, is the suggested one.

use eframe::egui;
use quickai_core::form;
use quickai_core::form::FormField;

use super::graphics::Fit;
use super::icons::Icon;
use super::theme;
use super::App;

/// The id of the field that the grid sets.
const CONTEXT: &str = "ctx_size";

/// The context of a first start (the settings' own), which the person did not choose.
const FIRST_RUN_CONTEXT: i64 = 4096;

/// The lengths of the grid, in tokens.
const LENGTHS: [i64; 7] = [4096, 8192, 16384, 32768, 65536, 131072, 262144];

/// How the person chose the context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunChoice {
    /// The length suited to this model on this computer (the mark on the grid), worked out again for each model; for
    /// Ollama, its own.
    Suggested,
    /// A length of the grid, kept for the next model as far as it takes it.
    Length(i64),
    /// A number of the person's own (Other…).
    Own,
}

impl RunChoice {
    /// How it is kept in the settings (`app.run_choice`): Balanced, Long and Light were kept as words, and are read so.
    pub(super) fn key(self) -> String {
        match self {
            RunChoice::Suggested => "balanced".to_string(),
            RunChoice::Length(n) => n.to_string(),
            RunChoice::Own => "custom".to_string(),
        }
    }

    pub(super) fn from_key(key: &str) -> Option<RunChoice> {
        match key {
            "balanced" => Some(RunChoice::Suggested),
            "long" => Some(RunChoice::Length(32768)),
            "light" => Some(RunChoice::Length(4096)),
            "custom" => Some(RunChoice::Own),
            n => n.parse().ok().filter(|n| *n > 0).map(RunChoice::Length),
        }
    }
}

/// A number of tokens of context as a person reads it: `4K`, `16K`, `128K` (for 131072, and for the 128000 a model
/// says it takes), or the number when it is not a round one.
pub(super) fn context_words(tokens: i64) -> String {
    if tokens >= 1000 && tokens % 1000 == 0 {
        format!("{}K", tokens / 1000)
    } else if tokens >= 1024 && tokens % 1024 == 0 {
        format!("{}K", tokens / 1024)
    } else {
        tokens.to_string()
    }
}

/// The number a length of the grid sets when the model takes up to `most`: the length, or `most` for one a little past it
/// (128K is 131072, and a model says 128000).
fn length_at(n: i64, most: i64) -> Option<i64> {
    if n <= most {
        Some(n)
    } else if n as f64 <= most as f64 * 1.05 {
        Some(most)
    } else {
        None
    }
}

/// A number of bytes in GB, as the hovers of the grid say it.
fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

impl App {
    /// The engine's context-size field, when it has one that is shown: only then is there a grid.
    pub(super) fn context_field(&self) -> Option<FormField> {
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

    /// Whether the panel shows the engine's field `id`: with the grid, the context and the graphics (the port and the
    /// rest of its primary fields are on the Settings' page of how it starts, C212); without it, every primary field.
    pub(super) fn panel_shows_field(&self, id: &str) -> bool {
        if self.context_field().is_none() || self.remote_in_use().is_some() {
            return true;
        }
        matches!(id, "ctx_size" | "n_gpu_layers" | "gpu")
    }

    /// The fields of the engine's primary section the panel does not show, for the Settings' page of how it starts; with
    /// them the GPU layers, as a number (the panel's Graphics says it as a choice, and has no box for it when no card is
    /// found).
    pub(super) fn primary_fields_elsewhere(&self) -> Vec<FormField> {
        let Some(section) = self.form().and_then(|f| f.section("primary").cloned()) else {
            return Vec::new();
        };
        section
            .fields
            .into_iter()
            .filter(|f| f.visible && (!self.panel_shows_field(&f.id) || f.id == "n_gpu_layers"))
            .collect()
    }

    /// The longest context there is: the field's most, and what the model was trained for when its file says.
    pub(super) fn max_context(&self) -> i64 {
        let field = self
            .context_field()
            .and_then(|f| f.range)
            .map_or(131072, |(_, high)| high as i64);
        match self.model_facts().and_then(|f| f.context_length) {
            Some(trained) => field.min(trained as i64),
            None => field,
        }
    }

    /// The suggested length: the longest of 4K to 32K that fits on the card (or comfortably in the memory) when the
    /// model's file says what it needs, else what the engine's document recommends; `None` leaves it to the engine.
    pub(super) fn suggested_context(&self) -> Option<i64> {
        let recommended = self.recommended_context()?;
        if self.need().is_none() {
            return Some(recommended);
        }
        let most = self.max_context();
        let fits = [32768, 16384, 8192, 4096]
            .into_iter()
            .filter(|n| *n <= most)
            .find(|n| self.fit(*n as u64) == Some(Fit::Good));
        Some(fits.unwrap_or(FIRST_RUN_CONTEXT.min(most)))
    }

    /// The choice the numbers are: the suggested length, a length of the grid, or a number of the person's own.
    pub(super) fn choice_now(&self) -> RunChoice {
        let now = self.values.int(CONTEXT);
        match now {
            None => RunChoice::Suggested,
            Some(n) if Some(n) == self.suggested_context() => RunChoice::Suggested,
            Some(n) if self.grid_index(n).is_some() => {
                RunChoice::Length(LENGTHS[self.grid_index(n).unwrap_or_default()])
            }
            Some(_) => RunChoice::Own,
        }
    }

    /// Sets the context of `choice`: the suggested length, a length as far as the model takes it, or the number as it
    /// is.
    fn apply_choice(&mut self, choice: RunChoice) {
        match choice {
            RunChoice::Suggested => match self.suggested_context() {
                Some(n) => self.values.set(CONTEXT, n),
                None => self.values.clear(CONTEXT),
            },
            RunChoice::Length(n) => {
                let most = self.max_context();
                self.values
                    .set(CONTEXT, length_at(n, most).unwrap_or(most).min(n));
            }
            RunChoice::Own => {}
        }
    }

    /// The person chose a length of the grid: the suggested one is kept as such (worked out again for the next model).
    pub(super) fn choose_length(&mut self, tokens: i64) {
        let choice = if Some(tokens) == self.suggested_context() {
            RunChoice::Suggested
        } else {
            RunChoice::Length(tokens)
        };
        self.change_custom = false;
        let at = length_at(tokens, self.max_context()).unwrap_or(tokens);
        self.values.set(CONTEXT, at);
        self.run_choice = Some(choice);
        self.auto_fit();
    }

    /// The box of the grid a number is, when it is one (128000 is 128K's for a model that takes that much).
    fn grid_index(&self, n: i64) -> Option<usize> {
        let most = self.max_context();
        LENGTHS.iter().position(|l| length_at(*l, most) == Some(n))
    }

    /// After another model was picked and its defaults were set: the choice kept is applied to it (a length is that
    /// length for every model that takes it); the person's own number stays (`kept`, the context before).
    pub(super) fn reapply_run_choice(&mut self, kept: Option<quickai_core::value::Value>) {
        if self.context_field().is_none() || self.remote_in_use().is_some() {
            return;
        }
        // where it runs first, for this model: the suggested length is what fits there
        self.gpu_auto = true;
        self.auto_fit();
        match self.run_choice {
            Some(RunChoice::Own) => match kept {
                Some(value) => self.values.set(CONTEXT, value),
                None => self.values.clear(CONTEXT),
            },
            Some(choice) => self.apply_choice(choice),
            None => {}
        }
        self.auto_fit();
    }

    /// The context as the window holds it now, for `reapply_run_choice`.
    pub(super) fn context_now(&self) -> Option<quickai_core::value::Value> {
        self.values.get(CONTEXT).cloned()
    }

    /// At the start: the choice kept is taken up, its number worked out again for this model and this computer. With
    /// none kept (an earlier version, or a first start), the numbers say which it is, as they are: a 4K that nobody chose
    /// stays 4K, since the configuration in use says 4K too (the suggested length has its dot in the grid).
    pub(super) fn take_up_run_choice(&mut self) {
        self.refresh_facts();
        let kept = self
            .stored
            .run_choice()
            .and_then(|k| RunChoice::from_key(&k));
        if self.context_field().is_none() || self.remote_in_use().is_some() {
            self.run_choice = kept;
            return;
        }
        let choice = kept.unwrap_or_else(|| self.choice_now());
        self.change_custom = choice == RunChoice::Own;
        self.apply_choice(choice);
        self.run_choice = Some(choice);
        self.derived_choice = if kept.is_some() { None } else { Some(choice) };
    }

    /// The choice `app.run_choice` keeps: none while it is still the one the numbers said (a file nobody chose in gets
    /// no key it did not have), the choice once the person made another.
    pub(super) fn run_choice_to_keep(&self) -> Option<RunChoice> {
        self.run_choice.filter(|c| Some(*c) != self.derived_choice)
    }

    /// How the model will run, in words.
    pub(super) fn run_summary(&self) -> String {
        if let Some(remote) = self.remote_in_use() {
            return format!("Runs on {}", remote.name);
        }
        let mut words = Vec::new();
        if let Some(field) = self.context_field() {
            words.push(match field.value.as_i64() {
                Some(tokens) => format!("{} context", context_words(tokens)),
                None => format!("{}'s own context", self.engine_name()),
            });
            words.push(match self.place_words() {
                Some(place) => place,
                None if self.gpu_offload() => "uses the graphics card".to_string(),
                None => "CPU only".to_string(),
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

    /// What is wrong with the fields of the panel (the rules of the engine's document that a value breaks), a line each.
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

    /// How the model runs (P1 of `the mockups`): the configuration in one line, the context as
    /// a grid of lengths, and Graphics. An engine with no context (ComfyUI), or one on another computer, has its fields.
    pub(super) fn show_change_panel(&mut self, ui: &mut egui::Ui) {
        let running = self.session.is_active();
        let grid = self.context_field().is_some() && self.remote_in_use().is_none();
        // the configuration first: it is what the rest belongs to
        self.show_configuration_section(ui);
        ui.add_space(6.0);
        if !grid {
            ui.add_space(8.0);
            self.show_controls(ui);
            return;
        }
        self.show_context_grid(ui, !running);
        ui.add_space(4.0);
        self.show_graphics_section(ui);
        if running {
            ui.add_space(4.0);
            ui.label(theme::muted("Stop the model to change these.").small());
        }
    }

    /// Context: the grid of lengths with their dots, and the number of the person's own under it.
    fn show_context_grid(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let p = theme::palette();
        let now = self.values.int(CONTEXT);
        let most = self.max_context();
        let trained = self.model_facts().and_then(|f| f.context_length);
        let suggested = self.suggested_context();
        let own = self.change_custom
            || self.run_choice == Some(RunChoice::Own)
            || now.is_some_and(|n| self.grid_index(n).is_none());
        let engine_decides = now.is_none();
        let mut back_to_engine = false;
        super::inspector::section_head(
            ui,
            Icon::Chat,
            "Context",
            Some(
                "How much of the chat the model remembers: what was said, and the documents put in it. Longer takes \
                 more memory. The dot says where each length would fit with the model.",
            ),
            |ui| {
                if engine_decides {
                    ui.label(theme::muted(format!("{} decides", self.engine_name())).small());
                } else if suggested.is_none() {
                    back_to_engine = ui
                        .add_enabled(
                            enabled,
                            egui::Link::new(egui::RichText::new(format!("{}'s own", self.engine_name())).small()),
                        )
                        .on_hover_text("Leave the context to the engine again.")
                        .clicked();
                } else if let Some(trained) = trained {
                    ui.label(
                        theme::muted(format!("model takes up to {}", context_words(trained as i64))).small(),
                    );
                }
            },
        );
        let mut cells: Vec<Cell> = LENGTHS
            .iter()
            .map(|&n| {
                let at = length_at(n, most);
                let off = at.is_none();
                let fit = at.and_then(|at| self.fit(at as u64));
                Cell {
                    text: context_words(n),
                    fit,
                    off,
                    marked: suggested == Some(n),
                    hover: self.length_hover(n, off, fit, suggested == Some(n)),
                }
            })
            .collect();
        cells.push(Cell {
            // a number of the person's own is said on its box; one that is a length of the grid is that box's
            text: match now.filter(|n| own && self.grid_index(*n).is_none()) {
                Some(n) => context_words(n),
                None => "Other…".to_string(),
            },
            fit: if own {
                now.and_then(|n| self.fit(n as u64))
            } else {
                None
            },
            off: false,
            marked: false,
            hover: "A number of tokens of your own.".to_string(),
        });
        let lit = if own {
            Some(LENGTHS.len())
        } else {
            now.and_then(|n| self.grid_index(n))
        };
        let chosen = ui
            .add_enabled_ui(enabled, |ui| length_grid(ui, &cells, lit))
            .inner;
        match chosen {
            Some(i) if i < LENGTHS.len() => self.choose_length(LENGTHS[i]),
            Some(_) => {
                self.change_custom = true;
                self.run_choice = Some(RunChoice::Own);
                if now.is_none() {
                    self.values
                        .set(CONTEXT, suggested.unwrap_or(FIRST_RUN_CONTEXT));
                }
            }
            None => {}
        }
        if back_to_engine {
            self.values.clear(CONTEXT);
            self.change_custom = false;
            self.run_choice = Some(RunChoice::Suggested);
        }
        // the dots, said once
        if cells.iter().any(|c| c.fit.is_some()) {
            let cards = !self.ticked_cards().is_empty()
                && self.place() != super::graphics::Place::Processor;
            let words = if cards {
                ["on the card", "partly in memory", "too big"]
            } else {
                ["fits", "tight", "too big"]
            };
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                for (word, color) in words.iter().zip([p.accent_text, p.warn, p.bad]) {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, color);
                    ui.label(egui::RichText::new(*word).size(11.0).color(p.faint));
                    ui.add_space(4.0);
                }
            });
        }
        // Other…: the number
        if own {
            if let Some(mut n) = self.values.int(CONTEXT) {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let range = self
                        .context_field()
                        .and_then(|f| f.range)
                        .unwrap_or((256.0, 131072.0));
                    let changed = ui
                        .add_enabled(
                            enabled,
                            egui::DragValue::new(&mut n)
                                .range(range.0 as i64..=range.1 as i64)
                                .speed(64.0)
                                .suffix(" tokens"),
                        )
                        .on_hover_text("Drag, or click and type a number.")
                        .changed();
                    if changed {
                        self.values.set(CONTEXT, n);
                        self.auto_fit();
                    }
                    if let Some(trained) = trained.filter(|t| n as u64 > *t) {
                        ui.label(
                            egui::RichText::new(format!(
                                "past the {} it was trained for",
                                context_words(trained as i64)
                            ))
                            .small()
                            .color(p.warn),
                        );
                    }
                });
            }
        }
    }

    /// What a length's box says when the pointer rests on it: what it takes, and where that fits.
    fn length_hover(&self, tokens: i64, off: bool, fit: Option<Fit>, marked: bool) -> String {
        let words = context_words(tokens);
        if off {
            return match self.model_facts().and_then(|f| f.context_length) {
                Some(trained) => format!(
                    "{words}: past the {} this model was trained for.",
                    context_words(trained as i64)
                ),
                None => format!("{words}: more than {} takes.", self.engine_name()),
            };
        }
        let mut said = match (self.need(), fit) {
            (Some(need), Some(fit)) => {
                let chat = need.per_token.saturating_mul(tokens as u64);
                let total = need.weights + chat;
                let cards = self.ticked_cards();
                let on_cards =
                    !cards.is_empty() && self.place() != super::graphics::Place::Processor;
                let card = match cards.as_slice() {
                    [one] => format!("the {}", one.name),
                    _ => "the cards".to_string(),
                };
                let base = format!(
                    "{words}: {} for the chat, {} with the model.",
                    gb(chat),
                    gb(total)
                );
                match (fit, on_cards) {
                    (Fit::Good, true) => format!("{base} It fits on {card}."),
                    (Fit::Partly, true) => {
                        format!("{base} More than {card} has free: part goes in the memory, and replies are slower.")
                    }
                    (Fit::Good, false) => format!("{base} It fits in the memory."),
                    (Fit::Partly, false) => format!("{base} It takes most of the memory."),
                    (Fit::TooBig, _) => format!("{base} More than this computer has."),
                }
            }
            _ => format!("{words} of context."),
        };
        if marked {
            said.push_str(" Suits this model on this computer.");
        }
        said
    }
}

/// A box of the grid.
struct Cell {
    text: String,
    fit: Option<Fit>,
    /// Past what the model takes: dashed, and not chosen.
    off: bool,
    /// The suggested length: a short line under its words.
    marked: bool,
    hover: String,
}

/// The grid: four boxes a row, the lit one in the accent, a dot at the top right of each that says where it fits. What
/// was clicked comes back.
fn length_grid(ui: &mut egui::Ui, cells: &[Cell], lit: Option<usize>) -> Option<usize> {
    let p = theme::palette();
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let gap = 4.0;
    let high = 32.0;
    let each = (width - gap * 3.0) / 4.0;
    let rows = cells.len().div_ceil(4);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(
            width,
            rows as f32 * high + (rows.saturating_sub(1)) as f32 * gap,
        ),
        egui::Sense::hover(),
    );
    let painter = ui.painter().clone();
    let mut chosen = None;
    for (i, cell) in cells.iter().enumerate() {
        let (row, col) = (i / 4, i % 4);
        let r = egui::Rect::from_min_size(
            egui::pos2(
                rect.left() + col as f32 * (each + gap),
                rect.top() + row as f32 * (high + gap),
            ),
            egui::vec2(each, high),
        );
        let on = lit == Some(i);
        let active = enabled && !cell.off;
        let response = ui
            .interact(
                r,
                ui.id().with(("length", i)),
                if active {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                },
            )
            .on_hover_text(&cell.hover);
        let (fill, stroke) = if on {
            (
                p.accent_soft,
                egui::Stroke::new(1.0, p.accent_text.gamma_multiply(0.5)),
            )
        } else if cell.off {
            (egui::Color32::TRANSPARENT, egui::Stroke::new(1.0, p.line))
        } else if active && response.hovered() {
            (p.raised_hover, egui::Stroke::new(1.0, p.outline))
        } else {
            (p.card, egui::Stroke::new(1.0, p.line))
        };
        painter.rect(r, 7.0, fill, stroke, egui::StrokeKind::Inside);
        if cell.off {
            // dashed: the lines of a box past what the model takes
            let dash = egui::Stroke::new(1.0, p.side);
            for k in 0..((r.width() / 6.0) as i32) {
                let x = r.left() + 4.0 + k as f32 * 6.0;
                painter.vline(x, r.top()..=r.top() + 1.0, dash);
                painter.vline(x, r.bottom() - 1.0..=r.bottom(), dash);
            }
        }
        let color = if on {
            p.accent_text
        } else if cell.off || !enabled {
            p.faint
        } else {
            p.body
        };
        let mut font = egui::TextStyle::Body.resolve(ui.style());
        font.size = 13.0;
        if on {
            font.family = theme::semibold(ui);
        }
        let galley = painter.layout_no_wrap(cell.text.clone(), font, color);
        painter.galley(r.center() - galley.size() / 2.0, galley, color);
        if let Some(fit) = cell.fit {
            let dot = match fit {
                Fit::Good => p.accent_text,
                Fit::Partly => p.warn,
                Fit::TooBig => p.bad,
            };
            painter.circle_filled(egui::pos2(r.right() - 6.0, r.top() + 6.0), 3.0, dot);
        }
        if cell.marked {
            painter.hline(
                r.center().x - 7.0..=r.center().x + 7.0,
                r.bottom() - 5.0,
                egui::Stroke::new(
                    2.0,
                    p.accent_text.gamma_multiply(if on { 1.0 } else { 0.6 }),
                ),
            );
        }
        // a length that is lit is chosen already; Other… lit is clicked to open its number again
        if response.clicked() && (!on || i == cells.len() - 1) {
            chosen = Some(i);
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// An application of `engine` with one model of `bytes` (not a real GGUF file: its facts are not known), on a
    /// computer of 16 GB.
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
        assert_eq!(context_words(128000), "128K");
        assert_eq!(context_words(3000), "3K");
        assert_eq!(context_words(3001), "3001");
        assert_eq!(context_words(512), "512");
    }

    #[test]
    fn a_length_a_little_past_what_the_model_takes_is_what_it_takes() {
        assert_eq!(length_at(131072, 128000), Some(128000));
        assert_eq!(length_at(65536, 128000), Some(65536));
        assert_eq!(length_at(262144, 128000), None);
    }

    #[test]
    fn the_choices_kept_by_an_earlier_version_are_read_as_lengths() {
        assert_eq!(RunChoice::from_key("balanced"), Some(RunChoice::Suggested));
        assert_eq!(RunChoice::from_key("long"), Some(RunChoice::Length(32768)));
        assert_eq!(RunChoice::from_key("light"), Some(RunChoice::Length(4096)));
        assert_eq!(RunChoice::from_key("custom"), Some(RunChoice::Own));
        assert_eq!(RunChoice::from_key("65536"), Some(RunChoice::Length(65536)));
        assert_eq!(RunChoice::from_key("nonsense"), None);
        for choice in [
            RunChoice::Suggested,
            RunChoice::Length(131072),
            RunChoice::Own,
        ] {
            assert_eq!(RunChoice::from_key(&choice.key()), Some(choice));
        }
    }

    #[test]
    fn without_the_models_facts_the_suggested_length_is_the_documents_and_a_length_is_kept() {
        let dir = scratch("grid-llama");
        // a 1 GB model on 16 GB: the document recommends 16K
        let mut app = app_with_model(&dir, "llamacpp", GB);
        assert_eq!(app.suggested_context(), Some(16384));
        app.choose_length(65536);
        assert_eq!(app.values.int("ctx_size"), Some(65536));
        assert_eq!(app.run_choice, Some(RunChoice::Length(65536)));
        assert!(
            app.run_summary().starts_with("64K context"),
            "{}",
            app.run_summary()
        );
        app.choose_length(16384);
        assert_eq!(app.run_choice, Some(RunChoice::Suggested));
        // where it runs is said as such
        app.values.set("n_gpu_layers", 0i64);
        assert!(
            app.run_summary().ends_with("CPU only"),
            "{}",
            app.run_summary()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_choice_nobody_made_is_not_written_and_one_made_is() {
        let dir = scratch("grid-kept");
        let mut app = app_with_model(&dir, "llamacpp", GB);
        app.take_up_run_choice();
        // a first start's 4K stays 4K (the configuration in use says 4K: nothing is changed that nobody changed)
        assert_eq!(app.values.int("ctx_size"), Some(4096));
        assert_eq!(app.run_choice, Some(RunChoice::Length(4096)));
        assert_eq!(app.settings_now().run_choice(), None, "the numbers say it");
        app.choose_length(65536);
        assert_eq!(app.settings_now().run_choice().as_deref(), Some("65536"));
        app.choose_length(4096);
        assert_eq!(
            app.settings_now().run_choice(),
            None,
            "the numbers say it again"
        );
        app.choose_length(16384);
        assert_eq!(
            app.settings_now().run_choice().as_deref(),
            Some("balanced"),
            "the suggested one, chosen"
        );
        // a choice the file kept stays written, whatever it is
        let mut settings = app.settings_now();
        settings.set_run_choice(Some("balanced"));
        app.stored = settings;
        app.take_up_run_choice();
        assert_eq!(app.settings_now().run_choice().as_deref(), Some("balanced"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ollamas_suggested_length_is_its_own_context() {
        let dir = scratch("grid-ollama");
        let mut app = app_with_model(&dir, "ollama", GB);
        assert_eq!(app.suggested_context(), None);
        assert_eq!(app.choice_now(), RunChoice::Suggested);
        assert!(
            app.run_summary().starts_with("Ollama's own context"),
            "{}",
            app.run_summary()
        );
        app.choose_length(16384);
        assert_eq!(app.values.int("ctx_size"), Some(16384));
        app.apply_choice(RunChoice::Suggested);
        assert_eq!(app.values.get("ctx_size"), None, "left to Ollama again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_engine_with_no_context_says_its_fields_and_has_no_grid() {
        let dir = scratch("grid-comfy");
        let app = app_with_model(&dir, "comfyui", GB);
        assert!(app.context_field().is_none());
        let said = app.run_summary();
        assert!(said.contains("Port"), "{said}");
        assert!(!said.contains("context"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
