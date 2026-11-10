//! Extras in the panel at the right (C213, X1 of `the mockups`, the user's pick of 2026-11-10:
//! "bring in the extra configuration like tools - i.e. the extra arguments and environment - e.g. show the projector
//! and so on - upon click"): what QuickAI adds when the model starts besides the context and the graphics. Closed, one
//! line says what there is; opened, a vision model's projector has a switch, the mode a model is started in is shown,
//! and the person's own arguments and lines of environment are a chip each: a click changes one, × takes it out, and
//! *+ Add* adds one. *Copy the whole command* copies what Start runs. They are the fields of Settings › Advanced, so
//! the two always agree. While the model runs they are chips in *How it runs* (R1).

use eframe::egui;
use quickai_core::plan::{is_secret_name, EnvMode, Start};
use quickai_core::value::Value;

use super::icons::{self, Icon};
use super::theme;
use super::App;

/// The fields Extras shows: llama.cpp's projector switch, and every engine's arguments and environment.
const PROJECTOR: &str = "projector";
const EXTRA_ARGS: &str = "extra_args";
const EXTRA_ENV: &str = "extra_env";

/// What a row of Extras is indented by: under the word, past the icon of its head.
const INDENT: f32 = 23.0;

/// What the (i) of Extras says.
const EXTRAS_HOVER: &str = "What QuickAI adds when the model starts: a vision model's projector, the mode some models are \
     started in, and your own arguments and environment. Yours win over QuickAI's. They are the same as in Settings › \
     Advanced.";

/// A part of a field's text that is one chip: where it starts and ends (in bytes), and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Piece {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// The person's arguments as chips: an option with the words after it that are not options (`--threads 8`), as they
/// were typed. Words are split as llama.cpp's field splits them (`quickai_core::plan::args::split_args`): quotes keep
/// spaces in a word. A word that is a number (`-1`) is a value, not an option.
pub(super) fn argument_pieces(text: &str) -> Vec<Piece> {
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    let mut quote: Option<char> = None;
    for (i, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                start.get_or_insert(i);
            }
            None if c.is_whitespace() => {
                if let Some(s) = start.take() {
                    words.push((s, i));
                }
            }
            None => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(s) = start {
        words.push((s, text.len()));
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for (s, e) in words {
        let word = &text[s..e];
        let option = word.starts_with('-') && word.len() > 1 && word.parse::<f64>().is_err();
        match pieces.last_mut() {
            Some(last) if !option => {
                last.end = e;
                last.text = text[last.start..e].to_string();
            }
            _ => pieces.push(Piece {
                start: s,
                end: e,
                text: word.to_string(),
            }),
        }
    }
    pieces
}

/// The person's lines of environment as chips: each line that is neither blank nor a comment.
pub(super) fn environment_pieces(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            pieces.push(Piece {
                start: at,
                end: at + body.len(),
                text: trimmed.to_string(),
            });
        }
        at += line.len();
    }
    pieces
}

/// `text` with `piece` said anew (`Some`), or taken out: a line of environment (`lines`) with its line's end, an
/// argument with the space before it.
pub(super) fn replaced(text: &str, piece: &Piece, with: Option<&str>, lines: bool) -> String {
    match with.map(str::trim).filter(|w| !w.is_empty()) {
        Some(new) => format!("{}{}{}", &text[..piece.start], new, &text[piece.end..]),
        None if lines => {
            let start = text[..piece.start].rfind('\n').map_or(0, |i| i + 1);
            let end = text[piece.end..]
                .find('\n')
                .map_or(text.len(), |i| piece.end + i + 1);
            format!("{}{}", &text[..start], &text[end..])
        }
        None => {
            let before = text[..piece.start].trim_end();
            let after = text[piece.end..].trim_start();
            if before.is_empty() || after.is_empty() {
                format!("{before}{after}")
            } else {
                format!("{before} {after}")
            }
        }
    }
}

/// `text` with `new` after what it has: an argument after a space, a line of environment on a line of its own.
pub(super) fn appended(text: &str, new: &str, lines: bool) -> String {
    let new = new.trim();
    if new.is_empty() {
        return text.to_string();
    }
    if text.trim().is_empty() {
        return new.to_string();
    }
    if lines {
        if text.ends_with('\n') {
            format!("{text}{new}")
        } else {
            format!("{text}\n{new}")
        }
    } else {
        format!("{} {new}", text.trim_end())
    }
}

