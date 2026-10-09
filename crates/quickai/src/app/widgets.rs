//! The widget registry (docs/architecture.md, section 9.2): how a field of each type is drawn and
//! edited. A widget is given what the form model says about a field and hands back what the person
//! did with it: the value they made it hold, an action they asked for, or nothing when they left it
//! alone. The label and the hover text belong to the panel that places the widget; the checkbox and
//! the button are the widgets that carry their own.
//!
//! A field's type chooses its widget, and `ui.widget` chooses another (`NAMED`: the names a document
//! may use, which the loader holds documents to). The runtime manager is drawn by its panel; the
//! progress bar is not a field's at all, it is drawn for the jobs that report how far along they are.

use std::path::Path;

use eframe::egui;
use quickai_core::doc::engine::FieldType;
use quickai_core::form::FormField;
use quickai_core::value::Value;

/// What a widget hands back.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// The value the person made the field hold.
    Set(Value),
    /// An action they asked for, by the id a button's document gave it (`open:settings`).
    Invoke(String),
}

/// The widgets a document may name with `ui.widget`, as opposed to the one the field's type has.
/// What the loader holds a document to (`prims::known`), so a name here is one that something draws.
pub const NAMED: &[&str] = &["runtime_manager", "button"];

/// The red of a message under a field.
pub(super) const PROBLEM: egui::Color32 = egui::Color32::from_rgb(192, 57, 43);

/// Rows a box of key-value lines shows before it scrolls.
const KV_ROWS: usize = 4;

/// How a path picker's browse button asks for a path, given the kind of path wanted and what is in
/// the box now: a native dialog in the application, a stand-in in a test.
pub type Browse<'a> = &'a mut dyn FnMut(FieldType, &str) -> Option<String>;

/// Draws the widget for the field: the one its document names, else the one its type has.
pub fn show(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    show_with(ui, field, &mut native_browse)
}

/// `show` with the way a path is browsed for given.
pub fn show_with(ui: &mut egui::Ui, field: &FormField, browse: Browse) -> Option<Reply> {
    if field.widget.as_deref() == Some("button") {
        return button(ui, field);
    }
    match field.ty {
        // a number that may be empty (the document gave no default and says what empty means) is
        // typed; every other is dragged
        FieldType::Int | FieldType::Float if field.hint.is_some() => number_box(ui, field),
        FieldType::Int | FieldType::Port => drag_integer(ui, field),
        FieldType::Float => drag_float(ui, field),
        FieldType::Bool => checkbox(ui, field),
        FieldType::Choice => combo(ui, field),
        FieldType::Hidden => None,
        FieldType::Dir | FieldType::File => path_picker(ui, field, browse),
        FieldType::KvLines => kv_lines(ui, field),
        FieldType::Text | FieldType::Url | FieldType::RuntimePath | FieldType::ModelRef => {
            text_line(ui, field)
        }
    }
}

/// The whole numbers a field may hold: its `range`, and a port is never outside 1 to 65535.
pub fn integer_range(field: &FormField) -> (i64, i64) {
    let (lo, hi) = match field.range {
        Some((lo, hi)) => (lo.ceil() as i64, hi.floor() as i64),
        None => (i64::MIN, i64::MAX),
    };
    if field.ty == FieldType::Port {
        (lo.max(1), hi.min(65535))
    } else {
        (lo, hi)
    }
}

/// How far one pixel of dragging moves a number: the field's `step`, else 1.
pub fn drag_speed(field: &FormField) -> f64 {
    field.step.unwrap_or(1.0)
}

fn set(value: Value) -> Option<Reply> {
    Some(Reply::Set(value))
}

fn drag_integer(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let mut number = field.value.as_i64().unwrap_or_default();
    let (lo, hi) = integer_range(field);
    let drag = egui::DragValue::new(&mut number)
        .range(lo..=hi)
        .speed(drag_speed(field));
    ui.add(drag)
        .changed()
        .then_some(Reply::Set(Value::Int(number)))
}

