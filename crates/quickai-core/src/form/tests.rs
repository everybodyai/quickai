//! The form model, attribute by attribute (docs/implementation-plan.md, Phase 2 test plan F1-F9).

use super::*;
use crate::heuristics::recommend_ctx_size;
use crate::plan::{plan, ModelInfo, Start};
use crate::registry::fixtures::registry_from;

const GB: u64 = 1024 * 1024 * 1024;

/// An engine with one field for every attribute the form understands.
const DOC: &str = r#"schema = 1
kind = "engine"
id = "llamacpp"
requires = { primitives = ["spawn@1", "locate.setting@1"] }

[engine]
name = "llama.cpp"

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["--model", "${model.main.path}", "--port", "${listen_port}"]

[[field]]
id = "port"
label = "Port"
help = "The TCP port."
type = "port"
default = 8080
legacy_key = "port"
bind.port = "public"
enabled_when = "not session.running"
ui = { section = "primary", order = 10 }

[[field]]
id = "ctx_size"
label = "Context size"
type = "int"
range = [256, 131072]
step = 64
default = { fn = "recommend_ctx_size", args = ["model.size_bytes", "sys.ram_bytes"] }
recompute_on = ["model"]
legacy_key = "ctx_size"
bind.arg = ["--ctx-size", "${value}"]
enabled_when = "not session.running"
validate = [ { check = "field.n_gpu_layers == 0 or value <= 32768", message = "With the GPU, keep the context at or below 32768." } ]
ui = { section = "primary", order = 20 }

[[field]]
id = "n_gpu_layers"
label = "GPU layers"
type = "int"
range = [0, 999]
default = 999
legacy_key = "n_gpu_layers"
bind.arg = ["--n-gpu-layers", "${value}"]
derive = { gpu_offload = "value > 0" }
enabled_when = "not session.running"
ui = { section = "primary", order = 30 }

[[field]]
id = "bin_override"
label = "Binary"
type = "runtime_path"
legacy_key = "llama_bin_override"
ui = { section = "advanced", widget = "runtime_manager", order = 1 }

[[field]]
id = "mode"
type = "choice"
options = ["auto", "attach", "spawn"]
default = "auto"
visible_when = "os == 'linux'"
applies = "spawn_only"
ui = { section = "advanced", order = 5 }

[[field]]
id = "big_model_note"
type = "text"
default = ""
visible_when = "model.size_bytes > 4294967296"
ui = { section = "advanced", order = 20 }

[[field]]
id = "name"
type = "text"
pattern = "[a-z]+"
required_when = "field.mode == 'attach'"
persist = "profile"
ui = { section = "advanced", order = 30, hint = "lower case letters" }

[[field]]
id = "temp"
type = "float"
range = [0.0, 2.0]
step = 0.1
default = 0.7
ui = { section = "advanced", order = 40 }

[[field]]
id = "version"
type = "text"
default = "1.2.3"
ui = { section = "advanced", order = 50 }

[[field]]
id = "gated"
type = "text"
visible_when = "semver_ge(field.version, '1.0.0')"
ui = { section = "advanced", order = 60 }

[[field]]
id = "verbose"
type = "bool"
default = false
ui = { section = "other", order = 1 }

[[field]]
id = "last_model"
type = "hidden"
legacy_key = "last_model_filename"
"#;

fn registry() -> Registry {
    let r = registry_from(DOC);
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
    r
}

fn facts() -> HostFacts {
    HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        ram_bytes: 16 * GB,
        cpus: 8,
        binary: Some("/opt/llama/llama-server".into()),
        ..Default::default()
    }
}

fn model(size_bytes: u64) -> ModelInfo {
    ModelInfo {
        id: "m.gguf".into(),
        display: "M".into(),
        path: "/models/m.gguf".into(),
        size_bytes,
    }
}

fn selection() -> Selection {
    Selection::new("llamacpp").with_model(model(GB))
}