/// What an argument does, when the field's help has it among its examples (`--threads 8   number of CPU threads to
/// use`).
fn described(help: &str, piece: &str) -> Option<String> {
    let option = piece.split_whitespace().next()?;
    help.lines()
        .skip_while(|l| !l.trim_start().starts_with("Examples"))
        .skip(1)
        .find_map(|l| {
            let (example, what) = l.split_once("  ")?;
            (example.split_whitespace().next() == Some(option)).then(|| what.trim().to_string())
        })
}

/// A word of a command line as a shell reads it: as it is when it is plain, else in single quotes.
fn shell_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=,@%+".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// `n` things, with the word in the plural when there are several.
fn counted(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// What is being typed in a chip of Extras: a new one (`index` none) or the one at `index`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Typing {
    field: &'static str,
    index: Option<usize>,
    text: String,
    /// Just opened: the cursor goes in it once.
    fresh: bool,
}

/// What was done to a chip.
enum ChipAction {
    Edit,
    Remove,
}

impl App {
    /// Whether the engine in use has the field `id`.
    fn engine_has_field(&self, id: &str) -> bool {
        self.engine_doc()
            .is_some_and(|e| e.fields.iter().any(|f| f.id == id))
    }

    /// The help and the hint of the engine's field `id`, as its document says them.
    fn field_words(&self, id: &str) -> (Option<String>, Option<String>) {
        self.engine_doc()
            .and_then(|e| e.doc.fields.iter().find(|f| f.id == id))
            .map_or((None, None), |f| (f.help.clone(), f.ui.hint.clone()))
    }

