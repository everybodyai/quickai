//! What Start runs, said in the settings of the engine (C189): the program, each of its arguments with what put it
//! there, and the variables of its environment. It is the plan Start makes, made the same way, so what the files of
//! a model add (a projector) and the mode it is started in are on it as the settings are, and nothing on it is a
//! guess. The words are the engine document's; this names no engine.

use std::path::Path;

use eframe::egui;
use quickai_core::plan::{
    is_secret_name, EnvMode, EnvOp, RunPlan, SpawnPlan, Start, TRAFFIC_PROXY_KEY,
};
use quickai_core::registry::Engine;
use quickai_core::value::Value;

use super::icons;
use super::runtime_manager::build_name;
use super::theme;
use super::App;

/// The widest the column that says where a row comes from may be.
const FROM_WIDTH: f32 = 170.0;

/// What put a word on the command line or a variable in the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
enum From {
    /// The engine's document: what QuickAI always gives it.
    Document,
    /// A setting, by its label.
    Setting(String),
    /// What goes with the model: a file fetched with it (`projector`) or the mode it is started in.
    Model(String),
}

/// How a row is said: the program's (with what the program is), or an argument's (with the names of the model's files).
enum Said<'a> {
    Program(&'a Names),
    Row(&'a From, &'a Names),
}

impl Row {
    fn said_with<'a>(&'a self, names: &'a Names) -> Said<'a> {
        Said::Row(&self.from, names)
    }
}

impl From {
    /// What the column on the right says.
    fn said(&self) -> String {
        match self {
            From::Document => "QuickAI".to_string(),
            From::Setting(label) | From::Model(label) => label.clone(),
        }
    }
}

/// One row: an option and the values that follow it, and what put them there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    words: Vec<String>,
    from: From,
}

impl App {
    /// The plan of what Start would run now, for the window to show and not to run: the model the page has selected
    /// with what its record attaches, the values as they stand, and the engine on the public port (the port of the
    /// live-traffic proxy is found only when it starts). An engine that can start with no model (Ollama's management
    /// mode) is shown so; one that cannot asks for a model. `Err` is the sentence to say instead.
    fn plan_to_show(&self) -> Result<RunPlan, String> {
        let model = if self.runs_alone() {
            None
        } else {
            self.models.get(self.selected).cloned()
        };
        let (mut selection, _) = self.selection_for(model.as_ref());
        selection
            .values
            .insert(TRAFFIC_PROXY_KEY.to_string(), Value::Bool(false));
        self.plan_run(&selection, None).map_err(|problems| {
            if model.is_none() && !self.runs_alone() && self.binary.is_file() {
                return "Choose a model to see the command line it starts with.".to_string();
            }
            problems
                .first()
                .map_or_else(String::new, |d| sentence(&d.message))
        })
    }

