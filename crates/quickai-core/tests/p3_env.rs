//! The environment of a spawned engine beyond its own entries (work package 3.12, plan decision
//! C31): the variables a field decides (`bind.env`) and the person's own lines of environment
//! (`bind.env_lines`), through the registry, the planner and the form, on a small engine document
//! of this file's own. The same behaviour through the real Ollama document is E03i and E09, which
//! wait for that document to be embedded (work package 3.7).

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::form::FormModel;
use quickai_core::plan::explain::render_explain;
use quickai_core::plan::{plan, HostFacts, Selection};
use quickai_core::registry::{KnownPrimitives, Registry};
use serde_json::Value;

const DOC: &str = r#"schema = 1
kind = "engine"
id = "envtest"
requires = { primitives = ["spawn@1"] }

[engine]
name = "Env test"

[provides]
apis = ["openai.chat"]
default_port = 11434

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["serve"]
env = [
  { name = "OLLAMA_HOST", value = "127.0.0.1:${listen_port}", locked = true },
  { name = "OLLAMA_NO_CLOUD", value = "1", locked = true },
  { name = "OLLAMA_KEEP_ALIVE", value = "-1" },
  { name = "LD_LIBRARY_PATH", prepend = "/opt/envtest/lib", sep = ":" },
]

[launch.ready]
probe = { use = "http", path = "/api/version", expect = 200 }

[[field]]
id = "port"
type = "port"
default = 11434
bind.port = "public"

[[field]]
id = "gpu"
type = "choice"
options = ["auto", "off"]
default = "auto"
bind.env = [
  { name = "OLLAMA_VULKAN", value = "0", when = "value == 'off'" },
  { name = "CUDA_VISIBLE_DEVICES", value = "-1", when = "value == 'off'" },
]
ui = { order = 30 }

[[field]]
id = "ctx_size"
type = "int"
range = [256, 131072]
optional = true
bind.env = [ { name = "OLLAMA_CONTEXT_LENGTH", value = "${value}" } ]
ui = { order = 20 }

[[field]]
id = "extra_env"
type = "kv_lines"
default = ""
bind.env_lines = { protect = ["OLLAMA_MODELS"] }
ui = { order = 90 }

[[field]]
id = "bin_override"
type = "runtime_path"
"#;

fn registry_of(doc: &str) -> Registry {
    let source = MemorySource::new(&[("embedded:engines/envtest.toml", doc)]);
    Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1"]),
    )
}

fn registry() -> Registry {
    let registry = registry_of(DOC);
    assert!(
        registry.diagnostics.is_empty() && registry.engine("envtest").is_some(),
        "the test document was not accepted by this build:\n{:#?}",
        registry.diagnostics
    );
    registry
}

fn facts(os: &str) -> HostFacts {
    HostFacts {
        os: os.into(),
        arch: "x86_64".into(),
        ram_bytes: 16 << 30,
        cpus: 8,
        binary: Some("/opt/envtest/bin/envtest".into()),
        data_dir: "/data".into(),
        config_dir: "/config".into(),
        home: "/home/u".into(),
        ..Default::default()
    }
}

fn selection() -> Selection {
    Selection::new("envtest")
}

/// What the spawn sets, in order: `(name, op, value, origin)`.
fn env_of(selection: &Selection, os: &str) -> Vec<(String, String, String, String)> {
    let plan = plan(&registry(), selection, &facts(os)).unwrap_or_else(|e| panic!("{e:#?}"));
    let plan = serde_json::to_value(&plan).unwrap();
    let spawn = plan["units"][0]["start"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["how"] == "spawn")
        .expect("a spawn");
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    spawn["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                text(&e["name"]),
                text(&e["mode"]["op"]),
                text(&e["mode"]["value"]),
                text(&e["origin"]),
            )
        })
        .collect()
}

