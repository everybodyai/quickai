//! Group A of docs/phase3-test-plan.md: configurations, a runtime as a file that is chosen, saved and
//! shared (WP4.17, docs/configurations.md). These are the tests of the core: the document, what
//! travels, what is captured and applied, and what a person is told. The store, the command line and
//! the windows are the application's.

mod common;

use std::collections::BTreeMap;

use quickai_core::config::Layer;
use quickai_core::diag::{Diagnostic, Severity};
use quickai_core::doc::engine::{Field, Share};
use quickai_core::plan::{plan, HostFacts, ModelInfo, Selection, TRAFFIC_PROXY_KEY};
use quickai_core::profile::notes::{self, NOTES};
use quickai_core::profile::state::{
    apply, capture, changes, Change, LeftOut, ModelFacts, WorkingState,
};
use quickai_core::profile::{Configuration, ModelSource};
use quickai_core::registry::Registry;
use quickai_core::value::Value;

use common::Shape;

const VERSION: &str = "0.1.0";

fn errors(r: &Registry) -> Vec<&Diagnostic> {
    r.diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn warnings(r: &Registry) -> Vec<String> {
    r.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.to_string())
        .collect()
}

fn person(name: &str) -> String {
    format!("/home/u/.config/quickai/profiles/{name}.toml")
}

fn user_registry(name: &str, text: &str) -> Registry {
    let file = person(name);
    common::shipped_registry(&[(file.as_str(), text)])
}

fn configuration<'a>(r: &'a Registry, id: &str) -> &'a Configuration {
    &r.profile(id)
        .unwrap_or_else(|| {
            panic!(
                "no configuration called {id}; {:?}",
                r.diagnostics
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
            )
        })
        .configuration
}

const VISION: &str = r#"schema = 1
kind = "profile"
id = "vision-chat"
name = "Vision chat"
engine = "ollama"
mystery = 1

[meta]
note = "Qwen3-VL 8B on Ollama, with Open WebUI beside it."
made_with = "QuickAI 0.1.0"
made_on = { os = "linux", arch = "x86_64", gpu = ["amd"], memory_gb = 32 }
flavour = "mild"

[runtime]
build = "v0.40.0"
version = "0.40.0"

[[model]]
role = "chat"
ollama = "qwen3-vl:8b"
size = 6140000000

[values.ollama]
ctx_size = 16384
extra_env = "OLLAMA_FLASH_ATTENTION=1"

[values.openwebui]
enabled = true
port = 3001

[app]
traffic_proxy = true
"#;

// ---- the document (WP4.17.1) ----------------------------------------------------------------------

#[test]
fn p3_a01_the_default_the_application_ships_loads_clean_and_is_built_in() {
    let r = common::shipped_registry(&[]);
    assert!(
        r.diagnostics.iter().all(|d| d.severity == Severity::Info),
        "{:?}",
        r.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
    );
    let entry = r.profile("default").expect("the Default");
    let c = &entry.configuration;
    assert_eq!(c.name, "Default");
    assert_eq!(c.engine, "llamacpp");
    assert!(c.models.is_empty() && c.values.is_empty() && c.traffic_proxy.is_none());
    assert_eq!(c.runtime.build, None);
    assert_eq!(entry.layer, Layer::Embedded);
    assert_eq!(r.profiles().count(), 1, "nothing else is shipped");
    // the engine it names is one a person can choose
    assert!(r.selectable_engine(&c.engine).is_some());
}

#[test]
fn p3_a02_a_configuration_is_read_and_written_again_unchanged() {
    let r = user_registry("vision-chat", VISION);
    let c = configuration(&r, "vision-chat");
    assert_eq!(c.name, "Vision chat");
    assert_eq!(c.engine, "ollama");
    assert_eq!(c.runtime.build.as_deref(), Some("v0.40.0"));
    assert_eq!(c.models.len(), 1);
    assert_eq!(
        c.models[0].source,
        ModelSource::Named {
            kind: "ollama".into(),
            name: "qwen3-vl:8b".into()
        }
    );
    assert_eq!(c.models[0].size, Some(6_140_000_000));
    assert_eq!(c.values["ollama.ctx_size"], Value::Int(16384));
    assert_eq!(
        c.values["ollama.extra_env"],
        Value::Str("OLLAMA_FLASH_ATTENTION=1".into())
    );
    assert_eq!(c.values["openwebui.enabled"], Value::Bool(true));
    assert_eq!(c.values["openwebui.port"], Value::Int(3001));
    assert_eq!(c.traffic_proxy, Some(true));
    let made_on = c.meta.made_on.as_ref().unwrap();
    assert_eq!(made_on.os.as_deref(), Some("linux"));
    assert_eq!(made_on.gpu, vec!["amd"]);
    assert_eq!(made_on.memory_gb, Some(32));

    // the two keys this version does not know are told, with their lines
    let told = warnings(&r);
    assert!(
        told.iter()
            .any(|w| w.contains("mystery") && w.contains("is not used by this version")),
        "{told:?}"
    );
    assert!(told.iter().any(|w| w.contains("meta.flavour")), "{told:?}");
    assert!(told.iter().all(|w| w.contains(&person("vision-chat"))));

    // written again, it is the same configuration, and still has what this version does not know
    let text = c.to_toml(r.engine_map(), VERSION, false);
    assert!(text.contains("mystery = 1"), "{text}");
    assert!(text.contains("flavour = \"mild\""), "{text}");
    assert!(text.contains("requires = { app = \">=0.1\" }"), "{text}");
    let again = user_registry("vision-chat", &text);
    assert_eq!(configuration(&again, "vision-chat"), c, "{text}");
    // and writing that gives the same text: nothing wanders
    assert_eq!(
        configuration(&again, "vision-chat").to_toml(again.engine_map(), VERSION, false),
        text
    );
    // the keys that are not tables come first, or they would belong to a table
    let first_table = text.find("\n[").expect("tables");
    for key in ["engine = ", "name = ", "mystery = "] {
        assert!(text.find(key).unwrap() < first_table, "{key} in\n{text}");
    }
}

#[test]
fn the_example_of_the_docs_loads_clean_and_says_what_the_page_says() {
    let text = include_str!("../../../docs/examples/profiles/vision-chat.toml");
    let r = user_registry("vision-chat", text);
    assert!(
        r.diagnostics.iter().all(|d| d.severity == Severity::Info),
        "{:?}",
        r.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
    );
    let c = configuration(&r, "vision-chat");
    assert_eq!(c.engine, "ollama");
    assert_eq!(c.values["ollama.ctx_size"], Value::Int(16384));
    assert_eq!(c.values["openwebui.enabled"], Value::Bool(true));
    // the sketch of a stack is not a configuration of this version and is not read as one
    let stack = include_str!("../../../docs/examples/profiles/voice-chat.toml");
    let r = user_registry("voice-chat", stack);
    assert!(r.profile("voice-chat").is_none());
}

/// `(what is wrong, the document, what the error says, whether it points at a line)`.
fn mistakes() -> Vec<(&'static str, String, &'static str, bool)> {
    let head = "schema = 1\nkind = \"profile\"\nid = \"bad\"\n";
    let with = |body: &str| format!("{head}{body}");
    vec![
        (
            "no engine",
            with("name = \"x\"\n"),
            "missing field `engine`",
            false,
        ),
        (
            "engine is not a text",
            with("engine = 5\n"),
            "expected a string",
            true,
        ),
        (
            "an engine that is not an id",
            with("engine = \"has space\"\n"),
            "is not the id of an engine",
            true,
        ),
        (
            "a model with two sources",
            with("engine = \"ollama\"\n[[model]]\nollama = \"a:b\"\nfile = \"x.gguf\"\n"),
            "exactly one source",
            true,
        ),
        (
            "a model with none",
            with("engine = \"ollama\"\n[[model]]\nrole = \"chat\"\n"),
            "exactly one source",
            true,
        ),
        (
            "a source that is a path",
            with("engine = \"ollama\"\n[[model]]\nollama = \"../../etc/passwd\"\n"),
            "not a name an engine pulls",
            true,
        ),
        (
            "a kind of name the engine does not take",
            with("engine = \"ollama\"\n[[model]]\nhuggingface = \"a/b\"\n"),
            "is not a kind of name this engine takes",
            true,
        ),
        (
            "a file with a folder in it",
            with("engine = \"llamacpp\"\n[[model]]\nfile = \"sub/x.gguf\"\n"),
            "a file is a name",
            true,
        ),
        (
            "a file with a dot dot",
            with("engine = \"llamacpp\"\n[[model]]\nfile = \"..x.gguf\"\n"),
            "a file is a name",
            true,
        ),
        (
            "a file with a drive",
            with("engine = \"llamacpp\"\n[[model]]\nfile = \"C:x.gguf\"\n"),
            "a file is a name",
            true,
        ),
        (
            "values that are not a table of tables",
            with("engine = \"ollama\"\nvalues = { ollama = 5 }\n"),
            "expected a map",
            true,
        ),
        (
            "a name of 61 characters",
            with(&format!(
                "engine = \"ollama\"\nname = \"{}\"\n",
                "n".repeat(61)
            )),
            "more than 60 characters",
            true,
        ),
        (
            "a build that is a path",
            with("engine = \"ollama\"\n[runtime]\nbuild = \"../bin\"\n"),
            "is not the label of a build",
            true,
        ),
        (
            "a role with a space",
            with("engine = \"llamacpp\"\n[[model]]\nrole = \"a b\"\nfile = \"x.gguf\"\n"),
            "a role is a word",
            true,
        ),
        (
            "a digest that is not one",
            with("engine = \"llamacpp\"\n[[model]]\nfile = \"x.gguf\"\nsha256 = \"abc\"\n"),
            "64 hexadecimal digits",
            true,
        ),
        (
            "not TOML",
            "engine = = 1".to_string(),
            "not valid TOML",
            true,
        ),
    ]
}

#[test]
fn p3_a03_each_mistake_is_said_with_its_file_and_line_and_the_rest_still_loads() {
    for (what, text, says, has_line) in mistakes() {
        let r = user_registry("bad", &text);
        assert!(r.profile("bad").is_none(), "{what}: it was kept");
        assert!(
            r.profile("default").is_some(),
            "{what}: the rest did not load"
        );
        let found = errors(&r);
        assert!(
            found.iter().any(|d| d.message.contains(says)),
            "{what}: wanted `{says}` in {:?}",
            found.iter().map(|d| d.to_string()).collect::<Vec<_>>()
        );
        let d = found.iter().find(|d| d.message.contains(says)).unwrap();
        assert_eq!(d.file.as_deref(), Some(person("bad").as_str()), "{what}");
        if has_line {
            assert!(d.line.is_some(), "{what}: no line in {d}");
        }
    }
}

#[test]
fn p3_a04_a_newer_schema_or_application_is_skipped_with_the_sentence_that_says_to_update() {
    let newer_app = "schema = 1\nkind = \"profile\"\nid = \"later\"\nrequires = { app = \">=0.3\" }\nengine = \"llamacpp\"\n";
    let newer_schema =
        "schema = 2\nkind = \"profile\"\nid = \"much-later\"\nengine = \"llamacpp\"\n";
    let ok = "schema = 1\nkind = \"profile\"\nid = \"now\"\nrequires = { app = \">=0.1\" }\nengine = \"llamacpp\"\n";
    let files = [
        (person("later"), newer_app),
        (person("much-later"), newer_schema),
        (person("now"), ok),
    ];
    let user: Vec<(&str, &str)> = files.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    let r = Shape {
        user: &user,
        with_catalogs: true,
        app_version: Some("0.1.0"),
        ..Shape::default()
    }
    .build();
    assert!(r.profile("later").is_none() && r.profile("much-later").is_none());
    assert!(r.profile("now").is_some() && r.profile("default").is_some());
    let told: Vec<String> = errors(&r).iter().map(|d| d.to_string()).collect();
    assert!(
        told.iter()
            .any(|t| t.contains("needs QuickAI >=0.3, this is 0.1.0")
                && t.contains("update QuickAI")),
        "{told:?}"
    );
    assert!(
        told.iter().any(
            |t| t.contains("newer than this application reads") && t.contains("update QuickAI")
        ),
        "{told:?}"
    );
}

#[test]
fn p3_a05_the_layers_decide_which_configuration_is_in_force() {
    let starter = "schema = 1\nkind = \"profile\"\nid = \"starter\"\nname = \"Starter\"\nengine = \"ollama\"\n";
    let package = [("/opt/quickai/config/profiles/starter.toml", starter)];
    let own = "schema = 1\nkind = \"profile\"\nid = \"default\"\nreplace = true\nname = \"My default\"\nengine = \"ollama\"\n[values.ollama]\nctx_size = 8192\n";
    let mine =
        "schema = 1\nkind = \"profile\"\nid = \"mine\"\nname = \"Mine\"\nengine = \"llamacpp\"\n";
    let files = [(person("default"), own), (person("mine"), mine)];
    let user: Vec<(&str, &str)> = files.iter().map(|(n, t)| (n.as_str(), *t)).collect();

    // a package ships one; the person's own file stands in for the built-in Default and adds one
    let r = Shape {
        bundle: &package,
        user: &user,
        with_catalogs: true,
        ..Shape::default()
    }
    .build();
    assert!(errors(&r).is_empty(), "{:?}", r.diagnostics);
    let starter = r.profile("starter").unwrap();
    assert_eq!(starter.layer, Layer::Bundle);
    let default = r.profile("default").unwrap();
    assert_eq!(default.configuration.name, "My default");
    assert_eq!(default.configuration.engine, "ollama");
    assert_eq!(default.layer, Layer::User);
    assert_eq!(default.file, person("default"));
    assert_eq!(
        default.configuration.values["ollama.ctx_size"],
        Value::Int(8192)
    );
    let mine = r.profile("mine").unwrap();
    assert_eq!(mine.layer, Layer::User);

    // removing the file brings the built-in one back
    let r = Shape {
        bundle: &package,
        with_catalogs: true,
        ..Shape::default()
    }
    .build();
    let default = r.profile("default").unwrap();
    assert_eq!(default.configuration.name, "Default");
    assert_eq!(default.layer, Layer::Embedded);
    assert!(r.profile("mine").is_none());

    // safe mode ignores the person's layer, which is the way out of a configuration that misbehaves
    let r = Shape {
        bundle: &package,
        user: &user,
        with_catalogs: true,
        safe_mode: true,
        ..Shape::default()
    }
    .build();
    assert_eq!(r.profile("default").unwrap().configuration.name, "Default");
    assert!(r.profile("mine").is_none() && r.profile("starter").is_some());
}