fn build(selection: &Selection, facts: &HostFacts) -> FormModel {
    FormModel::build(&registry(), selection, facts).expect("the engine exists")
}

fn field<'a>(form: &'a FormModel, id: &str) -> &'a FormField {
    form.field(id)
        .unwrap_or_else(|| panic!("no field `{id}` in {:?}", ids(form)))
}

fn ids(form: &FormModel) -> Vec<(String, Vec<String>)> {
    form.sections
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                s.fields.iter().map(|f| f.id.clone()).collect(),
            )
        })
        .collect()
}

#[test]
fn a_selection_value_wins_over_the_default_and_the_origin_says_which() {
    let form = build(&selection().set("llamacpp.port", 9000u16), &facts());
    let port = field(&form, "port");
    assert_eq!(port.value, Value::Int(9000));
    assert_eq!(port.origin, Origin::Selection);
    assert_eq!(port.key, "llamacpp.port");

    let ctx = field(&form, "ctx_size");
    assert_eq!(ctx.value, Value::from(recommend_ctx_size(GB, 16 * GB)));
    assert_eq!(ctx.origin, Origin::Computed("recommend_ctx_size".into()));

    assert_eq!(field(&form, "n_gpu_layers").origin, Origin::Default);
    assert_eq!(field(&form, "temp").value, Value::Float(0.7));
    let bin = field(&form, "bin_override");
    assert!(bin.value.is_null());
    assert_eq!(bin.origin, Origin::Unset);

    // a null in the selection means "not set": the default applies
    let form = build(&selection().set("llamacpp.port", Value::Null), &facts());
    assert_eq!(field(&form, "port").value, Value::Int(8080));
    assert_eq!(field(&form, "port").origin, Origin::Default);
}

#[test]
fn the_form_and_the_planner_share_one_resolver() {
    // Every default the form shows must be the number the command line then gets.
    let registry = registry();
    for size in [0, GB, 4 * GB, 8 * GB, 20 * GB] {
        for ram in [0, 4 * GB, 16 * GB, 160 * GB] {
            let mut f = facts();
            f.ram_bytes = ram;
            let sel = Selection::new("llamacpp").with_model(model(size));
            let form = FormModel::build(&registry, &sel, &f).unwrap();
            let shown = field(&form, "ctx_size").value.as_i64().unwrap();
            assert_eq!(shown, i64::from(recommend_ctx_size(size, ram)));

            let run = plan(&registry, &sel, &f).unwrap();
            let Start::Spawn(spawn) = &run.units[0].start[0];
            let args = spawn.arg_values();
            let at = args.iter().position(|a| a == "--ctx-size").unwrap();
            assert_eq!(args[at + 1], shown.to_string(), "size {size}, ram {ram}");
        }
    }
}

#[test]
fn text_help_and_widget_hints_come_from_the_document() {
    let form = build(&selection(), &facts());
    let port = field(&form, "port");
    assert_eq!(port.label, "Port");
    assert_eq!(port.help.as_deref(), Some("The TCP port."));
    assert_eq!(port.legacy_key.as_deref(), Some("port"));
    assert_eq!(port.ty, FieldType::Port);
    assert_eq!(port.applies, Applies::OnStart);
    assert_eq!(port.persist, Persist::Global);

    let ctx = field(&form, "ctx_size");
    assert_eq!(ctx.range, Some((256.0, 131072.0)));
    assert_eq!(ctx.step, Some(64.0));

    let mode = field(&form, "mode");
    assert_eq!(mode.label, "mode", "the id stands in for a missing label");
    assert_eq!(mode.options, ["auto", "attach", "spawn"]);
    assert_eq!(mode.applies, Applies::SpawnOnly);

    let name = field(&form, "name");
    assert_eq!(name.hint.as_deref(), Some("lower case letters"));
    assert_eq!(name.persist, Persist::Profile);
    assert_eq!(
        field(&form, "bin_override").widget.as_deref(),
        Some("runtime_manager")
    );
}

