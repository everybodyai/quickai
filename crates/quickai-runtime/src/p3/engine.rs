//! Group E of the test plan: the Ollama engine as the application ships it, a document in
//! `manifests/engines/ollama.toml` (work package 3.7): that it loads clean, what the planner makes
//! of it, what the form shows, what the settings file keeps.
//!
//! The plan is read as JSON (`serde_json::to_value`), so these tests say what a plan *contains*
//! (plan decision C18: `units[0].start` is a list of `attach` and `spawn`, `warm` and `unload`
//! are lists of calls) without depending on the Rust types that hold it.

use std::collections::BTreeMap;
use std::path::Path;

use quickai_core::form::FormModel;
use quickai_core::plan::{plan, HostFacts, ModelInfo, Selection, TRAFFIC_PROXY_KEY};
use quickai_core::registry::{Engine, Registry};
use serde_json::{json, Value};

use crate::config::{build_registry, ConfigDirs};

const BINARY: &str = "/opt/quickai/ollama/bin/ollama";
const GB: u64 = 1024 * 1024 * 1024;

fn registry() -> Registry {
    build_registry(&ConfigDirs::default())
}

fn ollama(registry: &Registry) -> &Engine {
    registry
        .engine("ollama")
        .expect("there is no `ollama` engine among the embedded documents")
}

fn facts() -> HostFacts {
    HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        ram_bytes: 16 * GB,
        cpus: 8,
        binary: Some(BINARY.into()),
        data_dir: "/data".into(),
        config_dir: "/config".into(),
        home: "/home/u".into(),
        ..Default::default()
    }
}

fn qwen() -> ModelInfo {
    ModelInfo {
        id: "qwen3:4b".into(),
        display: "qwen3:4b".into(),
        path: String::new(),
        size_bytes: 2_500_000_000,
    }
}

fn selection() -> Selection {
    Selection::new("ollama").with_model(qwen())
}

fn plan_of(selection: &Selection, facts: &HostFacts) -> Value {
    let registry = registry();
    ollama(&registry);
    match plan(&registry, selection, facts) {
        Ok(p) => serde_json::to_value(&p).unwrap(),
        Err(e) => panic!("the planner refused: {e:#?}"),
    }
}

fn strategy<'a>(plan: &'a Value, how: &str) -> Option<&'a Value> {
    plan["units"][0]["start"]
        .as_array()?
        .iter()
        .find(|s| s["how"] == how)
}

