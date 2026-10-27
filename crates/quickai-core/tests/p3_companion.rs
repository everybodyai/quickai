//! Group T of the test plan (work package 4.16): a program that goes with an engine (Open WebUI beside
//! Ollama), checked where its document is written and planned: that it is not an engine to choose, that
//! the documents that make it are held to what they say, that its plan is for the engine it goes with, and
//! that everything it writes is in a root of its own (the person's rule: the engines are isolated from each
//! other completely). What needs processes, servers and a window is in the application
//! (`p3/companion.rs`).

mod common;

use common::{manifest, registry_of, shipped_registry, texts};
use quickai_core::diag::Severity;
use quickai_core::plan::{plan, HostFacts, Parent, Selection};
use quickai_core::value::Value as V;
use serde_json::Value;

// ---- the shipped document ---------------------------------------------------------------------------

const DATA: &str = "/data";

fn facts(os: &str, parent_port: u16) -> HostFacts {
    HostFacts {
        os: os.into(),
        arch: "x86_64".into(),
        ram_bytes: 16 * 1024 * 1024 * 1024,
        cpus: 8,
        binary: Some("/data/openwebui/0.11.4/open-webui".into()),
        data_dir: DATA.into(),
        config_dir: "/config".into(),
        home: "/home/u".into(),
        parent: Some(Parent {
            id: "ollama".into(),
            port: parent_port,
        }),
        ..Default::default()
    }
}

fn plan_json(selection: &Selection, facts: &HostFacts) -> Value {
    let registry = shipped_registry(&[]);
    match plan(&registry, selection, facts) {
        Ok(p) => serde_json::to_value(&p).unwrap(),
        Err(e) => panic!("the planner refused: {e:#?}"),
    }
}

fn spawn_of(plan: &Value) -> &Value {
    plan["units"][0]["start"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["how"] == "spawn")
        .expect("a spawn strategy")
}