#[test]
fn sections_come_in_display_order_and_fields_by_their_order() {
    let form = build(&selection(), &facts());
    let sections: Vec<&str> = form.sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(sections, ["primary", "advanced", "other"]);
    let primary: Vec<&str> = form
        .section("primary")
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id.as_str())
        .collect();
    assert_eq!(primary, ["port", "ctx_size", "n_gpu_layers"]);
    let advanced: Vec<&str> = form
        .section("advanced")
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id.as_str())
        .collect();
    assert_eq!(
        advanced,
        [
            "bin_override",
            "mode",
            "big_model_note",
            "name",
            "temp",
            "version",
            "gated",
            "last_model"
        ],
        "by ui.order; a field with no order (and no section) comes last, in the advanced section"
    );
}

#[test]
fn sections_other_than_the_two_known_ones_are_sorted_by_name() {
    let doc = DOC.replace(
        "[[field]]\nid = \"verbose\"",
        "[[field]]\nid = \"zeta\"\ntype = \"bool\"\nui = { section = \"zebra\" }\n\n[[field]]\nid = \"alpha\"\ntype = \"bool\"\nui = { section = \"apple\" }\n\n[[field]]\nid = \"verbose\"",
    );
    let r = registry_from(&doc);
    let form = FormModel::build(&r, &selection(), &facts()).unwrap();
    let sections: Vec<&str> = form.sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(sections, ["primary", "advanced", "apple", "other", "zebra"]);
}

#[test]
fn visibility_follows_the_conditions() {
    // `os`
    let form = build(&selection(), &facts());
    assert!(field(&form, "mode").visible);
    let mut windows = facts();
    windows.os = "windows".into();
    assert!(!field(&build(&selection(), &windows), "mode").visible);

    // the model: shown only for a model over 4 GiB, and not at all with no model
    assert!(!field(&form, "big_model_note").visible);
    let big = Selection::new("llamacpp").with_model(model(8 * GB));
    assert!(field(&build(&big, &facts()), "big_model_note").visible);
    assert!(
        !field(
            &build(&Selection::new("llamacpp"), &facts()),
            "big_model_note"
        )
        .visible
    );

    // another field's value
    assert!(field(&form, "gated").visible, "1.2.3 is at least 1.0.0");
    let old = selection().set("llamacpp.version", "0.9.0");
    assert!(!field(&build(&old, &facts()), "gated").visible);

    // fields with no condition are visible, a hidden one never is
    assert!(field(&form, "port").visible && field(&form, "temp").visible);
    assert!(!field(&form, "last_model").visible);
}

#[test]
fn a_condition_that_cannot_be_evaluated_does_not_hold_and_is_reported() {
    // semver_ge refuses text that is not a version
    let sel = selection().set("llamacpp.version", "banana");
    let form = build(&sel, &facts());
    assert!(!field(&form, "gated").visible);
    assert_eq!(form.diagnostics.len(), 1, "{:#?}", form.diagnostics);
    let message = form.diagnostics[0].to_string();
    assert!(
        message.contains("field[gated].visible_when")
            && message.contains("`banana` is not a version"),
        "{message}"
    );
}

#[test]
fn enabled_state_follows_the_session() {
    let idle = build(&selection(), &facts());
    for id in ["port", "ctx_size", "n_gpu_layers"] {
        assert!(field(&idle, id).enabled, "{id} is editable while stopped");
    }
    let mut running = facts();
    running.session_running = true;
    let form = build(&selection(), &running);
    for id in ["port", "ctx_size", "n_gpu_layers"] {
        assert!(!field(&form, id).enabled, "{id} is locked while running");
    }
    for id in ["bin_override", "mode", "temp"] {
        assert!(
            field(&form, id).enabled,
            "{id} has no condition: always editable"
        );
    }
}

#[test]
fn derived_facts_are_evaluated_from_the_fields_value() {
    let on = build(&selection(), &facts());
    assert_eq!(on.derived("gpu_offload"), Some(&Value::Bool(true)));
    let off = build(&selection().set("llamacpp.n_gpu_layers", 0u32), &facts());
    assert_eq!(off.derived("gpu_offload"), Some(&Value::Bool(false)));
    assert_eq!(off.derived("nothing"), None);
}