    /// Whether the projector is attached when the model starts (llama.cpp's `projector`; on unless it was turned off).
    pub(super) fn projector_on(&self) -> bool {
        self.values
            .get(PROJECTOR)
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    /// The projector the selected model starts with, when its engine has the switch and the file is still there: its
    /// name and how big it is.
    pub(super) fn projector_of_selected(&self) -> Option<(String, std::path::PathBuf, u64)> {
        if !self.engine_has_field(PROJECTOR) {
            return None;
        }
        let model = self.models.get(self.selected)?;
        model
            .companions
            .iter()
            .find(|c| c.kind == "mmproj" && c.exists())
            .map(|c| {
                let bytes = std::fs::metadata(&c.path).map_or(0, |m| m.len());
                (c.name.clone(), c.path.clone(), bytes)
            })
    }

    /// The memory the projector takes when it is attached: it goes on the card with the model.
    pub(super) fn projector_bytes(&self) -> u64 {
        if !self.projector_on() {
            return 0;
        }
        self.projector_of_selected()
            .map_or(0, |(_, _, bytes)| bytes)
    }

    /// The mode the selected model is started in, when its record says one the engine has (`--reasoning off`, an
    /// embedding server's `--embeddings`).
    fn mode_of_selected(&self) -> Option<quickai_core::registry::engine::Mode> {
        let mode = self.models.get(self.selected)?.mode.clone()?;
        let engine = self.engine_doc()?;
        quickai_core::models::modes(engine)
            .iter()
            .find(|m| m.id == mode)
            .cloned()
    }

    /// The person's arguments and lines of environment, as chips (none for an engine without the field).
    fn extra_pieces(&self) -> (Vec<Piece>, Vec<Piece>) {
        let args = if self.engine_has_field(EXTRA_ARGS) {
            argument_pieces(self.values.text(EXTRA_ARGS).unwrap_or_default())
        } else {
            Vec::new()
        };
        let env = if self.engine_has_field(EXTRA_ENV) {
            environment_pieces(self.values.text(EXTRA_ENV).unwrap_or_default())
        } else {
            Vec::new()
        };
        (args, env)
    }

    /// What the closed line of Extras says, in a few words: the projector (on or off), that the model is started in a mode
    /// of its own (its words are in the opened rows: they can be long), and how many arguments and variables; with
    /// nothing, "none".
    pub(super) fn extras_summary(&self) -> String {
        let (args, env) = self.extra_pieces();
        let mut said = Vec::new();
        if self.projector_of_selected().is_some() {
            said.push(if self.projector_on() {
                "projector".to_string()
            } else {
                "projector off".to_string()
            });
        }
        if self.mode_of_selected().is_some() {
            said.push("mode".to_string());
        }
        if !args.is_empty() {
            said.push(counted(args.len(), "argument"));
        }
        if !env.is_empty() {
            said.push(counted(env.len(), "variable"));
        }
        if said.is_empty() {
            "none".to_string()
        } else {
            said.join(" · ")
        }
    }

    /// The line of what was added for *How it runs* while the model runs: how many, and on hover what they are.
    pub(super) fn extras_running(&self) -> Option<(String, String)> {
        let (args, env) = self.extra_pieces();
        let mode = self.mode_of_selected();
        let mut said: Vec<String> = Vec::new();
        if let Some(mode) = &mode {
            said.push(format!("{} ({})", mode.arg.join(" "), mode.label));
        }
        said.extend(args.iter().chain(&env).map(|p| p.text.clone()));
        if said.is_empty() {
            return None;
        }
        let words = if said.len() == 1 {
            "1 extra".to_string()
        } else {
            format!("{} extras", said.len())
        };
        Some((words, said.join("\n")))
    }

    /// What Start runs, as one line to paste in a terminal: the variables it sets, the program and its arguments. A
    /// variable whose name says it is a secret is left as `…`.
    pub(super) fn command_text(&self) -> Option<String> {
        let plan = self.plan_to_show().ok()?;
        let spawn = plan.units.first()?.start.iter().find_map(|s| match s {
            Start::Spawn(spawn) => Some(spawn),
            Start::Attach(_) => None,
        })?;
        let mut words = Vec::new();
        for op in &spawn.env {
            if let EnvMode::Set { value } = &op.mode {
                let value = if is_secret_name(&op.name) {
                    "…".to_string()
                } else {
                    shell_word(value)
                };
                words.push(format!("{}={value}", op.name));
            }
        }
        words.push(shell_word(&spawn.program.value));
        words.extend(spawn.args.iter().map(|a| shell_word(&a.value)));
        Some(words.join(" "))
    }

    /// Extras, under Graphics while the model is stopped: closed, a line of what there is; opened, the projector's
    /// switch, the mode, the chips of the arguments and of the environment, and *Copy the whole command*.
    pub(super) fn show_extras_rows(&mut self, ui: &mut egui::Ui) {
        let has_args = self.engine_has_field(EXTRA_ARGS);
        let has_env = self.engine_has_field(EXTRA_ENV);
        let projector = self.projector_of_selected();
        let mode = self.mode_of_selected();
        if !has_args && !has_env && projector.is_none() && mode.is_none() {
            return;
        }
        let enabled = !self.session.is_active();
        let p = theme::palette();
        let open_id = egui::Id::new("panel_extras_open");
        let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        let summary = self.extras_summary();
        let mut toggle = false;
        super::inspector::section_head(ui, Icon::Terminal, "Extras", Some(EXTRAS_HOVER), |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(16.0, 24.0), egui::Sense::click());
            let words = ui
                .add(
                    egui::Label::new(egui::RichText::new(&summary).small().color(if open {
                        p.text
                    } else {
                        p.muted
                    }))
                    .truncate()
                    .sense(egui::Sense::click()),
                )
                .on_hover_text(if open { "Hide them." } else { "Show them." });
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
        // the projector: a switch, its size, and its file under it
        if let Some((name, path, bytes)) = &projector {
            let mut on = self.projector_on();
            let (help, _) = self.field_words(PROJECTOR);
            let mut switched = false;
            ui.horizontal(|ui| {
                ui.set_min_height(28.0);
                ui.add_space(INDENT);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                icons::paint(ui.painter(), rect, Icon::Eye, p.muted);
                let label = ui.label(egui::RichText::new("Projector").color(p.text));
                if let Some(help) = &help {
                    label.on_hover_text(help);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    switched = theme::switch(ui, &mut on, enabled)
                        .on_hover_text(if on {
                            "On: the model reads pictures. Click to start it without the projector."
                        } else {
                            "Off: the model starts without it, and cannot read pictures. Click to attach it again."
                        })
                        .changed();
                    ui.label(theme::muted(super::run_settings::gb(*bytes)).small());
                });
            });
            ui.horizontal(|ui| {
                ui.add_space(INDENT);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(name)
                            .monospace()
                            .size(11.0)
                            .color(p.faint),
                    )
                    .truncate(),
                )
                .on_hover_text(path.display().to_string());
            });
            if switched {
                self.values.set(PROJECTOR, on);
                self.auto_fit();
            }
        }
        // the mode the model is started in, said: it comes with the model
        if let Some(mode) = &mode {
            ui.horizontal(|ui| {
                ui.set_min_height(28.0);
                ui.add_space(INDENT);
                ui.label(egui::RichText::new("Mode").color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    chip(
                        ui,
                        &mode.arg.join(" "),
                        false,
                        &format!("{}: {}", mode.label, mode.help),
                    );
                });
            });
        }
        if has_args {
            self.show_extra_chips(ui, EXTRA_ARGS, "Arguments", enabled);
        }
        if has_env {
            self.show_extra_chips(ui, EXTRA_ENV, "Environment", enabled);
        }
        // what Start runs, to paste in a terminal
        if let Some(command) = self.command_text() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(INDENT);
                let id = egui::Id::new("panel_extras_copied");
                let now = ui.input(|i| i.time);
                let just = ui
                    .data(|d| d.get_temp::<f64>(id))
                    .is_some_and(|at| now - at < 1.5);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
                icons::paint(
                    ui.painter(),
                    rect,
                    if just { Icon::Check } else { Icon::Copy },
                    p.accent_text,
                );
                let link = ui
                    .add(egui::Link::new(
                        egui::RichText::new(if just {
                            "Copied"
                        } else {
                            "Copy the whole command"
                        })
                        .small(),
                    ))
                    .on_hover_text(format!("What Start runs:\n{command}"));
                if link.clicked() {
                    ui.ctx().copy_text(command.clone());
                    ui.data_mut(|d| d.insert_temp(id, now));
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(1600));
                } else if just {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(200));
                }
            });
        }
    }

    /// The chips of the field `field` (the arguments, or the lines of environment), under `heading`: a click on one
    /// changes it in a box in its place, × takes it out, + Add opens a box at the end. Enter keeps what was typed,
    /// Escape leaves it as it was; a chip emptied is taken out.
    fn show_extra_chips(
        &mut self,
        ui: &mut egui::Ui,
        field: &'static str,
        heading: &str,
        enabled: bool,
    ) {
        let p = theme::palette();
        let lines = field == EXTRA_ENV;
        let text = self.values.text(field).unwrap_or_default().to_string();
        let pieces = if lines {
            environment_pieces(&text)
        } else {
            argument_pieces(&text)
        };
        let (help, hint) = self.field_words(field);
        let typing_id = egui::Id::new("panel_extras_typing");
        let before: Option<Typing> = ui.data(|d| d.get_temp(typing_id));
        let mut typing = before.clone().filter(|t| t.field == field && enabled);
        let mut started: Option<Typing> = None;
        let mut new_text: Option<String> = None;
        let mut done_typing = false;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            ui.label(egui::RichText::new(heading).size(11.5).color(p.faint));
        });
        // the chips wrap under the heading, indented as it is
        ui.horizontal(|ui| {
            ui.add_space(INDENT - ui.spacing().item_spacing.x);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                for (i, piece) in pieces.iter().enumerate() {
                    if let Some(t) = typing.as_mut().filter(|t| t.index == Some(i)) {
                        match typing_box(ui, t, hint.as_deref()) {
                            Some(true) => {
                                new_text = Some(replaced(&text, piece, Some(&t.text), lines));
                                done_typing = true;
                            }
                            Some(false) => done_typing = true,
                            None => {}
                        }
                        continue;
                    }
                    let shown = piece.text.split_whitespace().collect::<Vec<_>>().join(" ");
                    let hover = if lines {
                        format!(
                        "{shown}: a variable for the program. Click to change it, × takes it out."
                    )
                    } else {
                        match help.as_deref().and_then(|h| described(h, &piece.text)) {
                            Some(what) => {
                                format!("{shown}: {what}. Click to change it, × takes it out.")
                            }
                            None => format!("{shown}. Click to change it, × takes it out."),
                        }
                    };
                    match chip(ui, &shown, enabled, &hover) {
                        Some(ChipAction::Edit) => {
                            started = Some(Typing {
                                field,
                                index: Some(i),
                                text: piece.text.clone(),
                                fresh: true,
                            });
                        }
                        Some(ChipAction::Remove) => {
                            new_text = Some(replaced(&text, piece, None, lines));
                        }
                        None => {}
                    }
                }
                if let Some(t) = typing.as_mut().filter(|t| t.index.is_none()) {
                    match typing_box(ui, t, hint.as_deref()) {
                        Some(true) => {
                            new_text = Some(appended(&text, &t.text, lines));
                            done_typing = true;
                        }
                        Some(false) => done_typing = true,
                        None => {}
                    }
                } else if enabled {
                    if add_chip(ui).clicked() {
                        started = Some(Typing {
                            field,
                            index: None,
                            text: String::new(),
                            fresh: true,
                        });
                    }
                } else if pieces.is_empty() {
                    ui.label(egui::RichText::new("none").small().color(p.faint));
                }
            });
        });
        // what the form says is wrong with the field (a line that is not KEY=VALUE)
        if let Some(error) = self
            .form()
            .and_then(|f| f.field(field).and_then(|f| f.error.clone()))
        {
            for line in super::widgets::problem_lines(Some(&error)) {
                ui.horizontal(|ui| {
                    ui.add_space(INDENT);
                    ui.label(
                        egui::RichText::new(line)
                            .small()
                            .color(super::widgets::problem_color()),
                    );
                });
            }
        }
        if let Some(new) = new_text {
            self.values.set(field, new);
        }
        // what is typed is kept between frames; one box at a time
        if let Some(t) = started {
            ui.data_mut(|d| d.insert_temp(typing_id, t));
        } else if done_typing {
            ui.data_mut(|d| d.remove::<Typing>(typing_id));
        } else if let Some(t) = typing {
            if before.as_ref() != Some(&t) {
                ui.data_mut(|d| d.insert_temp(typing_id, t));
            }
        }
    }

    /// The chips of *How it runs* while the model runs: the context, where it runs, the projector and the extras.
    pub(super) fn show_run_chips(&self, ui: &mut egui::Ui) {
        let mut chips: Vec<(Option<Icon>, String, String)> = Vec::new();
        if let Some(remote) = self.remote_in_use() {
            chips.push((None, format!("on {}", remote.name), self.run_summary()));
        } else if let Some(field) = self.context_field() {
            chips.push(match field.value.as_i64() {
                Some(tokens) => (
                    None,
                    super::run_settings::context_words(tokens),
                    format!("Context: {tokens} tokens."),
                ),
                None => (
                    None,
                    format!("{}'s own context", self.engine_name()),
                    "The engine decides the context.".to_string(),
                ),
            });
            let place = self.place_chip();
            chips.push((
                None,
                place.clone(),
                format!("Runs {}.", self.place_words().unwrap_or(place)),
            ));
        } else {
            // an engine with no context (ComfyUI): its fields, as the line says them
            ui.label(egui::RichText::new(self.run_summary()).color(theme::palette().body));
            return;
        }
        if let Some((name, _, _)) = self.projector_of_selected().filter(|_| self.projector_on()) {
            chips.push((
                Some(Icon::Eye),
                "projector".to_string(),
                format!("Reads pictures through {name}."),
            ));
        }
        if let Some((words, said)) = self.extras_running() {
            chips.push((None, words, said));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
            for (icon, words, hover) in &chips {
                pill(ui, *icon, words, hover);
            }
        });
    }

    /// Where it runs, in a word or two for a chip: the card, the cards, with the processor, or the processor.
    fn place_chip(&self) -> String {
        use super::graphics::Place;
        if !self.lists_devices() {
            // an engine that places it itself (Ollama): its cards, and the share it says it put on them
            if !self.gpu_offload() {
                return "processor".to_string();
            }
            let named = match self.ticked_cards().as_slice() {
                [one] => one.name.clone(),
                [] => "graphics card".to_string(),
                many => format!("{} cards", many.len()),
            };
            let share = self.card_share();
            return if share >= 0.995 {
                named
            } else if share <= 0.005 {
                "processor".to_string()
            } else {
                format!("{:.0}% on {named}", share * 100.0)
            };
        }
        let cards = self.ticked_cards();
        let named = match cards.as_slice() {
            [one] => one.name.clone(),
            [] => return "processor".to_string(),
            many => format!("{} cards", many.len()),
        };
        match self.place() {
            Place::Card => named,
            Place::Mixed(_) => format!("{named} + processor"),
            Place::Processor => "processor".to_string(),
        }
    }
}