    /// The names of the plan's files and program as the window says them: the selected model's files by the names they
    /// came with, and the program by the build it is of when it is one of those installed.
    fn names_of(&self, spawn: &SpawnPlan) -> Names {
        let files = self
            .models
            .get(self.selected)
            .map(|m| {
                m.companions
                    .iter()
                    .map(|c| (c.path.display().to_string(), c.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let program = self
            .runtime
            .disk
            .builds
            .iter()
            .find(|(b, _)| b.binary_path.display().to_string() == spawn.program.value)
            .map(|(b, backend)| build_name(&b.label, backend).0);
        Names { files, program }
    }

    /// The command line of the next Start and its environment, under the engine's settings.
    pub(super) fn show_command_line(&self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        if let Some(there) = self.setup_in_use().or(self.remote_in_use()) {
            ui.label(theme::muted(format!(
                "The model runs on {}: its command line is made there.",
                there.name
            )));
            return;
        }
        let plan = match self.plan_to_show() {
            Ok(plan) => plan,
            Err(said) => {
                heading(ui, "Command line");
                ui.label(theme::muted(said));
                return;
            }
        };
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let starts: &[Start] = plan.units.first().map_or(&[], |u| u.start.as_slice());
        let server = self.server_name();
        let Some(spawn) = starts.iter().find_map(|s| match s {
            Start::Spawn(spawn) => Some(spawn),
            Start::Attach(_) => None,
        }) else {
            heading(ui, "Command line");
            ui.label(theme::muted(format!(
                "QuickAI starts no {server}: it uses the one that runs on port {}.",
                self.port()
            )));
            return;
        };
        let mut notes = Vec::new();
        if starts.iter().any(|s| matches!(s, Start::Attach(_))) {
            notes.push(format!(
                "Only when no {server} runs on port {} already: one that does is used as it is.",
                self.port()
            ));
        }
        if self.traffic_proxy_on() {
            notes.push(format!(
                "With the live traffic shown, {server} is given a free port of its own at Start, and QuickAI \
                 listens on {} in front of it.",
                self.port()
            ));
        }
        show_spawn(ui, engine, spawn, "engine", &notes, &self.names_of(spawn));
    }

    /// The same for the program that goes with the engine, under its settings.
    pub(super) fn show_companion_command_line(&self, ui: &mut egui::Ui) {
        let Some(engine) = self.companion_doc() else {
            return;
        };
        ui.add_space(10.0);
        match self.companion_plan() {
            Ok(plan) => {
                if let Some(spawn) = plan
                    .units
                    .iter()
                    .take(1)
                    .flat_map(|u| u.start.iter())
                    .find_map(|s| match s {
                        Start::Spawn(spawn) => Some(spawn),
                        Start::Attach(_) => None,
                    })
                {
                    show_spawn(ui, engine, spawn, "companion", &[], &Names::default());
                }
            }
            Err(problems) => {
                heading(ui, "Command line");
                ui.label(theme::muted(
                    problems
                        .first()
                        .map_or_else(String::new, |d| sentence(&d.message)),
                ));
            }
        }
    }
}

/// What the window knows of the words of a plan that the plan does not say: the name each file fetched with the model
/// came with (a projector is kept under its repository's name, and said by its own), and what the program is (the build
/// it is of).
#[derive(Default)]
struct Names {
    /// (where it is, the name it came with).
    files: Vec<(String, String)>,
    program: Option<String>,
}

/// The program and its arguments, a row for each option, with a button that copies the line, and `notes` on when it
/// is so under it; then the variables in full, with a button that copies them.
fn show_spawn(
    ui: &mut egui::Ui,
    engine: &Engine,
    spawn: &SpawnPlan,
    salt: &str,
    notes: &[String],
    names: &Names,
) {
    ui.horizontal(|ui| {
        heading(ui, "Command line").on_hover_text(
            "What Start runs, made the way Start makes it. Each row says what put it there.",
        );
        ui.label(theme::muted("what Start runs"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            icons::copy_button(ui, &format!("command_line_{salt}"), &shell_line(spawn));
        });
    });
    let program = vec![spawn.program.value.clone()];
    let rows = rows(engine, spawn);
    let width = from_width(ui, rows.iter().map(|r| &r.from));
    theme::well_frame(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        show_row(ui, &program, &Said::Program(names), width);
        for row in &rows {
            show_row(ui, &row.words, &row.said_with(names), width);
        }
    });
    for note in notes {
        ui.label(theme::muted(note));
    }
    let variables: Vec<Variable> = spawn
        .env
        .iter()
        .map(|op| Variable::of(engine, op))
        .collect();
    if variables.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        heading(ui, "Environment").on_hover_text(
            "The variables Start sets for it, made the way Start makes them; the rest it has from QuickAI's. Each \
             says what put it there.",
        );
        ui.label(theme::muted("what Start sets"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let lines: Vec<String> = variables.iter().map(Variable::line).collect();
            icons::copy_button(ui, &format!("command_line_env_{salt}"), &lines.join("\n"));
        });
    });
    let width = from_width(ui, variables.iter().map(|v| &v.from));
    theme::well_frame(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for variable in &variables {
            show_variable(ui, variable, width);
        }
    });
}

/// A variable as the window says it: its name, its value in lines (a line to each directory of a list that goes in
/// front of what the variable was, and that last), and what put it there. A secret's value is not said.
struct Variable {
    name: String,
    lines: Vec<String>,
    from: From,
}