fn error_of(selection: &Selection, id: &str) -> Option<String> {
    field(&build(selection, &facts()), id).error.clone()
}

#[test]
fn a_value_in_range_has_no_error_and_one_outside_keeps_its_value() {
    assert_eq!(error_of(&selection(), "ctx_size"), None);
    let low = selection().set("llamacpp.ctx_size", 100u32);
    let form = build(&low, &facts());
    assert_eq!(
        field(&form, "ctx_size").error.as_deref(),
        Some("Must be between 256 and 131072.")
    );
    assert_eq!(
        field(&form, "ctx_size").value,
        Value::Int(100),
        "the value is kept and shown"
    );
    let hot = selection().set("llamacpp.temp", 2.5);
    assert_eq!(
        error_of(&hot, "temp").as_deref(),
        Some("Must be between 0 and 2.")
    );
    assert_eq!(
        error_of(&selection().set("llamacpp.temp", 1.5), "temp"),
        None
    );
}

#[test]
fn a_value_of_the_wrong_type_is_an_error_not_a_crash() {
    let wrong = selection().set("llamacpp.ctx_size", "big");
    assert_eq!(
        error_of(&wrong, "ctx_size").as_deref(),
        Some("Expected a whole number, found string.")
    );
    let port = selection().set("llamacpp.port", 70000u32);
    assert_eq!(
        error_of(&port, "port").as_deref(),
        Some("70000 is not a port (1 to 65535).")
    );
}

#[test]
fn a_pattern_must_match_the_whole_value() {
    assert_eq!(
        error_of(&selection().set("llamacpp.name", "abc"), "name"),
        None
    );
    for bad in ["ABC", "abc1", "ab c"] {
        assert_eq!(
            error_of(&selection().set("llamacpp.name", bad), "name").as_deref(),
            Some("Must match the pattern `[a-z]+`."),
            "{bad}"
        );
    }
    // an empty optional text is not checked against a pattern
    assert_eq!(
        error_of(&selection().set("llamacpp.name", ""), "name"),
        None
    );
}

#[test]
fn a_choice_must_be_one_of_its_options() {
    assert_eq!(
        error_of(&selection().set("llamacpp.mode", "attach"), "mode"),
        None
    );
    assert_eq!(
        error_of(&selection().set("llamacpp.mode", "bogus"), "mode").as_deref(),
        Some("Must be one of: auto, attach, spawn.")
    );
}

#[test]
fn a_required_field_needs_a_value_while_its_condition_holds() {
    // required_when = "field.mode == 'attach'"
    assert_eq!(
        error_of(&selection(), "name"),
        None,
        "not required in auto mode"
    );
    let attach = selection().set("llamacpp.mode", "attach");
    assert_eq!(
        error_of(&attach, "name").as_deref(),
        Some("A value is required.")
    );
    assert!(field(&build(&attach, &facts()), "name").required);
    assert!(!field(&build(&selection(), &facts()), "name").required);
    let blank = attach.clone().set("llamacpp.name", "  ");
    assert_eq!(
        error_of(&blank, "name").as_deref(),
        Some("A value is required.")
    );
    let named = attach.set("llamacpp.name", "abc");
    assert_eq!(error_of(&named, "name"), None);
}

#[test]
fn a_validation_rule_can_look_at_other_fields() {
    // 40000 is fine on the CPU and too much with the GPU on
    let gpu = selection().set("llamacpp.ctx_size", 40000u32);
    assert_eq!(
        error_of(&gpu, "ctx_size").as_deref(),
        Some("With the GPU, keep the context at or below 32768.")
    );
    let cpu = gpu.set("llamacpp.n_gpu_layers", 0u32);
    assert_eq!(error_of(&cpu, "ctx_size"), None);
}