/// A chip: its words in the monospace font on a raised box, and × at its right when it can be changed. What was done
/// with it comes back.
fn chip(ui: &mut egui::Ui, words: &str, enabled: bool, hover: &str) -> Option<ChipAction> {
    let p = theme::palette();
    let font = egui::FontId::monospace(11.5);
    let galley = ui.painter().layout_no_wrap(words.to_string(), font, p.text);
    let close = if enabled { 16.0 } else { 0.0 };
    let width = (galley.size().x + 16.0 + close).min(ui.available_width().max(60.0));
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, 24.0),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let x_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - close - 2.0, rect.top()),
        rect.right_bottom(),
    );
    let x = enabled.then(|| ui.interact(x_rect, response.id.with("x"), egui::Sense::click()));
    let on_x = x.as_ref().is_some_and(egui::Response::hovered);
    let response = response.on_hover_text(hover);
    if ui.is_rect_visible(rect) {
        let fill = if enabled && response.hovered() && !on_x {
            p.raised_hover
        } else {
            p.card
        };
        ui.painter().rect(
            rect,
            6.0,
            fill,
            egui::Stroke::new(1.0, p.line),
            egui::StrokeKind::Inside,
        );
        let clip = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.right() - close - 4.0, rect.bottom()),
        );
        ui.painter().with_clip_rect(clip).galley(
            egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
            galley,
            p.text,
        );
        if enabled {
            ui.painter().text(
                egui::pos2(x_rect.center().x - 1.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                "×",
                egui::FontId::proportional(14.0),
                if on_x { p.text } else { p.faint },
            );
        }
    }
    if x.is_some_and(|x| x.on_hover_text("Take it out").clicked()) {
        return Some(ChipAction::Remove);
    }
    response.clicked().then_some(ChipAction::Edit)
}