impl Variable {
    fn of(engine: &Engine, op: &EnvOp) -> Self {
        let lines = match &op.mode {
            EnvMode::Set { .. } if is_secret_name(&op.name) => vec!["***".to_string()],
            EnvMode::Set { value } => vec![value.clone()],
            EnvMode::Prepend { value, sep } => value
                .split(sep.as_str())
                .filter(|part| !part.is_empty())
                .map(|part| format!("{part}{sep}"))
                .chain(std::iter::once(format!("${}", op.name)))
                .collect(),
        };
        Variable {
            name: op.name.clone(),
            lines,
            from: from(engine, &op.origin),
        }
    }

    /// As a line of a shell sets it.
    fn line(&self) -> String {
        format!("{}={}", self.name, self.lines.concat())
    }
}

/// A variable in full, on as many lines as it takes: `NAME=value` on a row, or a list under its name a line to each of
/// its parts, what it was last.
fn show_variable(ui: &mut egui::Ui, variable: &Variable, from_width: f32) {
    let p = theme::palette();
    match variable.lines.as_slice() {
        [value] => {
            let line = format!("{}={value}", variable.name);
            full_row(ui, &line, p.body, Some(&variable.from), from_width, 0.0);
        }
        lines => {
            let name = format!("{}=", variable.name);
            full_row(ui, &name, p.body, Some(&variable.from), from_width, 0.0);
            let indent = ui.text_style_height(&egui::TextStyle::Monospace) * 1.5;
            let (was, parts) = lines.split_last().expect("a list ends with what it was");
            for part in parts {
                full_row(ui, part, p.body, None, from_width, indent);
            }
            full_row(ui, was, p.muted, None, from_width, indent).on_hover_text(format!(
                "What {} was: the list goes in front of it.",
                variable.name
            ));
        }
    }
}

/// A row that says all of `text` in the font of a command line, `indent` in from the left, on as many lines as it
/// takes (a path broken after a `/`), and what put it there in the column on the right of its first line.
fn full_row(
    ui: &mut egui::Ui,
    text: &str,
    color: egui::Color32,
    said: Option<&From>,
    from_width: f32,
    indent: f32,
) -> egui::Response {
    let height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Min),
        |ui| {
            ui.add_space(indent);
            let words_width = match said {
                Some(_) => (ui.available_width() - from_width - 8.0).max(60.0),
                None => ui.available_width(),
            };
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let glyph = ui
                .painter()
                .layout_no_wrap("0".repeat(10), font, color)
                .size()
                .x
                / 10.0;
            let text = fit(text, ((words_width - 1.0) / glyph) as usize);
            let response = ui
                .allocate_ui_with_layout(
                    egui::vec2(words_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(words_width);
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).monospace().color(color))
                                .wrap(),
                        )
                    },
                )
                .inner;
            if let Some(from) = said {
                let color = match from {
                    From::Model(_) => theme::palette().accent_text,
                    _ => theme::palette().muted,
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), height),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(from.said()).small().color(color),
                                )
                                .truncate(),
                            )
                        },
                    );
                });
            }
            response
        },
    )
    .inner
}

/// `text` in lines of at most `width` characters, each broken after the last `/` or space that fits, else after the
/// last `-` (a name longer than the line), else where it fills the line.
fn fit(text: &str, width: usize) -> String {
    let width = width.max(8);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut start = 0;
    while chars.len() - start > width {
        let line = &chars[start..start + width];
        let after = |at: Option<usize>| at.filter(|at| *at > 0).map(|at| at + 1);
        let cut = after(line.iter().rposition(|c| *c == '/' || *c == ' '))
            .or_else(|| after(line.iter().rposition(|c| *c == '-')))
            .unwrap_or(width);
        out.extend(&chars[start..start + cut]);
        out.push('\n');
        start += cut;
    }
    out.extend(&chars[start..]);
    out
}