fn names(env: &[(String, String, String, String)]) -> Vec<&str> {
    env.iter().map(|e| e.0.as_str()).collect()
}

fn value<'a>(env: &'a [(String, String, String, String)], name: &str) -> Option<&'a str> {
    env.iter()
        .rev()
        .find(|e| e.0 == name && e.1 == "set")
        .map(|e| e.2.as_str())
}

#[test]
fn p3_e03j_a_fields_variables_are_set_when_its_condition_holds_and_never_while_it_has_no_value() {
    // nothing decided: the document's own variables and nothing else
    let env = env_of(&selection(), "linux");
    assert_eq!(
        names(&env),
        [
            "OLLAMA_HOST",
            "OLLAMA_NO_CLOUD",
            "OLLAMA_KEEP_ALIVE",
            "LD_LIBRARY_PATH"
        ]
    );

    // a number the person gave is a variable, after the document's own, with its field as origin
    let env = env_of(&selection().set("envtest.ctx_size", 4096u32), "linux");
    assert_eq!(value(&env, "OLLAMA_CONTEXT_LENGTH"), Some("4096"));
    let at = |name: &str| env.iter().position(|e| e.0 == name).unwrap();
    assert!(at("OLLAMA_CONTEXT_LENGTH") > at("LD_LIBRARY_PATH"));
    assert_eq!(env[at("OLLAMA_CONTEXT_LENGTH")].3, "field envtest.ctx_size");

    // a choice sets what its condition says, and only then
    let env = env_of(&selection().set("envtest.gpu", "off"), "linux");
    assert_eq!(value(&env, "OLLAMA_VULKAN"), Some("0"));
    assert_eq!(value(&env, "CUDA_VISIBLE_DEVICES"), Some("-1"));
    let env = env_of(&selection().set("envtest.gpu", "auto"), "linux");
    assert!(value(&env, "OLLAMA_VULKAN").is_none(), "{env:?}");

    // bindings go in the order of their fields' `ui.order` (20, then 30), whatever the document's
    let env = env_of(
        &selection()
            .set("envtest.gpu", "off")
            .set("envtest.ctx_size", 2048u32),
        "linux",
    );
    let at = |name: &str| env.iter().position(|e| e.0 == name).unwrap();
    assert!(at("OLLAMA_CONTEXT_LENGTH") < at("OLLAMA_VULKAN"));
}

#[test]
fn p3_e03k_the_persons_lines_come_last_replace_in_place_and_cannot_touch_what_the_document_locks() {
    let lines = "# comment\nOLLAMA_DEBUG=1\n\nOLLAMA_HOST=0.0.0.0:80\nollama_no_cloud=0\n\
                 OLLAMA_KEEP_ALIVE=5m\nOLLAMA_MODELS=/x\nLD_LIBRARY_PATH=/evil\nOLLAMA_NUM_PARALLEL = 2\nOLLAMA_DEBUG=2";
    let env = env_of(&selection().set("envtest.extra_env", lines), "linux");

    // locked by the document, protected by the field, a prepend of the engine's: none can be set
    assert_eq!(value(&env, "OLLAMA_HOST"), Some("127.0.0.1:11434"));
    assert_eq!(
        value(&env, "OLLAMA_NO_CLOUD"),
        Some("1"),
        "not to be got round with another spelling"
    );
    assert!(
        names(&env)
            .iter()
            .all(|n| !["ollama_no_cloud", "OLLAMA_MODELS"].contains(n)),
        "{env:?}"
    );
    let library: Vec<_> = env.iter().filter(|e| e.0 == "LD_LIBRARY_PATH").collect();
    assert_eq!(library.len(), 1, "the engine's own prepend, alone: {env:?}");
    assert_eq!(library[0].1, "prepend");

    // a variable of the engine's that is not locked is the person's to replace, where it stood
    let at = |name: &str| env.iter().position(|e| e.0 == name).unwrap();
    assert_eq!(value(&env, "OLLAMA_KEEP_ALIVE"), Some("5m"));
    assert_eq!(at("OLLAMA_KEEP_ALIVE"), 2, "in place, not at the end");
    assert_eq!(env[2].3, "field envtest.extra_env");

    // the rest follow, once each, after everything of the engine's; a later line wins
    assert_eq!(value(&env, "OLLAMA_DEBUG"), Some("2"));
    assert_eq!(value(&env, "OLLAMA_NUM_PARALLEL"), Some("2"));
    assert_eq!(
        names(&env).iter().filter(|n| **n == "OLLAMA_DEBUG").count(),
        1
    );
    assert!(at("OLLAMA_DEBUG") > at("LD_LIBRARY_PATH"));
    assert!(at("OLLAMA_NUM_PARALLEL") > at("OLLAMA_DEBUG"));
}

