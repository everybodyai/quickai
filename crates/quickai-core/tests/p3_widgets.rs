//! The widgets a document may name (work package 3.14, docs/phase3-test-plan.md group W): a field
//! that names a widget the interface does not have is a load error that says where, and a button is
//! a field that says what a click does.

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::form::FormModel;
use quickai_core::plan::{HostFacts, Selection};
use quickai_core::registry::{KnownPrimitives, Registry};

const BASE: &str = r#"schema = 1
kind = "engine"
id = "acme"
requires = { primitives = ["spawn@1", "probe.http@1", "locate.setting@1"] }

[engine]
name = "Acme"

[provides]
apis = ["acme.talk"]
default_port = 9000

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["serve", "--port", "${listen_port}"]

[launch.ready]
probe = { use = "http", path = "/health", expect = 200 }

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"
ui = { section = "advanced", widget = "runtime_manager" }

FIELD
"#;

const WIDGETS: [&str; 2] = ["runtime_manager", "button"];

fn document(field: &str) -> String {
    BASE.replace("FIELD", field)
}

fn registry(text: &str, widgets: Option<&[&str]>) -> Registry {
    let source = MemorySource::new(&[("embedded:engines/acme.toml", text)]);
    let mut known = KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1"]);
    if let Some(widgets) = widgets {
        known = known.with_widgets(widgets);
    }
    Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &known,
    )
}

fn errors(registry: &Registry) -> Vec<(Option<u32>, String, String)> {
    registry
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.line, d.to_string(), d.hint.clone().unwrap_or_default()))
        .collect()
}

/// The 1-based line of the first line of `text` that contains `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    text.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no `{needle}`")) as u32
        + 1
}

/// The same for the last such line.
fn last_line_of(text: &str, needle: &str) -> u32 {
    text.lines()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .map(|(i, _)| i as u32 + 1)
        .last()
        .unwrap_or_else(|| panic!("no `{needle}`"))
}

#[test]
fn p3_w05_a_field_naming_a_widget_the_registry_lacks_is_a_load_diagnostic_with_file_and_line() {
    let text = document(
        "[[field]]\nid = \"models\"\ntype = \"text\"\nui = { section = \"advanced\", widget = \"runtime_managr\" }\n",
    );
    let registry = registry(&text, Some(&WIDGETS));
    assert!(registry.engine("acme").is_none(), "the engine loaded");
    let found = errors(&registry);
    assert_eq!(found.len(), 1, "{found:#?}");
    let (line, shown, hint) = &found[0];
    assert_eq!(*line, Some(line_of(&text, "runtime_managr")), "{shown}");
    assert!(
        shown.contains("embedded:engines/acme.toml")
            && shown.contains("field[models].ui.widget")
            && shown.contains("`runtime_managr` is not a widget this version of QuickAI has"),
        "{shown}"
    );
    assert_eq!(hint, "did you mean `runtime_manager`?");

    // a name nothing is near lists the widgets there are
    let text = text.replace("runtime_managr", "slider3000");
    let found = errors(&self::registry(&text, Some(&WIDGETS)));
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].2, "the widgets are button, runtime_manager");

    // a widget there is loads, and a registry built with no list of widgets holds nobody to one
    let ok = text.replace("slider3000", "runtime_manager");
    assert!(self::registry(&ok, Some(&WIDGETS)).engine("acme").is_some());
    assert!(self::registry(&text, None).engine("acme").is_some());
}

#[test]
fn p3_w05b_a_button_says_what_a_click_does_and_nothing_else_has_an_action() {
    let button = |ui: &str| {
        document(&format!(
            "[[field]]\nid = \"stop_other\"\nlabel = \"Open Settings\"\ntype = \"hidden\"\nui = {{ {ui} }}\n"
        ))
    };
    let text = button("section = \"advanced\", widget = \"button\", action = \"open:settings\"");
    let ok = registry(&text, Some(&WIDGETS));
    assert!(ok.diagnostics.is_empty(), "{:#?}", ok.diagnostics);
    assert!(ok.engine("acme").is_some());

    let cases = [
        (
            "section = \"advanced\", widget = \"button\"",
            "the button `stop_other` does not say what a click does",
            "field[stop_other].ui.widget",
        ),
        (
            "section = \"advanced\", widget = \"button\", action = \"Open Settings\"",
            "`Open Settings` is not an action id",
            "field[stop_other].ui.action",
        ),
        (
            "section = \"advanced\", widget = \"button\", action = \"open:\"",
            "`open:` is not an action id",
            "field[stop_other].ui.action",
        ),
        (
            "section = \"advanced\", action = \"open:settings\"",
            "`action` belongs to a button",
            "field[stop_other].ui.action",
        ),
    ];
    for (ui, wanted, at) in cases {
        let text = button(ui);
        let registry = registry(&text, Some(&WIDGETS));
        assert!(registry.engine("acme").is_none(), "{ui}");
        let found = errors(&registry);
        assert_eq!(found.len(), 1, "{ui}: {found:#?}");
        assert!(
            found[0].1.contains(wanted) && found[0].1.contains(at),
            "{ui}: {}",
            found[0].1
        );
        assert_eq!(found[0].0, Some(last_line_of(&text, "ui = {")), "{ui}");
    }
}

#[test]
fn p3_w03b_a_hidden_field_with_a_widget_is_shown_and_the_button_carries_its_action() {
    let text = document(
        "[[field]]\nid = \"open\"\nlabel = \"Open Settings\"\nhelp = \"Where the engine is set up\"\ntype = \"hidden\"\nenabled_when = \"not session.running\"\nui = { section = \"advanced\", widget = \"button\", action = \"open:settings\" }\n\n[[field]]\nid = \"secret\"\ntype = \"hidden\"\n",
    );
    let registry = registry(&text, Some(&WIDGETS));
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let facts = HostFacts::default();
    let form = FormModel::build(&registry, &Selection::new("acme"), &facts)
        .unwrap_or_else(|e| panic!("{e:#?}"));
    let field = |id: &str| {
        form.sections
            .iter()
            .flat_map(|s| &s.fields)
            .find(|f| f.id == id)
            .unwrap_or_else(|| panic!("no field `{id}`"))
    };
    let open = field("open");
    assert!(open.visible && open.enabled);
    assert_eq!(open.widget.as_deref(), Some("button"));
    assert_eq!(open.action.as_deref(), Some("open:settings"));
    assert_eq!(open.label, "Open Settings");
    assert!(
        !field("secret").visible,
        "a hidden field that names no widget is still not shown"
    );

    // while the session runs the button is there and cannot be pressed
    let running = HostFacts {
        session_running: true,
        ..HostFacts::default()
    };
    let form = FormModel::build(&registry, &Selection::new("acme"), &running)
        .unwrap_or_else(|e| panic!("{e:#?}"));
    let open = form
        .sections
        .iter()
        .flat_map(|s| &s.fields)
        .find(|f| f.id == "open")
        .expect("the button");
    assert!(open.visible && !open.enabled);
}