/// One row: the words in the font of a command line, shortened at the end when they do not fit (a path is its file's
/// name), and what put them there in the column on the right. All of it is on the row's hover.
fn show_row(ui: &mut egui::Ui, words: &[String], said: &Said<'_>, from_width: f32) {
    let p = theme::palette();
    let (from, names, program) = match said {
        Said::Program(names) => (&From::Document, *names, true),
        Said::Row(from, names) => (*from, *names, false),
    };
    let shown: Vec<String> = words.iter().map(|w| short(w, Some(names))).collect();
    let said = match said {
        Said::Program(names) => names.program.clone().unwrap_or_default(),
        _ => from.said(),
    };
    let color = match from {
        From::Model(_) => p.accent_text,
        _ => p.muted,
    };
    let height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let words_width = (ui.available_width() - from_width - 8.0).max(60.0);
            let text = egui::RichText::new(shown.join(" "))
                .monospace()
                .color(match from {
                    From::Model(_) => p.accent_text,
                    _ if program => p.text,
                    _ => p.body,
                });
            ui.allocate_ui_with_layout(
                egui::vec2(words_width, height),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| ui.add(egui::Label::new(text).truncate()),
            )
            .inner
            .on_hover_text(words.join(" "));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(said).small().color(color)).truncate());
            });
        },
    );
}