/// + Add, a chip with a dashed edge.
fn add_chip(ui: &mut egui::Ui) -> egui::Response {
    let p = theme::palette();
    let font = egui::FontId::proportional(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap("+ Add".to_string(), font, p.muted);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 18.0, 24.0),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() { p.text } else { p.muted };
        if response.hovered() {
            ui.painter().rect_filled(rect, 6.0, p.raised_hover);
        }
        // the dashes of the edge
        let stroke = egui::Stroke::new(1.0, p.outline);
        let r = rect.shrink(0.5);
        let mut x = r.left() + 5.0;
        while x < r.right() - 5.0 {
            let to = (x + 3.0).min(r.right() - 5.0);
            ui.painter().hline(x..=to, r.top(), stroke);
            ui.painter().hline(x..=to, r.bottom(), stroke);
            x += 6.0;
        }
        let mut y = r.top() + 5.0;
        while y < r.bottom() - 5.0 {
            let to = (y + 3.0).min(r.bottom() - 5.0);
            ui.painter().vline(r.left(), y..=to, stroke);
            ui.painter().vline(r.right(), y..=to, stroke);
            y += 6.0;
        }
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response.on_hover_text("Add one")
}

/// The box a chip is typed in: `Some(true)` once Enter (or a click away) keeps it, `Some(false)` once Escape leaves it.
fn typing_box(ui: &mut egui::Ui, typing: &mut Typing, hint: Option<&str>) -> Option<bool> {
    let mut edit = egui::TextEdit::singleline(&mut typing.text)
        .font(egui::FontId::monospace(11.5))
        .desired_width(170.0_f32.min(ui.available_width().max(80.0)));
    if let Some(hint) = hint {
        edit = edit.hint_text(hint);
    }
    let response = ui.add(edit);
    if typing.fresh {
        response.request_focus();
        typing.fresh = false;
    }
    if response.lost_focus() {
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        return Some(!escape);
    }
    None
}