fn drag_float(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let mut number = field.value.as_f64().unwrap_or_default();
    let mut drag = egui::DragValue::new(&mut number).speed(drag_speed(field));
    if let Some((lo, hi)) = field.range {
        drag = drag.range(lo..=hi);
    }
    ui.add(drag)
        .changed()
        .then_some(Reply::Set(Value::Float(number)))
}

/// A number in a box that can be empty: the hint says what empty means, typing a number sets it, and
/// emptying the box takes it away again. Whatever is typed that is not a digit (or, for a float, one
/// point) is dropped; the range is the form's to report, since a number half typed is not out of it.
fn number_box(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let mut text = match &field.value {
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let float = field.ty == FieldType::Float;
    let mut edit = egui::TextEdit::singleline(&mut text).desired_width(72.0);
    if let Some(hint) = &field.hint {
        edit = edit.hint_text(hint);
    }
    let changed = ui.add(edit).changed();
    for problem in problem_lines(field.error.as_deref()) {
        ui.colored_label(PROBLEM, problem);
    }
    if !changed {
        return None;
    }
    let mut points = 0;
    text.retain(|c| {
        if c == '.' && float && points == 0 {
            points += 1;
            true
        } else {
            c.is_ascii_digit()
        }
    });
    let value = if text.is_empty() {
        Value::Null
    } else if float {
        text.parse::<f64>().map_or(Value::Null, Value::Float)
    } else {
        text.parse::<i64>().map_or(Value::Null, Value::Int)
    };
    set(value)
}

fn checkbox(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let mut on = field.value.as_bool().unwrap_or_default();
    let response = ui.checkbox(&mut on, &field.label);
    let response = match &field.help {
        Some(help) => response.on_hover_text(help),
        None => response,
    };
    response.changed().then_some(Reply::Set(Value::Bool(on)))
}

fn combo(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let current = field.value.as_str().unwrap_or_default().to_string();
    let mut chosen = current.clone();
    egui::ComboBox::from_id_salt(&field.key)
        .selected_text(&current)
        .show_ui(ui, |ui| {
            for option in &field.options {
                ui.selectable_value(&mut chosen, option.clone(), option);
            }
        });
    // what is wrong with the choice is said under it; in a row (the controls of the main window) it is said under the
    // row, by whoever drew it, since a sentence beside the box would be off the window
    if ui.layout().main_dir().is_vertical() {
        for problem in problem_lines(field.error.as_deref()) {
            ui.colored_label(PROBLEM, problem);
        }
    }
    (chosen != current).then_some(Reply::Set(Value::Str(chosen)))
}

fn text_line(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let mut text = field.value.as_str().unwrap_or_default().to_string();
    let mut edit = egui::TextEdit::singleline(&mut text).desired_width(ui.available_width());
    if let Some(hint) = &field.hint {
        edit = edit.hint_text(hint);
    }
    ui.add(edit)
        .changed()
        .then_some(Reply::Set(Value::Str(text)))
}

/// Lines of `KEY=VALUE`: a box of several rows, and under it what the form says is wrong with
/// them, a message to a row. The text is handed back exactly as typed; reading it is the planner's.
fn kv_lines(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let mut text = field.value.as_str().unwrap_or_default().to_string();
    let mut edit = egui::TextEdit::multiline(&mut text)
        .font(egui::TextStyle::Monospace)
        .desired_rows(KV_ROWS)
        .desired_width(ui.available_width());
    if let Some(hint) = &field.hint {
        edit = edit.hint_text(hint);
    }
    let changed = ui.add(edit).changed();
    for problem in problem_lines(field.error.as_deref()) {
        ui.colored_label(PROBLEM, problem);
    }
    changed.then_some(Reply::Set(Value::Str(text)))
}

/// The messages of a field's error, one to a row: the form puts each problem on a line of its own.
pub fn problem_lines(error: Option<&str>) -> Vec<&str> {
    error
        .map(|e| e.lines().filter(|l| !l.trim().is_empty()).collect())
        .unwrap_or_default()
}

/// A folder or a file: a text box, which can be typed in and pasted into, and a button that asks
/// for the path in a dialog. A path is kept exactly as typed or as chosen.
pub fn path_picker(ui: &mut egui::Ui, field: &FormField, browse: Browse) -> Option<Reply> {
    let mut text = field.value.as_str().unwrap_or_default().to_string();
    let mut reply = None;
    ui.horizontal(|ui| {
        // the button first, from the right: the box takes what is left
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Browse…").clicked() {
                if let Some(path) = browse(field.ty, &text) {
                    reply = set(Value::Str(path));
                }
            }
            let mut edit =
                egui::TextEdit::singleline(&mut text).desired_width(ui.available_width());
            if let Some(hint) = &field.hint {
                edit = edit.hint_text(hint);
            }
            if ui.add(edit).changed() {
                reply = set(Value::Str(text.clone()));
            }
        });
    });
    reply
}