/// How wide the column of what put the rows there is: its widest label, up to `FROM_WIDTH`.
fn from_width<'a>(ui: &egui::Ui, froms: impl Iterator<Item = &'a From>) -> f32 {
    let font = egui::TextStyle::Small.resolve(ui.style());
    froms
        .map(|from| {
            ui.painter()
                .layout_no_wrap(from.said(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max)
        .min(FROM_WIDTH)
}

/// The name of a list, as a setting's name is said.
fn heading(ui: &mut egui::Ui, name: &str) -> egui::Response {
    ui.label(egui::RichText::new(name).color(theme::palette().text))
}

/// The arguments in rows: an option and the words after it that are not options, as long as the same thing put them
/// there.
fn rows(engine: &Engine, spawn: &SpawnPlan) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for arg in &spawn.args {
        let from = from(engine, &arg.origin);
        match rows.last_mut() {
            Some(last) if last.from == from && !is_option(&arg.value) => {
                last.words.push(arg.value.clone())
            }
            _ => rows.push(Row {
                words: vec![arg.value.clone()],
                from,
            }),
        }
    }
    rows
}

/// Whether a word is an option (`--port`, `-c`) and not a value (`8080`, `-1`).
fn is_option(word: &str) -> bool {
    word.starts_with('-') && word.len() > 1 && word[1..].parse::<f64>().is_err()
}

/// What the planner's origin of a word says, in the words of the window: the label of a setting, what goes with the
/// model, or the document.
fn from(engine: &Engine, origin: &str) -> From {
    if let Some(key) = origin.strip_prefix("field ") {
        let key = key.split(' ').next().unwrap_or(key);
        let id = key.rsplit_once('.').map_or(key, |(_, id)| id);
        let label = engine
            .doc
            .fields
            .iter()
            .find(|f| f.id == id)
            .map_or_else(|| id.to_string(), quickai_core::profile::label_of);
        // the hint in brackets is the box's, not the setting's name
        let label = label
            .split_once(" (")
            .map_or(label.as_str(), |(name, _)| name);
        return From::Setting(label.to_string());
    }
    if let Some(kind) = origin.strip_prefix("component ") {
        let label = quickai_core::models::bundles(engine)
            .iter()
            .flat_map(|b| b.parts.iter())
            .find(|part| part.kind == kind)
            .map_or_else(|| kind.to_string(), |part| part.label.clone());
        return From::Model(format!("the model's {label}"));
    }
    if let Some(id) = origin.strip_prefix("mode ") {
        let label = quickai_core::models::modes(engine)
            .iter()
            .find(|m| m.id == id)
            .map_or_else(|| id.to_string(), |m| m.label.clone());
        return From::Model(format!("the model: {label}"));
    }
    From::Document
}

/// What the planner says, as a sentence: a capital first and a full stop last.
fn sentence(said: &str) -> String {
    let mut letters = said.chars();
    let mut out: String = letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    });
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// A path is said by its file's name, or by the name the file came with when it was fetched with the model; anything else
/// as it is.
fn short(word: &str, names: Option<&Names>) -> String {
    if let Some((_, name)) = names.and_then(|n| n.files.iter().find(|(path, _)| path == word)) {
        return format!("…/{name}");
    }
    let path = Path::new(word);
    match path.file_name() {
        Some(name) if path.is_absolute() && path.parent().is_some() => {
            format!("…/{}", name.to_string_lossy())
        }
        _ => word.to_string(),
    }
}

/// The command as a line to paste into a shell: a word with a space or a quote in it is quoted.
fn shell_line(spawn: &SpawnPlan) -> String {
    std::iter::once(&spawn.program.value)
        .chain(spawn.args.iter().map(|a| &a.value))
        .map(|w| {
            if w.is_empty() || w.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
                format!("\"{}\"", w.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                w.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::plan::Traced;

    fn spawn(args: &[(&str, &str)]) -> SpawnPlan {
        SpawnPlan {
            program: Traced {
                value: "/opt/server".into(),
                origin: "engine launch.spawn.program".into(),
            },
            args: args
                .iter()
                .map(|(value, origin)| Traced {
                    value: value.to_string(),
                    origin: origin.to_string(),
                })
                .collect(),
            env: Vec::new(),
            group: false,
            identity: None,
            scrub_loader_env: false,
            cwd: None,
            mkdir: Vec::new(),
            write: Vec::new(),
        }
    }

    #[test]
    fn an_option_and_its_values_are_a_row_and_what_put_them_there_is_said_in_words() {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let engine = registry.engine("llamacpp").expect("the shipped engine");
        let plan = spawn(&[
            ("--model", "engine launch.spawn.args"),
            ("/m/a.gguf", "engine launch.spawn.args"),
            ("--port", "engine launch.spawn.args"),
            ("8080", "engine launch.spawn.args"),
            ("--ctx-size", "field llamacpp.ctx_size (default)"),
            ("4096", "field llamacpp.ctx_size (default)"),
            ("--mmproj", "component mmproj"),
            ("/m/p.gguf", "component mmproj"),
            ("--embeddings", "mode embedding"),
            ("--rope-freq-base", "field llamacpp.extra_args"),
            ("-1", "field llamacpp.extra_args"),
        ]);
        let rows = rows(engine, &plan);
        let said: Vec<(String, From)> = rows
            .iter()
            .map(|r| (r.words.join(" "), r.from.clone()))
            .collect();
        assert_eq!(
            said,
            [
                ("--model /m/a.gguf".to_string(), From::Document),
                ("--port 8080".to_string(), From::Document),
                (
                    "--ctx-size 4096".to_string(),
                    From::Setting("Context size".into())
                ),
                (
                    "--mmproj /m/p.gguf".to_string(),
                    From::Model("the model's projector".into())
                ),
                (
                    "--embeddings".to_string(),
                    From::Model("the model: embedding server".into())
                ),
                (
                    "--rope-freq-base -1".to_string(),
                    From::Setting("Extra command-line arguments".into())
                ),
            ]
        );
        assert_eq!(short("/m/p.gguf", None), "…/p.gguf");
        assert_eq!(short("8080", None), "8080");
        let names = Names {
            files: vec![("/m/me--r--mmproj-f16.gguf".into(), "mmproj-f16.gguf".into())],
            program: None,
        };
        assert_eq!(
            short("/m/me--r--mmproj-f16.gguf", Some(&names)),
            "…/mmproj-f16.gguf",
            "a file fetched with the model by the name it came with"
        );
        assert_eq!(
            fit("/home/me/.quickai/llama-bin/b1-llama-bin-x64/llama-b1:", 30),
            "/home/me/.quickai/llama-bin/\nb1-llama-bin-x64/llama-b1:",
            "a path broken after a `/`"
        );
        assert_eq!(
            fit("/opt/b1-bin-ubuntu-x64/", 16),
            "/opt/\nb1-bin-ubuntu-\nx64/",
            "a name longer than the line broken after a `-`"
        );
        assert_eq!(fit("A=0123456789abcdef", 8), "A=012345\n6789abcd\nef");
        assert_eq!(
            shell_line(&spawn(&[
                ("--alias", "engine launch.spawn.args"),
                ("a b", "x")
            ])),
            "/opt/server --alias \"a b\""
        );
    }
}