#[test]
fn the_first_problem_is_the_one_reported() {
    // a wrong type beats a range, a range beats a rule
    let both = selection()
        .set("llamacpp.ctx_size", 200000u32)
        .set("llamacpp.n_gpu_layers", 999u32);
    assert_eq!(
        error_of(&both, "ctx_size").as_deref(),
        Some("Must be between 256 and 131072."),
        "the range comes before the cross-field rule"
    );
}

#[test]
fn recompute_replaces_what_was_typed_for_the_fields_that_name_the_trigger() {
    let sel = Selection::new("llamacpp")
        .with_model(model(8 * GB))
        .set("llamacpp.ctx_size", 123u32)
        .set("llamacpp.port", 9000u16);
    let edits = recompute(&registry(), &sel, &facts(), "model").unwrap();
    assert_eq!(
        edits,
        vec![Edit {
            key: "llamacpp.ctx_size".into(),
            value: Value::from(recommend_ctx_size(8 * GB, 16 * GB)),
        }],
        "only ctx_size names the trigger; the port the user chose is left alone"
    );
    assert!(recompute(&registry(), &sel, &facts(), "elsewhere")
        .unwrap()
        .is_empty());
}

#[test]
fn recompute_follows_the_model_it_is_given() {
    let small = Selection::new("llamacpp").with_model(model(GB));
    let large = Selection::new("llamacpp").with_model(model(20 * GB));
    let a = recompute(&registry(), &small, &facts(), "model").unwrap();
    let b = recompute(&registry(), &large, &facts(), "model").unwrap();
    assert_ne!(
        a[0].value, b[0].value,
        "a bigger model gets a smaller suggestion"
    );
}

#[test]
fn set_accepts_a_good_value_and_clears_with_null() {
    let registry = registry();
    let mut sel = selection();
    set(&registry, &mut sel, &facts(), "port", Value::from(8081u16)).unwrap();
    assert_eq!(sel.values["llamacpp.port"], Value::Int(8081));
    set(&registry, &mut sel, &facts(), "port", Value::Null).unwrap();
    assert!(
        !sel.values.contains_key("llamacpp.port"),
        "null clears the value"
    );
}

#[test]
fn set_refuses_a_bad_value_and_leaves_the_selection_as_it_was() {
    let registry = registry();
    let mut sel = selection().set("llamacpp.port", 8080u16);
    for (id, value, needle) in [
        ("port", Value::from(70000u32), "70000 is not a port"),
        ("port", Value::from("abc"), "whole number"),
        (
            "ctx_size",
            Value::from(100u32),
            "Must be between 256 and 131072.",
        ),
        (
            "ctx_size",
            Value::from(40000u32),
            "keep the context at or below 32768",
        ),
        ("mode", Value::from("bogus"), "Must be one of"),
        ("name", Value::from("ABC"), "Must match the pattern"),
        ("nope", Value::from(1u32), "no field `nope`"),
    ] {
        let before = sel.values.clone();
        let error = set(&registry, &mut sel, &facts(), id, value).unwrap_err();
        assert!(error.contains(needle), "{id}: {error}");
        assert_eq!(sel.values, before, "{id}: nothing changed");
    }
}

#[test]
fn an_unknown_engine_is_an_error() {
    let errors = FormModel::build(&registry(), &Selection::new("nope"), &facts()).unwrap_err();
    assert!(errors[0].message.contains("no engine called `nope`"));
    assert!(recompute(&registry(), &Selection::new("nope"), &facts(), "model").is_err());
}

#[test]
fn the_form_serializes_for_golden_files() {
    let form = build(&selection(), &facts());
    let json = serde_json::to_value(&form).unwrap();
    let port = &json["sections"][0]["fields"][0];
    assert_eq!(port["id"], "port");
    assert_eq!(port["value"], 8080);
    assert_eq!(port["origin"], "default");
    assert_eq!(port["type"], "port");
    assert_eq!(json["derived"]["gpu_offload"], true);
}