fn args_of(spawn: &Value) -> Vec<String> {
    spawn["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["value"].as_str().unwrap().to_string())
        .collect()
}

fn env_of(spawn: &Value) -> std::collections::BTreeMap<String, String> {
    spawn["env"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["mode"]["op"] == "set")
        .map(|e| {
            (
                e["name"].as_str().unwrap().to_string(),
                e["mode"]["value"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn companion_plan(os: &str, parent_port: u16, set: &[(&str, V)]) -> Value {
    let mut selection = Selection::new("openwebui");
    for (key, value) in set {
        selection
            .values
            .insert(format!("openwebui.{key}"), value.clone());
    }
    plan_json(&selection, &facts(os, parent_port))
}

#[test]
fn p3_t01_the_embedded_open_webui_document_is_the_example_document() {
    let example = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/examples/engines/openwebui.toml"),
    )
    .expect("docs/examples/engines/openwebui.toml");
    assert_eq!(
        manifest("engines/openwebui.toml"),
        example,
        "the document the application ships and the example in the docs are one document"
    );
}

#[test]
fn p3_t02_open_webui_is_a_companion_of_ollama_that_loads_clean_and_says_what_it_is() {
    let registry = shipped_registry(&[]);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let engine = registry.engine("openwebui").expect("an openwebui engine");
    assert_eq!(engine.doc.engine.name, "Open WebUI");
    assert_eq!(engine.doc.engine.companion_of.as_deref(), Some("ollama"));
    assert!(
        !engine.doc.engine.bundled,
        "a program of its own, under a licence that keeps its name, is never shipped"
    );
    assert_eq!(
        engine.doc.engine.license.as_deref(),
        Some("Open WebUI License")
    );
    assert!(
        engine.doc.engine.egress.is_empty(),
        "offline by default, and what it contacts online is the person's to ask for"
    );
    assert!(!engine.doc.provides.needs_model);
    assert!(engine.doc.provides.apis.is_empty(), "it is a page");
    assert_eq!(engine.doc.provides.default_port, Some(3000));
    assert_eq!(
        engine
            .doc
            .provides
            .web_ui
            .as_ref()
            .map(|w| (w.path.as_str(), w.label.as_str())),
        Some(("/", "Open Chat UI"))
    );
    // it is only ever started: there is no running one of someone else's to attach to
    assert_eq!(engine.launch.strategies.len(), 1);
}

#[test]
fn p3_t03_a_companion_is_not_an_engine_to_choose_and_is_found_by_the_engine_it_goes_with() {
    let registry = shipped_registry(&[]);
    let chosen: Vec<&str> = registry.selectable().map(|e| e.id.as_str()).collect();
    assert_eq!(chosen, ["llamacpp", "ollama", "comfyui"]);
    assert!(registry.selectable_engine("openwebui").is_none());
    assert!(registry.selectable_engine("ollama").is_some());
    assert!(
        registry.engine("openwebui").is_some(),
        "it is a document of an engine, found by its id like the others"
    );
    let with = |id: &str| -> Vec<String> {
        registry
            .companions_of(id)
            .map(|e| e.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(with("ollama"), ["openwebui"]);
    assert!(
        with("llamacpp").is_empty(),
        "llama.cpp has a page of its own"
    );
    assert!(with("comfyui").is_empty());
    assert!(with("openwebui").is_empty());
}

// ---- a companion that goes with nothing ----------------------------------------------------------------

fn plain(id: &str, extra: &str) -> (String, String) {
    (
        format!("embedded:engines/{id}.toml"),
        format!(
            r#"schema = 1
kind = "engine"
id = "{id}"
requires = {{ primitives = ["spawn@1"] }}

[engine]
name = "{id}"
{extra}

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "x"
"#
        ),
    )
}

#[test]
fn p3_t04_a_companion_of_nothing_of_itself_or_of_another_companion_is_reported_and_not_kept() {
    let docs = [
        plain("host", ""),
        plain("good", "companion_of = \"host\""),
        plain("lost", "companion_of = \"nosuch\""),
        plain("self", "companion_of = \"self\""),
        plain("second", "companion_of = \"good\""),
    ];
    let borrowed: Vec<(&str, &str)> = docs.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    let registry = registry_of(&borrowed);
    assert!(registry.engine("good").is_some());
    for id in ["lost", "self", "second"] {
        assert!(registry.engine(id).is_none(), "{id} was kept");
    }
    let errors = texts(&registry, Severity::Error);
    assert_eq!(errors.len(), 3, "{errors:#?}");
    let said = |id: &str| {
        errors
            .iter()
            .find(|e| e.contains(&format!("`{id}` is not used")))
    };
    assert!(
        said("lost").is_some_and(|e| e.contains("`nosuch`") && e.contains("companion_of")),
        "{errors:#?}"
    );
    assert!(
        said("self").is_some_and(|e| e.contains("itself")),
        "{errors:#?}"
    );
    assert!(
        said("second").is_some_and(|e| e.contains("itself a companion")),
        "{errors:#?}"
    );
}

// ---- its plan ------------------------------------------------------------------------------------------

#[test]
fn p3_t05_the_plan_starts_open_webui_on_its_own_port_beside_the_engine_it_goes_with() {
    let p = companion_plan("linux", 11500, &[]);
    assert_eq!(p["engine"], "openwebui");
    assert_eq!(p["public_port"], 3000);
    assert_eq!(p["gateway"], Value::Null, "nothing sits in front of a page");
    assert_eq!(p["model"], Value::Null);
    assert_eq!(p["units"][0]["needs_model"], false);
    assert_eq!(p["units"][0]["warm"], serde_json::json!([]));
    assert_eq!(p["units"][0]["unload"], serde_json::json!([]));
    let hows: Vec<&str> = p["units"][0]["start"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["how"].as_str().unwrap())
        .collect();
    assert_eq!(
        hows,
        ["spawn"],
        "there is no running Open WebUI to attach to"
    );
    let spawn = spawn_of(&p);
    assert_eq!(
        spawn["program"]["value"],
        "/data/openwebui/0.11.4/open-webui"
    );
    assert_eq!(
        args_of(spawn),
        ["serve", "--host", "127.0.0.1", "--port", "3000"],
        "on this computer only"
    );
    assert_eq!(spawn["group"], true);
    assert_eq!(p["units"][0]["stop"]["policy"], "graceful");
    assert_eq!(p["units"][0]["ready"]["path"], "/health");
    assert_eq!(p["units"][0]["ready"]["on_timeout"], "wait");
    // the engine it goes with, where its settings put it: the port is the engine's, not its own
    let env = env_of(spawn);
    assert_eq!(env["OLLAMA_BASE_URL"], "http://127.0.0.1:11500");
    assert_eq!(env["ENABLE_OLLAMA_API"], "true");
    assert_eq!(env["ENABLE_OPENAI_API"], "false");
    let other = companion_plan("linux", 21434, &[]);
    assert_eq!(
        env_of(spawn_of(&other))["OLLAMA_BASE_URL"],
        "http://127.0.0.1:21434"
    );
    // its own port is its own setting
    let moved = companion_plan("linux", 11500, &[("port", V::from(3999u16))]);
    assert_eq!(moved["public_port"], 3999);
    assert_eq!(
        args_of(spawn_of(&moved)),
        ["serve", "--host", "127.0.0.1", "--port", "3999"]
    );
}

#[test]
fn p3_t06_everything_it_writes_is_below_a_root_of_its_own_and_none_below_another_engines_or_the_persons(
) {
    let p = companion_plan("linux", 11434, &[]);
    let spawn = spawn_of(&p);
    let own = format!("{DATA}/openwebui");
    let below = |path: &str| path == own || path.starts_with(&format!("{own}/"));

    // where it starts, and the folders that are made for it
    let cwd = spawn["cwd"]
        .as_str()
        .expect("a folder of its own to start in");
    assert!(below(cwd), "{cwd}");
    let made: Vec<&str> = spawn["mkdir"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    assert!(
        !made.is_empty() && made.iter().all(|d| below(d)),
        "{made:?}"
    );

    // every place it is told to keep something in
    let env = env_of(spawn);
    for name in [
        "DATA_DIR",
        "STATIC_DIR",
        "XDG_CACHE_HOME",
        "HF_HOME",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TMPDIR",
        "TEMP",
        "TMP",
    ] {
        let value = env.get(name).unwrap_or_else(|| panic!("{name} is not set"));
        assert!(below(value), "{name}={value}");
    }
    // and nothing it is told reaches into another engine's root, or into the person's home
    for (name, value) in &env {
        for elsewhere in [
            format!("{DATA}/ollama"),
            format!("{DATA}/comfyui"),
            format!("{DATA}/llama-bin"),
            format!("{DATA}/models"),
            "/home/u".to_string(),
        ] {
            assert!(
                !value.contains(&elsewhere),
                "{name}={value} reaches into {elsewhere}"
            );
        }
        // nor the system's temporary folder
        assert!(
            value != "/tmp" && !value.starts_with("/tmp/") && !value.starts_with("/var/tmp"),
            "{name}={value} is in the system's temporary folder"
        );
    }
    // the variables of the person's own Python are put away
    assert_eq!(env["PYTHONPATH"], "");
    assert_eq!(env["PYTHONHOME"], "");
    assert_eq!(env["PYTHONNOUSERSITE"], "1");
}

#[test]
fn p3_t07_it_is_offline_by_default_and_online_takes_that_one_variable_away() {
    let offline = env_of(spawn_of(&companion_plan("linux", 11434, &[])));
    assert_eq!(offline["OFFLINE_MODE"], "true");
    let online = env_of(spawn_of(&companion_plan(
        "linux",
        11434,
        &[("network", V::from("online"))],
    )));
    assert!(!online.contains_key("OFFLINE_MODE"), "{online:#?}");
    let mut kept = offline.clone();
    kept.remove("OFFLINE_MODE");
    assert_eq!(kept, online, "nothing else depends on it");
}

#[test]
fn p3_t08_nothing_is_sent_unasked_whatever_the_network_setting_is() {
    for network in ["offline", "online"] {
        let env = env_of(spawn_of(&companion_plan(
            "linux",
            11434,
            &[("network", V::from(network))],
        )));
        assert_eq!(env["ANONYMIZED_TELEMETRY"], "false", "{network}");
        assert_eq!(env["DO_NOT_TRACK"], "true", "{network}");
        assert_eq!(env["SCARF_NO_ANALYTICS"], "true", "{network}");
        assert_eq!(
            env["ENABLE_VERSION_UPDATE_CHECK"], "false",
            "{network}: QuickAI installs the versions"
        );
        // its documents are read with the Ollama it goes with, not with a model it fetches at its start
        assert_eq!(env["RAG_EMBEDDING_ENGINE"], "ollama", "{network}");
    }
}

#[test]
fn p3_t09_its_page_may_be_reached_from_its_own_address_and_no_other() {
    let env = env_of(spawn_of(&companion_plan("linux", 11434, &[])));
    assert_eq!(
        env["CORS_ALLOW_ORIGIN"],
        "http://127.0.0.1:3000;http://localhost:3000"
    );
    let moved = env_of(spawn_of(&companion_plan(
        "linux",
        11434,
        &[("port", V::from(4100u16))],
    )));
    assert_eq!(
        moved["CORS_ALLOW_ORIGIN"],
        "http://127.0.0.1:4100;http://localhost:4100"
    );
    assert!(!env["CORS_ALLOW_ORIGIN"].contains('*'));
}

#[test]
fn p3_t10_a_person_cannot_lines_away_what_keeps_it_inside_its_folder() {
    let p = companion_plan(
        "linux",
        11434,
        &[(
            "extra_env",
            V::from(
                "DATA_DIR=/elsewhere\nHF_HOME=/home/u/.cache/huggingface\nOFFLINE_MODE=false\nPYTHONPATH=/home/u/src\nWEBUI_NAME=Mine\n",
            ),
        )],
    );
    let env = env_of(spawn_of(&p));
    assert_eq!(env["WEBUI_NAME"], "Mine", "what is not protected is theirs");
    assert_eq!(env["DATA_DIR"], "/data/openwebui/data");
    assert_eq!(env["HF_HOME"], "/data/openwebui/cache/huggingface");
    assert_eq!(
        env["OFFLINE_MODE"], "true",
        "the network setting decides it"
    );
    assert_eq!(env["PYTHONPATH"], "");
}

// ---- how it is installed ----------------------------------------------------------------------------------

#[test]
fn p3_t11_the_wheel_of_the_tested_release_is_pinned_and_the_install_asks_for_python_3_12_or_3_11() {
    use quickai_core::acquire;
    use quickai_core::doc::engine::{Assets, Feed, Install, Verify};
    let registry = shipped_registry(&[]);
    let engine = registry.engine("openwebui").unwrap();
    let acquire_doc = engine.doc.acquire.as_ref().expect("an [acquire] table");
    assert!(matches!(
        acquire_doc.feed,
        Some(Feed::Pypi { ref package, .. }) if package == "open-webui"
    ));
    assert!(matches!(acquire_doc.assets, Some(Assets::Wheel { .. })));
    let tag = acquire::tested_tag(engine).expect("a tested release");
    let wheel = format!("open_webui-{tag}-py3-none-any.whl");
    let pin = acquire::pin_for(engine, &wheel).expect("the tested release is pinned");
    assert_eq!(pin.len(), 64);
    assert!(pin.chars().all(|c| c.is_ascii_hexdigit()), "{pin}");
    assert!(
        acquire::pin_for(engine, "open_webui-0.0.1-py3-none-any.whl").is_none(),
        "only the tested release has one"
    );
    let Some(Install::PythonApp {
        entry,
        console_script,
        python,
        verify,
        ..
    }) = &acquire_doc.install
    else {
        panic!("a python_app install")
    };
    assert_eq!(entry, &None, "a wheel has no source to run a script of");
    assert_eq!(console_script.as_deref(), Some("open-webui"));
    assert_eq!(
        python.versions,
        ["3.12", "3.11"],
        "Open WebUI says `<3.13`: a newer Python is not looked for"
    );
    assert_eq!(*verify, Verify::Digest);
    // the label of a build is its release
    assert_eq!(acquire::install_label(engine, tag, &wheel), tag);
}

#[test]
fn p3_z44_open_webui_and_comfyui_pin_quickais_own_python_for_every_kind_of_computer_quickai_is_built_for(
) {
    // C181: the environments are made with a Python QuickAI downloads and checks, so that the computer's own is neither
    // needed nor touched; one Python, the same for both engines, so that one was checked
    use quickai_core::acquire;
    use quickai_core::doc::engine::Install;
    let registry = shipped_registry(&[]);
    let mut seen = Vec::new();
    for id in ["openwebui", "comfyui"] {
        let engine = registry.engine(id).unwrap();
        let Some(Install::PythonApp { python, .. }) = &engine.doc.acquire.as_ref().unwrap().install
        else {
            panic!("{id}: a python_app install")
        };
        let own = python
            .own
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: no own Python"));
        assert!(
            own.url.starts_with(
                "https://github.com/astral-sh/python-build-standalone/releases/download/"
            ),
            "{id}: {}",
            own.url
        );
        let wanted = [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ];
        assert_eq!(
            own.files.keys().map(String::as_str).collect::<Vec<_>>(),
            {
                let mut sorted = wanted.to_vec();
                sorted.sort();
                sorted
            },
            "{id}"
        );
        for (target, file) in &own.files {
            // the file is the build of that version for that computer, as the release names it, and its sum is a sha256
            assert!(
                file.file.starts_with(&format!("cpython-{}+", own.version))
                    && file
                        .file
                        .ends_with(&format!("-{target}-install_only.tar.gz")),
                "{id} {target}: {}",
                file.file
            );
            assert_eq!(file.sha256.len(), 64, "{id} {target}");
            assert!(file
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        }
        // a Python the person names must still be one the engine runs on: the own one is among them
        let short = own.version.rsplit_once('.').unwrap().0;
        assert!(
            python.versions.iter().any(|v| v == short),
            "{id}: {:?}",
            python.versions
        );
        assert_eq!(
            acquire::own_python_sentence(engine).unwrap(),
            format!(
                "Its Python is QuickAI's own ({}), downloaded once: nothing of this computer's Python is used.",
                own.version
            )
        );
        seen.push(own.clone());
    }
    assert_eq!(seen[0], seen[1], "the same Python for both");
}

#[test]
fn p3_t12_the_pip_steps_give_linux_the_pytorch_for_the_processor_and_every_machine_the_wheel() {
    use quickai_core::acquire;
    let registry = shipped_registry(&[]);
    let engine = registry.engine("openwebui").unwrap();
    let ids = |os: &str, vendors: &[&str]| -> Vec<String> {
        let mut f = facts(os, 11434);
        f.gpu_known = !vendors.is_empty();
        f.gpu_vendors = vendors.iter().map(|v| v.to_string()).collect();
        acquire::pip_steps(engine, &f)
            .iter()
            .map(|s| s.id.clone())
            .collect()
    };
    // whatever the card: Ollama does the models, and the one for NVIDIA cards is 3 GB of libraries
    for vendors in [&["nvidia"][..], &["amd"][..], &[][..]] {
        assert_eq!(
            ids("linux", vendors),
            ["torch_cpu", "open_webui"],
            "{vendors:?}"
        );
    }
    // elsewhere the index's own PyTorch is the one for the processor
    assert_eq!(ids("windows", &["nvidia"]), ["open_webui"]);
    assert_eq!(ids("macos", &["apple"]), ["open_webui"]);
    let steps = acquire::pip_steps(engine, &facts("linux", 11434));
    assert_eq!(
        steps[0].index.as_deref(),
        Some("https://download.pytorch.org/whl/cpu"),
        "from that index alone"
    );
    assert_eq!(steps[0].packages, ["torch"]);
    assert!(steps[1].wheel && steps[1].packages.is_empty() && steps[1].requirements.is_none());
    let sentence = acquire::install_summary(engine, &facts("linux", 11434)).expect("a sentence");
    assert!(
        sentence.contains("PyTorch for the processor")
            && sentence.contains("Open WebUI and what it needs")
            && sentence.contains("8GiB free"),
        "{sentence}"
    );
}

// ---- what a document may say to install one -----------------------------------------------------------------

/// A small companion document with the sections a wheel install needs, and the parts that change as given.
fn wheel_doc(feed: &str, assets: &str, install: &str) -> String {
    format!(
        r#"schema = 1
kind = "engine"
id = "wheelapp"
requires = {{ primitives = ["spawn@1", "probe.http@1", "feed.pypi@1", "feed.github_release@1", "install.python_app@1", "install.archive@1"] }}

[engine]
name = "Wheelapp"

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${{runtime.binary}}"

[launch.ready]
probe = {{ use = "http", path = "/", expect = 200 }}

[acquire]
root = "${{data}}/wheelapp"
feed = {feed}
assets = {assets}

{install}
"#
    )
}

const PYPI: &str = r#"{ use = "pypi", package = "wheel-app" }"#;
const WHEEL: &str = r#"{ by = "wheel" }"#;
const GOOD_INSTALL: &str = r#"[acquire.install]
use = "python_app"
binary = { name = "wheelapp" }
console_script = "wheel-app"
python = { versions = ["3.12"] }

[[acquire.install.pip]]
id = "app"
wheel = true
"#;

fn errors_of(text: &str) -> Vec<String> {
    let registry = registry_of(&[("embedded:engines/wheelapp.toml", text)]);
    texts(&registry, Severity::Error)
}

#[test]
fn p3_t13_a_document_for_a_wheel_that_is_right_loads_and_each_thing_that_is_wrong_is_said() {
    assert_eq!(
        errors_of(&wheel_doc(PYPI, WHEEL, GOOD_INSTALL)),
        Vec::<String>::new()
    );

    let said = |text: String, wanted: &str| {
        let errors = errors_of(&text);
        assert!(
            errors.iter().any(|e| e.contains(wanted)),
            "wanted `{wanted}` in {errors:#?}"
        );
    };
    // what the launcher runs: one of two things
    said(
        wheel_doc(
            PYPI,
            WHEEL,
            &GOOD_INSTALL.replace(
                "console_script = \"wheel-app\"",
                "console_script = \"wheel-app\"\nentry = \"main.py\"",
            ),
        ),
        "both say what the launcher runs",
    );
    said(
        wheel_doc(
            PYPI,
            WHEEL,
            &GOOD_INSTALL.replace("console_script = \"wheel-app\"\n", ""),
        ),
        "does not say what the launcher runs",
    );
    said(
        wheel_doc(
            PYPI,
            WHEEL,
            &GOOD_INSTALL.replace("wheel-app\"", "../wheel-app\""),
        ),
        "is not the name of a program",
    );
    // a feed of the index and the assets that go with it
    said(
        wheel_doc(PYPI, r#"{ by = "source" }"#, GOOD_INSTALL),
        "its `assets` is `by = \"wheel\"`",
    );
    said(
        wheel_doc(
            r#"{ use = "github_release", repo = "a/b" }"#,
            WHEEL,
            GOOD_INSTALL,
        ),
        "not a package index",
    );
    said(
        wheel_doc(
            r#"{ use = "pypi", package = "wheel app" }"#,
            WHEEL,
            GOOD_INSTALL,
        ),
        "is not the name of a package",
    );
    said(
        wheel_doc(
            r#"{ use = "pypi", package = "wheel-app", show = 0 }"#,
            WHEEL,
            GOOD_INSTALL,
        ),
        "`show` must be at least 1",
    );
    said(
        wheel_doc(
            r#"{ use = "pypi", package = "wheel-app", api = "ftp://x" }"#,
            WHEEL,
            GOOD_INSTALL,
        ),
        "is not an http or https address",
    );
    // a program of the environment is run by a build that is a wheel, and a wheel is installed by one step
    said(
        wheel_doc(
            r#"{ use = "github_release", repo = "a/b" }"#,
            r#"{ by = "source" }"#,
            GOOD_INSTALL,
        ),
        "run by a build that is a wheel",
    );
    said(
        wheel_doc(
            PYPI,
            WHEEL,
            &GOOD_INSTALL.replace("wheel = true", "packages = [\"torch\"]"),
        ),
        "exactly one step that has `wheel = true`, and this install has 0",
    );
    said(
        wheel_doc(
            PYPI,
            WHEEL,
            &format!("{GOOD_INSTALL}\n[[acquire.install.pip]]\nid = \"again\"\nwheel = true\n"),
        ),
        "and this install has 2",
    );
    said(
        wheel_doc(
            PYPI,
            WHEEL,
            &GOOD_INSTALL.replace("wheel = true", "wheel = true\npackages = [\"torch\"]"),
        ),
        "names no packages or requirements file",
    );
    said(
        wheel_doc(
            r#"{ use = "github_release", repo = "a/b" }"#,
            r#"{ by = "source" }"#,
            &GOOD_INSTALL.replace("console_script = \"wheel-app\"", "entry = \"main.py\""),
        ),
        "`wheel = true` installs the wheel the build is, and the build is not one",
    );
    // a wheel is not an archive to unpack
    said(
        wheel_doc(
            PYPI,
            WHEEL,
            "[acquire.install]\nuse = \"archive\"\nbinary = { name = \"x\" }\n",
        ),
        "a wheel is not an archive to unpack",
    );
}

// ---- cwd and the unit's `needs_model` ---------------------------------------------------------------------

#[test]
fn p3_t14_the_folder_a_program_starts_in_is_a_template_that_is_checked_and_a_plan_without_one_has_none(
) {
    let doc = |cwd: &str| {
        format!(
            r#"schema = 1
kind = "engine"
id = "demo"
requires = {{ primitives = ["spawn@1"] }}

[engine]
name = "Demo"

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "x"
{cwd}
"#
        )
    };
    let errors = errors_of_doc(&doc("cwd = \"${nosuch}/x\""));
    assert!(
        errors.iter().any(|e| e.contains("launch.spawn.cwd")),
        "{errors:#?}"
    );
    let registry = registry_of(&[(
        "embedded:engines/demo.toml",
        &doc("cwd = \"${data}/demo/home\""),
    )]);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let mut f = facts("linux", 11434);
    f.parent = None;
    let p = serde_json::to_value(plan(&registry, &Selection::new("demo"), &f).unwrap()).unwrap();
    assert_eq!(spawn_of(&p)["cwd"], "/data/demo/home");
    // the text of `plan --explain` says it too
    let text = quickai_core::plan::explain::render_explain(
        &plan(&registry, &Selection::new("demo"), &f).unwrap(),
    );
    assert!(text.contains("cwd   /data/demo/home"), "{text}");
}

fn errors_of_doc(text: &str) -> Vec<String> {
    let registry = registry_of(&[("embedded:engines/demo.toml", text)]);
    texts(&registry, Severity::Error)
}

#[test]
fn p3_t15_the_plans_of_the_other_engines_are_what_they_were_and_say_whether_they_need_a_model() {
    let mut f = facts("linux", 11434);
    f.parent = None;
    f.binary = Some("/bin/x".into());
    for (id, needs_model) in [("llamacpp", true), ("ollama", true), ("comfyui", false)] {
        let mut selection = Selection::new(id);
        if id == "llamacpp" {
            selection.model = Some(quickai_core::plan::ModelInfo {
                id: "m".into(),
                display: "m".into(),
                path: "/m.gguf".into(),
                size_bytes: 1,
            });
        }
        let registry = shipped_registry(&[]);
        let p = serde_json::to_value(plan(&registry, &selection, &f).unwrap()).unwrap();
        assert_eq!(p["units"][0]["needs_model"], needs_model, "{id}");
        let text = p.to_string();
        assert!(
            !text.contains("\"cwd\""),
            "{id}: a program that was given no folder starts where it always did: {text}"
        );
    }
}

// ---- the engines are isolated from each other, all of them ----------------------------------------------------
//
// The person's rule of 2026-10-10: "Always isolate llamacpp/comfyUI/ollama - completely". Found by running each real
// engine with an empty home (the audit of WP4.16): Ollama wrote its key pair to `~/.ollama` and the shader caches of the
// graphics driver to `~/.cache`, ComfyUI the compiler cache of ROCm to `~/.cache` and its compiled kernels to the
// system's temporary folder; llama.cpp (a CPU build) and Open WebUI wrote nothing. The documents of the two now give them a
// home and a temporary folder of their own.

fn engine_plan(id: &str, set: &[(&str, V)]) -> Value {
    let mut selection = Selection::new(id);
    for (key, value) in set {
        selection
            .values
            .insert(format!("{id}.{key}"), value.clone());
    }
    let mut f = facts("linux", 11434);
    f.parent = None;
    f.binary = Some(format!("/data/{id}/build/{id}"));
    plan_json(&selection, &f)
}

#[test]
fn p3_t40_ollama_keeps_its_identity_and_its_caches_in_a_home_of_its_own_unless_the_person_asked_for_its_own_store(
) {
    let own = engine_plan("ollama", &[]);
    let env = env_of(spawn_of(&own));
    assert_eq!(env["HOME"], "/data/ollama/home");
    assert_eq!(
        env["USERPROFILE"], "/data/ollama/home",
        "Windows says it otherwise"
    );
    for name in ["TMPDIR", "TEMP", "TMP"] {
        assert_eq!(env[name], "/data/ollama/cache/tmp", "{name}");
    }
    assert_eq!(
        env["OLLAMA_MODELS"], "/data/ollama/models",
        "its models are where they were"
    );
    assert_eq!(
        spawn_of(&own)["mkdir"],
        serde_json::json!(["/data/ollama/home", "/data/ollama/cache/tmp"])
    );
    // the person's own lines cannot undo it
    let lines = engine_plan(
        "ollama",
        &[(
            "extra_env",
            V::from("HOME=/home/u\nTMPDIR=/tmp\nOLLAMA_DEBUG=1\n"),
        )],
    );
    let env = env_of(spawn_of(&lines));
    assert_eq!(
        (env["HOME"].as_str(), env["TMPDIR"].as_str()),
        ("/data/ollama/home", "/data/ollama/cache/tmp")
    );
    assert_eq!(env["OLLAMA_DEBUG"], "1");

    // Ollama's own store is the home of the Ollama the person runs themselves: it is left as it is, and only the temporary
    // folder is still Ollama's here
    let theirs = engine_plan("ollama", &[("use_default_store", V::Bool(true))]);
    let env = env_of(spawn_of(&theirs));
    assert!(
        !env.contains_key("HOME")
            && !env.contains_key("USERPROFILE")
            && !env.contains_key("OLLAMA_MODELS"),
        "{env:#?}"
    );
    assert_eq!(env["TMPDIR"], "/data/ollama/cache/tmp");
}

#[test]
fn p3_t41_comfyui_keeps_its_caches_and_its_kernels_in_a_home_and_a_temporary_folder_of_its_own_and_puts_the_persons_python_away(
) {
    let p = engine_plan("comfyui", &[]);
    let spawn = spawn_of(&p);
    let env = env_of(spawn);
    assert_eq!(env["HOME"], "/data/comfyui/home");
    assert_eq!(env["USERPROFILE"], "/data/comfyui/home");
    for name in ["TMPDIR", "TEMP", "TMP"] {
        assert_eq!(env[name], "/data/comfyui/cache/tmp", "{name}");
    }
    assert_eq!(env["PYTHONNOUSERSITE"], "1");
    assert_eq!(env["PYTHONPATH"], "");
    assert_eq!(env["PYTHONHOME"], "");
    assert_eq!(env["PYTHONUNBUFFERED"], "1", "what it had");
    let made: Vec<&str> = spawn["mkdir"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    assert!(
        made.contains(&"/data/comfyui/home") && made.contains(&"/data/comfyui/cache/tmp"),
        "{made:?}"
    );
    // the person's own lines cannot undo it, and the one that was theirs to set still is
    let lines = engine_plan(
        "comfyui",
        &[("extra_env", V::from("HOME=/home/u\nPYTHONPATH=/home/u/src\nHSA_OVERRIDE_GFX_VERSION=10.3.0\nMY_FLAG=1\n"))],
    );
    let env = env_of(spawn_of(&lines));
    assert_eq!(env["HOME"], "/data/comfyui/home");
    assert_eq!(env["PYTHONPATH"], "");
    assert_eq!(env["MY_FLAG"], "1");
    assert_ne!(
        env.get("HSA_OVERRIDE_GFX_VERSION").map(String::as_str),
        Some("10.3.0"),
        "the document's own value of it is protected"
    );
}

#[test]
fn p3_t42_the_engines_that_are_downloaded_keep_everything_below_roots_of_their_own_and_no_root_is_in_another(
) {
    // every place each plan writes: the folder it starts in, the folders made, and the paths its environment names
    let writes = |plan: &Value| -> Vec<String> {
        let spawn = spawn_of(plan);
        let mut paths: Vec<String> = Vec::new();
        paths.extend(spawn["cwd"].as_str().map(str::to_string));
        paths.extend(
            spawn["mkdir"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d.as_str().unwrap().to_string()),
        );
        paths.extend(
            env_of(spawn)
                .into_iter()
                .filter(|(name, value)| {
                    value.starts_with('/')
                        && [
                            "HOME",
                            "USERPROFILE",
                            "TMPDIR",
                            "TEMP",
                            "TMP",
                            "XDG_CACHE_HOME",
                            "HF_HOME",
                            "DATA_DIR",
                            "STATIC_DIR",
                            "OLLAMA_MODELS",
                        ]
                        .contains(&name.as_str())
                })
                .map(|(_, value)| value),
        );
        paths
    };
    let mut roots = Vec::new();
    for (id, plan) in [
        ("ollama", engine_plan("ollama", &[])),
        ("comfyui", engine_plan("comfyui", &[])),
        ("openwebui", {
            let mut selection = Selection::new("openwebui");
            selection
                .values
                .insert("openwebui.network".into(), V::from("offline"));
            plan_json(&selection, &facts("linux", 11434))
        }),
    ] {
        let root = format!("{DATA}/{id}");
        let paths = writes(&plan);
        assert!(paths.len() >= 4, "{id}: {paths:?}");
        for path in &paths {
            assert!(
                path == &root || path.starts_with(&format!("{root}/")),
                "{id} writes {path}, which is not below {root}"
            );
        }
        roots.push(root);
    }
    // and no root is the same as, or inside, another
    for a in &roots {
        for b in &roots {
            if a != b {
                assert!(!b.starts_with(&format!("{a}/")), "{b} is inside {a}");
            }
        }
    }
}