fn how_list(plan: &Value) -> Vec<String> {
    plan["units"][0]["start"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| s["how"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The variables the spawn sets, by name.
fn env_of(spawn: &Value) -> BTreeMap<String, String> {
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

// ---- the document ----------------------------------------------------------------------------------

#[test]
fn p3_e01_the_embedded_set_has_an_ollama_engine_that_loads_clean() {
    let registry = registry();
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let engine = ollama(&registry);
    assert_eq!(engine.doc.engine.name, "Ollama");
    assert!(
        !engine.doc.engine.bundled,
        "Ollama is downloaded on request, never shipped"
    );
    assert!(
        engine.doc.provides.web_ui.is_none(),
        "`ollama serve` has no chat page"
    );
    assert!(
        engine.doc.provides.model_name_matters,
        "a request naming another model loads that one instead"
    );
    assert_eq!(engine.doc.provides.default_port, Some(11434));
    assert_eq!(
        engine.doc.engine.egress,
        ["registry.ollama.ai", "hf.co", "*.cdn.hf.co"],
        "the hosts a real pull was seen to contact (X05)"
    );
}

#[test]
fn p3_e01b_the_engines_are_listed_with_llamacpp_first_for_the_selector() {
    let registry = registry();
    // what a person can choose between: Open WebUI is a companion of Ollama's and is offered by its settings
    let ids: Vec<&str> = registry.selectable().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["llamacpp", "ollama", "comfyui"]);
    // every document there is, in the same order, has it last
    let all: Vec<&str> = registry.engines().map(|e| e.id.as_str()).collect();
    assert_eq!(all, ["llamacpp", "ollama", "comfyui", "openwebui"]);
}

/// The sha256 of each Ollama v0.35.1 asset as `docs/engine-facts.md` records them.
fn digests_in_the_facts() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/engine-facts.md"),
    )
    .expect("engine-facts.md");
    let after = text
        .split("Release assets of v0.35.1:")
        .nth(1)
        .expect("the asset table");
    let table = after.split("Ignored:").next().unwrap();
    table
        .lines()
        .filter(|l| {
            l.starts_with('|') && l.contains(".tar.zst") || l.contains(".zip") || l.contains(".tgz")
        })
        .filter_map(|l| {
            let cells: Vec<&str> = l.split('|').map(str::trim).collect();
            let name = cells.get(2)?.trim_matches('`');
            let sum = cells.get(4)?.trim_matches('`');
            (sum.len() == 64).then(|| (name.to_string(), format!("sha256:{sum}")))
        })
        .collect()
}

#[test]
fn p3_e02a_the_digests_in_the_facts_are_the_ones_of_the_recorded_release() {
    let facts = digests_in_the_facts();
    assert_eq!(facts.len(), 7, "{facts:#?}");
    let releases = crate::testkit::fake_github::recorded_releases();
    let release = releases
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["tag_name"] == "v0.35.1")
        .unwrap();
    for (name, digest) in &facts {
        let asset = release["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name.as_str());
        let asset = asset.unwrap_or_else(|| panic!("{name} is not in the recorded release"));
        assert_eq!(asset["digest"], digest.as_str(), "{name}");
    }
}

#[test]
fn p3_e02_the_pins_of_the_embedded_document_are_those_digests() {
    let registry = registry();
    let engine = ollama(&registry);
    let acquire = engine
        .merged
        .body
        .get("acquire")
        .and_then(|a| a.as_table())
        .expect("an [acquire] table");
    let tested = acquire
        .get("tested")
        .and_then(|t| t.get("tag"))
        .and_then(|t| t.as_str());
    assert_eq!(tested, Some("v0.35.1"));
    let pins = acquire
        .get("pins")
        .and_then(|p| p.as_table())
        .expect("[acquire.pins]");
    let got: BTreeMap<String, String> = pins
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    assert_eq!(got, digests_in_the_facts());
}

#[test]
fn p3_e22_the_embedded_ollama_document_is_the_example_document() {
    let read = |path: &str| {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .replace("\r\n", "\n")
    };
    assert_eq!(
        read("manifests/engines/ollama.toml"),
        read("../../docs/examples/engines/ollama.toml"),
        "the document the application ships and the one `docs/examples` shows are one text"
    );
}

#[test]
fn p3_e28_the_embedded_ollama_document_can_look_up_and_install_a_build_on_every_platform() {
    use crate::prims::feed::LookupSpec;
    use crate::prims::install::InstallSpec;
    let registry = registry();
    let engine = ollama(&registry);
    for (os, arch) in [
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("windows", "x86_64"),
        ("windows", "aarch64"),
        ("macos", "aarch64"),
    ] {
        let facts = HostFacts {
            os: os.into(),
            arch: arch.into(),
            ..facts()
        };
        assert!(
            LookupSpec::from_engine(engine, &facts).is_some(),
            "{os}/{arch}: nothing to look up"
        );
        // the window's Download button does nothing when there is no spec: the document must name
        // where builds go and the executable to look for in them
        let spec = InstallSpec::from_engine(engine, &facts)
            .unwrap_or_else(|| panic!("{os}/{arch}: the document gives no way to install"));
        assert!(spec.binary.starts_with("ollama"), "{}", spec.binary);
        assert_eq!(spec.root, Path::new("/data/ollama"));
        assert_eq!(
            spec.smoke.as_ref().map(|s| s.args.clone()),
            Some(vec!["--version".to_string()]),
            "an unpacked build must run before it is kept"
        );
    }
}

// ---- the planner -----------------------------------------------------------------------------------

#[test]
fn p3_e03a_the_default_plan_attaches_if_it_can_and_otherwise_starts_ollama_serve() {
    let p = plan_of(&selection(), &facts());
    assert_eq!(how_list(&p), ["attach", "spawn"]);
    assert_eq!(p["public_port"], 11434);
    assert_eq!(p["gateway"], Value::Null);
    let spawn = strategy(&p, "spawn").unwrap();
    assert_eq!(spawn["program"]["value"], BINARY);
    assert_eq!(
        spawn["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["value"].clone())
            .collect::<Vec<_>>(),
        [json!("serve")]
    );
    assert_eq!(spawn["group"], true);
    let env = env_of(spawn);
    assert_eq!(env["OLLAMA_HOST"], "127.0.0.1:11434");
    assert_eq!(env["OLLAMA_NO_CLOUD"], "1");
    assert_eq!(env["OLLAMA_KEEP_ALIVE"], "-1");
    assert_eq!(env["OLLAMA_MAX_LOADED_MODELS"], "1");
    assert_eq!(
        env["OLLAMA_MODELS"], "/data/ollama/models",
        "QuickAI's own store, beside its other data"
    );
    assert!(
        !env.contains_key("OLLAMA_CONTEXT_LENGTH"),
        "with no context size chosen the engine's own default (tiered by VRAM) stands: {env:?}"
    );
    for name in [
        "OLLAMA_VULKAN",
        "CUDA_VISIBLE_DEVICES",
        "ROCR_VISIBLE_DEVICES",
    ] {
        assert!(
            !env.contains_key(name),
            "{name} was set although the GPU is on automatic"
        );
    }
    // the attach strategy knows where to look and how to tell Ollama from something else
    let attach = strategy(&p, "attach").unwrap();
    assert_eq!(attach["endpoint"]["port"], 11434);
    assert_eq!(attach["identity"]["path"], "/api/version");
    // the model is loaded for ever with no context size of its own, and unloaded at the end
    assert_eq!(
        p["units"][0]["warm"][0]["body"],
        json!({"model": "qwen3:4b", "keep_alive": -1})
    );
    assert_eq!(p["units"][0]["warm"][0]["path"], "/api/generate");
    assert_eq!(
        p["units"][0]["unload"][0]["body"],
        json!({"model": "qwen3:4b", "keep_alive": 0})
    );
    assert_eq!(p["units"][0]["unload"][0]["best_effort"], true);
    assert_eq!(p["units"][0]["ready"]["path"], "/api/version");
}

#[test]
fn p3_e03b_a_context_size_the_user_chose_is_passed_as_the_environment_and_never_in_the_warm_up() {
    let p = plan_of(&selection().set("ollama.ctx_size", 8192u32), &facts());
    let env = env_of(strategy(&p, "spawn").unwrap());
    assert_eq!(env["OLLAMA_CONTEXT_LENGTH"], "8192");
    assert!(
        p["units"][0]["warm"][0]["body"].get("options").is_none(),
        "a per-request num_ctx does not stick (A7)"
    );
    assert!(p["units"][0]["warm"][0]["body"].get("num_ctx").is_none());
}

#[test]
fn p3_e03c_turning_the_gpu_off_sets_the_three_variables_that_do_it() {
    let p = plan_of(&selection().set("ollama.gpu", "off"), &facts());
    let env = env_of(strategy(&p, "spawn").unwrap());
    assert_eq!(env["OLLAMA_VULKAN"], "0");
    assert_eq!(env["CUDA_VISIBLE_DEVICES"], "-1");
    assert_eq!(env["ROCR_VISIBLE_DEVICES"], "-1");
}

#[test]
fn p3_e03d_the_model_store_is_quickais_own_unless_ollamas_is_asked_for() {
    let own = env_of(strategy(&plan_of(&selection(), &facts()), "spawn").unwrap());
    assert!(own.contains_key("OLLAMA_MODELS"));
    let theirs = plan_of(&selection().set("ollama.use_default_store", true), &facts());
    assert!(!env_of(strategy(&theirs, "spawn").unwrap()).contains_key("OLLAMA_MODELS"));
    let elsewhere = plan_of(
        &selection().set("ollama.models_dir", "/mnt/big/models"),
        &facts(),
    );
    assert_eq!(
        env_of(strategy(&elsewhere, "spawn").unwrap())["OLLAMA_MODELS"],
        "/mnt/big/models"
    );
}

#[test]
fn p3_e03e_the_mode_decides_which_strategies_the_plan_has() {
    assert_eq!(
        how_list(&plan_of(&selection().set("ollama.mode", "auto"), &facts())),
        ["attach", "spawn"]
    );
    assert_eq!(
        how_list(&plan_of(
            &selection().set("ollama.mode", "attach"),
            &facts()
        )),
        ["attach"]
    );
    assert_eq!(
        how_list(&plan_of(&selection().set("ollama.mode", "spawn"), &facts())),
        ["spawn"]
    );
}

#[test]
fn p3_e03f_another_port_is_the_port_of_the_attach_the_spawn_and_the_plan() {
    let p = plan_of(&selection().set("ollama.port", 9999u16), &facts());
    assert_eq!(p["public_port"], 9999);
    assert_eq!(strategy(&p, "attach").unwrap()["endpoint"]["port"], 9999);
    assert_eq!(
        env_of(strategy(&p, "spawn").unwrap())["OLLAMA_HOST"],
        "127.0.0.1:9999"
    );
}

#[test]
fn p3_e03g_with_the_proxy_on_the_daemon_listens_on_the_internal_port_and_the_gateway_on_the_public_one(
) {
    let mut f = facts();
    f.internal_port = Some(41234);
    let p = plan_of(&selection().set(TRAFFIC_PROXY_KEY, true), &f);
    assert_eq!(
        p["gateway"],
        json!({"public_port": 11434, "upstream_port": 41234})
    );
    assert_eq!(
        env_of(strategy(&p, "spawn").unwrap())["OLLAMA_HOST"],
        "127.0.0.1:41234"
    );
    assert_eq!(p["public_port"], 11434);
}

#[test]
fn p3_e03h_without_a_model_the_plan_has_no_warm_up_and_no_unload() {
    let p = plan_of(&Selection::new("ollama"), &facts());
    assert_eq!(p["model"], Value::Null);
    assert_eq!(p["units"][0]["warm"], json!([]));
    assert_eq!(p["units"][0]["unload"], json!([]));
    assert_eq!(
        how_list(&p),
        ["attach", "spawn"],
        "management mode starts a daemon like any other run"
    );
}

#[test]
fn p3_e03i_the_users_environment_lines_follow_the_documents_and_cannot_replace_the_locked_ones() {
    let p = plan_of(
        &selection().set(
            "ollama.extra_env",
            "# comment\nOLLAMA_DEBUG=1\n\nOLLAMA_HOST=0.0.0.0:80\nollama_no_cloud=0\nOLLAMA_NUM_PARALLEL = 2",
        ),
        &facts(),
    );
    let spawn = strategy(&p, "spawn").unwrap();
    let env = env_of(spawn);
    assert_eq!(env["OLLAMA_HOST"], "127.0.0.1:11434", "locked");
    assert_eq!(
        env["OLLAMA_NO_CLOUD"], "1",
        "locked, and not to be got round with another spelling"
    );
    assert_eq!(env["OLLAMA_DEBUG"], "1");
    assert_eq!(
        env["OLLAMA_NUM_PARALLEL"], "2",
        "spaces around the = are not part of the value"
    );
    assert!(!env.contains_key("ollama_no_cloud"));
    // the user's variables come after the engine's own
    let names: Vec<&str> = spawn["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    let at = |n: &str| names.iter().position(|x| *x == n).unwrap();
    assert!(at("OLLAMA_DEBUG") > at("OLLAMA_NO_CLOUD"));
}

#[test]
fn p3_e04_an_attach_only_plan_configures_nothing_for_a_daemon_that_is_not_ours() {
    let p = plan_of(
        &selection()
            .set("ollama.mode", "attach")
            .set("ollama.gpu", "off")
            .set("ollama.ctx_size", 2048u32),
        &facts(),
    );
    assert!(strategy(&p, "spawn").is_none());
    let attach = strategy(&p, "attach").unwrap();
    assert!(
        attach.get("env").is_none() || attach["env"] == json!([]),
        "{attach}"
    );
}

#[test]
fn p3_e05_plan_explain_of_the_default_ollama_selection_is_a_golden() {
    let registry = registry();
    ollama(&registry);
    let mut machine = facts();
    machine.internal_port = Some(41234);
    let default = plan(&selection(), &registry, &machine);
    let with_proxy = plan(
        &selection().set(TRAFFIC_PROXY_KEY, true),
        &registry,
        &machine,
    );
    let no_model = plan(&Selection::new("ollama"), &registry, &machine);
    let text = format!(
        "== a model, defaults ==\n{default}\n== a model, the proxy on ==\n{with_proxy}\n== no model ==\n{no_model}"
    );
    crate::golden::assert_golden("explain_ollama.txt", &text);

    fn plan(selection: &Selection, registry: &Registry, facts: &HostFacts) -> String {
        match quickai_core::plan::plan(registry, selection, facts) {
            Ok(p) => quickai_core::plan::explain::render_explain(&p),
            Err(e) => panic!("the planner refused: {e:#?}"),
        }
    }
}

#[test]
fn p3_e15_a_document_needing_a_primitive_this_build_lacks_is_skipped_with_it_named() {
    use crate::prims::PROVIDED;
    use quickai_core::registry::KnownPrimitives;
    let without: Vec<&str> = PROVIDED
        .iter()
        .copied()
        .filter(|p| *p != "attach@1")
        .collect();
    let registry = crate::config::build_registry_with(
        &ConfigDirs::default(),
        &KnownPrimitives::new(&without).with_widgets(crate::prims::WIDGET_NAMES),
    );
    // both engines that attach to a daemon that is already there need it, and Open WebUI, which goes with
    // Ollama, is left without the engine it goes with
    for id in ["ollama", "comfyui", "openwebui"] {
        assert!(registry.engine(id).is_none(), "{id} was loaded");
    }
    assert!(
        registry.engine("llamacpp").is_some(),
        "the other engine is not touched"
    );
    let errors: Vec<String> = registry
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    assert_eq!(errors.len(), 3, "{errors:#?}");
    for (id, error) in ["comfyui", "ollama"].iter().zip(&errors) {
        assert!(
            error.contains("`attach@1`") && error.contains(id),
            "{id}: {error}"
        );
    }
    assert!(
        errors[2].contains("companion_of") && errors[2].contains("`ollama`"),
        "{}",
        errors[2]
    );
}

// ---- the form ---------------------------------------------------------------------------------------

#[test]
fn p3_e06_the_form_of_the_ollama_engine_in_four_states_is_a_golden() {
    let registry = registry();
    ollama(&registry);
    let form = |selection: &Selection, facts: &HostFacts| {
        serde_json::to_value(FormModel::build(&registry, selection, facts).unwrap()).unwrap()
    };
    let running = HostFacts {
        session_running: true,
        ..facts()
    };
    let attached = HostFacts {
        session_running: true,
        session_attached: true,
        ..facts()
    };
    let golden = json!({
        "no_model": form(&Selection::new("ollama"), &facts()),
        "a_model": form(&selection(), &facts()),
        "running_spawned": form(&selection(), &running),
        "running_attached": form(&selection(), &attached),
    });
    crate::golden::assert_golden(
        "form_ollama.json",
        &(serde_json::to_string_pretty(&golden).unwrap() + "\n"),
    );
    // what only a spawn can set is not the person's to change while an engine of someone else's runs
    let field = |value: &Value, id: &str| {
        value["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["fields"].as_array().unwrap())
            .find(|f| f["id"] == id)
            .unwrap()
            .clone()
    };
    for (id, spawned_enabled) in [("ctx_size", false), ("gpu", false), ("port", false)] {
        // while it runs nothing is editable: the fields say `not session.running`
        assert_eq!(
            field(&golden["running_spawned"], id)["enabled"],
            spawned_enabled,
            "{id}"
        );
        assert_eq!(
            field(&golden["running_attached"], id)["enabled"],
            false,
            "{id}"
        );
    }
    assert_eq!(
        field(&golden["a_model"], "ctx_size")["applies"],
        "spawn_only"
    );
}

#[test]
fn p3_e07_the_form_has_the_ollama_fields_in_the_documents_sections_and_order() {
    let registry = registry();
    ollama(&registry);
    let form = FormModel::build(&registry, &selection(), &facts()).unwrap();
    let layout: Vec<(String, Vec<String>)> = form
        .sections
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                s.fields
                    .iter()
                    .filter(|f| f.visible)
                    .map(|f| f.id.clone())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        layout,
        [
            (
                "primary".to_string(),
                vec!["port".to_string(), "ctx_size".into(), "gpu".into()]
            ),
            (
                "advanced".to_string(),
                vec![
                    "mode".to_string(),
                    "bin_override".into(),
                    "models_dir".into(),
                    "use_default_store".into(),
                    "extra_env".into()
                ]
            ),
        ]
    );
    let field = |id: &str| {
        form.sections
            .iter()
            .flat_map(|s| &s.fields)
            .find(|f| f.id == id)
            .unwrap()
            .clone()
    };
    assert_eq!(field("port").value, quickai_core::value::Value::Int(11434));
    assert_eq!(field("mode").options, ["auto", "attach", "spawn"]);
    assert_eq!(field("gpu").options, ["auto", "off"]);
    assert!(
        field("ctx_size").value.is_null(),
        "no number is made up for Ollama's context size"
    );
    assert!(
        field("ctx_size").hint.is_some(),
        "an empty box says what empty means"
    );
    assert_eq!(
        field("models_dir").value,
        quickai_core::value::Value::Str("/data/ollama/models".into())
    );
    assert_eq!(
        field("extra_env").ty,
        quickai_core::doc::engine::FieldType::KvLines
    );
    assert_eq!(
        field("bin_override").widget.as_deref(),
        Some("runtime_manager")
    );
    // the form and the planner share one resolver: the number shown is the number used
    assert_eq!(
        form.derived["gpu_offload"],
        quickai_core::value::Value::Bool(true)
    );
}

#[test]
fn p3_e08_while_the_session_runs_the_settings_that_need_a_restart_cannot_be_edited() {
    let registry = registry();
    ollama(&registry);
    let mut running = facts();
    running.session_running = true;
    let form = FormModel::build(&registry, &selection(), &running).unwrap();
    for id in [
        "port",
        "ctx_size",
        "gpu",
        "mode",
        "models_dir",
        "use_default_store",
        "extra_env",
    ] {
        let f = form
            .sections
            .iter()
            .flat_map(|s| &s.fields)
            .find(|f| f.id == id)
            .unwrap();
        assert!(!f.enabled, "{id} can be edited while Ollama runs");
    }
    let idle = FormModel::build(&registry, &selection(), &facts()).unwrap();
    assert!(idle
        .sections
        .iter()
        .flat_map(|s| &s.fields)
        .filter(|f| f.visible)
        .all(|f| f.enabled));
}

#[test]
fn p3_e09_a_bad_environment_line_is_an_error_under_the_field_and_the_value_is_kept() {
    let registry = registry();
    ollama(&registry);
    let sel = selection().set(
        "ollama.extra_env",
        "OLLAMA_DEBUG=1\nnot a line\nOLLAMA_HOST=evil",
    );
    let form = FormModel::build(&registry, &sel, &facts()).unwrap();
    let f = form
        .sections
        .iter()
        .flat_map(|s| &s.fields)
        .find(|f| f.id == "extra_env")
        .unwrap();
    let error = f.error.as_deref().expect("an error under the field");
    assert!(
        error.contains("line 2") || error.contains("not a line"),
        "{error}"
    );
    assert!(
        error.contains("OLLAMA_HOST"),
        "the protected name is named: {error}"
    );
    assert_eq!(
        f.value.to_string(),
        "OLLAMA_DEBUG=1\nnot a line\nOLLAMA_HOST=evil",
        "what was typed stays"
    );
}

// ---- the API panel and the advice ---------------------------------------------------------------------

/// What the window showed before the panel came from the document: every sentence of it.
fn legacy_curl(base_url: &str, model: &str) -> String {
    format!(
        "curl {base_url}/chat/completions \\\n  -H \"Content-Type: application/json\" \\\n  -d '{{\"model\":\"{model}\",\"messages\":[{{\"role\":\"user\",\"content\":\"Hello\"}}]}}'"
    )
}

#[test]
fn p3_e13_the_api_panel_data_for_ollama_is_its_document_and_llamacpps_is_what_it_always_was() {
    use quickai_core::api_info::{api_panel, Note};
    let registry = registry();

    let panel = api_panel(&registry, &selection(), &facts()).expect("Ollama has an [api_info]");
    assert_eq!(panel.base_url, "http://127.0.0.1:11434/v1");
    assert_eq!(
        panel.extra_urls,
        [(
            "Ollama API".to_string(),
            "http://127.0.0.1:11434".to_string()
        )]
    );
    assert_eq!(
        panel.api_key,
        Some(Note {
            value: "none".into(),
            note: Some("Ollama doesn't require one.".into())
        })
    );
    let model = panel.model.expect("the model to name");
    assert_eq!(model.value, "qwen3:4b", "exactly the name the engine lists");
    assert!(
        model
            .note
            .as_deref()
            .is_some_and(|n| n.contains("a request that names another model loads that one")),
        "{model:?}"
    );
    let example = panel.example.expect("a curl example");
    assert_eq!(example.label, "Text");
    assert_eq!(
        example.curl,
        legacy_curl("http://127.0.0.1:11434/v1", "qwen3:4b")
    );

    // another port is in every address, and no model is a name that still works in a template
    let moved = api_panel(
        &registry,
        &Selection::new("ollama").set("ollama.port", 9999u16),
        &facts(),
    )
    .unwrap();
    assert_eq!(moved.base_url, "http://127.0.0.1:9999/v1");
    assert_eq!(moved.extra_urls[0].1, "http://127.0.0.1:9999");
    assert_eq!(moved.model.unwrap().value, "local-model");

    // llama.cpp: the panel of the window as it always was
    let llama = Selection::new("llamacpp").with_model(ModelInfo {
        id: "m.gguf".into(),
        display: "Bonsai 8B (1-bit Q1_0)".into(),
        path: "/models/m.gguf".into(),
        size_bytes: 1,
    });
    let panel = api_panel(&registry, &llama, &facts()).expect("llama.cpp has an [api_info]");
    assert_eq!(panel.base_url, "http://127.0.0.1:8080/v1");
    assert!(panel.extra_urls.is_empty());
    assert_eq!(panel.api_key.as_ref().unwrap().value, "none");
    assert_eq!(
        panel.api_key.unwrap().note.as_deref(),
        Some("llama.cpp doesn't require one by default \u{2014} leave it blank, or use any placeholder value if your client insists on a non-empty string.")
    );
    let model = panel.model.unwrap();
    assert_eq!(model.value, "Bonsai 8B (1-bit Q1_0)");
    assert_eq!(
        model.note.as_deref(),
        Some("Only one model is ever loaded at a time, so most OpenAI-compatible clients work with any value here.")
    );
    assert_eq!(
        panel.example.unwrap().curl,
        legacy_curl("http://127.0.0.1:8080/v1", "Bonsai 8B (1-bit Q1_0)")
    );
}

#[test]
fn p3_e14_the_documents_diagnostic_rules_match_the_log_and_say_what_to_do() {
    use quickai_core::advice::{advice, Advice};
    let registry = registry();
    let said = |lines: &[&str], port: Option<u16>| {
        let log: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let mut selection = selection();
        if let Some(port) = port {
            selection = selection.set("ollama.port", port);
        }
        advice(&registry, &selection, &facts(), &log, None)
    };

    assert_eq!(
        said(
            &["Error: listen tcp 127.0.0.1:11434: bind: address already in use"],
            None
        ),
        [Advice {
            text: "Another program is using port 11434. Pick another port.".into(),
            action: Some("focus_field:port".into())
        }]
    );
    assert_eq!(
        said(
            &["Error: listen tcp 127.0.0.1:9999: bind: address already in use"],
            Some(9999)
        )[0]
        .text,
        "Another program is using port 9999. Pick another port.",
        "the hint is filled in from the settings"
    );
    let memory = said(
        &["llm: model requires more system memory (8.2 GiB) than is available (3.0 GiB)"],
        None,
    );
    assert_eq!(memory.len(), 1, "{memory:?}");
    assert!(
        memory[0]
            .text
            .starts_with("The model does not fit in memory."),
        "{memory:?}"
    );
    assert_eq!(memory[0].action, None);
    let gpu = said(&["no compatible GPUs were discovered"], None);
    assert!(gpu[0].text.contains("Vulkan drivers (Mesa)"), "{gpu:?}");

    // a log that says none of it is not advised about; two things said are two pieces of advice
    assert!(said(
        &["time=... level=INFO msg=\"Listening on 127.0.0.1:11434\""],
        None
    )
    .is_empty());
    assert!(said(&[], None).is_empty());
    assert_eq!(
        said(&["no usable GPUs", "address already in use"], None).len(),
        2
    );
    // llama.cpp's document has no rules: its crash panel is the window's own
    let llama = advice(
        &registry,
        &Selection::new("llamacpp"),
        &facts(),
        &["address already in use".to_string()],
        Some("starting"),
    );
    assert!(llama.is_empty());
}

// ---- the settings file -----------------------------------------------------------------------------

#[test]
fn p3_e10_ollamas_values_live_under_values_and_the_nine_keys_of_the_file_are_untouched() {
    use crate::settings::Settings;
    use quickai_core::value::Value as V;
    let registry = registry();
    let engine = ollama(&registry);
    let mut settings = Settings::default();
    let before = serde_json::to_value(&settings).unwrap();
    let values = BTreeMap::from([
        ("ollama.port".to_string(), V::Int(12345)),
        ("ollama.mode".to_string(), V::Str("attach".into())),
        ("ollama.gpu".to_string(), V::Str("off".into())),
        (
            "ollama.extra_env".to_string(),
            V::Str("OLLAMA_DEBUG=1".into()),
        ),
    ]);
    settings.set_engine_values(engine, &values).unwrap();
    let after = serde_json::to_value(&settings).unwrap();
    // every key llama.cpp users have is where it was, with the value it had
    for (key, value) in before.as_object().unwrap() {
        assert_eq!(&after[key], value, "{key} changed");
    }
    assert_eq!(
        after["values"],
        json!({"ollama.port": 12345, "ollama.mode": "attach", "ollama.gpu": "off", "ollama.extra_env": "OLLAMA_DEBUG=1"})
    );
    assert_eq!(settings.engine_values(engine), values);
}

#[test]
fn p3_e21_the_chosen_engine_is_read_and_written_under_values_and_llamacpps_file_keeps_its_nine_keys(
) {
    use crate::settings::Settings;
    use quickai_core::value::Value as V;
    let registry = registry();
    let engine = ollama(&registry);
    let mut settings = Settings::default();
    let nine = serde_json::to_value(&settings).unwrap();
    assert_eq!(settings.chosen_engine(), None, "no choice is llama.cpp");

    // choosing Ollama is one key in `values`, and the nine keys are where they were
    settings.set_chosen_engine("ollama");
    assert_eq!(settings.chosen_engine().as_deref(), Some("ollama"));
    let after = serde_json::to_value(&settings).unwrap();
    for (key, value) in nine.as_object().unwrap() {
        assert_eq!(&after[key], value, "{key} changed");
    }
    assert_eq!(after["values"], json!({"app.engine": "ollama"}));

    // the engine's own settings and the choice live side by side
    let values = BTreeMap::from([("ollama.port".to_string(), V::Int(12345))]);
    settings.set_engine_values(engine, &values).unwrap();
    assert_eq!(settings.chosen_engine().as_deref(), Some("ollama"));
    assert_eq!(
        serde_json::to_value(&settings).unwrap()["values"],
        json!({"app.engine": "ollama", "ollama.port": 12345})
    );

    // going back takes the choice away and leaves what Ollama had
    settings.set_chosen_engine("llamacpp");
    assert_eq!(settings.chosen_engine(), None);
    assert_eq!(
        serde_json::to_value(&settings).unwrap()["values"],
        json!({"ollama.port": 12345}),
        "the Ollama values stay"
    );
    assert_eq!(settings.engine_values(engine), values);

    // and a file that never held anything else is the nine keys again
    let mut clean = Settings::default();
    clean.set_chosen_engine("ollama");
    clean.set_chosen_engine("llamacpp");
    assert_eq!(serde_json::to_value(&clean).unwrap(), nine);
    // the key survives a round trip through the file's text
    let mut chosen = Settings::default();
    chosen.set_chosen_engine("ollama");
    let text = serde_json::to_string(&chosen).unwrap();
    let again: Settings = serde_json::from_str(&text).unwrap();
    assert_eq!(again.chosen_engine().as_deref(), Some("ollama"));
}

#[test]
fn p3_e12_an_engine_the_settings_name_that_has_no_document_falls_back_to_llamacpp_and_is_not_forgotten(
) {
    use crate::config::engine_in_use;
    use crate::settings::Settings;
    let registry = registry();
    let mut settings = Settings::default();
    assert_eq!(
        engine_in_use(&registry, &settings),
        ("llamacpp".to_string(), None)
    );
    settings.set_chosen_engine("ollama");
    assert_eq!(
        engine_in_use(&registry, &settings),
        ("ollama".to_string(), None)
    );

    // an engine of a newer version: llama.cpp runs, with a sentence about it
    settings.set_chosen_engine("mystery");
    let (engine, said) = engine_in_use(&registry, &settings);
    assert_eq!(engine, "llamacpp");
    let said = said.expect("a sentence for the log");
    assert!(
        said.contains("`mystery`") && said.contains("using llama.cpp"),
        "{said}"
    );
    // what is stored for it, and the choice, are still in the file
    let mut stored = settings.clone();
    stored
        .set_engine_values(registry.engine("llamacpp").unwrap(), &BTreeMap::new())
        .unwrap();
    assert_eq!(stored.chosen_engine().as_deref(), Some("mystery"));

    // the same when the engine's document is the one that failed to load
    let broken = crate::config::build_registry_with(
        &ConfigDirs::default(),
        &quickai_core::registry::KnownPrimitives::new(&["spawn@1"]),
    );
    settings.set_chosen_engine("ollama");
    let (engine, said) = engine_in_use(&broken, &settings);
    assert_eq!(engine, "llamacpp");
    assert!(said.is_some());
}