/// A native dialog for a folder or a file, opened where the box already points when it points
/// somewhere that exists.
fn native_browse(ty: FieldType, current: &str) -> Option<String> {
    let mut dialog = rfd::FileDialog::new();
    let at = Path::new(current);
    let start = if at.is_dir() { Some(at) } else { at.parent() };
    if let Some(dir) = start.filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    let picked = if ty == FieldType::Dir {
        dialog.pick_folder()
    } else {
        dialog.pick_file()
    };
    picked.map(|path| path.to_string_lossy().into_owned())
}

/// A button for a field that holds nothing: its label, its help as hover text, and a click hands
/// back the action its document gave it. Pressed only while the field is enabled.
fn button(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let action = field.action.as_ref()?;
    let mut response = ui.add_enabled(field.enabled, egui::Button::new(&field.label));
    if let Some(help) = &field.help {
        response = response.on_hover_text(help);
    }
    response.clicked().then(|| Reply::Invoke(action.clone()))
}

/// A bar for how far along something is, from 0 to 1: beyond that it is full or empty, and `None`
/// (or something that is not a number) is work whose size is not known, an empty bar that moves.
pub fn progress_bar(ui: &mut egui::Ui, fraction: Option<f32>) {
    let bar = match fraction.filter(|f| !f.is_nan()) {
        Some(f) => egui::ProgressBar::new(f.clamp(0.0, 1.0)),
        None => egui::ProgressBar::new(0.0).animate(true),
    };
    ui.add(bar);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::doc::engine::{Applies, Persist};
    use quickai_core::plan::Origin;

    fn field(
        ty: FieldType,
        range: Option<(f64, f64)>,
        step: Option<f64>,
        value: Value,
    ) -> FormField {
        FormField {
            id: "f".into(),
            key: "llamacpp.f".into(),
            label: "F".into(),
            help: None,
            hint: None,
            ty,
            widget: None,
            action: None,
            value,
            origin: Origin::Selection,
            visible: true,
            enabled: true,
            required: false,
            error: None,
            range,
            step,
            options: vec!["a".into(), "b".into()],
            applies: Applies::OnStart,
            persist: Persist::Global,
            legacy_key: None,
        }
    }

    fn set_of(value: Value) -> Option<Reply> {
        Some(Reply::Set(value))
    }

    #[test]
    fn an_integer_field_is_limited_to_its_range() {
        let ctx = field(
            FieldType::Int,
            Some((256.0, 131072.0)),
            Some(64.0),
            Value::Int(4096),
        );
        assert_eq!(integer_range(&ctx), (256, 131072));
        assert_eq!(drag_speed(&ctx), 64.0);
        let layers = field(FieldType::Int, Some((0.0, 999.0)), None, Value::Int(999));
        assert_eq!(integer_range(&layers), (0, 999));
        assert_eq!(drag_speed(&layers), 1.0, "no step: one per pixel");
        let free = field(FieldType::Int, None, None, Value::Int(0));
        assert_eq!(integer_range(&free), (i64::MIN, i64::MAX));
    }

    #[test]
    fn a_port_stays_within_1_to_65535_whatever_the_document_says() {
        let port = field(FieldType::Port, None, None, Value::Int(8080));
        assert_eq!(integer_range(&port), (1, 65535));
        let wide = field(
            FieldType::Port,
            Some((0.0, 100000.0)),
            None,
            Value::Int(8080),
        );
        assert_eq!(integer_range(&wide), (1, 65535));
        let narrow = field(
            FieldType::Port,
            Some((2000.0, 3000.0)),
            None,
            Value::Int(2500),
        );
        assert_eq!(integer_range(&narrow), (2000, 3000));
    }

    /// One egui context that stays alive between frames, as a window's does: a click is decided
    /// against the layout of the frame before it, so a widget has to have been drawn once already.
    /// What a widget hands back as a value is fed to the next frame, as the application does.
    struct Harness {
        ctx: egui::Context,
        field: FormField,
        time: f64,
        /// What the stand-in for the browse dialog answers, and what it was asked.
        picks: Option<String>,
        asked: Vec<(FieldType, String)>,
    }

    impl Harness {
        fn new(field: FormField) -> Self {
            let mut harness = Harness {
                ctx: egui::Context::default(),
                field,
                time: 0.0,
                picks: None,
                asked: Vec::new(),
            };
            harness.frame(vec![]);
            harness.frame(vec![]);
            harness
        }

        /// Runs one frame of `show` with `events` delivered first. Returns what the widget handed
        /// back and where it was drawn.
        fn frame(&mut self, events: Vec<egui::Event>) -> (Option<Reply>, egui::Rect) {
            let (reply, rect) = self.draw(events, |ui, field, browse| show_with(ui, field, browse));
            if let Some(Reply::Set(value)) = &reply {
                self.field.value = value.clone();
            }
            (reply, rect)
        }

        /// One frame of whatever `widget` draws, in the same kind of panel.
        fn draw<R>(
            &mut self,
            events: Vec<egui::Event>,
            mut widget: impl FnMut(&mut egui::Ui, &FormField, Browse) -> R,
        ) -> (R, egui::Rect) {
            self.time += 0.1;
            let mut rect = egui::Rect::NOTHING;
            let mut out = None;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let field = &self.field;
            let (picks, asked) = (&self.picks, &mut self.asked);
            let mut output = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    // a child Ui starting at the cursor, so its used area is the widget's own
                    let mut child = ui.new_child(
                        egui::UiBuilder::new().max_rect(ui.available_rect_before_wrap()),
                    );
                    let mut browse = |ty: FieldType, now: &str| {
                        asked.push((ty, now.to_string()));
                        picks.clone()
                    };
                    out = Some(widget(&mut child, field, &mut browse));
                    rect = child.min_rect();
                });
            });
            // nothing is painted, so the texture changes are dropped on purpose
            output.textures_delta.clear();
            (out.expect("the widget was drawn"), rect)
        }

        fn press(&mut self, at: egui::Pos2, pressed: bool) -> Option<Reply> {
            self.frame(vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }])
            .0
        }

        /// A click at `at`: the pointer arrives, presses and lets go. Whatever the widget handed
        /// back on the way (the release is where a button answers) comes out.
        fn click_at(&mut self, at: egui::Pos2) -> Option<Reply> {
            self.frame(vec![egui::Event::PointerMoved(at)]);
            let down = self.press(at, true);
            let up = self.press(at, false);
            up.or(down)
        }

        fn click(&mut self) -> Option<Reply> {
            let at = self.frame(vec![]).1.left_center() + egui::vec2(8.0, 0.0);
            self.click_at(at)
        }

        /// Presses on the widget and moves `dx` pixels, then releases; every change handed back on
        /// the way is returned, the last one being where the drag ended.
        fn drag(&mut self, dx: f32) -> Vec<Value> {
            let start = self.frame(vec![]).1.center();
            let mut seen = Vec::new();
            let mut take = |reply: Option<Reply>| {
                if let Some(Reply::Set(value)) = reply {
                    seen.push(value);
                }
            };
            self.frame(vec![egui::Event::PointerMoved(start)]);
            let down = self.press(start, true);
            take(down);
            for step in 1..=4 {
                let to = start + egui::vec2(dx * step as f32 / 4.0, 0.0);
                let moved = self.frame(vec![egui::Event::PointerMoved(to)]).0;
                take(moved);
            }
            let up = self.press(start + egui::vec2(dx, 0.0), false);
            take(up);
            seen
        }

        /// Types into the box under the pointer: `\n` is the Enter key and the rest is text, so a
        /// multi-line box gets what a person's keyboard would give it.
        fn type_text(&mut self, text: &str) {
            let key = |key: egui::Key| egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            };
            for (i, piece) in text.split('\n').enumerate() {
                if i > 0 {
                    self.frame(vec![key(egui::Key::Enter)]);
                }
                if !piece.is_empty() {
                    self.frame(vec![egui::Event::Text(piece.to_string())]);
                }
            }
        }
    }

    #[test]
    fn a_widget_that_is_left_alone_hands_back_nothing() {
        for f in [
            field(FieldType::Int, Some((0.0, 10.0)), None, Value::Int(3)),
            field(FieldType::Port, None, None, Value::Int(8080)),
            field(
                FieldType::Float,
                Some((0.0, 2.0)),
                Some(0.1),
                Value::Float(0.7),
            ),
            field(FieldType::Bool, None, None, Value::Bool(true)),
            field(FieldType::Choice, None, None, Value::from("a")),
            field(FieldType::Text, None, None, Value::from("hello")),
            field(FieldType::KvLines, None, None, Value::from("A=1\nB=2")),
            field(FieldType::Dir, None, None, Value::from("/models")),
            field(FieldType::File, None, None, Value::from("/models/a.gguf")),
            field(FieldType::Hidden, None, None, Value::Null),
        ] {
            let ty = f.ty;
            assert_eq!(Harness::new(f).frame(vec![]).0, None, "{ty:?}");
        }
        let mut button = field(FieldType::Hidden, None, None, Value::Null);
        button.widget = Some("button".into());
        button.action = Some("open:settings".into());
        assert_eq!(Harness::new(button).frame(vec![]).0, None);
    }

    #[test]
    fn dragging_a_number_moves_it_in_the_drag_direction_and_stops_at_the_range() {
        let f = field(
            FieldType::Int,
            Some((0.0, 1000.0)),
            Some(64.0),
            Value::Int(512),
        );
        let right = Harness::new(f.clone()).drag(400.0);
        let Some(Value::Int(far)) = right.last() else {
            panic!("a long drag to the right changed nothing: {right:?}");
        };
        assert_eq!(
            *far, 1000,
            "it runs into the top of the range and stays there"
        );
        let left = Harness::new(f).drag(-400.0);
        let Some(Value::Int(near)) = left.last() else {
            panic!("a long drag to the left changed nothing: {left:?}");
        };
        assert_eq!(*near, 0, "and into the bottom");
    }

    #[test]
    fn the_step_is_how_fast_a_drag_moves_the_number() {
        // The same drag moves a field with a step of 64 about 64 times as far as one with no step.
        let moved = |step: Option<f64>| {
            let f = field(
                FieldType::Int,
                Some((0.0, 1_000_000.0)),
                step,
                Value::Int(1000),
            );
            let seen = Harness::new(f).drag(8.0);
            let Some(Value::Int(end)) = seen.last() else {
                panic!("the drag changed nothing: {seen:?}");
            };
            end - 1000
        };
        let (fast, slow) = (moved(Some(64.0)), moved(None));
        assert!(slow >= 1, "even with no step a drag moves a number: {slow}");
        assert!(
            fast >= slow * 32,
            "step 64 moved {fast}, no step moved {slow}"
        );
    }

    #[test]
    fn clicking_a_checkbox_hands_back_the_opposite() {
        let off = field(FieldType::Bool, None, None, Value::Bool(false));
        assert_eq!(Harness::new(off).click(), set_of(Value::Bool(true)));
        let on = field(FieldType::Bool, None, None, Value::Bool(true));
        assert_eq!(Harness::new(on).click(), set_of(Value::Bool(false)));
    }

    fn kv(value: &str, error: Option<&str>) -> FormField {
        let mut f = field(FieldType::KvLines, None, None, Value::from(value));
        f.error = error.map(str::to_string);
        f
    }

    #[test]
    fn p3_w01_key_value_lines_are_a_box_of_several_rows_that_hands_back_the_text_as_typed() {
        // several rows: well over what a one-line box takes
        let line = Harness::new(field(FieldType::Text, None, None, Value::from("")))
            .frame(vec![])
            .1
            .height();
        let rows = Harness::new(kv("", None)).frame(vec![]).1.height();
        assert!(
            rows > 2.5 * line,
            "a box of {rows}px against a line of {line}px"
        );

        // typed into an empty box: nothing is trimmed, dropped or reordered on the way
        let mut h = Harness::new(kv("", None));
        let at = h.frame(vec![]).1.center();
        h.click_at(at);
        let typed = "  KEY = a value  \n\n# a note, then a line with = in it\nB=\"quoted\" # x";
        h.type_text(typed);
        assert_eq!(h.field.value, Value::from(typed));
        // and the widget hands back each state it was left in
        let last = h.frame(vec![egui::Event::Text("!".into())]).0;
        assert_eq!(last, set_of(Value::from(format!("{typed}!"))));
    }

    #[test]
    fn p3_w01b_the_forms_messages_show_under_the_box_one_to_a_bad_line() {
        let text = "A=1\nnot a line\nOLLAMA_HOST=x";
        let height = |error: Option<&str>| Harness::new(kv(text, error)).frame(vec![]).1.height();
        let none = height(None);
        let one = height(Some("Line 2: `not a line` is not NAME=VALUE."));
        let two = height(Some(
            "Line 2: `not a line` is not NAME=VALUE.\nLine 3: OLLAMA_HOST is not yours.",
        ));
        assert!(one > none + 8.0, "one message adds a row: {none} -> {one}");
        let (first, second) = (one - none, two - one);
        assert!(
            (first - second).abs() < 1.0,
            "a second message adds a second row, not more or less: {first} then {second}"
        );

        // the split of an error into its rows
        assert_eq!(problem_lines(None), Vec::<&str>::new());
        assert_eq!(problem_lines(Some("")), Vec::<&str>::new());
        assert_eq!(
            problem_lines(Some(
                "Line 2: a.\n\nLine 5: b. Line 9: not a new message.\n"
            )),
            ["Line 2: a.", "Line 5: b. Line 9: not a new message."]
        );
    }

    #[test]
    fn p3_w06_a_number_with_a_hint_is_an_empty_box_until_typed_into_and_empties_again() {
        let mut f = field(
            FieldType::Int,
            Some((256.0, 131072.0)),
            Some(64.0),
            Value::Null,
        );
        f.hint = Some("Ollama's own default".into());
        let mut h = Harness::new(f);
        let at = h.frame(vec![]).1.left_center() + egui::vec2(8.0, 0.0);
        // an empty number is not 0, and left alone it hands back nothing
        assert_eq!(h.frame(vec![]).0, None);
        h.click_at(at);
        h.type_text("8x1a9y2");
        assert_eq!(h.field.value, Value::Int(8192), "only the digits count");
        // emptied again, it is no number at all
        for _ in 0..4 {
            h.frame(vec![egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: Some(egui::Key::Backspace),
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }]);
        }
        assert_eq!(h.field.value, Value::Null);
        // a number with no hint is the drag box it has always been (no way to be empty)
        let plain = field(
            FieldType::Int,
            Some((256.0, 131072.0)),
            Some(64.0),
            Value::Int(4096),
        );
        let drag = Harness::new(plain).frame(vec![]).1.width();
        let boxed = Harness::new({
            let mut b = field(FieldType::Int, None, None, Value::Int(4096));
            b.hint = Some("x".into());
            b
        })
        .frame(vec![])
        .1
        .width();
        assert!(drag < boxed || boxed > 60.0, "{drag} {boxed}");
        // a float may have one point
        let mut g = field(FieldType::Float, None, None, Value::Null);
        g.hint = Some("default".into());
        let mut h = Harness::new(g);
        let at = h.frame(vec![]).1.left_center() + egui::vec2(8.0, 0.0);
        h.click_at(at);
        h.type_text("0.7.5");
        assert_eq!(h.field.value, Value::Float(0.75));
    }

    #[test]
    fn p3_w02_the_progress_bar_draws_for_every_state_it_can_be_given() {
        let fractions = [
            None,
            Some(0.0),
            Some(0.4),
            Some(1.0),
            Some(1.5),
            Some(-0.5),
            Some(f32::NAN),
            Some(f32::INFINITY),
            Some(f32::NEG_INFINITY),
        ];
        let mut h = Harness::new(field(FieldType::Hidden, None, None, Value::Null));
        for fraction in fractions {
            let (_, rect) = h.draw(vec![], |ui, _, _| progress_bar(ui, fraction));
            assert!(
                rect.width() >= 96.0 && rect.height() > 4.0,
                "{fraction:?} drew {rect:?}"
            );
        }
        // from the byte counts of a job: a total of 0, more than the total, exactly the total
        for (done, total) in [(5, Some(0)), (150, Some(100)), (100, Some(100)), (7, None)] {
            let fraction = crate::jobs::fraction_of(done, total);
            let (_, rect) = h.draw(vec![], |ui, _, _| progress_bar(ui, fraction));
            assert!(rect.width() > 0.0, "{done} of {total:?}");
        }
    }

    #[test]
    fn p3_w02b_only_a_bar_with_no_known_end_keeps_the_window_repainting() {
        // an indeterminate bar moves, so it asks for the next frame at once; a bar that says how far
        // along it is, or is full, only changes when the number does
        let mut h = Harness::new(field(FieldType::Hidden, None, None, Value::Null));
        let wants_repaint = |h: &mut Harness, fraction: Option<f32>| {
            h.time += 0.1;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                time: Some(h.time),
                ..Default::default()
            };
            let output = h.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| progress_bar(ui, fraction));
            });
            output
                .viewport_output
                .values()
                .any(|v| v.repaint_delay.is_zero())
        };
        // (a widget drawn for the first time asks for a frame to settle in, so each is drawn twice)
        wants_repaint(&mut h, None);
        assert!(
            wants_repaint(&mut h, None),
            "an unknown end is drawn moving"
        );
        wants_repaint(&mut h, Some(0.4));
        assert!(!wants_repaint(&mut h, Some(0.4)));
        wants_repaint(&mut h, Some(1.0));
        assert!(!wants_repaint(&mut h, Some(1.0)));
    }

    fn button_field(enabled: bool) -> FormField {
        let mut f = field(FieldType::Hidden, None, None, Value::Null);
        f.label = "Open Settings".into();
        f.widget = Some("button".into());
        f.action = Some("open:settings".into());
        f.enabled = enabled;
        f
    }

    #[test]
    fn p3_w03_a_click_on_a_button_hands_back_its_action_once() {
        let mut h = Harness::new(button_field(true));
        assert_eq!(
            h.click(),
            Some(Reply::Invoke("open:settings".to_string())),
            "the click is answered with the action"
        );
        assert_eq!(
            h.frame(vec![]).0,
            None,
            "and only the click: not the frames after it"
        );
        // the pointer is still on the button: nothing more until it is pressed again
        assert_eq!(h.frame(vec![]).0, None);
        assert_eq!(
            h.click(),
            Some(Reply::Invoke("open:settings".to_string())),
            "a second click is a second action"
        );
    }

    #[test]
    fn p3_w03c_a_button_whose_field_is_disabled_cannot_be_pressed() {
        let mut h = Harness::new(button_field(false));
        for _ in 0..3 {
            assert_eq!(h.click(), None);
        }
        // the same button, enabled, answers: the difference is the field's `enabled_when`
        h.field.enabled = true;
        assert_eq!(h.click(), Some(Reply::Invoke("open:settings".to_string())));
        // a button with no action to hand back draws and does nothing (the loader refuses one)
        let mut none = button_field(true);
        none.action = None;
        assert_eq!(Harness::new(none).click(), None);
    }

    fn picker(ty: FieldType, value: &str) -> FormField {
        field(ty, None, None, Value::from(value))
    }

    #[test]
    fn p3_w04_a_path_picker_is_a_box_to_type_in_and_a_button_that_browses() {
        let mut h = Harness::new(picker(FieldType::Dir, ""));
        let rect = h.frame(vec![]).1;
        // the box on the left takes what is typed, with spaces and letters of any script
        h.click_at(rect.left_center() + egui::vec2(8.0, 0.0));
        let typed = "/home/me/My Models/модели/café 日本";
        h.type_text(typed);
        assert_eq!(h.field.value, Value::from(typed), "kept as it was typed");
        assert!(h.asked.is_empty(), "typing does not open a dialog");

        // spaces at either end are the person's too: nothing is trimmed until something reads the path
        let mut spaced = Harness::new(picker(FieldType::Dir, ""));
        let at = spaced.frame(vec![]).1.left_center() + egui::vec2(8.0, 0.0);
        spaced.click_at(at);
        spaced.type_text("  /srv/with space/ ");
        assert_eq!(spaced.field.value, Value::from("  /srv/with space/ "));

        // the button on the right asks for a path, and what it brings back is kept as it is
        h.picks = Some("/data/ollama models/Ünïcode dir".to_string());
        let rect = h.frame(vec![]).1;
        let reply = h.click_at(rect.right_center() - egui::vec2(8.0, 0.0));
        assert_eq!(
            reply,
            set_of(Value::from("/data/ollama models/Ünïcode dir"))
        );
        assert_eq!(
            h.asked,
            [(FieldType::Dir, typed.to_string())],
            "it is told the kind of path and where the box points now"
        );

        // a file field asks for a file
        let mut files = Harness::new(picker(FieldType::File, "/a/b.gguf"));
        files.picks = Some("/c/d e.gguf".to_string());
        let rect = files.frame(vec![]).1;
        files.click_at(rect.right_center() - egui::vec2(8.0, 0.0));
        assert_eq!(files.field.value, Value::from("/c/d e.gguf"));
        assert_eq!(files.asked, [(FieldType::File, "/a/b.gguf".to_string())]);

        // a dialog that is cancelled changes nothing
        let mut cancelled = Harness::new(picker(FieldType::Dir, "/keep"));
        let rect = cancelled.frame(vec![]).1;
        assert_eq!(
            cancelled.click_at(rect.right_center() - egui::vec2(8.0, 0.0)),
            None
        );
        assert_eq!(cancelled.field.value, Value::from("/keep"));
        assert_eq!(cancelled.asked.len(), 1);
    }

    #[test]
    fn the_widgets_a_document_may_name_are_the_ones_the_loader_is_told_about() {
        assert_eq!(NAMED, ["runtime_manager", "button"]);
        let known = crate::prims::known();
        let told: Vec<&str> = known
            .widget_names()
            .expect("the application says which widgets it has")
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(told, ["button", "runtime_manager"]);
    }
}