#[test]
fn p3_e03l_on_windows_two_spellings_of_a_name_are_one_variable_and_the_protected_ones_stay_protected(
) {
    let lines = "Ollama_Debug=1\nOLLAMA_DEBUG=2\nollama_host=evil\nollama_keep_alive=9m";
    let env = env_of(&selection().set("envtest.extra_env", lines), "windows");
    assert_eq!(value(&env, "OLLAMA_HOST"), Some("127.0.0.1:11434"));
    assert!(
        env.iter().all(|e| e.0 != "ollama_host"),
        "a spelling of a locked name: {env:?}"
    );
    // one variable, the later line's value, under the name of the first
    let debug: Vec<_> = env
        .iter()
        .filter(|e| e.0.eq_ignore_ascii_case("OLLAMA_DEBUG"))
        .collect();
    assert_eq!(debug.len(), 1, "{env:?}");
    assert_eq!(debug[0].2, "2");
    // the engine's own variable is replaced by the same name in another case, in place
    assert_eq!(env[2].2, "9m");
    assert_eq!(env.iter().filter(|e| e.2 == "-1").count(), 0);

    // on Linux the same two spellings are two variables
    let env = env_of(
        &selection().set("envtest.extra_env", "Ollama_Debug=1\nOLLAMA_DEBUG=2"),
        "linux",
    );
    assert_eq!(value(&env, "Ollama_Debug"), Some("1"));
    assert_eq!(value(&env, "OLLAMA_DEBUG"), Some("2"));
}

fn extra_env_error(text: &str, os: &str) -> (Option<String>, String) {
    let registry = registry();
    let form = FormModel::build(
        &registry,
        &selection().set("envtest.extra_env", text),
        &facts(os),
    )
    .unwrap_or_else(|e| panic!("{e:#?}"));
    let field = form
        .sections
        .iter()
        .flat_map(|s| &s.fields)
        .find(|f| f.id == "extra_env")
        .expect("the field");
    (field.error.clone(), field.value.to_string())
}

#[test]
fn p3_e09b_a_bad_line_is_the_fields_error_with_its_number_and_the_text_stays() {
    let typed = "OLLAMA_DEBUG=1\nnot a line\nOLLAMA_HOST=evil\n\n# fine\n=novalue";
    let (error, kept) = extra_env_error(typed, "linux");
    let error = error.expect("an error under the field");
    assert!(
        error.contains("Line 2: `not a line` is not NAME=VALUE."),
        "{error}"
    );
    assert!(
        error.contains("Line 3: OLLAMA_HOST is set by QuickAI and cannot be changed here."),
        "{error}"
    );
    assert!(
        error.contains("Line 6: there is no name before the ="),
        "{error}"
    );
    assert!(
        !error.contains("Line 1") && !error.contains("Line 5"),
        "{error}"
    );
    assert_eq!(kept, typed, "what was typed stays");
    // a problem to a line of the message, so a widget can put each under the text box (W01)
    assert_eq!(error.lines().count(), 3, "{error}");
    assert!(error.lines().all(|l| l.starts_with("Line ")), "{error}");

    // good lines, comments and nothing at all are no problem
    for fine in [
        "",
        "OLLAMA_DEBUG=1\n# a comment\n\nOLLAMA_NUM_PARALLEL=2",
        "  \n",
    ] {
        assert_eq!(extra_env_error(fine, "linux").0, None, "`{fine}`");
    }
    // the form says what the plan leaves out, in any spelling, as the planner does
    let (error, _) = extra_env_error("ollama_no_cloud=0", "linux");
    assert!(error
        .unwrap()
        .contains("ollama_no_cloud is OLLAMA_NO_CLOUD, which is set by QuickAI"),);
}

