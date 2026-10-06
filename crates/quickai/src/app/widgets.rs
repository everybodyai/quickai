//! The widget registry (docs/architecture.md, section 9.2): how a field of each type is drawn and
//! edited. A widget is given what the form model says about a field and hands back the value the
//! person made it, or nothing when they left it alone. The label and the hover text belong to the
//! panel that places the widget; the checkbox is the one widget that carries its own label.
//!
//! Only the widgets the engine documents use today are here. Key-value lines, a path picker with a
//! browse button and a progress bar arrive with the first form that needs them.

use eframe::egui;
use quickai_core::doc::engine::FieldType;
use quickai_core::form::FormField;
use quickai_core::value::Value;

/// Draws the widget for the field's type.
pub fn show(ui: &mut egui::Ui, field: &FormField) -> Option<Value> {
    match field.ty {
        FieldType::Int | FieldType::Port => drag_integer(ui, field),
        FieldType::Float => drag_float(ui, field),
        FieldType::Bool => checkbox(ui, field),
        FieldType::Choice => combo(ui, field),
        FieldType::Hidden => None,
        FieldType::Text
        | FieldType::Dir
        | FieldType::File
        | FieldType::Url
        | FieldType::RuntimePath
        | FieldType::KvLines
        | FieldType::ModelRef => text_line(ui, field),
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

fn drag_integer(ui: &mut egui::Ui, field: &FormField) -> Option<Value> {
    let mut number = field.value.as_i64().unwrap_or_default();
    let (lo, hi) = integer_range(field);
    let drag = egui::DragValue::new(&mut number)
        .range(lo..=hi)
        .speed(drag_speed(field));
    ui.add(drag).changed().then_some(Value::Int(number))
}

fn drag_float(ui: &mut egui::Ui, field: &FormField) -> Option<Value> {
    let mut number = field.value.as_f64().unwrap_or_default();
    let mut drag = egui::DragValue::new(&mut number).speed(drag_speed(field));
    if let Some((lo, hi)) = field.range {
        drag = drag.range(lo..=hi);
    }
    ui.add(drag).changed().then_some(Value::Float(number))
}

fn checkbox(ui: &mut egui::Ui, field: &FormField) -> Option<Value> {
    let mut on = field.value.as_bool().unwrap_or_default();
    let response = ui.checkbox(&mut on, &field.label);
    let response = match &field.help {
        Some(help) => response.on_hover_text(help),
        None => response,
    };
    response.changed().then_some(Value::Bool(on))
}

fn combo(ui: &mut egui::Ui, field: &FormField) -> Option<Value> {
    let current = field.value.as_str().unwrap_or_default().to_string();
    let mut chosen = current.clone();
    egui::ComboBox::from_id_salt(&field.key)
        .selected_text(&current)
        .show_ui(ui, |ui| {
            for option in &field.options {
                ui.selectable_value(&mut chosen, option.clone(), option);
            }
        });
    (chosen != current).then_some(Value::Str(chosen))
}

fn text_line(ui: &mut egui::Ui, field: &FormField) -> Option<Value> {
    let mut text = field.value.as_str().unwrap_or_default().to_string();
    let mut edit = egui::TextEdit::singleline(&mut text).desired_width(ui.available_width());
    if let Some(hint) = &field.hint {
        edit = edit.hint_text(hint);
    }
    ui.add(edit).changed().then_some(Value::Str(text))
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
    struct Harness {
        ctx: egui::Context,
        field: FormField,
        time: f64,
    }

    impl Harness {
        fn new(field: FormField) -> Self {
            let mut harness = Harness {
                ctx: egui::Context::default(),
                field,
                time: 0.0,
            };
            harness.frame(vec![]);
            harness.frame(vec![]);
            harness
        }

        /// Runs one frame of `show` with `events` delivered first. Returns what the widget handed
        /// back and where it was drawn.
        fn frame(&mut self, events: Vec<egui::Event>) -> (Option<Value>, egui::Rect) {
            self.time += 0.1;
            let mut rect = egui::Rect::NOTHING;
            let mut edit = None;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let field = &self.field;
            let mut output = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    // a child Ui starting at the cursor, so its used area is the widget's own
                    let mut child = ui.new_child(
                        egui::UiBuilder::new().max_rect(ui.available_rect_before_wrap()),
                    );
                    edit = show(&mut child, field);
                    rect = child.min_rect();
                });
            });
            // nothing is painted, so the texture changes are dropped on purpose
            output.textures_delta.clear();
            (edit, rect)
        }

        fn press(&mut self, at: egui::Pos2, pressed: bool) -> Option<Value> {
            self.frame(vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }])
            .0
        }

        fn click(&mut self) -> Option<Value> {
            let at = self.frame(vec![]).1.left_center() + egui::vec2(8.0, 0.0);
            self.frame(vec![egui::Event::PointerMoved(at)]);
            self.press(at, true);
            self.press(at, false)
        }

        /// Presses on the widget and moves `dx` pixels, then releases; every change handed back on
        /// the way is returned, the last one being where the drag ended.
        fn drag(&mut self, dx: f32) -> Vec<Value> {
            let start = self.frame(vec![]).1.center();
            let mut seen = Vec::new();
            self.frame(vec![egui::Event::PointerMoved(start)]);
            seen.extend(self.press(start, true));
            for step in 1..=4 {
                let to = start + egui::vec2(dx * step as f32 / 4.0, 0.0);
                seen.extend(self.frame(vec![egui::Event::PointerMoved(to)]).0);
            }
            seen.extend(self.press(start + egui::vec2(dx, 0.0), false));
            seen
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
            field(FieldType::Hidden, None, None, Value::Null),
        ] {
            let ty = f.ty;
            assert_eq!(Harness::new(f).frame(vec![]).0, None, "{ty:?}");
        }
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
        assert_eq!(Harness::new(off).click(), Some(Value::Bool(true)));
        let on = field(FieldType::Bool, None, None, Value::Bool(true));
        assert_eq!(Harness::new(on).click(), Some(Value::Bool(false)));
    }
}