fn field(ty: &str, extra: &str) -> Field {
    toml::from_str(&format!("id = \"x\"\ntype = \"{ty}\"\n{extra}")).unwrap()
}

#[test]
fn p3_a07_share_is_what_a_document_says_else_what_the_type_says() {
    use Share::{Never, Review, Yes};
    for (ty, want) in [
        ("int", Some(Yes)),
        ("float", Some(Yes)),
        ("bool", Some(Yes)),
        ("choice", Some(Yes)),
        ("port", Some(Yes)),
        ("text", Some(Review)),
        ("url", Some(Review)),
        ("kv_lines", Some(Review)),
        ("dir", Some(Never)),
        ("file", Some(Never)),
        ("runtime_path", Some(Never)),
        ("hidden", None),
        ("model_ref", None),
    ] {
        assert_eq!(field(ty, "").share(), want, "{ty}");
    }
    // the document's own word wins, and so does its sentence
    let f = field(
        "bool",
        "share = \"review\"\nshare_note = \"It widens access.\"\n",
    );
    assert_eq!(f.share(), Some(Review));
    assert_eq!(f.share_note.as_deref(), Some("It widens access."));
    assert_eq!(field("text", "share = \"never\"").share(), Some(Never));
    // a hidden value is not a setting, whatever it says
    assert_eq!(field("hidden", "share = \"yes\"").share(), None);

    // a word that is none of the three is an error that names the field, and the shipped engine stays
    let patch = "schema = 1\nkind = \"engine\"\nid = \"ollama\"\n[[field]]\nid = \"port\"\nshare = \"sometimes\"\n";
    let file = "/home/u/.config/quickai/engines/ollama.toml";
    let r = common::shipped_registry(&[(file, patch)]);
    let found = errors(&r);
    assert!(
        found
            .iter()
            .any(|d| d.path.contains("field[port].share") && d.message.contains("unknown variant")),
        "{:?}",
        found.iter().map(|d| d.to_string()).collect::<Vec<_>>()
    );
    assert!(
        r.engine("ollama").is_some(),
        "the embedded document stays in force"
    );

    // a folder given away, or a hidden value that says it travels, is told
    let engine = r#"schema = 1
kind = "engine"
id = "ollama"
[[field]]
id = "models_dir"
type = "dir"
share = "yes"
[[field]]
id = "last_model"
type = "hidden"
share = "yes"
[[field]]
id = "port"
type = "port"
share_note = "nothing to say"
"#;
    let r = common::shipped_registry(&[(file, engine)]);
    let told = warnings(&r);
    assert!(
        told.iter().any(|w| w.contains("belongs to a computer")),
        "{told:?}"
    );
    assert!(
        told.iter()
            .any(|w| w.contains("has no effect on a hidden value")),
        "{told:?}"
    );
    assert!(told.iter().any(|w| w.contains("is neither")), "{told:?}");
}

/// What each engine's fields do in a configuration: `engine.field share`, `-` for a field that is not a
/// setting. A change of one word here is a change of what a stranger's file can reach or what leaves
/// this computer, and is read as one.
const SHARES: &str = "\
comfyui.bin_override never
comfyui.extra_args review
comfyui.extra_env review
comfyui.gfx_override never
comfyui.gpu yes
comfyui.memory yes
comfyui.mode yes
comfyui.models_dir never
comfyui.network review
comfyui.output_dir never
comfyui.page_downloads review
comfyui.port yes
comfyui.python never
llamacpp.bin_override never
llamacpp.ctx_size yes
llamacpp.extra_args review
llamacpp.last_model -
llamacpp.models_dir never
llamacpp.n_gpu_layers yes
llamacpp.port yes
ollama.bin_override never
ollama.ctx_size yes
ollama.extra_env review
ollama.gpu yes
ollama.last_model -
ollama.mode yes
ollama.models_dir never
ollama.port yes
ollama.use_default_store review
openwebui.bin_override never
openwebui.enabled yes
openwebui.extra_env never
openwebui.first_ollama -
openwebui.network review
openwebui.port yes
openwebui.python never
";

#[test]
fn p3_a08_the_four_shipped_documents_say_what_the_design_says_travels() {
    let r = common::shipped_registry(&[]);
    let mut lines = Vec::new();
    for engine in r.engines() {
        for f in &engine.doc.fields {
            let share = f.share().map_or("-", Share::name);
            lines.push(format!("{}.{} {share}", engine.id, f.id));
        }
    }
    lines.sort();
    let got = lines.join("\n") + "\n";
    assert_eq!(got, SHARES, "the shares changed:\n{got}");
    // nothing that names a folder, a program or a card travels as it is
    for engine in r.engines() {
        for f in &engine.doc.fields {
            if matches!(
                f.ty,
                quickai_core::doc::engine::FieldType::Dir
                    | quickai_core::doc::engine::FieldType::File
                    | quickai_core::doc::engine::FieldType::RuntimePath
            ) {
                assert_eq!(f.share(), Some(Share::Never), "{}.{}", engine.id, f.id);
            }
        }
    }
    // every field that is asked about says why
    for engine in r.engines() {
        for f in &engine.doc.fields {
            if f.share() == Some(Share::Review) {
                assert!(
                    f.share_note.is_some(),
                    "{}.{} is asked about and has no sentence",
                    engine.id,
                    f.id
                );
            }
        }
    }
    // a companion's reason for what it keeps to itself is in its own document
    let note = r
        .engine("openwebui")
        .unwrap()
        .doc
        .fields
        .iter()
        .find(|f| f.id == "extra_env")
        .and_then(|f| f.share_note.clone())
        .unwrap();
    assert!(note.contains("database"), "{note}");
}

#[test]
fn p3_a09_the_vocabulary_says_every_note_and_the_names_a_file_may_not_set() {
    let r = common::shipped_registry(&[]);
    let v = r.vocabulary();
    assert!(v.has_findings());
    for spec in NOTES {
        let sentence = v
            .finding(spec.id)
            .unwrap_or_else(|| panic!("no sentence for {}", spec.id));
        for hole in notes::holes_of(sentence) {
            assert!(
                spec.holes.contains(&hole.as_str()),
                "{}: `{{{hole}}}` is not one of {:?}",
                spec.id,
                spec.holes
            );
        }
    }
    // the names a file may not set: the code's floor, and the vocabulary's
    assert!(notes::env_name_denied("ld_preload", v.deny_env()));
    assert!(notes::env_name_denied("NODE_OPTIONS", v.deny_env()));
    assert!(notes::env_name_denied("perl5lib", v.deny_env()));
    assert!(!notes::env_name_denied(
        "OLLAMA_FLASH_ATTENTION",
        v.deny_env()
    ));

    // a vocabulary that words some of the notes and not all is an error that names the others
    let file = "/home/u/.config/quickai/vocab/words.toml";
    let partial = "schema = 1\nkind = \"vocab\"\nid = \"words\"\n[[finding]]\nid = \"memory\"\ntext = \"Short of memory.\"\n";
    let only = common::registry_of(&[("embedded:vocab/words.toml", partial)]);
    assert!(errors(&only).iter().any(|d| d
        .message
        .contains("no sentence for the notes about a configuration")
        && d.message.contains("too_new")
        && !d.message.contains("memory,")));

    // a sentence may use only the words its note fills in, and say something
    for (what, doc, says) in [
        (
            "a word the note does not have",
            "[[finding]]\nid = \"memory\"\ntext = \"{nope}\"\n",
            "uses `{nope}`",
        ),
        (
            "an empty sentence",
            "[[finding]]\nid = \"memory\"\ntext = \"  \"\n",
            "is empty",
        ),
        (
            "a sentence given twice",
            "[[finding]]\nid = \"memory\"\ntext = \"a\"\n[[finding]]\nid = \"memory\"\ntext = \"b\"\n",
            "given a sentence twice",
        ),
        (
            "a name that is not a variable",
            "[profile]\ndeny_env = [\"no spaces\"]\n",
            "not the name of an environment variable",
        ),
        ("a star alone", "[profile]\ndeny_env = [\"*\"]\n", "would refuse every variable"),
    ] {
        let text = format!("schema = 1\nkind = \"vocab\"\nid = \"words\"\n{doc}");
        let r = common::registry_of(&[("embedded:vocab/words.toml", text.as_str())]);
        assert!(
            errors(&r).iter().any(|d| d.message.contains(says)),
            "{what}: {:?}",
            r.diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>()
        );
    }
    // a note this version does not make may be for a newer one: a warning, with the nearest suggested
    let text = "schema = 1\nkind = \"vocab\"\nid = \"words\"\n[[finding]]\nid = \"to_new\"\ntext = \"x\"\n";
    let r = common::registry_of(&[("embedded:vocab/words.toml", text)]);
    let told = r
        .diagnostics
        .iter()
        .find(|d| d.message.contains("is not a note this version"))
        .expect("a warning");
    assert_eq!(told.severity, Severity::Warning);
    assert_eq!(told.hint.as_deref(), Some("did you mean `too_new`?"));
    let _ = file;
}

// ---- capture, apply, difference (WP4.17.2) ----------------------------------------------------------

fn s(text: &str) -> Value {
    Value::Str(text.to_string())
}

fn i(n: i64) -> Value {
    Value::Int(n)
}

fn b(on: bool) -> Value {
    Value::Bool(on)
}

fn registry() -> Registry {
    common::shipped_registry(&[])
}