#[test]
fn p3_e25_the_bindings_of_environment_are_checked_where_they_are_written() {
    let errors_of = |doc: String| -> Vec<String> {
        registry_of(&doc)
            .diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect()
    };
    let cases: [(&str, &str, &str); 7] = [
        (
            "bind.env_lines = { protect = [\"OLLAMA_MODELS\"] }",
            "bind.env_lines = { protect = [\"OLLAMA MODELS\"] }",
            "is not an environment variable name",
        ),
        (
            "id = \"extra_env\"\ntype = \"kv_lines\"",
            "id = \"extra_env\"\ntype = \"text\"",
            "needs a field of type `kv_lines`",
        ),
        (
            "bind.env = [ { name = \"OLLAMA_CONTEXT_LENGTH\", value = \"${value}\" } ]",
            "bind.env = [ { name = \"OLLAMA_CONTEXT_LENGTH\", value = \"${valu}\" } ]",
            "field[ctx_size].bind.env[0]",
        ),
        (
            "bind.env = [ { name = \"OLLAMA_CONTEXT_LENGTH\", value = \"${value}\" } ]",
            "bind.env = [ { name = \"OLLAMA_CONTEXT_LENGTH\", value = \"${value}\", prepend = \"x\" } ]",
            "`value` or `prepend`, not both",
        ),
        (
            "bind.env = [ { name = \"OLLAMA_CONTEXT_LENGTH\", value = \"${value}\" } ]",
            "bind.env = [ { name = \"OLLAMA_CONTEXT_LENGTH\", value = \"${value}\", locked = true } ]",
            "`locked` is for the document's own variables",
        ),
        (
            "{ name = \"OLLAMA_VULKAN\", value = \"0\", when = \"value == 'off'\" }",
            "{ name = \"OLLAMA_VULKAN\", value = \"0\", when = \"valu == 'off'\" }",
            "unknown name `valu`",
        ),
        (
            "{ name = \"OLLAMA_VULKAN\", value = \"0\", when = \"value == 'off'\" }",
            "{ name = \"A=B\", value = \"0\", when = \"value == 'off'\" }",
            "is not a valid environment variable name",
        ),
    ];
    for (old, new, wanted) in cases {
        assert!(DOC.contains(old), "the document has no `{old}`");
        let errors = errors_of(DOC.replace(old, new));
        assert!(
            errors.iter().any(|e| e.contains(wanted)),
            "`{new}` should be reported with `{wanted}`: {errors:#?}"
        );
    }
}

#[test]
fn p3_e26_explain_says_where_every_variable_came_from() {
    let sel = selection()
        .set("envtest.ctx_size", 4096u32)
        .set("envtest.extra_env", "OLLAMA_DEBUG=1");
    let plan = plan(&registry(), &sel, &facts("linux")).unwrap();
    let text = render_explain(&plan);
    for wanted in [
        "OLLAMA_HOST",
        "engine launch.spawn.env",
        "OLLAMA_CONTEXT_LENGTH",
        "field envtest.ctx_size",
        "OLLAMA_DEBUG",
        "field envtest.extra_env",
    ] {
        assert!(text.contains(wanted), "`{wanted}` is not in:\n{text}");
    }
}