/// A chip of *How it runs* while the model runs: its words (and picture) on a rounded raised box.
fn pill(ui: &mut egui::Ui, icon: Option<Icon>, words: &str, hover: &str) {
    let p = theme::palette();
    let font = egui::FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(words.to_string(), font, p.body);
    let picture = if icon.is_some() { 17.0 } else { 0.0 };
    let width = (galley.size().x + 18.0 + picture).min(ui.available_width().max(60.0));
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
    response.on_hover_text(hover);
    if ui.is_rect_visible(rect) {
        // raised on the dark panel; on the light one, whose raised grey is the panel's, white with an edge
        if ui.visuals().dark_mode {
            ui.painter().rect_filled(rect, 12.0, p.raised);
        } else {
            ui.painter().rect(
                rect,
                12.0,
                p.card,
                egui::Stroke::new(1.0, p.line),
                egui::StrokeKind::Inside,
            );
        }
        let mut x = rect.left() + 9.0;
        if let Some(icon) = icon {
            let square = egui::Rect::from_min_size(
                egui::pos2(x, rect.center().y - 6.0),
                egui::vec2(12.0, 12.0),
            );
            icons::paint(ui.painter(), square, icon, p.muted);
            x += picture;
        }
        let clip = rect.shrink2(egui::vec2(6.0, 0.0));
        ui.painter().with_clip_rect(clip).galley(
            egui::pos2(x, rect.center().y - galley.size().y / 2.0),
            galley,
            p.body,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(pieces: &[Piece]) -> Vec<&str> {
        pieces.iter().map(|p| p.text.as_str()).collect()
    }

    #[test]
    fn arguments_are_a_chip_for_each_option_with_its_values() {
        let text = "--threads 8 --flash-attn on --no-mmap";
        assert_eq!(
            texts(&argument_pieces(text)),
            ["--threads 8", "--flash-attn on", "--no-mmap"]
        );
        // a number is a value, and quotes keep their spaces
        assert_eq!(
            texts(&argument_pieces("--seed -1 --alias \"my model\"")),
            ["--seed -1", "--alias \"my model\""]
        );
        // words before any option are a chip of their own
        assert_eq!(
            texts(&argument_pieces("model.gguf -c 4096")),
            ["model.gguf", "-c 4096"]
        );
        assert!(argument_pieces("   ").is_empty());
    }

    #[test]
    fn a_chip_taken_out_or_changed_leaves_the_rest_as_it_was_typed() {
        let text = "--threads 8  --flash-attn on --no-mmap";
        let pieces = argument_pieces(text);
        assert_eq!(
            replaced(text, &pieces[1], None, false),
            "--threads 8 --no-mmap"
        );
        assert_eq!(
            replaced(text, &pieces[0], None, false),
            "--flash-attn on --no-mmap"
        );
        assert_eq!(
            replaced(text, &pieces[2], None, false),
            "--threads 8  --flash-attn on"
        );
        assert_eq!(
            replaced(text, &pieces[0], Some("--threads 4"), false),
            "--threads 4  --flash-attn on --no-mmap"
        );
        // emptied is taken out
        assert_eq!(
            replaced(text, &pieces[2], Some("  "), false),
            "--threads 8  --flash-attn on"
        );
        assert_eq!(
            appended(text, "--batch-size 512", false),
            format!("{text} --batch-size 512")
        );
        assert_eq!(
            appended("", " --batch-size 512 ", false),
            "--batch-size 512"
        );
    }

    #[test]
    fn environment_is_a_chip_for_each_line_and_comments_stay() {
        let text = "# for the first card\nGGML_VK_VISIBLE_DEVICES=0\n\nLLAMA_ARG_THREADS=8\n";
        let pieces = environment_pieces(text);
        assert_eq!(
            texts(&pieces),
            ["GGML_VK_VISIBLE_DEVICES=0", "LLAMA_ARG_THREADS=8"]
        );
        assert_eq!(
            replaced(text, &pieces[0], None, true),
            "# for the first card\n\nLLAMA_ARG_THREADS=8\n"
        );
        assert_eq!(
            replaced(text, &pieces[1], Some("LLAMA_ARG_THREADS=4"), true),
            "# for the first card\nGGML_VK_VISIBLE_DEVICES=0\n\nLLAMA_ARG_THREADS=4\n"
        );
        assert_eq!(appended(text, "A=1", true), format!("{text}A=1"));
        assert_eq!(appended("A=1", "B=2", true), "A=1\nB=2");
    }

    #[test]
    fn an_argument_is_described_by_the_examples_of_the_help() {
        let help = "Appended.\n\nExamples:\n--threads 8         number of CPU threads to use\n--no-mmap           \
                    disable memory-mapping the model file";
        assert_eq!(
            described(help, "--threads 4").as_deref(),
            Some("number of CPU threads to use")
        );
        assert_eq!(
            described(help, "--no-mmap").as_deref(),
            Some("disable memory-mapping the model file")
        );
        assert_eq!(described(help, "--temp 0.7"), None);
    }

    #[test]
    fn a_word_of_the_command_is_quoted_when_a_shell_would_split_it() {
        assert_eq!(shell_word("--ctx-size"), "--ctx-size");
        assert_eq!(shell_word("/home/a b/model.gguf"), "'/home/a b/model.gguf'");
        assert_eq!(shell_word("it's"), "'it'\\''s'");
        assert_eq!(shell_word(""), "''");
    }
}