/// What a first run stores for the settings that have always been in the settings file (the
/// application's `Settings::default()`): the llama.cpp ones.
fn first_run() -> BTreeMap<String, Value> {
    [
        ("llamacpp.port", i(8080)),
        ("llamacpp.ctx_size", i(4096)),
        ("llamacpp.n_gpu_layers", i(999)),
        ("llamacpp.extra_args", s("")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn state(engine: &str, pairs: &[(&str, Value)]) -> WorkingState {
    WorkingState {
        engine: engine.to_string(),
        values: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        build: None,
        traffic_proxy: false,
    }
}

/// What the settings file does when it is written and read again: a setting that has always been in
/// it comes back with what a first run holds when it was not set.
fn as_stored(mut state: WorkingState) -> WorkingState {
    for (key, value) in first_run() {
        if state.engine == "llamacpp" {
            state.values.entry(key).or_insert(value);
        }
    }
    state
}

fn llama_state() -> WorkingState {
    let mut st = state(
        "llamacpp",
        &[
            ("llamacpp.port", i(8099)),
            ("llamacpp.ctx_size", i(8192)),
            ("llamacpp.n_gpu_layers", i(20)),
            ("llamacpp.extra_args", s("--threads 8 --flash-attn on")),
            ("llamacpp.last_model", s("Qwen3-4B-Q4_K_M.gguf")),
            ("llamacpp.models_dir", s("/data/a/models")),
            ("llamacpp.bin_override", s("/opt/a-tools/llama-server")),
        ],
    );
    st.traffic_proxy = true;
    st
}

fn ollama_state() -> WorkingState {
    state(
        "ollama",
        &[
            ("ollama.port", i(11500)),
            ("ollama.ctx_size", i(16384)),
            ("ollama.gpu", s("off")),
            ("ollama.mode", s("auto")),
            ("ollama.models_dir", s("/data/a/ollama")),
            ("ollama.use_default_store", b(true)),
            ("ollama.extra_env", s("OLLAMA_FLASH_ATTENTION=1")),
            ("ollama.bin_override", s("/opt/a-tools/ollama")),
            ("ollama.last_model", s("qwen3-vl:8b")),
            ("openwebui.enabled", b(true)),
            ("openwebui.port", i(3000)),
            ("openwebui.network", s("online")),
            ("openwebui.python", s("/usr/bin/python3.12")),
            ("openwebui.extra_env", s("WEBUI_NAME=x")),
        ],
    )
}

fn comfy_state() -> WorkingState {
    state(
        "comfyui",
        &[
            ("comfyui.port", i(8199)),
            ("comfyui.gpu", s("off")),
            ("comfyui.memory", s("auto")),
            ("comfyui.network", s("online")),
            ("comfyui.page_downloads", s("browser")),
            ("comfyui.extra_args", s("--preview-method auto")),
            ("comfyui.extra_env", s("FOO=1")),
            ("comfyui.models_dir", s("/data/a/comfy/models")),
            ("comfyui.output_dir", s("/data/a/comfy/out")),
            ("comfyui.python", s("/usr/bin/python3.12")),
            ("comfyui.gfx_override", s("10.3.0")),
        ],
    )
}

fn weights() -> ModelFacts {
    ModelFacts {
        size: Some(2_497_281_120),
        ..ModelFacts::default()
    }
}

fn keys(c: &Configuration) -> Vec<&str> {
    c.values.keys().map(String::as_str).collect()
}

#[test]
fn p3_a11_what_is_captured_from_the_working_state_and_what_never_is() {
    let r = registry();
    let mut st = ollama_state();
    // other things the working state may hold: another engine's settings, the programs the person allowed
    st.values.insert("llamacpp.port".into(), i(9999));
    st.values.insert(
        "app.confirmed_engines".into(),
        Value::List(vec![s("abc123")]),
    );
    st.build = Some("v0.40.0".into());
    st.traffic_proxy = true;
    let facts = ModelFacts {
        size: Some(6_140_000_000),
        display: Some("Qwen3-VL 8B".into()),
        ..ModelFacts::default()
    };
    let got = capture(&r, &st, &first_run(), Some(&facts));
    let c = &got.configuration;
    assert_eq!(c.engine, "ollama");
    // the numbers, choices and flags that differ, the lines of text that are asked about, and the companion's
    assert_eq!(
        keys(c),
        [
            "ollama.ctx_size",
            "ollama.extra_env",
            "ollama.gpu",
            "ollama.port",
            "ollama.use_default_store",
            "openwebui.enabled",
            "openwebui.network",
        ]
    );
    assert_eq!(c.values["openwebui.network"], s("online"));
    assert_eq!(c.runtime.build.as_deref(), Some("v0.40.0"));
    assert_eq!(c.traffic_proxy, Some(true));
    assert_eq!(c.models.len(), 1);
    assert_eq!(
        c.models[0].source,
        ModelSource::Named {
            kind: "ollama".into(),
            name: "qwen3-vl:8b".into()
        }
    );
    assert_eq!(c.models[0].size, Some(6_140_000_000));
    assert_eq!(c.models[0].name.as_deref(), Some("Qwen3-VL 8B"));
    // what stays with the computer: the folders, the programs of the companion, its environment
    let private: Vec<&str> = got
        .left_out
        .iter()
        .filter_map(|l| match l {
            LeftOut::Private { key } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        private,
        [
            "ollama.models_dir",
            "openwebui.python",
            "openwebui.extra_env"
        ]
    );
    // nothing of another engine, of the consents or of the program of the build is in it
    let text = c.to_toml(r.engine_map(), VERSION, false);
    for never in [
        "9999",
        "abc123",
        "/opt/a-tools/ollama",
        "/data/a",
        "/usr/bin/python3.12",
        "WEBUI_NAME",
    ] {
        assert!(!text.contains(never), "{never} in\n{text}");
    }
    // a program that is not an installed build is one that stays with the computer
    st.build = None;
    let got = capture(&r, &st, &first_run(), None);
    assert!(got.left_out.contains(&LeftOut::Private {
        key: "ollama.bin_override".into()
    }));
    assert_eq!(got.configuration.runtime.build, None);
}

#[test]
fn p3_a12_only_what_differs_from_the_default_is_written() {
    let r = registry();
    // a first run: every setting is what it has always been, and nothing is written
    let first = as_stored(state("llamacpp", &[]));
    let got = capture(&r, &first, &first_run(), None);
    assert!(
        got.configuration.values.is_empty(),
        "{:?}",
        got.configuration.values
    );
    assert!(got.configuration.models.is_empty() && got.left_out.is_empty());
    // the same value typed again is no change; another is, and only it is written
    let mut st = first.clone();
    st.values.insert("llamacpp.port".into(), i(8080));
    st.values.insert("llamacpp.ctx_size".into(), i(8192));
    let got = capture(&r, &st, &first_run(), None);
    assert_eq!(keys(&got.configuration), ["llamacpp.ctx_size"]);
    // a computed default is not frozen: a context size that is not stored is not written, and one that is
    // a first run's is not either
    st.values.remove("llamacpp.ctx_size");
    assert!(capture(&r, &st, &first_run(), None)
        .configuration
        .values
        .is_empty());
    // the literal default of a document counts where no first run holds a value
    let mut st = state(
        "ollama",
        &[("ollama.gpu", s("auto")), ("ollama.mode", s("auto"))],
    );
    assert!(capture(&r, &st, &first_run(), None)
        .configuration
        .values
        .is_empty());
    // a setting that has no default is written as soon as it is set
    st.values.insert("ollama.ctx_size".into(), i(4096));
    assert_eq!(
        keys(&capture(&r, &st, &first_run(), None).configuration),
        ["ollama.ctx_size"]
    );
    // an empty text is the default of a text
    st.values.insert("ollama.extra_env".into(), s(""));
    assert_eq!(
        keys(&capture(&r, &st, &first_run(), None).configuration),
        ["ollama.ctx_size"]
    );
}

fn config_for(engine: &str, pairs: &[(&str, Value)]) -> Configuration {
    Configuration {
        id: "c".into(),
        name: "C".into(),
        engine: engine.into(),
        values: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        ..Configuration::default()
    }
}

#[test]
fn p3_a13_applying_sets_what_is_listed_and_returns_the_rest_to_the_default() {
    let r = registry();
    // the working state of someone who has been using other things
    let mut base = ollama_state();
    base.values.insert("ollama.ctx_size".into(), i(2048));
    base.values.insert("ollama.gpu".into(), s("off"));
    base.values.insert("llamacpp.port".into(), i(9999));
    base.values
        .insert("llamacpp.models_dir".into(), s("/data/a/models"));
    base.values.insert(
        "app.confirmed_engines".into(),
        Value::List(vec![s("abc123")]),
    );
    base.build = Some("v0.35.1".into());
    let wanted = config_for(
        "ollama",
        &[("ollama.port", i(11600)), ("openwebui.port", i(3100))],
    );
    let applied = apply(&r, &wanted).expect("an engine there is");
    assert_eq!(applied.engine, "ollama");
    assert_eq!(applied.set.len(), 2);
    // the rest of the engine and of its companion goes back to the default (and the model is the caller's)
    for key in [
        "ollama.ctx_size",
        "ollama.gpu",
        "ollama.extra_env",
        "ollama.use_default_store",
        "openwebui.enabled",
        "openwebui.network",
    ] {
        assert!(
            applied.cleared.iter().any(|k| k == key),
            "{key}: {:?}",
            applied.cleared
        );
    }
    // the folders, the programs and the consents are not settings of a configuration at all
    for kept in [
        "ollama.models_dir",
        "ollama.bin_override",
        "openwebui.python",
        "openwebui.extra_env",
    ] {
        assert!(!applied.cleared.iter().any(|k| k == kept), "{kept}");
        assert!(!applied.set.contains_key(kept));
    }
    let after = base.after(&r, &applied, None, None);
    assert_eq!(after.values["ollama.port"], i(11600));
    assert_eq!(after.values["openwebui.port"], i(3100));
    for gone in [
        "ollama.ctx_size",
        "ollama.gpu",
        "ollama.extra_env",
        "ollama.last_model",
        "openwebui.enabled",
        "openwebui.network",
    ] {
        assert!(!after.values.contains_key(gone), "{gone}");
    }
    // what a configuration does not carry stays where it was
    for kept in [
        "ollama.models_dir",
        "ollama.bin_override",
        "openwebui.python",
        "openwebui.extra_env",
        "llamacpp.port",
        "llamacpp.models_dir",
        "app.confirmed_engines",
    ] {
        assert_eq!(after.values[kept], base.values[kept], "{kept}");
    }
    assert_eq!(
        after.build.as_deref(),
        Some("v0.35.1"),
        "a program that is not named is left alone"
    );
    // another engine: the first one's settings are not touched, the new one's start from nothing
    let llama = config_for("llamacpp", &[("llamacpp.ctx_size", i(16384))]);
    let applied = apply(&r, &llama).unwrap();
    let after = base.after(&r, &applied, Some("m.gguf"), Some("b11007"));
    assert_eq!(after.engine, "llamacpp");
    assert_eq!(after.values["llamacpp.ctx_size"], i(16384));
    assert_eq!(after.values["llamacpp.last_model"], s("m.gguf"));
    assert_eq!(after.values["ollama.port"], base.values["ollama.port"]);
    // the new engine's own settings start from their defaults, and its folder is still the person's
    assert!(!after.values.contains_key("llamacpp.port"));
    assert_eq!(after.values["llamacpp.models_dir"], s("/data/a/models"));
    assert_eq!(after.build.as_deref(), Some("b11007"));
}

#[test]
fn p3_a14_the_new_state_is_made_whole_before_anything_is_stored_and_what_does_not_fit_is_said() {
    let r = registry();
    let wanted = config_for(
        "ollama",
        &[
            ("ollama.port", i(99999)),
            ("ollama.gpu", s("turbo")),
            ("ollama.ctx_size", s("big")),
            (
                "ollama.extra_env",
                s("LD_PRELOAD=/tmp/x.so\nOLLAMA_HOST=0.0.0.0:1\nPath=/bin\nOLLAMA_FLASH_ATTENTION=1\n# keep\nPYTHONSTARTUP=/x"),
            ),
            ("ollama.models_dir", s("/etc")),
            ("openwebui.network", s("online")),
        ],
    );
    let base = ollama_state();
    let before = base.clone();
    let applied = apply(&r, &wanted).expect("an engine there is");
    // a value that does not fit is said, and the field goes back to its default
    let mut bad: Vec<&str> = applied
        .notes
        .iter()
        .filter(|n| n.id == "bad_value")
        .filter_map(|n| n.subject.as_deref())
        .collect();
    bad.sort();
    assert_eq!(bad, ["ollama.ctx_size", "ollama.gpu", "ollama.port"]);
    for key in bad {
        assert!(
            applied.cleared.iter().any(|k| k == key) && !applied.set.contains_key(key),
            "{key}"
        );
    }
    // a line that a file may not set, or that the engine protects, is left out, whatever its case
    let refused: Vec<&str> = applied
        .notes
        .iter()
        .filter(|n| n.id == "refused_line")
        .filter_map(|n| n.hole("name"))
        .collect();
    assert_eq!(
        refused,
        ["LD_PRELOAD", "OLLAMA_HOST", "Path", "PYTHONSTARTUP"]
    );
    assert_eq!(
        applied.set["ollama.extra_env"],
        s("OLLAMA_FLASH_ATTENTION=1\n# keep")
    );
    // a folder is not a setting a configuration carries, even when a file gives one
    assert!(!applied.set.contains_key("ollama.models_dir"));
    assert_eq!(applied.set["openwebui.network"], s("online"));
    // the state is computed, not changed: what was held is as it was
    let after = base.after(&r, &applied, None, None);
    assert_eq!(base, before);
    assert_eq!(
        after.values["ollama.models_dir"],
        before.values["ollama.models_dir"]
    );

    // an engine this version has not, and a companion named as the engine, stop it
    for engine in ["whispercpp", "openwebui", ""] {
        let note = apply(&r, &config_for(engine, &[])).expect_err(engine);
        assert_eq!(
            (note.id, note.level),
            ("unknown_engine", notes::Level::Stops)
        );
    }
}

fn model_name(c: &Configuration) -> Option<&str> {
    c.models.first().map(|m| m.source.label())
}

#[test]
fn p3_a15_a_round_trip_for_each_of_the_four_engines() {
    let r = registry();
    let cases = [
        ("llamacpp", as_stored(llama_state()), Some(weights())),
        (
            "ollama",
            ollama_state(),
            Some(ModelFacts {
                size: Some(6_140_000_000),
                ..ModelFacts::default()
            }),
        ),
        ("comfyui", comfy_state(), None),
    ];
    for (engine, st, facts) in cases {
        let first = capture(&r, &st, &first_run(), facts.as_ref()).named("rt", "Round trip");
        let c1 = first.configuration;
        assert!(!c1.values.is_empty(), "{engine}");
        let text = c1.to_toml(r.engine_map(), VERSION, false);
        // written, read
        let reread = user_registry("rt", &text);
        assert!(
            reread
                .diagnostics
                .iter()
                .all(|d| d.severity == Severity::Info),
            "{engine}: {:?}\n{text}",
            reread
                .diagnostics
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
        );
        let c2 = configuration(&reread, "rt").clone();
        assert_eq!(c2, c1, "{engine}\n{text}");
        // applied to a state that holds nothing, it makes the state the settings were
        let applied = apply(&reread, &c2).expect(engine);
        assert!(applied.notes.is_empty(), "{engine}: {:?}", applied.notes);
        let rebuilt = as_stored(WorkingState::default().after(
            &reread,
            &applied,
            model_name(&c2),
            applied.runtime.build.as_deref(),
        ));
        for (key, value) in &c1.values {
            assert_eq!(&rebuilt.values[key], value, "{engine} {key}");
        }
        // captured again it is the same configuration, byte for byte
        let again =
            capture(&reread, &rebuilt, &first_run(), facts.as_ref()).named("rt", "Round trip");
        assert!(again.configuration.same_run(&c1), "{engine}");
        assert_eq!(
            again.configuration.to_toml(r.engine_map(), VERSION, false),
            text,
            "{engine}"
        );
    }
}

fn plan_of(
    r: &Registry,
    st: &WorkingState,
    model: Option<ModelInfo>,
    facts: &HostFacts,
) -> serde_json::Value {
    let mut selection = Selection::new(&st.engine);
    selection.values = st.values.clone();
    if st.traffic_proxy {
        selection.values.insert(TRAFFIC_PROXY_KEY.into(), b(true));
    }
    selection.model = model;
    match plan(r, &selection, facts) {
        Ok(p) => serde_json::to_value(&p).unwrap(),
        Err(e) => panic!("the planner refused: {e:#?}"),
    }
}

#[test]
fn p3_a16_the_plan_an_imported_configuration_makes_is_the_plan_of_the_computer_it_came_from() {
    let r = registry();
    let facts = HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        ram_bytes: 16 * 1024 * 1024 * 1024,
        cpus: 8,
        binary: Some("/opt/bin/program".into()),
        data_dir: "/home/u/.quickai".into(),
        config_dir: "/home/u/.config/quickai".into(),
        home: "/home/u".into(),
        internal_port: Some(41999),
        ..Default::default()
    };
    let model = |path: &str, id: &str| ModelInfo {
        id: id.into(),
        display: id.into(),
        path: path.into(),
        size_bytes: 2_497_281_120,
    };
    // (the exporter's state, its model, the importer's own folders: the same names below another root)
    let cases = [
        (
            "llamacpp",
            as_stored(llama_state()),
            Some(model(
                "/data/a/models/Qwen3-4B-Q4_K_M.gguf",
                "Qwen3-4B-Q4_K_M.gguf",
            )),
            Some(weights()),
        ),
        (
            "ollama",
            ollama_state(),
            Some(model("", "qwen3-vl:8b")),
            None,
        ),
        ("comfyui", comfy_state(), None, None),
    ];
    for (engine, a, model_a, model_facts) in cases {
        // the exporter: its plan
        let plan_a = plan_of(&r, &a, model_a.clone(), &facts);
        // sent through a file
        let text = capture(&r, &a, &first_run(), model_facts.as_ref())
            .named("shared", "Shared")
            .configuration
            .to_toml(r.engine_map(), VERSION, false);
        let there = user_registry("shared", &text);
        let wanted = configuration(&there, "shared");
        // the importer: a state of its own, with folders and programs of its own
        let mut b = state("llamacpp", &[]);
        for (key, value) in &a.values {
            if key.ends_with("models_dir")
                || key.ends_with("bin_override")
                || key.ends_with("output_dir")
                || key.ends_with("python")
                || key.ends_with("extra_env") && key.starts_with("openwebui")
            {
                if let Value::Str(path) = value {
                    b.values.insert(
                        key.clone(),
                        s(&path
                            .replace("/data/a", "/mnt/b")
                            .replace("/opt/a-tools", "/opt/b-tools")),
                    );
                }
            }
        }
        b.values.insert("comfyui.gfx_override".into(), s("11.0.0"));
        // and settings left from what it was doing before, which the configuration does not give
        // and which must not be in the plan
        match engine {
            "ollama" => {
                b.values.insert("ollama.mode".into(), s("attach"));
                b.values.insert("openwebui.port".into(), i(3999));
            }
            "comfyui" => {
                b.values.insert("comfyui.mode".into(), s("spawn"));
            }
            _ => {
                b.values.insert("llamacpp.ctx_size".into(), i(2048));
            }
        }
        let applied = apply(&there, wanted).expect(engine);
        let name = model_name(wanted).map(|n| n.replace("/data/a", "/mnt/b"));
        let b = as_stored(b.after(&there, &applied, name.as_deref(), None));
        let model_b = model_a.map(|m| ModelInfo {
            path: m.path.replace("/data/a", "/mnt/b"),
            ..m
        });
        let plan_b = plan_of(&there, &b, model_b, &facts);
        // the two plans differ in the names of folders and in nothing else
        let normalised = serde_json::to_string(&plan_b)
            .unwrap()
            .replace("/mnt/b", "/data/a")
            .replace("/opt/b-tools", "/opt/a-tools");
        let mut want = serde_json::to_string(&plan_a).unwrap();
        // the exporter's card is named in its own override of the ROCm architecture, which stays with it
        if engine == "comfyui" {
            want = want.replace("10.3.0", "11.0.0");
        }
        assert_eq!(normalised, want, "{engine}");
    }
}

#[test]
fn p3_a17_the_sign_of_unsaved_changes_shows_a_change_of_the_run_and_nothing_else() {
    let r = registry();
    let saved_state = as_stored(llama_state());
    let facts = Some(weights());
    let saved = capture(&r, &saved_state, &first_run(), facts.as_ref()).configuration;
    let unchanged = |st: &WorkingState| {
        capture(&r, st, &first_run(), facts.as_ref())
            .configuration
            .same_run(&saved)
    };
    assert!(unchanged(&saved_state), "the state it was saved from");

    // a changed port shows it, the same value typed again does not
    let mut st = saved_state.clone();
    st.values.insert("llamacpp.port".into(), i(8100));
    assert!(!unchanged(&st), "a port");
    st.values.insert("llamacpp.port".into(), i(8099));
    assert!(unchanged(&st), "the same port typed again");
    // a number written as a float is the same number
    st.values
        .insert("llamacpp.port".into(), Value::Float(8099.0));
    assert!(unchanged(&st), "8099.0");
    // another model, the engine, the proxy, the build, a line of arguments
    let mut other = saved_state.clone();
    other
        .values
        .insert("llamacpp.last_model".into(), s("Other-Q4_K_M.gguf"));
    assert!(!unchanged(&other), "a model");
    let mut other = saved_state.clone();
    other.engine = "ollama".into();
    assert!(!unchanged(&other), "the engine");
    let mut other = saved_state.clone();
    other.traffic_proxy = false;
    assert!(!unchanged(&other), "the proxy");
    let mut other = saved_state.clone();
    other.build = Some("b11007".into());
    assert!(!unchanged(&other), "the program");
    let mut other = saved_state.clone();
    other
        .values
        .insert("llamacpp.extra_args".into(), s("--threads 4"));
    assert!(!unchanged(&other), "the arguments");

    // what is not part of a run does not show: a folder, the program of a person's own, the consents,
    // another engine's settings (the theme is not in the working state at all)
    let mut quiet = saved_state.clone();
    quiet
        .values
        .insert("llamacpp.models_dir".into(), s("/elsewhere"));
    quiet
        .values
        .insert("llamacpp.bin_override".into(), s("/elsewhere/llama-server"));
    quiet
        .values
        .insert("app.confirmed_engines".into(), Value::List(vec![s("x")]));
    quiet.values.insert("ollama.port".into(), i(1234));
    assert!(unchanged(&quiet), "things that are not a run");

    // a context size that is not stored (a default that follows the model) and one a first run stores are one
    let mut follows = saved_state.clone();
    follows.values.insert("llamacpp.ctx_size".into(), i(4096));
    let mut stored = saved_state.clone();
    stored.values.remove("llamacpp.ctx_size");
    let a = capture(&r, &follows, &first_run(), facts.as_ref()).configuration;
    let b = capture(&r, &stored, &first_run(), facts.as_ref()).configuration;
    assert!(a.same_run(&b));
    // a configuration that names no model does not mind which one is selected (the Default is one: the
    // application chooses a model when none was chosen), and one that names a model does
    let no_model = config_for("llamacpp", &[]);
    let with_model =
        capture(&r, &as_stored(llama_state()), &first_run(), facts.as_ref()).configuration;
    assert!(!with_model.same_run(&no_model));
    assert!(Configuration {
        values: BTreeMap::new(),
        traffic_proxy: None,
        ..with_model.clone()
    }
    .matches_run(&no_model));
    let mut other_model = with_model.clone();
    other_model.models[0].source = ModelSource::File {
        name: "Other.gguf".into(),
    };
    assert!(!other_model.matches_run(&with_model));
    // the name and the note are not part of the run
    let mut renamed = saved.clone();
    renamed.name = "Another name".into();
    renamed.meta.note = Some("a note".into());
    assert!(renamed.same_run(&saved));
}

#[test]
fn p3_a71_a_model_is_the_same_whether_it_is_said_as_a_file_or_as_the_entry_of_the_list_that_has_that_file(
) {
    let r = registry();
    let mut st = as_stored(llama_state());
    st.values
        .insert("llamacpp.last_model".into(), s("Bonsai-1.7B-Q1_0.gguf"));
    // the file of the recommended list, of the size the list says: it is said as the entry
    let listed = ModelFacts {
        size: Some(248_302_272),
        ..ModelFacts::default()
    };
    let by_entry = capture(&r, &st, &first_run(), Some(&listed)).configuration;
    assert!(
        matches!(by_entry.models[0].source, ModelSource::Catalog { .. }),
        "{:?}",
        by_entry.models
    );
    // the same name at another size (a copy of the person's own, or a stand-in): a plain file
    let own = ModelFacts {
        size: Some(262_144_000),
        ..ModelFacts::default()
    };
    let by_file = capture(&r, &st, &first_run(), Some(&own)).configuration;
    assert!(
        matches!(by_file.models[0].source, ModelSource::File { .. }),
        "{:?}",
        by_file.models
    );
    // it is one model as far as a window can tell: no unsaved change either way
    assert!(by_entry.same_run(&by_file) && by_file.same_run(&by_entry));
    assert!(by_file.matches_run(&by_entry));
    for facts in [&listed, &own] {
        for wanted in [&by_entry, &by_file] {
            let list = changes(&r, &st, &first_run(), Some(facts), wanted);
            assert!(
                list.iter().all(|c| !matches!(c, Change::Model { .. })),
                "{list:?}"
            );
        }
    }
    // another file is another model, whichever way it is said
    let mut other = st.clone();
    other
        .values
        .insert("llamacpp.last_model".into(), s("Bonsai-8B-Q1_0.gguf"));
    let elsewhere = capture(&r, &other, &first_run(), None).configuration;
    assert!(!elsewhere.same_run(&by_entry) && !elsewhere.same_run(&by_file));
}

#[test]
fn p3_a19_the_difference_is_a_list_of_settings_in_the_order_of_the_form() {
    let r = registry();
    let st = as_stored(state("llamacpp", &[("llamacpp.port", i(8080))]));
    // the order of the engine's form (port, context size), not the alphabet's
    let wanted = config_for(
        "llamacpp",
        &[("llamacpp.ctx_size", i(16384)), ("llamacpp.port", i(8081))],
    );
    let list = changes(&r, &st, &first_run(), None, &wanted);
    let said: Vec<String> = list.iter().map(Change::describe).collect();
    assert_eq!(said, ["Port: 8080 to 8081", "Context size: 4096 to 16384"]);
    // none when there is none, whatever the folders
    let same = config_for("llamacpp", &[]);
    let mut st_folders = st.clone();
    st_folders
        .values
        .insert("llamacpp.models_dir".into(), s("/data"));
    assert!(changes(&r, &st_folders, &first_run(), None, &same).is_empty());
    // a switch of engine says so first and then what would change there (the model and the settings of the
    // engine it goes to, which keeps its own), not what the old engine had
    let mut on_ollama = ollama_state();
    on_ollama.values.insert("llamacpp.port".into(), i(8085));
    let list = changes(&r, &on_ollama, &first_run(), None, &wanted);
    let said: Vec<String> = list.iter().map(Change::describe).collect();
    assert_eq!(
        said,
        [
            "Engine: ollama to llamacpp",
            "Port: 8085 to 8081",
            "Context size: 4096 to 16384"
        ]
    );
    // the model, the proxy and the program are listed too
    let mut with_all = wanted.clone();
    with_all.models.push(quickai_core::profile::ModelPick {
        role: "main".into(),
        source: ModelSource::File {
            name: "x.gguf".into(),
        },
        name: None,
        size: None,
        sha256: None,
        from: None,
    });
    with_all.traffic_proxy = Some(true);
    with_all.runtime.build = Some("b11007".into());
    let list = changes(&r, &st, &first_run(), None, &with_all);
    let said: Vec<String> = list.iter().map(Change::describe).collect();
    assert_eq!(
        said,
        [
            "Model: none to x.gguf",
            "Program: none to b11007",
            "Live traffic proxy: off to on",
            "Port: 8080 to 8081",
            "Context size: 4096 to 16384"
        ]
    );
    // a default that is worked out has no number to show
    let ollama = config_for("ollama", &[("ollama.ctx_size", i(16384))]);
    let list = changes(&r, &state("ollama", &[]), &first_run(), None, &ollama);
    assert_eq!(
        list.iter().map(Change::describe).collect::<Vec<_>>(),
        ["Context size: default to 16384"]
    );
}

// ---- what a person is told (WP4.17.3) ---------------------------------------------------------------

use quickai_core::profile::check::{
    accepted, export, human_bytes, open_text, review, BuildHere, EngineHere, ExportMeta, FileHere,
    Inventory, ModelsHere, NamedHere, Opened, Report,
};
use quickai_core::profile::notes::{Level, Note};
use quickai_core::profile::MadeOn;
use std::collections::BTreeSet;

const GB: u64 = 1024 * 1024 * 1024;

/// A computer: Linux, x86_64, 16 GB, with nothing installed or running unless a test says so.
fn here() -> Inventory {
    Inventory {
        app_version: VERSION.into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        ram_bytes: 16 * GB,
        ..Inventory::default()
    }
}

fn with_engine(mut inv: Inventory, id: &str, on_hand: EngineHere) -> Inventory {
    inv.engines.insert(id.into(), on_hand);
    inv
}

fn file_text(engine: &str, body: &str) -> String {
    format!("schema = 1\nkind = \"profile\"\nid = \"given\"\nname = \"Given\"\nengine = \"{engine}\"\n{body}")
}

fn open(text: &str) -> Opened {
    let r = registry();
    open_text(&r, text, VERSION).unwrap_or_else(|n| panic!("{}: {:?}", n.id, n.holes))
}

fn review_of(opened: &Opened, inv: &Inventory, outside: bool) -> Report {
    review(
        &registry(),
        &opened.configuration,
        &opened.notes,
        inv,
        outside,
    )
}

fn ids(report: &Report) -> Vec<&'static str> {
    report.notes.iter().map(|n| n.id).collect()
}

fn text_of(note: &Note) -> String {
    note.text(registry().vocabulary())
}

#[test]
fn p3_a20_the_notes_that_stop_each_from_the_smallest_file_that_makes_them() {
    let r = registry();
    let cases: Vec<(&str, String, &str)> = vec![
        ("not TOML", "this is = not toml = at all".into(), "not_a_configuration"),
        ("an engine document", "schema = 1\nkind = \"engine\"\nid = \"x\"\n[engine]\nname = \"x\"\n".into(), "not_a_configuration"),
        ("no id", "schema = 1\nkind = \"profile\"\nengine = \"ollama\"\n".into(), "not_a_configuration"),
        ("no engine", "schema = 1\nkind = \"profile\"\nid = \"x\"\n".into(), "not_a_configuration"),
        ("over 256 KiB", format!("# {}\n{}", "x".repeat(300_000), file_text("ollama", "")), "not_a_configuration"),
        ("a newer schema", "schema = 2\nkind = \"profile\"\nid = \"x\"\nengine = \"ollama\"\n".into(), "too_new"),
        ("a newer application", "schema = 1\nkind = \"profile\"\nid = \"x\"\nrequires = { app = \">=0.3\" }\nengine = \"ollama\"\n".into(), "too_new"),
        ("a requirement that is not one", "schema = 1\nkind = \"profile\"\nid = \"x\"\nrequires = { app = \"soon\" }\nengine = \"ollama\"\n".into(), "not_a_configuration"),
    ];
    for (what, text, id) in cases {
        let stop = open_text(&r, &text, VERSION).expect_err(what);
        assert_eq!((stop.id, stop.level), (id, Level::Stops), "{what}");
        let said = text_of(&stop);
        assert!(!said.contains('{'), "{what}: {said}");
    }
    // the words of the two that matter most
    let newer = open_text(&r, "schema = 1\nkind = \"profile\"\nid = \"x\"\nrequires = { app = \">=0.3\" }\nengine = \"ollama\"\n", VERSION).unwrap_err();
    assert_eq!(
        text_of(&newer),
        "This configuration was made by a newer QuickAI: it needs QuickAI >=0.3, and this is 0.1.0. Update QuickAI to use it."
    );
    let big = open_text(&r, &"#".repeat(MAX), VERSION).unwrap_err();
    assert!(
        text_of(&big).contains("larger than 256 KiB"),
        "{}",
        text_of(&big)
    );

    // an engine this version has not, and a companion named as the engine, are read and then stop
    for engine in ["whispercpp", "openwebui"] {
        let opened = open(&file_text(engine, ""));
        let report = review_of(&opened, &here(), true);
        let stop = report.stop().expect(engine);
        assert_eq!(stop.id, "unknown_engine", "{engine}");
        assert_eq!(
            report.notes.len(),
            1,
            "{engine}: nothing else is said of a file that cannot be used"
        );
        assert_eq!(
            text_of(stop),
            format!("It is for {engine}, which this QuickAI does not have.")
        );
    }
}

const MAX: usize = quickai_core::profile::MAX_FILE_BYTES + 1;

#[test]
fn p3_a21_the_notes_that_ask_show_what_would_be_passed_on_in_the_documents_own_words() {
    let opened = open(&file_text(
        "ollama",
        "[values.ollama]\nport = 11600\nextra_env = \"OLLAMA_FLASH_ATTENTION=1\"\nuse_default_store = true\n[values.openwebui]\nenabled = true\nnetwork = \"online\"\nport = 3100\n",
    ));
    let report = review_of(&opened, &here(), true);
    let asked: Vec<(&str, &str)> = report
        .asks()
        .map(|n| (n.subject.as_deref().unwrap(), n.id))
        .collect();
    // in the order of the engine's form: a flag widens what a program may do; a text becomes an
    // environment line; a number is only taken
    assert_eq!(
        asked,
        [
            ("ollama.use_default_store", "wider_access"),
            ("ollama.extra_env", "free_text"),
            ("openwebui.network", "wider_access"),
        ]
    );
    // the sentence is the document's own where it has one, and the value is there to be shown in full
    let by_key = |key: &str| {
        report
            .asks()
            .find(|n| n.subject.as_deref() == Some(key))
            .unwrap()
    };
    assert_eq!(
        text_of(by_key("ollama.extra_env")),
        "Lines of environment, set for Ollama as they are written."
    );
    assert!(text_of(by_key("openwebui.network")).contains("may use the internet"));
    assert_eq!(
        opened.configuration.values["ollama.extra_env"],
        Value::Str("OLLAMA_FLASH_ATTENTION=1".into())
    );
    // the vocabulary's own words are for a field whose document says nothing
    let mut c = opened.configuration.clone();
    c.engine = "llamacpp".into();
    c.values.clear();
    c.values
        .insert("llamacpp.extra_args".into(), s("--threads 8"));
    let r = registry();
    let report = review(&r, &c, &[], &here(), true);
    let ask = report.asks().next().unwrap();
    assert_eq!(ask.id, "free_text");
    assert_eq!(
        text_of(ask),
        "Command-line words, passed to llama-server as they are written."
    );
    // a configuration of the person's own is not asked about
    let report = review(&r, &c, &[], &here(), false);
    assert_eq!(report.asks().count(), 0);
}

#[test]
fn p3_a22_what_is_not_ticked_is_not_stored_and_what_is_stored_asks_nothing() {
    let r = registry();
    let opened = open(&file_text(
        "ollama",
        "[values.ollama]\nport = 11600\nextra_env = \"OLLAMA_FLASH_ATTENTION=1\"\nuse_default_store = true\n[values.openwebui]\nnetwork = \"online\"\n",
    ));
    let c = &opened.configuration;
    // nothing is ticked at first
    let (nothing, _) = accepted(&r, c, &BTreeSet::new());
    assert_eq!(keys(&nothing), ["ollama.port"]);
    // a ticked item is stored, and an unticked one is not
    let ticked: BTreeSet<String> = ["ollama.extra_env".to_string()].into();
    let (some, _) = accepted(&r, c, &ticked);
    assert_eq!(keys(&some), ["ollama.extra_env", "ollama.port"]);
    // a key that is ticked and is not asked about changes nothing
    let ticked: BTreeSet<String> = ["ollama.port".to_string(), "nope".to_string()].into();
    assert_eq!(keys(&accepted(&r, c, &ticked).0), ["ollama.port"]);
    // everything ticked
    let all: BTreeSet<String> = report_keys(&review_of(&opened, &here(), true));
    let (everything, notes) = accepted(&r, c, &all);
    assert_eq!(everything.values, c.values);
    assert!(notes.is_empty());
    // and what was stored is the person's own: choosing it asks nothing
    let report = review(&r, &everything, &[], &here(), false);
    assert_eq!(report.asks().count(), 0);
    assert!(apply(&r, &everything).unwrap().notes.is_empty());
}

fn report_keys(report: &Report) -> BTreeSet<String> {
    report.asks().filter_map(|n| n.subject.clone()).collect()
}

#[test]
fn p3_a23_the_names_a_file_may_not_set_are_refused_even_when_ticked() {
    let r = registry();
    let lines = "LD_PRELOAD=/tmp/evil.so\\nld_library_path=/tmp\\nDYLD_INSERT_LIBRARIES=/x\\nPath=/bin\\nPYTHONPATH=/x\\nOLLAMA_HOST=0.0.0.0:1\\nOLLAMA_MODELS=/x\\nHOME=/tmp\\nOLLAMA_DEBUG=1";
    let opened = open(&file_text(
        "ollama",
        &format!("[values.ollama]\nextra_env = \"{lines}\"\n"),
    ));
    let report = review_of(&opened, &here(), true);
    let refused: Vec<&str> = report
        .of("refused_line")
        .iter()
        .filter_map(|n| n.hole("name"))
        .collect();
    assert_eq!(
        refused,
        [
            "LD_PRELOAD",
            "ld_library_path",
            "DYLD_INSERT_LIBRARIES",
            "Path",
            "PYTHONPATH",
            "OLLAMA_HOST",
            "OLLAMA_MODELS",
            "HOME"
        ]
    );
    assert!(report
        .of("refused_line")
        .iter()
        .all(|n| n.level == Level::Warns));
    // ticked, it is stored without them
    let ticked: BTreeSet<String> = ["ollama.extra_env".to_string()].into();
    let (stored, notes) = accepted(&r, &opened.configuration, &ticked);
    assert_eq!(stored.values["ollama.extra_env"], s("OLLAMA_DEBUG=1"));
    assert_eq!(notes.len(), 8);
    // and a file that was put in the folder by hand is held to the same at the moment of choosing
    let applied = apply(&r, &opened.configuration).unwrap();
    assert_eq!(applied.set["ollama.extra_env"], s("OLLAMA_DEBUG=1"));
    assert_eq!(
        applied
            .notes
            .iter()
            .filter(|n| n.id == "refused_line")
            .count(),
        8
    );
    // a vocabulary of the person's can add names, and cannot take any away
    let vocab =
        "schema = 1\nkind = \"vocab\"\nid = \"core\"\n[profile]\ndeny_env = [\"OLLAMA_DEBUG\"]\n";
    let r2 = common::shipped_registry(&[("/home/u/.config/quickai/vocab/core.toml", vocab)]);
    let applied = apply(&r2, &opened.configuration).unwrap();
    assert_eq!(applied.set["ollama.extra_env"], s(""));
    assert!(quickai_core::profile::notes::env_name_denied(
        "LD_PRELOAD",
        r2.vocabulary().deny_env()
    ));
}

#[test]
fn p3_a24_the_notes_that_warn_about_the_file() {
    let opened = open(&file_text(
        "llamacpp",
        "mystery = 1\n[values.llamacpp]\nextra_args = \"--lora /home/alice/adapter.gguf --threads 4\"\nctx_size = 99999999\nnonsense = 1\nmodels_dir = \"/home/alice/models\"\nn_gpu_layers = \"many\"\n[values.ollama]\nport = 11500\n",
    ));
    let report = review_of(&opened, &here(), true);
    let by = |id: &str| report.of(id);
    assert_eq!(by("names_a_path").len(), 1);
    assert_eq!(
        by("names_a_path")[0].hole("path"),
        Some("/home/alice/adapter.gguf")
    );
    assert_eq!(text_of(by("names_a_path")[0]), "Extra command-line arguments names /home/alice/adapter.gguf, a folder on the computer it was made on.");
    // a setting the engine has not, one it does not carry, and one of an engine that does not go with this one
    let unknown: Vec<&str> = by("unknown_setting")
        .iter()
        .filter_map(|n| n.hole("setting"))
        .collect();
    assert_eq!(unknown, ["llamacpp.nonsense", "ollama.port"]);
    assert_eq!(by("private_setting").len(), 1);
    assert_eq!(
        by("private_setting")[0].hole("setting"),
        Some("llamacpp.models_dir")
    );
    // values that do not fit say what is wrong, and the default is used
    let bad: Vec<(&str, &str)> = by("bad_value")
        .iter()
        .map(|n| (n.hole("field").unwrap(), n.hole("value").unwrap()))
        .collect();
    assert_eq!(bad, [("Context size", "99999999"), ("GPU layers", "many")]);
    assert!(text_of(by("bad_value")[0]).contains("the default is used"));
    assert!(!opened
        .configuration
        .values
        .contains_key("llamacpp.ctx_size"));
    // keys this version does not know are told as one note
    assert_eq!(by("unknown_keys").len(), 1);
    assert!(by("unknown_keys")[0]
        .hole("keys")
        .unwrap()
        .contains("mystery"));
    assert!(report.notes.iter().all(|n| n.level != Level::Stops));
}

#[test]
fn p3_a25_the_notes_that_warn_about_this_computer() {
    let opened = open(&file_text(
        "llamacpp",
        "[meta]\nmade_on = { os = \"windows\", arch = \"x86_64\", gpu = [\"nvidia\"], memory_gb = 64 }\n[runtime]\nbuild = \"b11007-cuda\"\n[[model]]\nfile = \"Big-Q4_K_M.gguf\"\nsize = 12884901888\n",
    ));
    let mut inv = here();
    inv.ram_bytes = 8 * GB;
    inv.engines.insert(
        "llamacpp".into(),
        EngineHere {
            installed: true,
            builds: vec![BuildHere {
                label: "b11007-cuda".into(),
                warning: Some(
                    "b11007-cuda is built for NVIDIA cards, and this computer has none.".into(),
                ),
            }],
            models: ModelsHere::Files(vec![FileHere {
                name: "Big-Q4_K_M.gguf".into(),
                size: 12_884_901_888,
            }]),
            ..EngineHere::default()
        },
    );
    let report = review_of(&opened, &inv, false);
    assert_eq!(ids(&report), ["other_system", "other_gpu", "memory"]);
    let said = |id: &str| text_of(report.of(id)[0]);
    assert_eq!(said("other_system"), "Made on Windows x86_64; this computer is Linux x86_64. Arguments and builds made there may not exist here.");
    assert_eq!(
        said("other_gpu"),
        "b11007-cuda is built for NVIDIA cards, and this computer has none."
    );
    assert_eq!(
        said("memory"),
        "Big-Q4_K_M.gguf needs about 14.4 GB of memory, and this computer has 8.0 GB."
    );
    // the same file on a computer like the one it came from has nothing to say about it
    let mut like = here();
    like.os = "windows".into();
    like.ram_bytes = 64 * GB;
    like.engines.insert(
        "llamacpp".into(),
        EngineHere {
            installed: true,
            builds: vec![BuildHere {
                label: "b11007-cuda".into(),
                warning: None,
            }],
            models: ModelsHere::Files(vec![FileHere {
                name: "Big-Q4_K_M.gguf".into(),
                size: 12_884_901_888,
            }]),
            ..EngineHere::default()
        },
    );
    assert_eq!(ids(&review_of(&opened, &like, false)), Vec::<&str>::new());
    // another architecture is another computer, and a memory that is not known judges nothing
    let mut arm = like.clone();
    arm.arch = "aarch64".into();
    arm.ram_bytes = 0;
    assert_eq!(ids(&review_of(&opened, &arm, false)), ["other_system"]);
    assert_eq!(human_bytes(6_140_000_000), "5.7 GB");
}

#[test]
fn p3_a26_the_notes_about_what_is_missing() {
    let r = registry();
    let name = |n: &str| NamedHere {
        name: n.into(),
        size: None,
    };
    let ollama = |running: bool, listed: Option<Vec<NamedHere>>| EngineHere {
        installed: true,
        running,
        models: ModelsHere::Named { listed },
        ..EngineHere::default()
    };
    let opened = open(&file_text(
        "ollama",
        "[[model]]\nollama = \"qwen3-vl:8b\"\nsize = 6140000000\n",
    ));
    // Ollama answers and does not have it: it can be got from there
    let report = review_of(
        &opened,
        &with_engine(here(), "ollama", ollama(true, Some(vec![name("other:1b")]))),
        false,
    );
    assert_eq!(ids(&report), ["model_to_get"]);
    assert_eq!(
        text_of(report.of("model_to_get")[0]),
        "Not here: qwen3-vl:8b (5.7 GB). It can be got from Ollama."
    );
    // it has it
    let report = review_of(
        &opened,
        &with_engine(
            here(),
            "ollama",
            ollama(true, Some(vec![name("qwen3-vl:8b")])),
        ),
        false,
    );
    assert!(ids(&report).is_empty());
    // Ollama is stopped: it cannot be told, and it is never said to be missing
    for listed in [None, Some(vec![name("other:1b")])] {
        let report = review_of(
            &opened,
            &with_engine(here(), "ollama", ollama(false, listed)),
            false,
        );
        assert_eq!(ids(&report), ["model_unseen"]);
        assert_eq!(report.notes[0].level, Level::Says);
        assert_eq!(text_of(&report.notes[0]), "Ollama is not running, so it cannot be told whether qwen3-vl:8b is here. Start Ollama to see.");
    }
    // a model of the recommended list is looked for by its file and its size
    let model = r
        .catalog()
        .for_engine("llamacpp")
        .find(|m| matches!(m.runs.get("llamacpp"), Some(quickai_core::registry::catalog::Run::Files(f)) if f.files.iter().any(|x| x.size.is_some())))
        .expect("a model with a known size");
    let quickai_core::registry::catalog::Run::Files(run) = &model.runs["llamacpp"] else {
        unreachable!()
    };
    let file = run.files.iter().find(|f| f.size.is_some()).unwrap();
    let base = file.path.rsplit('/').next().unwrap().to_string();
    let text = file_text(
        "llamacpp",
        &format!("[[model]]\ncatalog = \"{}\"\nfile = \"{base}\"\n", model.id),
    );
    let opened = open(&text);
    let llama = |files: Vec<FileHere>| EngineHere {
        installed: true,
        models: ModelsHere::Files(files),
        ..EngineHere::default()
    };
    let report = review_of(
        &opened,
        &with_engine(here(), "llamacpp", llama(vec![])),
        false,
    );
    assert_eq!(ids(&report), ["model_to_get"]);
    let said = text_of(report.of("model_to_get")[0]);
    assert!(
        said.contains(&base) && said.contains(&format!("huggingface.co/{}", run.repo)),
        "{said}"
    );
    assert!(review_of(
        &opened,
        &with_engine(
            here(),
            "llamacpp",
            llama(vec![FileHere {
                name: base.clone(),
                size: file.size.unwrap()
            }])
        ),
        false
    )
    .notes
    .is_empty());
    let wrong_size = vec![FileHere {
        name: base.clone(),
        size: file.size.unwrap() + 1,
    }];
    assert_eq!(
        ids(&review_of(
            &opened,
            &with_engine(here(), "llamacpp", llama(wrong_size)),
            false
        )),
        ["model_to_get"]
    );
    // a file that is only a name has no source to be got from
    let opened = open(&file_text(
        "llamacpp",
        "[[model]]\nfile = \"mine.gguf\"\nsize = 2800000000\n",
    ));
    let report = review_of(
        &opened,
        &with_engine(here(), "llamacpp", llama(vec![])),
        false,
    );
    assert_eq!(ids(&report), ["model_unknown"]);
    assert_eq!(
        text_of(&report.notes[0]),
        "mine.gguf (2.6 GB) is not here, and the file does not say where it came from."
    );
    // a build that is not installed says which are, or that none is
    let opened = open(&file_text("ollama", "[runtime]\nbuild = \"v0.40.0\"\n"));
    let builds = |labels: &[&str]| EngineHere {
        installed: true,
        builds: labels
            .iter()
            .map(|l| BuildHere {
                label: (*l).into(),
                warning: None,
            })
            .collect(),
        ..EngineHere::default()
    };
    let report = review_of(
        &opened,
        &with_engine(here(), "ollama", builds(&["v0.35.1", "v0.39.2"])),
        false,
    );
    assert_eq!(
        text_of(&report.notes[0]),
        "Made with v0.40.0, which is not installed here. The builds that are: v0.35.1, v0.39.2."
    );
    let report = review_of(&opened, &with_engine(here(), "ollama", builds(&[])), false);
    assert_eq!(ids(&report), ["build_none"]);
    assert!(review_of(
        &opened,
        &with_engine(here(), "ollama", builds(&["v0.40.0"])),
        false
    )
    .notes
    .is_empty());
    // Open WebUI switched on and not installed
    let opened = open(&file_text("ollama", "[values.openwebui]\nenabled = true\n"));
    let mut inv = here();
    inv.engines.insert(
        "openwebui".into(),
        EngineHere {
            installed: false,
            ..EngineHere::default()
        },
    );
    let report = review_of(&opened, &inv, false);
    assert_eq!(
        text_of(&report.notes[0]),
        "Open WebUI is switched on and is not installed here."
    );
    inv.engines.insert(
        "openwebui".into(),
        EngineHere {
            installed: true,
            ..EngineHere::default()
        },
    );
    assert!(review_of(&opened, &inv, false).notes.is_empty());
}

#[test]
fn p3_a27_the_notes_about_ports() {
    // a port that is set and is in use now
    let opened = open(&file_text("ollama", "[values.ollama]\nport = 11600\n"));
    let mut inv = here();
    inv.busy_ports.insert(11600);
    inv.busy_ports.insert(3000);
    let report = review_of(&opened, &inv, false);
    assert_eq!(ids(&report), ["port_taken"]);
    assert_eq!(
        text_of(&report.notes[0]),
        "Ollama Port: port 11600 is in use on this computer now."
    );
    // a port that is left to its default is the engine's own business: the person's own Ollama is on 11434
    let opened = open(&file_text("ollama", ""));
    inv.busy_ports.insert(11434);
    assert!(ids(&review_of(&opened, &inv, false)).is_empty());
    // two ports in the file that are the same, however each came to be
    let opened = open(&file_text("ollama", "[values.ollama]\nport = 3000\n"));
    let report = review_of(&opened, &here(), false);
    assert_eq!(ids(&report), ["ports_clash"]);
    assert_eq!(
        text_of(&report.notes[0]),
        "Ollama Port, Open WebUI Port are all set to port 3000."
    );
    let opened = open(&file_text(
        "ollama",
        "[values.ollama]\nport = 3100\n[values.openwebui]\nport = 3100\n",
    ));
    assert_eq!(ids(&review_of(&opened, &here(), false)), ["ports_clash"]);
    let opened = open(&file_text(
        "ollama",
        "[values.ollama]\nport = 3100\n[values.openwebui]\nport = 3200\n",
    ));
    assert!(ids(&review_of(&opened, &here(), false)).is_empty());
}

#[test]
fn p3_a28_the_name_of_an_import_that_is_in_use_asks_and_what_will_change_is_said() {
    let r = registry();
    let taken = Note::new("name_taken", &[("name", "Vision chat".to_string())]);
    assert_eq!((taken.id, taken.level), ("name_taken", Level::Asks));
    assert_eq!(
        taken.text(r.vocabulary()),
        "There is already a configuration called Vision chat."
    );
    // the list of what changes: six, and how many more
    let st = as_stored(state("llamacpp", &[]));
    let mut many = config_for(
        "llamacpp",
        &[
            ("llamacpp.port", i(8081)),
            ("llamacpp.ctx_size", i(16384)),
            ("llamacpp.n_gpu_layers", i(10)),
            ("llamacpp.extra_args", s("--a")),
        ],
    );
    many.traffic_proxy = Some(true);
    many.runtime.build = Some("b1".into());
    many.models.push(quickai_core::profile::ModelPick {
        role: "main".into(),
        source: ModelSource::File {
            name: "m.gguf".into(),
        },
        name: None,
        size: None,
        sha256: None,
        from: None,
    });
    let list = changes(&r, &st, &first_run(), None, &many);
    assert_eq!(list.len(), 7);
    let note = Report::will_change(&list).expect("changes");
    assert_eq!((note.id, note.level), ("will_change", Level::Says));
    let said = note.text(r.vocabulary());
    assert!(
        said.starts_with("These settings change: Model: none to m.gguf; Program: none to b1;"),
        "{said}"
    );
    assert!(said.ends_with("; and 1 more."), "{said}");
    assert!(Report::will_change(&[]).is_none());
}

#[test]
fn p3_a29_an_export_leaves_out_what_belongs_to_a_computer_or_a_person_and_says_so() {
    let r = registry();
    let mut st = as_stored(llama_state());
    st.values.insert(
        "llamacpp.extra_args".into(),
        s("--threads 8 --api-key hunter2 --ctx-size 2048"),
    );
    st.values
        .insert("llamacpp.last_model".into(), s("mine.gguf"));
    st.values
        .insert("llamacpp.models_dir".into(), s("/home/alice/models"));
    let captured = capture(&r, &st, &first_run(), Some(&weights()));
    let exported = export(
        &r,
        captured,
        "mine",
        "Mine",
        ExportMeta {
            note: Some("For you.".into()),
            made_with: "QuickAI 0.1.0".into(),
            made_on: MadeOn {
                os: Some("linux".into()),
                arch: Some("x86_64".into()),
                gpu: vec!["amd".into()],
                memory_gb: Some(32),
            },
        },
    );
    let text = exported
        .configuration
        .to_toml(r.engine_map(), VERSION, false);
    // secrets are not written, the rest of the line is
    assert!(text.contains("--threads 8 --ctx-size 2048"), "{text}");
    for secret in ["hunter2", "--api-key"] {
        assert!(!text.contains(secret), "{secret} in\n{text}");
    }
    // folders and programs are not written, and no value of a setting that is not carried
    for private in ["/home/alice", "/opt/a-tools", "models_dir", "bin_override"] {
        assert!(!text.contains(private), "{private} in\n{text}");
    }
    let said: Vec<String> = exported
        .notes
        .iter()
        .map(|n| n.text(r.vocabulary()))
        .collect();
    assert_eq!(
        said,
        [
            "Settings left out because they belong to this computer (a folder, a program): 2."
                .to_string(),
            "Lines left out because they look like secrets (a key, a token, a password): 1."
                .to_string(),
            "mine.gguf has no known source: the file says its name and size only.".to_string(),
        ]
    );
    // what the person said is in it, and the kind of computer
    assert!(text.contains("note = \"For you.\""), "{text}");
    assert!(
        text.contains(
            "made_on = { os = \"linux\", arch = \"x86_64\", gpu = [\"amd\"], memory_gb = 32 }"
        ),
        "{text}"
    );
    // a line of environment too
    let mut st = ollama_state();
    st.values.insert(
        "ollama.extra_env".into(),
        s("A=1\nHF_TOKEN=abc\nOLLAMA_API_KEY=k\nB=2"),
    );
    let exported = export(
        &r,
        capture(&r, &st, &first_run(), None),
        "o",
        "O",
        ExportMeta::default(),
    );
    assert_eq!(
        exported.configuration.values["ollama.extra_env"],
        s("A=1\nB=2")
    );
    // a text that was only secrets is not written at all
    st.values
        .insert("ollama.extra_env".into(), s("HF_TOKEN=abc"));
    let exported = export(
        &r,
        capture(&r, &st, &first_run(), None),
        "o",
        "O",
        ExportMeta::default(),
    );
    assert!(!exported
        .configuration
        .values
        .contains_key("ollama.extra_env"));
    // a text that names a folder says so to whoever is about to give it away
    st.values
        .insert("ollama.extra_env".into(), s("HF_HOME=/home/alice/hf"));
    let exported = export(
        &r,
        capture(&r, &st, &first_run(), None),
        "o",
        "O",
        ExportMeta::default(),
    );
    assert!(exported.notes.iter().any(|n| n.id == "names_a_path"));
}

#[test]
fn p3_a30_an_export_holds_no_name_of_the_person_the_computer_or_the_home() {
    let r = registry();
    let mut states = vec![as_stored(llama_state()), ollama_state(), comfy_state()];
    for st in &mut states {
        for (key, value) in st.values.clone() {
            if let Value::Str(text) = value {
                if text.starts_with('/') {
                    st.values.insert(
                        key,
                        s(&text.replace("/data/a", "/home/alice/workstation-7")),
                    );
                }
            }
        }
    }
    let meta = ExportMeta {
        made_with: "QuickAI 0.1.0".into(),
        made_on: MadeOn {
            os: Some("linux".into()),
            arch: Some("x86_64".into()),
            gpu: vec!["amd".into()],
            memory_gb: Some(32),
        },
        ..ExportMeta::default()
    };
    for st in states {
        let exported = export(
            &r,
            capture(&r, &st, &first_run(), Some(&weights())),
            "x",
            "X",
            meta.clone(),
        );
        let text = exported
            .configuration
            .to_toml(r.engine_map(), VERSION, false);
        for name in ["alice", "workstation", "/home/", "/data/"] {
            assert!(!text.contains(name), "{}: {name} in\n{text}", st.engine);
        }
    }
}

#[test]
fn p3_a31_the_sentences_of_the_notes_are_the_vocabularys_and_their_levels_are_the_codes() {
    let r = registry();
    // every note, with its level and its sentence, as shipped
    let mut lines = Vec::new();
    for spec in NOTES {
        let sentence = r.vocabulary().finding(spec.id).unwrap();
        lines.push(format!("{} {}: {sentence}", spec.id, spec.level.label()));
    }
    lines.sort();
    let got = lines.join("\n") + "\n";
    assert_eq!(got, NOTE_SENTENCES, "the sentences changed:\n{got}");
    // a document of the person's can word one again
    let patch = "schema = 1\nkind = \"vocab\"\nid = \"core\"\n[[finding]]\nid = \"too_new\"\ntext = \"Get a newer QuickAI ({needs}).\"\n";
    let r2 = common::shipped_registry(&[("/home/u/.config/quickai/vocab/core.toml", patch)]);
    let note = Note::new(
        "too_new",
        &[("needs", "QuickAI >=0.3".into()), ("this", "0.1.0".into())],
    );
    assert_eq!(
        note.text(r2.vocabulary()),
        "Get a newer QuickAI (QuickAI >=0.3)."
    );
    // and cannot make a question a statement: the level is not a key of a finding
    let patch = "schema = 1\nkind = \"vocab\"\nid = \"core\"\n[[finding]]\nid = \"free_text\"\nlevel = \"says\"\ntext = \"ok\"\n";
    let r3 = common::shipped_registry(&[("/home/u/.config/quickai/vocab/core.toml", patch)]);
    assert!(
        r3.diagnostics
            .iter()
            .any(|d| d.message.contains("finding[free_text].level")),
        "{:?}",
        r3.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
    );
    let note = Note::new(
        "free_text",
        &[("field", "F".into()), ("engine", "E".into())],
    );
    assert_eq!(note.level, Level::Asks);
    // a vocabulary with no sentence gives the id and the words, so that nothing is lost
    let bare = common::registry_of(&[]);
    assert_eq!(
        note.text(bare.vocabulary()),
        "free_text (field: F, engine: E)"
    );
}

const NOTE_SENTENCES: &str = "\
bad_value Warns: {field}: {value} is not accepted ({why}); the default is used.
build_missing Warns: Made with {build}, which is not installed here. The builds that are: {have}.
build_none Warns: Made with {build}, which is not installed here, and no build of this program is.
companion_address Warns: {companion} keeps the address of {engine} it was first started with ({first}); it would be given {now} now. Change it in {companion}'s own settings.
companion_missing Warns: {companion} is switched on and is not installed here.
free_text Asks: {field} for {engine}: this text is passed on as it is written.
left_out_private Says: Settings left out because they belong to this computer (a folder, a program): {count}.
left_out_secrets Says: Lines left out because they look like secrets (a key, a token, a password): {count}.
left_out_source Says: {model} has no known source: the file says its name and size only.
memory Warns: {model} needs about {need} of memory, and this computer has {have}.
model_to_get Warns: Not here: {model}. It can be got from {from}.
model_unknown Warns: {model} is not here, and the file does not say where it came from.
model_unseen Says: {engine} is not running, so it cannot be told whether {model} is here. Start {engine} to see.
name_taken Asks: There is already a configuration called {name}.
names_a_path Warns: {field} names {path}, a folder on the computer it was made on.
not_a_configuration Stops: This file is not a QuickAI configuration ({why}).
other_gpu Warns: {text}
other_system Warns: Made on {made}; this computer is {this}. Arguments and builds made there may not exist here.
port_taken Warns: {field}: port {port} is in use on this computer now.
ports_clash Warns: {fields} are all set to port {port}.
private_setting Warns: {setting} belongs to a computer and is not carried in a configuration; it is ignored.
refused_line Warns: {name} cannot be set from a configuration ({field}); the line is left out.
too_new Stops: This configuration was made by a newer QuickAI: it needs {needs}, and this is {this}. Update QuickAI to use it.
unknown_engine Stops: It is for {engine}, which this QuickAI does not have.
unknown_keys Warns: This QuickAI does not know {keys}; they are kept in the file and ignored.
unknown_setting Warns: {setting} is not a setting this QuickAI has; it is ignored.
wider_access Asks: {field} for {engine} is set to {value}, which widens what it may do.
will_change Says: These settings change: {list}.
";

#[test]
fn p3_a32_a_table_of_hostile_files_is_refused_or_cleaned_and_never_crashes_anything() {
    let r = registry();
    let big_name = "n".repeat(100_000);
    let many_values: String = (0..5000).map(|n| format!("k{n} = {n}\n")).collect();
    let many_models = "[[model]]\nfile = \"a.gguf\"\n".repeat(1000);
    let mut cases: Vec<(&str, String, bool)> = vec![
        ("arrays nested 100 000 deep", format!("{}a = {}{}\n", file_text("ollama", ""), "[".repeat(100_000), "]".repeat(100_000)), true),
        ("inline tables nested 20 000 deep", format!("{}a = {}1{}\n", file_text("ollama", ""), "{a=".repeat(20_000), "}".repeat(20_000)), true),
        ("more than 256 KiB", format!("{}# {}\n", file_text("ollama", ""), "x".repeat(300_000)), true),
        ("5000 settings", format!("{}[values.ollama]\n{many_values}", file_text("ollama", "")), true),
        ("a name of 100 000 characters", format!("schema = 1\nkind = \"profile\"\nid = \"x\"\nname = \"{big_name}\"\nengine = \"ollama\"\n"), true),
        ("a thousand models", format!("{}{many_models}", file_text("llamacpp", "")), true),
        ("a model name with a NUL", file_text("llamacpp", "[[model]]\nfile = \"a\\u0000b.gguf\"\n"), true),
        ("a build with a direction character", file_text("ollama", "[runtime]\nbuild = \"a\\u202Eb\"\n"), true),
        ("a model named by a path", file_text("llamacpp", "[[model]]\nfile = \"../../etc/passwd\"\n"), true),
        ("a model name with a space", file_text("ollama", "[[model]]\nollama = \"x y\"\n"), true),
        ("a number past the whole numbers", file_text("llamacpp", "[values.llamacpp]\nctx_size = 99999999999999999999\n"), true),
        ("a schema that wraps round to 1", "schema = 4294967297\nkind = \"profile\"\nid = \"x\"\nengine = \"ollama\"\n".to_string(), true),
        ("binary", "\u{0}\u{1}\u{2}\u{7f}\u{feff}\u{ffff}".to_string(), true),
        ("a key twice", file_text("ollama", "engine = \"llamacpp\"\n"), true),
        ("only a comment", "# nothing here\n".to_string(), true),
        ("empty", String::new(), true),
        // not refused: cleaned
        ("control and direction characters in a name", "schema = 1\nkind = \"profile\"\nid = \"x\"\nname = \"A\\u202EB\\u0007C\\u200FD\"\nengine = \"ollama\"\n".to_string(), false),
        ("the biggest whole number as a context", file_text("llamacpp", "[values.llamacpp]\nctx_size = 9223372036854775807\n"), false),
        ("ports that are none", file_text("ollama", "[values.ollama]\nport = 0\n[values.openwebui]\nport = -5\n"), false),
        ("a text of 5000 characters", file_text("llamacpp", &format!("[values.llamacpp]\nextra_args = \"{}\"\n", "a".repeat(5000))), false),
        ("a NUL in a line of environment", file_text("ollama", "[values.ollama]\nextra_env = \"A=1\\u0000B\"\n"), false),
        ("settings with names that are not names", file_text("ollama", "[values.ollama]\n\"../x\" = 1\n\"\" = 2\n[values.\"\"]\nx = 1\n"), false),
        ("a note of a million characters in a small file", file_text("ollama", &format!("[meta]\nnote = \"{}\"\n", "n".repeat(200_000))), false),
    ];
    // every case is read and, when it can be, reviewed
    for (what, text, refused) in cases.drain(..) {
        let result = open_text(&r, &text, VERSION);
        match (&result, refused) {
            (Err(note), true) => assert_eq!(note.level, Level::Stops, "{what}"),
            (Ok(_), false) => {}
            (Err(note), false) => panic!("{what}: refused, {}", text_of(note)),
            (Ok(_), true) => panic!("{what}: was taken"),
        }
        if let Ok(opened) = result {
            let report = review(&r, &opened.configuration, &opened.notes, &here(), true);
            assert!(report.stop().is_none(), "{what}");
        }
    }
    // what was cleaned is clean
    let name = open("schema = 1\nkind = \"profile\"\nid = \"x\"\nname = \"A\\u202EB\\u0007C\\u200FD\"\nengine = \"ollama\"\n");
    assert_eq!(name.configuration.name, "ABCD");
    let ctx = open(&file_text(
        "llamacpp",
        "[values.llamacpp]\nctx_size = 9223372036854775807\n",
    ));
    assert!(ctx.configuration.values.is_empty() && ctx.notes.iter().any(|n| n.id == "bad_value"));
    let ports = open(&file_text(
        "ollama",
        "[values.ollama]\nport = 0\n[values.openwebui]\nport = -5\n",
    ));
    assert_eq!(
        ports.notes.iter().filter(|n| n.id == "bad_value").count(),
        2
    );
    let long = open(&file_text(
        "llamacpp",
        &format!("[values.llamacpp]\nextra_args = \"{}\"\n", "a".repeat(5000)),
    ));
    assert!(long.configuration.values.is_empty() && long.notes.iter().any(|n| n.id == "bad_value"));
    let note = open(&file_text(
        "ollama",
        &format!("[meta]\nnote = \"{}\"\n", "n".repeat(200_000)),
    ));
    assert_eq!(
        note.configuration
            .meta
            .note
            .as_ref()
            .unwrap()
            .chars()
            .count(),
        1000
    );
    let nul = open(&file_text(
        "ollama",
        "[values.ollama]\nextra_env = \"A=1\\u0000B\"\n",
    ));
    assert!(nul.configuration.values.is_empty());
    let odd = open(&file_text(
        "ollama",
        "[values.ollama]\n\"../x\" = 1\n\"\" = 2\n[values.\"\"]\nx = 1\n",
    ));
    assert_eq!(
        odd.notes
            .iter()
            .filter(|n| n.id == "unknown_setting")
            .count(),
        3
    );
}

#[test]
fn p3_a34_the_notes_are_the_same_for_the_same_file_and_the_same_computer() {
    let opened = open(&file_text(
        "llamacpp",
        "mystery = 1\n[meta]\nmade_on = { os = \"windows\", arch = \"x86_64\" }\n[runtime]\nbuild = \"b1\"\n[[model]]\nfile = \"m.gguf\"\n[values.llamacpp]\nextra_args = \"--lora /home/a/x.gguf\"\nctx_size = 99999999\nnonsense = 1\n",
    ));
    let mut inv = here();
    inv.engines.insert(
        "llamacpp".into(),
        EngineHere {
            installed: true,
            builds: vec![BuildHere {
                label: "b2".into(),
                warning: None,
            }],
            models: ModelsHere::Files(vec![]),
            ..EngineHere::default()
        },
    );
    let first = review_of(&opened, &inv, true);
    let second = review_of(&opened, &inv, true);
    let said = |r: &Report| {
        r.notes
            .iter()
            .map(|n| (n.id, text_of(n)))
            .collect::<Vec<_>>()
    };
    assert_eq!(said(&first), said(&second));
    // from the one that matters most to the one that matters least, and in the order of the file inside a level
    let levels: Vec<Level> = first.notes.iter().map(|n| n.level).collect();
    let mut sorted = levels.clone();
    sorted.sort();
    assert_eq!(levels, sorted);
    assert_eq!(first.notes[0].level, Level::Asks);
    assert!(levels.contains(&Level::Warns));
    // the order in which the computer's facts were put together does not matter
    let mut other = Inventory {
        engines: BTreeMap::new(),
        ..inv.clone()
    };
    other.engines.insert("zzz".into(), EngineHere::default());
    other
        .engines
        .insert("llamacpp".into(), inv.engines["llamacpp"].clone());
    assert_eq!(said(&first), said(&review_of(&opened, &other, true)));
}

// ---- a fifth engine that is only a document (A10) -------------------------------------------------

const ECHO: &str = r#"schema = 1
kind = "engine"
id = "echo"
requires = { primitives = ["spawn@1", "locate.setting@1"] }

[engine]
name = "Echo"
order = 50

[provides]
accepts = { refs = ["echo"] }

[locate]
strategies = [ { use = "setting", field = "bin" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = []

[[field]]
id = "port"
type = "port"
default = 9100
bind.port = "public"

[[field]]
id = "threads"
type = "int"
range = [1, 64]
default = 4

[[field]]
id = "voice"
type = "choice"
options = ["a", "b"]
default = "a"

[[field]]
id = "flags"
type = "text"
default = ""
share_note = "Words for Echo, as they are written."
bind.arg_split = true

[[field]]
id = "shout"
type = "bool"
default = false
share = "review"
share_note = "Echo shouts."

[[field]]
id = "cache"
type = "dir"

[[field]]
id = "bin"
type = "runtime_path"

[[field]]
id = "last_model"
type = "hidden"
"#;

const ECHO_PAGE: &str = r#"schema = 1
kind = "engine"
id = "echo-page"
requires = { primitives = ["spawn@1", "locate.setting@1"] }

[engine]
name = "Echo page"
companion_of = "echo"

[locate]
strategies = [ { use = "setting", field = "bin" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = []

[[field]]
id = "enabled"
type = "bool"
default = false

[[field]]
id = "port"
type = "port"
default = 9101
bind.port = "public"

[[field]]
id = "bin"
type = "runtime_path"
"#;

#[test]
fn p3_a10_an_engine_that_is_only_a_document_takes_part_in_configurations() {
    let dir = "/home/u/.config/quickai/engines";
    let echo_file = format!("{dir}/echo.toml");
    let page_file = format!("{dir}/echo-page.toml");
    let r =
        common::shipped_registry(&[(echo_file.as_str(), ECHO), (page_file.as_str(), ECHO_PAGE)]);
    assert!(
        errors(&r).is_empty(),
        "{:?}",
        r.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
    );
    assert!(r.selectable_engine("echo").is_some() && r.selectable_engine("echo-page").is_none());

    // its shares are what its document says, and by type where it says nothing
    let share = |field: &str| {
        r.engine("echo")
            .unwrap()
            .doc
            .fields
            .iter()
            .find(|f| f.id == field)
            .unwrap()
            .share()
    };
    assert_eq!(share("threads"), Some(Share::Yes));
    assert_eq!(share("flags"), Some(Share::Review));
    assert_eq!(share("shout"), Some(Share::Review));
    assert_eq!(share("cache"), Some(Share::Never));
    assert_eq!(share("last_model"), None);

    // captured from the working state
    let st = state(
        "echo",
        &[
            ("echo.port", i(9100)),
            ("echo.threads", i(8)),
            ("echo.voice", s("b")),
            ("echo.flags", s("--loud --token abc")),
            ("echo.shout", b(true)),
            ("echo.cache", s("/home/alice/echo")),
            ("echo.last_model", s("hello")),
            ("echo-page.enabled", b(true)),
        ],
    );
    let got = capture(&r, &st, &first_run(), None);
    assert_eq!(
        keys(&got.configuration),
        [
            "echo-page.enabled",
            "echo.flags",
            "echo.shout",
            "echo.threads",
            "echo.voice"
        ]
    );
    assert_eq!(
        got.configuration.models[0].source,
        ModelSource::Named {
            kind: "echo".into(),
            name: "hello".into()
        }
    );
    assert_eq!(
        got.left_out,
        vec![LeftOut::Private {
            key: "echo.cache".into()
        }]
    );

    // written out for someone else: the secret is left out, the folder is not there
    let exported = export(
        &r,
        got,
        "echoes",
        "Echoes",
        ExportMeta {
            made_with: "QuickAI 0.1.0".into(),
            ..ExportMeta::default()
        },
    );
    let text = exported
        .configuration
        .to_toml(r.engine_map(), VERSION, false);
    assert!(text.contains("flags = \"--loud\""), "{text}");
    assert!(!text.contains("abc") && !text.contains("alice"), "{text}");

    // read in again, on a computer where the page is not installed
    let opened = open_text(&r, &text, VERSION).unwrap_or_else(|n| panic!("{}", text_of(&n)));
    let mut inv = here();
    inv.engines.insert(
        "echo-page".into(),
        EngineHere {
            installed: false,
            ..EngineHere::default()
        },
    );
    let report = review(&r, &opened.configuration, &opened.notes, &inv, true);
    let asked: Vec<(&str, String)> = report
        .asks()
        .map(|n| (n.subject.as_deref().unwrap(), n.text(r.vocabulary())))
        .collect();
    assert_eq!(
        asked,
        [
            (
                "echo.flags",
                "Words for Echo, as they are written.".to_string()
            ),
            ("echo.shout", "Echo shouts.".to_string()),
        ]
    );
    assert_eq!(report.of("companion_missing").len(), 1);
    assert_eq!(
        text_of(report.of("companion_missing")[0]),
        "Echo page is switched on and is not installed here."
    );

    // taken with a tick, applied, and the state is what it was
    let ticked: BTreeSet<String> = report_keys(&report);
    let (stored, _) = accepted(&r, &opened.configuration, &ticked);
    let applied = apply(&r, &stored).unwrap();
    assert!(applied.notes.is_empty());
    let after = WorkingState::default().after(&r, &applied, Some("hello"), None);
    assert_eq!(after.engine, "echo");
    assert_eq!(after.values["echo.threads"], i(8));
    assert_eq!(after.values["echo-page.enabled"], b(true));
    assert_eq!(after.values["echo.last_model"], s("hello"));
    let again = capture(&r, &after, &first_run(), None).configuration;
    assert!(again.same_run(&stored));
    // and the plan of it is made with no line of code that has heard of it
    let mut selection = Selection::new("echo");
    selection.values = after.values.clone();
    let facts = HostFacts {
        os: "linux".into(),
        binary: Some("/opt/echo".into()),
        ..Default::default()
    };
    let planned = plan(&r, &selection, &facts).expect("a plan");
    assert_eq!(planned.public_port, Some(9100));
}

#[test]
fn p3_a63_a_file_that_remembers_where_it_came_from_can_be_got_from_there() {
    let r = registry();
    // the application remembers where a file was fetched from: it is in the file, as a hint
    let st = as_stored(state(
        "llamacpp",
        &[("llamacpp.last_model", s("Mine-Q4_K_M.gguf"))],
    ));
    let facts = ModelFacts {
        size: Some(2_800_000_000),
        source: Some("huggingface:acme/Mine-GGUF".into()),
        ..ModelFacts::default()
    };
    let got = capture(&r, &st, &first_run(), Some(&facts));
    assert_eq!(
        got.configuration.models[0].from.as_deref(),
        Some("huggingface:acme/Mine-GGUF")
    );
    assert!(
        !got.left_out
            .iter()
            .any(|l| matches!(l, LeftOut::ModelSource { .. })),
        "its source is known"
    );
    let exported = export(&r, got, "mine", "Mine", ExportMeta::default());
    let text = exported
        .configuration
        .to_toml(r.engine_map(), VERSION, false);
    assert!(
        text.contains("from = \"huggingface:acme/Mine-GGUF\""),
        "{text}"
    );
    assert!(!exported.notes.iter().any(|n| n.id == "left_out_source"));
    // read in again, on a computer that has not got the file: it says where to get it, and does not fetch it
    let opened = open(&text);
    assert_eq!(
        opened.configuration.models[0].from.as_deref(),
        Some("huggingface:acme/Mine-GGUF")
    );
    let nothing = EngineHere {
        installed: true,
        models: ModelsHere::Files(vec![]),
        ..EngineHere::default()
    };
    let report = review_of(&opened, &with_engine(here(), "llamacpp", nothing), false);
    assert_eq!(ids(&report), ["model_to_get"]);
    assert_eq!(
        text_of(&report.notes[0]),
        "Not here: Mine-Q4_K_M.gguf (2.6 GB). It can be got from huggingface.co/acme/Mine-GGUF."
    );
    // a file whose source is not known says so, as it did
    let got = capture(&r, &st, &first_run(), Some(&weights()));
    assert!(got
        .left_out
        .iter()
        .any(|l| matches!(l, LeftOut::ModelSource { .. })));
    assert!(got.configuration.models[0].from.is_none());
    // a place that is not a kind of place and a name is refused
    for bad in [
        "huggingface:owner",
        "huggingface:../x",
        "https://evil.example/x",
        "owner/repo",
        ":a/b",
        "huggingface:a/b/c",
        "Hugging:a/b",
    ] {
        let doc = format!("schema = 1\nkind = \"profile\"\nid = \"x\"\nengine = \"llamacpp\"\n[[model]]\nfile = \"m.gguf\"\nfrom = \"{bad}\"\n");
        let refused = open_text(&r, &doc, VERSION);
        assert!(refused.is_err(), "{bad} was taken");
    }
}

#[test]
fn p3_a65_a_companion_contributes_only_what_quickai_sets_at_every_start_and_the_code_names_none() {
    let r = registry();
    let mut st = ollama_state();
    st.values.insert("openwebui.port".into(), i(3555));
    st.values
        .insert("openwebui.first_ollama".into(), s("http://127.0.0.1:11434"));
    let got = capture(&r, &st, &first_run(), None);
    let c = &got.configuration;
    // its box, its port and its internet access travel; nothing else of it does
    for key in ["openwebui.enabled", "openwebui.port", "openwebui.network"] {
        assert!(c.values.contains_key(key), "{key}: {:?}", keys(c));
    }
    for never in [
        "openwebui.extra_env",
        "openwebui.python",
        "openwebui.bin_override",
        "openwebui.first_ollama",
    ] {
        assert!(!c.values.contains_key(never), "{never}");
    }
    let text = c.to_toml(r.engine_map(), VERSION, false);
    for never in [
        "WEBUI_NAME",
        "/usr/bin/python3.12",
        "11434\"",
        "first_ollama",
    ] {
        assert!(!text.contains(never), "{never} in\n{text}");
    }
    // the document says why its environment does not travel
    let note = r
        .engine("openwebui")
        .unwrap()
        .doc
        .fields
        .iter()
        .find(|f| f.id == "extra_env")
        .and_then(|f| f.share_note.clone())
        .unwrap();
    assert!(note.contains("database"), "{note}");
    // a file that carries them anyway is told and they are ignored
    let doc = file_text(
        "ollama",
        "[values.openwebui]\nenabled = true\nextra_env = \"WEBUI_NAME=x\"\npython = \"/usr/bin/python3\"\nfirst_ollama = \"http://127.0.0.1:1\"\n",
    );
    let opened = open(&doc);
    assert_eq!(keys(&opened.configuration), ["openwebui.enabled"]);
    assert_eq!(
        opened
            .notes
            .iter()
            .filter(|n| n.id == "private_setting")
            .count(),
        3
    );

    // and no line of the code that does this names an engine or a program that goes with one
    for file in ["mod.rs", "check.rs", "notes.rs", "screen.rs", "state.rs"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/profile")
            .join(file);
        let source = std::fs::read_to_string(&path).unwrap();
        let code = source.split("#[cfg(test)]").next().unwrap();
        for (n, line) in code.lines().enumerate() {
            let line = line.split("//").next().unwrap_or("");
            for word in [
                "llama",
                "ollama",
                "comfy",
                "openwebui",
                "open-webui",
                "Open WebUI",
            ] {
                assert!(
                    !line.to_lowercase().contains(&word.to_lowercase()),
                    "{file}:{}: `{word}` in {line}",
                    n + 1
                );
            }
        }
    }
}

#[test]
fn p3_a66_a_program_that_goes_with_the_engine_keeps_its_first_address_and_the_notes_say_when_it_would_be_another(
) {
    let r = registry();
    let mut inv = here();
    inv.engines.insert(
        "openwebui".into(),
        EngineHere {
            installed: true,
            ..EngineHere::default()
        },
    );
    // nothing remembered: nothing to say
    let opened = open(&file_text(
        "ollama",
        "[values.ollama]\nport = 11500\n[values.openwebui]\nenabled = true\n",
    ));
    assert!(review_of(&opened, &inv, false)
        .notes
        .iter()
        .all(|n| n.id != "companion_address"));
    // it was first started with the engine on 11434, and the configuration puts it on 11500
    inv.companion_first
        .insert("openwebui".into(), "http://127.0.0.1:11434".into());
    let report = review_of(&opened, &inv, false);
    let note = report.of("companion_address");
    assert_eq!(note.len(), 1, "{:?}", ids(&report));
    assert_eq!(note[0].level, Level::Warns);
    assert_eq!(
        text_of(note[0]),
        "Open WebUI keeps the address of Ollama it was first started with (http://127.0.0.1:11434); it would be given http://127.0.0.1:11500 now. Change it in Open WebUI's own settings."
    );
    // the same address, or the program not wanted: nothing
    let same = open(&file_text("ollama", "[values.openwebui]\nenabled = true\n"));
    assert!(review_of(&same, &inv, false)
        .notes
        .iter()
        .all(|n| n.id != "companion_address"));
    let off = open(&file_text("ollama", "[values.ollama]\nport = 11500\n"));
    assert!(review_of(&off, &inv, false)
        .notes
        .iter()
        .all(|n| n.id != "companion_address"));
    // the planner and the note agree on what the address is
    assert_eq!(
        quickai_core::plan::parent_url(11500),
        "http://127.0.0.1:11500"
    );
    let _ = r;

    // the program's document says which field of its own keeps the address, and is held to it
    assert_eq!(
        common::shipped_registry(&[])
            .engine("openwebui")
            .and_then(|e| e.doc.engine.first_address_field.clone())
            .as_deref(),
        Some("first_ollama")
    );
    let patched = |engine: &str, text: &str| {
        let file = format!("/home/u/.config/quickai/engines/{engine}.toml");
        common::shipped_registry(&[(file.as_str(), text)])
    };
    let missing = patched(
        "openwebui",
        "schema = 1\nkind = \"engine\"\nid = \"openwebui\"\n[engine]\nfirst_address_field = \"nonesuch\"\n",
    );
    assert!(
        errors(&missing).iter().any(|d| d
            .to_string()
            .contains("`nonesuch` is not a field of this document")),
        "{:?}",
        missing.diagnostics
    );
    let visible = patched(
        "openwebui",
        "schema = 1\nkind = \"engine\"\nid = \"openwebui\"\n[engine]\nfirst_address_field = \"port\"\n",
    );
    assert!(
        errors(&visible)
            .iter()
            .any(|d| d.to_string().contains("`port` is not a hidden field")),
        "{:?}",
        visible.diagnostics
    );
    let plain = patched(
        "ollama",
        "schema = 1\nkind = \"engine\"\nid = \"ollama\"\n[engine]\nfirst_address_field = \"ctx_size\"\n",
    );
    assert!(
        warnings(&plain)
            .iter()
            .any(|w| w.contains("`first_address_field` is for a companion")),
        "{:?}",
        plain.diagnostics
    );
}
