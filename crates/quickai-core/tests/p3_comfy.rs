//! Group F of the test plan (work package 4.11): what a document may say to install and run an
//! application written in Python (ComfyUI), checked where the document is written. The tests that
//! read the shipped ComfyUI document and plan a run are in the application (`p3/comfy.rs`). Rows P13
//! and P14 (work package 4.12) are here too: what a document may say to put the application's own
//! files where a program reads them.

mod common;

use common::{manifest, registry_of, shipped_registry, texts};
use quickai_core::diag::Severity;
use quickai_core::plan::explain::render_explain;
use quickai_core::plan::{plan, HostFacts, Selection};
use serde_json::{json, Value};

/// A small engine document with the sections a `python_app` install needs, and `install` as given.
fn engine(install: &str) -> String {
    format!(
        r#"schema = 1
kind = "engine"
id = "pyapp"
requires = {{ primitives = ["spawn@1", "probe.http@1", "feed.github_release@1", "install.python_app@1"] }}

[engine]
name = "Pyapp"

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${{runtime.binary}}"

[launch.ready]
probe = {{ use = "http", path = "/", expect = 200 }}

[acquire]
root = "${{data}}/pyapp"
feed = {{ use = "github_release", repo = "a/b" }}
assets = {{ by = "source" }}

{install}
"#
    )
}

/// The install of a document that is right.
const GOOD: &str = r#"[acquire.install]
use = "python_app"
binary = { name = "pyapp" }
entry = "main.py"
python = { versions = ["3.13", "3.12"] }
verify = "digest"
smoke = ["${binary}", "--check"]
smoke_timeout = "60s"
space = "10GiB"

[[acquire.install.pip]]
id = "torch"
packages = ["torch", "torchvision==0.29.1"]
index = "https://download.example.com/whl/rocm"
when = "has(gpu.vendors, 'amd')"
weight = 5

[[acquire.install.pip]]
id = "requirements"
requirements = "requirements.txt"

[[acquire.install.nodes]]
id = "gguf"
url = "https://example.com/gguf.tar.gz"
sha256 = "sha256:688126d5c2b8c4ff061f56b23c3c6791ac01dc366e98d41ef1aaef374c36f283"
into = "custom_nodes/gguf"
requirements = "requirements.txt"
"#;

fn errors_of(install: &str) -> Vec<String> {
    let registry = registry_of(&[("embedded:engines/pyapp.toml", &engine(install))]);
    texts(&registry, Severity::Error)
}

#[test]
fn p3_f10a_an_install_that_is_right_loads_and_compiles_its_conditions() {
    let registry = registry_of(&[("embedded:engines/pyapp.toml", &engine(GOOD))]);
    let problems = texts(&registry, Severity::Error);
    assert!(problems.is_empty(), "{problems:#?}");
    let engine = registry.engine("pyapp").expect("the engine loaded");
    let compiled = engine.acquire.as_ref().expect("a compiled acquire");
    assert_eq!(compiled.pip_when.len(), 2);
    assert!(compiled.pip_when[0].is_some() && compiled.pip_when[1].is_none());
    assert_eq!(compiled.node_when.len(), 1);
    // a build is the source of a release, named after it
    assert_eq!(
        compiled
            .label
            .render(&quickai_core::value::map([("tag", "v1".into())]))
            .unwrap()
            .to_string(),
        "v1"
    );
}

#[test]
fn p3_f10_an_install_is_held_to_what_it_may_say() {
    // (what is wrong, the piece of the document that says it, what it is changed to, what the error says)
    let bad: &[(&str, &str, &str, &str)] = &[
        (
            "an entry that climbs out",
            "entry = \"main.py\"",
            "entry = \"../main.py\"",
            "not a path inside the unpacked source",
        ),
        (
            "an entry that is absolute",
            "entry = \"main.py\"",
            "entry = \"/etc/passwd\"",
            "not a path inside the unpacked source",
        ),
        (
            "no Python to look for",
            "python = { versions = [\"3.13\", \"3.12\"] }",
            "python = { versions = [] }",
            "`versions` is empty",
        ),
        (
            "a Python that is not a version",
            "[\"3.13\", \"3.12\"]",
            "[\"three\"]",
            "is not a Python version",
        ),
        (
            "a smoke test that does not start with the launcher",
            "smoke = [\"${binary}\", \"--check\"]",
            "smoke = [\"python\", \"--check\"]",
            "starts with it",
        ),
        (
            "a launcher with a path in its name",
            "binary = { name = \"pyapp\" }",
            "binary = { name = \"bin/pyapp\" }",
            "not the name of a launcher",
        ),
        (
            "a size that is not one",
            "space = \"10GiB\"",
            "space = \"plenty\"",
            "is not a size",
        ),
        (
            "a wait that is not a duration",
            "smoke_timeout = \"60s\"",
            "smoke_timeout = \"a while\"",
            "smoke_timeout",
        ),
        (
            "a step that installs nothing",
            "requirements = \"requirements.txt\"\n\n[[acquire.install.nodes]]",
            "weight = 3\n\n[[acquire.install.nodes]]",
            "names neither",
        ),
        (
            "a package that is an option",
            "packages = [\"torch\", \"torchvision==0.29.1\"]",
            "packages = [\"--index-url=https://evil.example\"]",
            "is not a package",
        ),
        (
            "a package with a space",
            "packages = [\"torch\", \"torchvision==0.29.1\"]",
            "packages = [\"torch torchvision\"]",
            "is not a package",
        ),
        (
            "an index that is not an address",
            "index = \"https://download.example.com/whl/rocm\"",
            "index = \"a folder\"",
            "not an http or https address",
        ),
        (
            "a requirements file that climbs out",
            "requirements = \"requirements.txt\"\n\n[[acquire.install.nodes]]",
            "requirements = \"../requirements.txt\"\n\n[[acquire.install.nodes]]",
            "not a path inside the unpacked source",
        ),
        (
            "a node pack that is not an archive of a commit",
            "url = \"https://example.com/gguf.tar.gz\"",
            "url = \"https://example.com/gguf.zip\"",
            "is not the address of a .tar.gz",
        ),
        (
            "a node pack with no real digest",
            "sha256 = \"sha256:688126d5c2b8c4ff061f56b23c3c6791ac01dc366e98d41ef1aaef374c36f283\"",
            "sha256 = \"sha256:abc\"",
            "is not a sha256 digest",
        ),
        (
            "a node pack that goes outside the source",
            "into = \"custom_nodes/gguf\"",
            "into = \"../../gguf\"",
            "not a path inside the unpacked source",
        ),
        (
            "a condition that names nothing",
            "when = \"has(gpu.vendors, 'amd')\"",
            "when = \"has(gpu.vendorz, 'amd')\"",
            "vendorz",
        ),
    ];
    for (what, from, to, says) in bad {
        let install = GOOD.replace(from, to);
        assert_ne!(
            install, GOOD,
            "{what}: the test did not change the document"
        );
        let found = errors_of(&install);
        assert!(!found.is_empty(), "{what}: the document was accepted");
        assert!(
            found
                .iter()
                .any(|e| e.contains("pyapp.toml") && e.contains(says)),
            "{what}: no error says `{says}`: {found:#?}"
        );
    }
    // the same step id twice
    let twice = GOOD.replace("id = \"requirements\"", "id = \"torch\"");
    assert!(
        errors_of(&twice).iter().any(|e| e.contains("used twice")),
        "{:#?}",
        errors_of(&twice)
    );
    // a source build needs a feed to take the source from
    let no_feed = engine(GOOD).replace("feed = { use = \"github_release\", repo = \"a/b\" }\n", "");
    let registry = registry_of(&[("embedded:engines/pyapp.toml", &no_feed)]);
    assert!(
        texts(&registry, Severity::Error)
            .iter()
            .any(|e| e.contains("no feed")),
        "{:#?}",
        texts(&registry, Severity::Error)
    );
}

#[test]
fn p3_f11_the_arguments_of_an_option_are_for_a_choice_field_and_its_options() {
    let with = |field: &str| {
        let text = format!("{}\n{field}\n", engine(GOOD));
        texts(
            &registry_of(&[("embedded:engines/pyapp.toml", &text)]),
            Severity::Error,
        )
    };
    let good = "[[field]]\nid = \"memory\"\ntype = \"choice\"\noptions = [\"auto\", \"low\"]\ndefault = \"auto\"\nbind.arg_by_value = { low = [\"--lowvram\"] }\n";
    assert!(with(good).is_empty(), "{:#?}", with(good));
    let not_a_choice = "[[field]]\nid = \"memory\"\ntype = \"text\"\nbind.arg_by_value = { low = [\"--lowvram\"] }\n";
    assert!(
        with(not_a_choice)
            .iter()
            .any(|e| e.contains("needs a choice field")),
        "{:#?}",
        with(not_a_choice)
    );
    let unknown = good.replace("low = [", "medium = [");
    assert!(
        with(&unknown)
            .iter()
            .any(|e| e.contains("is not one of the field's options")),
        "{:#?}",
        with(&unknown)
    );
    let empty_word = good.replace("\"--lowvram\"", "\" \"");
    assert!(
        with(&empty_word)
            .iter()
            .any(|e| e.contains("cannot be empty")),
        "{:#?}",
        with(&empty_word)
    );
}

#[test]
fn p3_f12_a_kind_of_file_reaches_the_program_by_an_argument_or_by_its_folder() {
    let with = |components: &str| {
        let text = format!("{}\n{components}\n", engine(GOOD));
        texts(
            &registry_of(&[("embedded:engines/pyapp.toml", &text)]),
            Severity::Error,
        )
    };
    assert!(
        with("[components.vae]\nplace = \"vae\"\n").is_empty(),
        "{:#?}",
        with("[components.vae]\nplace = \"vae\"\n")
    );
    assert!(
        with("[components.mmproj]\nbind.arg = [\"--mmproj\", \"${component.path}\"]\n").is_empty()
    );
    let neither = with("[components.vae]\norder = 5\n");
    assert!(
        neither
            .iter()
            .any(|e| e.contains("does not say how it reaches the program")),
        "{neither:#?}"
    );
    for folder in ["../vae", "Vae", "a/b", ""] {
        let found = with(&format!("[components.vae]\nplace = \"{folder}\"\n"));
        assert!(
            found
                .iter()
                .any(|e| e.contains("is not the name of a folder")),
            "`{folder}`: {found:#?}"
        );
    }
    // the weights' folder is held to the same rule
    let weights = with("[models.acquire]\nuse = \"file\"\ndir = \"${data}/m\"\nplace = \"../x\"\n");
    assert!(
        weights
            .iter()
            .any(|e| e.contains("is not the name of a folder")),
        "{weights:#?}"
    );
    let sources = with("[[models.sources]]\nuse = \"mystery\"\npath = \"${data}/m\"\n");
    assert!(
        sources
            .iter()
            .any(|e| e.contains("is not a place models are found")),
        "{sources:#?}"
    );
}

// ---- the shipped document ---------------------------------------------------------------------------

const LINUX_AMD: &[&str] = &["amd"];

fn facts(vendors: &[&str], targets: &[&str], os: &str) -> HostFacts {
    HostFacts {
        os: os.into(),
        arch: "x86_64".into(),
        ram_bytes: 16 * 1024 * 1024 * 1024,
        cpus: 8,
        gpu_known: !vendors.is_empty(),
        gpu_vendors: vendors.iter().map(|v| v.to_string()).collect(),
        gpu_targets: targets.iter().map(|t| t.to_string()).collect(),
        binary: Some("/data/comfyui/v0.39.0/comfyui".into()),
        data_dir: "/data".into(),
        config_dir: "/config".into(),
        home: "/home/u".into(),
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

#[test]
fn p3_f01_the_embedded_set_has_a_comfyui_engine_that_loads_clean() {
    let registry = shipped_registry(&[]);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let engine = registry.engine("comfyui").expect("a comfyui engine");
    assert_eq!(engine.doc.engine.name, "ComfyUI");
    assert!(
        !engine.doc.engine.bundled,
        "a GPL program of its own is never shipped"
    );
    assert_eq!(engine.doc.engine.license.as_deref(), Some("GPL-3.0"));
    assert_eq!(
        engine.doc.engine.egress,
        [
            "huggingface.co",
            "*.cdn.hf.co",
            "raw.githubusercontent.com",
            "api.comfy.org",
            "github.com"
        ],
        "the page is held to --offline; ComfyUI itself contacts the hub, when the person presses Download in its \
         page (F22 of docs/engine-facts.md), and ComfyUI-Manager, which is an option of the install, its lists \
         and the site a model of its own list is on (F35); X08 holds it to what a real run contacted"
    );
    assert!(!engine.doc.provides.needs_model, "it starts on its own");
    assert_eq!(engine.doc.provides.default_port, Some(8188));
    assert_eq!(
        engine
            .doc
            .provides
            .web_ui
            .as_ref()
            .map(|w| (w.path.as_str(), w.label.as_str())),
        Some(("/", "Open ComfyUI"))
    );
    assert_eq!(
        engine
            .doc
            .provides
            .accepts
            .as_ref()
            .map(|a| a.formats.clone()),
        Some(vec!["gguf".to_string(), "safetensors".to_string()])
    );
    // the selector shows llama.cpp first, then Ollama, then this (Open WebUI is Ollama's companion, and not in it)
    let order: Vec<&str> = registry.selectable().map(|e| e.id.as_str()).collect();
    assert_eq!(order, ["llamacpp", "ollama", "comfyui"]);
    // the other two still need a model, as they always did
    for id in ["llamacpp", "ollama"] {
        assert!(
            registry.engine(id).unwrap().doc.provides.needs_model,
            "{id}"
        );
    }
}

#[test]
fn p3_f02_the_embedded_comfyui_document_is_the_example_document() {
    let example = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/examples/engines/comfyui.toml"),
    )
    .expect("docs/examples/engines/comfyui.toml");
    assert_eq!(
        manifest("engines/comfyui.toml"),
        example,
        "the document the application ships and the example in the docs are one document"
    );
}

#[test]
fn p3_f03_the_tested_release_and_the_node_pack_are_pinned_by_digests() {
    use quickai_core::acquire;
    let registry = shipped_registry(&[]);
    let engine = registry.engine("comfyui").unwrap();
    let tag = acquire::tested_tag(engine).expect("a tested release");
    let archive = acquire::source_archive_name(tag);
    assert_eq!(archive, format!("{tag}.tar.gz"));
    let pin = acquire::pin_for(engine, &archive).expect("the tested release is pinned");
    assert_eq!(pin.len(), 64);
    assert!(pin.chars().all(|c| c.is_ascii_hexdigit()), "{pin}");
    assert!(
        acquire::pin_for(engine, "v0.0.1.tar.gz").is_none(),
        "no other release has a pin"
    );
    let quickai_core::doc::engine::Install::PythonApp { nodes, verify, .. } = engine
        .doc
        .acquire
        .as_ref()
        .unwrap()
        .install
        .as_ref()
        .unwrap()
    else {
        panic!("a python_app install");
    };
    assert_eq!(*verify, quickai_core::doc::engine::Verify::Digest);
    assert!(!nodes.is_empty(), "the GGUF pack is part of the build");
    for node in nodes {
        let digest = node.sha256.trim_start_matches("sha256:");
        assert_eq!(digest.len(), 64, "{}", node.id);
        assert!(node.url.starts_with("https://github.com/"), "{}", node.url);
        // a pack is pinned to one commit: its address has the commit's id and not a branch
        assert!(
            node.url
                .trim_end_matches(".tar.gz")
                .rsplit('/')
                .next()
                .unwrap()
                .len()
                == 40,
            "{}",
            node.url
        );
    }
}

#[test]
fn p3_f04_a_release_is_built_from_its_source_archive() {
    use quickai_core::acquire;
    let registry = shipped_registry(&[]);
    let engine = registry.engine("comfyui").unwrap();
    assert!(acquire::builds_from_source(engine));
    assert!(!acquire::builds_from_source(
        registry.engine("ollama").unwrap()
    ));
    assert_eq!(
        acquire::source_archive_url(engine, "v0.40.0").as_deref(),
        Some("https://github.com/Comfy-Org/ComfyUI/archive/refs/tags/v0.40.0.tar.gz")
    );
    assert_eq!(
        acquire::source_archive_url(registry.engine("ollama").unwrap(), "v1"),
        Some("https://github.com/ollama/ollama/archive/refs/tags/v1.tar.gz".into())
    );
    // the build lives in a directory named after the release
    assert_eq!(
        acquire::install_label(engine, "v0.40.0", "v0.40.0.tar.gz"),
        "v0.40.0"
    );
    // a document of the person's can name another host for the archive (a mirror, or a test's fake)
    let patched = shipped_registry(&[(
        "user:engines/comfyui.toml",
        "schema = 1\nkind = \"engine\"\nid = \"comfyui\"\n[acquire]\nfeed = { archive = \"http://127.0.0.1:9999/\" }\n",
    )]);
    let engine = patched.engine("comfyui").expect("the patched engine");
    assert_eq!(
        acquire::source_archive_url(engine, "v0.40.0").as_deref(),
        Some("http://127.0.0.1:9999/Comfy-Org/ComfyUI/archive/refs/tags/v0.40.0.tar.gz")
    );
}

#[test]
fn p3_f05_the_pip_steps_follow_the_graphics_card() {
    use quickai_core::acquire;
    let registry = shipped_registry(&[]);
    let engine = registry.engine("comfyui").unwrap();
    let ids = |vendors: &[&str], os: &str| -> Vec<String> {
        acquire::pip_steps(engine, &facts(vendors, &[], os))
            .iter()
            .map(|s| s.id.clone())
            .collect()
    };
    assert_eq!(ids(LINUX_AMD, "linux"), ["torch_amd", "requirements"]);
    assert_eq!(ids(&["nvidia"], "linux"), ["torch_nvidia", "requirements"]);
    assert_eq!(
        ids(&["nvidia"], "windows"),
        ["torch_nvidia", "requirements"]
    );
    // an AMD integrated graphics card beside an NVIDIA one: the NVIDIA one is what PyTorch uses
    assert_eq!(
        ids(&["amd", "nvidia"], "linux"),
        ["torch_nvidia", "requirements"]
    );
    assert_eq!(ids(&["intel"], "linux"), ["torch_cpu", "requirements"]);
    assert_eq!(ids(&[], "linux"), ["torch_cpu", "requirements"]);
    // AMD on Windows has no wheel on PyTorch's index: the processor's
    assert_eq!(ids(LINUX_AMD, "windows"), ["torch_cpu", "requirements"]);
    assert_eq!(ids(&["apple"], "macos"), ["torch_mac", "requirements"]);
    // every machine gets exactly one PyTorch
    for (vendors, os) in [
        (LINUX_AMD, "linux"),
        (&["nvidia"][..], "windows"),
        (&[][..], "macos"),
        (&["intel"][..], "linux"),
    ] {
        let torches = ids(vendors, os)
            .iter()
            .filter(|i| i.starts_with("torch"))
            .count();
        assert_eq!(torches, 1, "{vendors:?} on {os}");
    }
    // the AMD one is the one the machine this was measured on runs: ROCm 7.2, and from that index alone
    let amd = acquire::pip_steps(engine, &facts(LINUX_AMD, &[], "linux"));
    assert_eq!(
        amd[0].index.as_deref(),
        Some("https://download.pytorch.org/whl/rocm7.2")
    );
    assert_eq!(amd[0].packages, ["torch", "torchvision"]);
    assert!(
        !amd[0].packages.iter().any(|p| p.contains("torchaudio")),
        "ComfyUI does not import torchaudio, and its latest wheel pins an older torch"
    );
}

#[test]
fn p3_f06_what_an_install_puts_on_the_machine_is_said_in_a_sentence() {
    use quickai_core::acquire;
    let registry = shipped_registry(&[]);
    let engine = registry.engine("comfyui").unwrap();
    let sentence = acquire::install_summary(engine, &facts(LINUX_AMD, &["gfx1031"], "linux"))
        .expect("a python_app says what it installs");
    assert!(
        sentence.contains("PyTorch for AMD GPUs (ROCm 7.2)")
            && sentence.contains("ComfyUI-GGUF")
            && sentence.contains("30GiB free"),
        "{sentence}"
    );
    assert!(!sentence.contains("NVIDIA"), "{sentence}");
    let mac = acquire::install_summary(engine, &facts(&["apple"], &[], "macos")).unwrap();
    assert!(
        mac.contains("PyTorch for the Mac") && !mac.contains("AMD"),
        "{mac}"
    );
    // an engine that unpacks an archive has nothing to say
    assert_eq!(
        acquire::install_summary(
            registry.engine("ollama").unwrap(),
            &facts(&[], &[], "linux")
        ),
        None
    );
}

/// An install with one option (the Manager of ComfyUI, in a document of its own) and the one step that belongs to it.
const WITH_OPTION: &str = r#"[acquire.install]
use = "python_app"
binary = { name = "pyapp" }
entry = "main.py"
python = { versions = ["3.13"] }

[[acquire.install.options]]
id = "manager"
label = "Install the Manager"
badge = "highly recommended"
default = true
launch_args = ["--enable-manager"]

[[acquire.install.pip]]
id = "requirements"
requirements = "requirements.txt"

[[acquire.install.pip]]
id = "manager"
option = "manager"
requirements = "manager_requirements.txt"
"#;

#[test]
fn p3_f93_the_document_offers_comfyui_manager_as_a_ticked_option_of_the_install_that_adds_a_step_and_a_word(
) {
    use quickai_core::acquire;
    let registry = shipped_registry(&[]);
    let engine = registry.engine("comfyui").unwrap();
    let on = facts(LINUX_AMD, &["gfx1031"], "linux");

    // what the window offers: one box, ticked, with the words of a recommendation and what the pointer is told
    let options = acquire::install_options(engine, &on);
    assert_eq!(options.len(), 1, "{options:#?}");
    let manager = options[0];
    assert_eq!(manager.id, "manager");
    assert_eq!(manager.label, "Install ComfyUI-Manager");
    assert_eq!(manager.badge.as_deref(), Some("highly recommended"));
    assert!(manager.default, "it is ticked until the person unticks it");
    let help = manager.help.as_deref().unwrap_or_default();
    for word in [
        "Model Manager",
        "raw.githubusercontent.com",
        "api.comfy.org",
        "another person's program",
    ] {
        assert!(help.contains(word), "the box says `{word}`: {help}");
    }
    assert_eq!(acquire::default_options(engine, &on), ["manager"]);

    // chosen nothing (the headless runner, an install of before): the steps of before and no word for the launcher
    let ids = |chosen: &[&str]| -> Vec<String> {
        acquire::pip_steps_with(engine, &on, chosen)
            .iter()
            .map(|s| s.id.clone())
            .collect()
    };
    assert_eq!(ids(&[]), ["torch_amd", "requirements"]);
    assert_eq!(
        acquire::pip_steps(engine, &on).len(),
        2,
        "pip_steps is the install of nobody's choice"
    );
    assert!(acquire::launch_args(engine, &[]).is_empty());

    // chosen: ComfyUI's own file of what the Manager needs, after what ComfyUI needs, and the flag of the classic interface
    assert_eq!(ids(&["manager"]), ["torch_amd", "requirements", "manager"]);
    let steps = acquire::pip_steps_with(engine, &on, &["manager"]);
    assert_eq!(
        steps[2].requirements.as_deref(),
        Some("manager_requirements.txt")
    );
    assert!(
        steps[2].packages.is_empty() && steps[2].index.is_none(),
        "from PyPI, as ComfyUI's release names it"
    );
    assert_eq!(
        acquire::launch_args(engine, &["manager"]),
        ["--enable-manager-legacy-ui"]
    );
    // an option the document does not have changes nothing
    assert_eq!(ids(&["nothing"]), ids(&[]));
    assert!(acquire::launch_args(engine, &["nothing"]).is_empty());

    // it is offered on every machine the install is, and PyTorch is the same one with it
    for (vendors, os) in [(&["nvidia"][..], "windows"), (&[][..], "macos")] {
        let machine = facts(vendors, &[], os);
        assert_eq!(acquire::default_options(engine, &machine), ["manager"]);
        let with = ids_of(engine, &machine, &["manager"]);
        let without = ids_of(engine, &machine, &[]);
        assert_eq!(with[..without.len()], without[..]);
        assert_eq!(with.last().map(String::as_str), Some("manager"));
    }

    // the sentence under the button says what is ticked
    let said = |chosen: &[&str]| acquire::install_summary_with(engine, &on, chosen).unwrap();
    assert!(
        said(&["manager"]).contains("ComfyUI-Manager"),
        "{}",
        said(&["manager"])
    );
    assert!(!said(&[]).contains("ComfyUI-Manager"), "{}", said(&[]));
    assert_eq!(
        acquire::install_summary(engine, &on).unwrap(),
        said(&[]),
        "the sentence of before"
    );

    // an install that stopped is gone on with what it was begun with: its steps say it
    let stopped = |ids: &[&str]| -> Vec<String> { ids.iter().map(|s| s.to_string()).collect() };
    assert_eq!(
        acquire::options_of_steps(
            engine,
            &stopped(&[
                "source",
                "venv",
                "pip:torch_amd",
                "pip:requirements",
                "pip:manager",
                "check"
            ])
        ),
        ["manager"]
    );
    assert!(acquire::options_of_steps(
        engine,
        &stopped(&[
            "source",
            "venv",
            "pip:torch_amd",
            "pip:requirements",
            "check"
        ])
    )
    .is_empty());
    assert!(acquire::options_of_steps(engine, &[]).is_empty());

    // the other engines have no option, and no box
    for other in ["ollama", "llamacpp"] {
        if let Some(engine) = registry.engine(other) {
            assert!(acquire::install_options(engine, &on).is_empty(), "{other}");
        }
    }
}

fn ids_of(
    engine: &quickai_core::registry::Engine,
    machine: &HostFacts,
    chosen: &[&str],
) -> Vec<String> {
    quickai_core::acquire::pip_steps_with(engine, machine, chosen)
        .iter()
        .map(|s| s.id.clone())
        .collect()
}

#[test]
fn p3_f94_an_option_is_held_to_what_it_may_say_and_the_words_it_puts_in_the_launcher_are_plain() {
    // the document that is right loads, and its option and its step are as written
    let registry = registry_of(&[("embedded:engines/pyapp.toml", &engine(WITH_OPTION))]);
    let problems = texts(&registry, Severity::Error);
    assert!(problems.is_empty(), "{problems:#?}");
    let pyapp = registry.engine("pyapp").unwrap();
    let on = facts(&[], &[], "linux");
    assert_eq!(
        quickai_core::acquire::launch_args(pyapp, &["manager"]),
        ["--enable-manager"]
    );

    // (what is wrong, the piece of the document that says it, what it is changed to, what the error says)
    let bad: &[(&str, &str, &str, &str)] = &[
        (
            "a step of an option the document does not have",
            "option = \"manager\"",
            "option = \"another\"",
            "is not an option of this install",
        ),
        (
            "an option with a name that is no name",
            "id = \"manager\"\nlabel",
            "id = \"the manager\"\nlabel",
            "is not a valid option id",
        ),
        (
            "an option with no words for its box",
            "label = \"Install the Manager\"",
            "label = \"  \"",
            "no words for its box",
        ),
        (
            "two options of the same name",
            "[[acquire.install.pip]]\nid = \"requirements\"",
            "[[acquire.install.options]]\nid = \"manager\"\nlabel = \"Again\"\n\n[[acquire.install.pip]]\nid = \"requirements\"",
            "the option id `manager` is used twice",
        ),
        (
            "a word for the launcher that a shell would read as two",
            "launch_args = [\"--enable-manager\"]",
            "launch_args = [\"--enable-manager --listen\"]",
            "is not one plain word of a command line",
        ),
        (
            "a word for the launcher that ends the command",
            "launch_args = [\"--enable-manager\"]",
            "launch_args = [\"--x;rm\"]",
            "is not one plain word of a command line",
        ),
        (
            "a word for the launcher that expands",
            "launch_args = [\"--enable-manager\"]",
            "launch_args = [\"--x=$HOME\"]",
            "is not one plain word of a command line",
        ),
        (
            "a word for the launcher that is not an option",
            "launch_args = [\"--enable-manager\"]",
            "launch_args = [\"main.py\"]",
            "is not one plain word of a command line",
        ),
        (
            "a word for the launcher that is nothing",
            "launch_args = [\"--enable-manager\"]",
            "launch_args = [\"--\"]",
            "is not one plain word of a command line",
        ),
    ];
    for (what, from, to, says) in bad {
        assert!(
            WITH_OPTION.contains(from),
            "{what}: `{from}` is not in the document"
        );
        let found = errors_of(&WITH_OPTION.replacen(from, to, 1));
        assert!(
            found.iter().any(|e| e.contains(says)),
            "{what}: expected `{says}` in {found:#?}"
        );
    }
    // and a plain word with a value is fine, as is a short option
    for good in ["--cache-lru=10", "-v", "--enable-manager-legacy-ui"] {
        let ok = errors_of(&WITH_OPTION.replacen(
            "launch_args = [\"--enable-manager\"]",
            &format!("launch_args = [\"{good}\"]"),
            1,
        ));
        assert!(ok.is_empty(), "{good}: {ok:#?}");
    }
    // an option nobody ticks and no step belongs to is not offered (the engine has nothing to put in it)
    let lonely = WITH_OPTION.replacen("option = \"manager\"\n", "", 1);
    let registry = registry_of(&[("embedded:engines/pyapp.toml", &engine(&lonely))]);
    assert!(
        quickai_core::acquire::install_options(registry.engine("pyapp").unwrap(), &on).is_empty()
    );
}

#[test]
fn p3_f07_the_default_plan_attaches_if_it_can_and_otherwise_starts_comfyui() {
    let p = plan_json(
        &Selection::new("comfyui"),
        &facts(LINUX_AMD, &["gfx1031"], "linux"),
    );
    let hows: Vec<&str> = p["units"][0]["start"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["how"].as_str().unwrap())
        .collect();
    assert_eq!(hows, ["attach", "spawn"]);
    assert_eq!(p["public_port"], 8188);
    assert_eq!(p["gateway"], Value::Null);
    assert_eq!(p["model"], Value::Null, "it starts with no model chosen");
    let attach = &p["units"][0]["start"][0];
    assert_eq!(attach["identity"]["path"], "/system_stats");
    assert_eq!(attach["identity"]["pointer"], "/system/comfyui_version");
    let spawn = spawn_of(&p);
    assert_eq!(spawn["program"]["value"], "/data/comfyui/v0.39.0/comfyui");
    assert_eq!(spawn["group"], true);
    assert_eq!(
        args_of(spawn),
        [
            "--listen",
            "127.0.0.1",
            "--port",
            "8188",
            "--disable-auto-launch",
            "--models-directory",
            "/data/comfyui/models",
            "--output-directory",
            "/data/comfyui/output",
            "--input-directory",
            "/data/comfyui/input",
            "--user-directory",
            "/data/comfyui/user",
            "--temp-directory",
            "/data/comfyui/temp",
            "--database-url",
            "sqlite:////data/comfyui/user/comfyui.db",
            "--extra-model-paths-config",
            "/data/comfyui/extra_model_paths.yaml",
            "--offline",
        ]
    );
    // the folders it refuses when they are not there are made first, with the home and the temporary folder it is given
    // (WP4.16: the engines are isolated from each other completely), and nothing more
    assert_eq!(
        spawn["mkdir"],
        json!([
            "/data/comfyui/models",
            "/data/comfyui/output",
            "/data/comfyui/input",
            "/data/comfyui/user",
            "/data/comfyui/temp",
            "/data/comfyui/home",
            "/data/comfyui/cache/tmp"
        ])
    );
    let env = env_of(spawn);
    assert_eq!(env["PYTHONUNBUFFERED"], "1");
    // nothing it is given is a model, and nothing is loaded or unloaded around it
    assert_eq!(p["units"][0]["warm"], json!([]));
    assert_eq!(p["units"][0]["unload"], json!([]));
    assert_eq!(p["units"][0]["ready"]["path"], "/system_stats");
    assert_eq!(
        p["units"][0]["ready"]["on_timeout"], "wait",
        "the first start is slow, and is waited for"
    );
    assert_eq!(p["units"][0]["stop"]["policy"], "graceful");
}

#[test]
fn p3_f08_the_gpu_memory_and_network_fields_add_their_arguments() {
    let on = facts(LINUX_AMD, &["gfx1031"], "linux");
    let args = |selection: &Selection| args_of(spawn_of(&plan_json(selection, &on)));
    let default = args(&Selection::new("comfyui"));
    assert!(default.contains(&"--offline".to_string()));
    for flag in ["--cpu", "--lowvram", "--novram", "--highvram"] {
        assert!(!default.contains(&flag.to_string()), "{flag}");
    }
    assert!(
        args(&Selection::new("comfyui").set("comfyui.gpu", "off")).contains(&"--cpu".to_string())
    );
    for (value, flag) in [
        ("low", "--lowvram"),
        ("none", "--novram"),
        ("high", "--highvram"),
    ] {
        let a = args(&Selection::new("comfyui").set("comfyui.memory", value));
        assert!(a.contains(&flag.to_string()), "{value}: {a:?}");
    }
    let online = args(&Selection::new("comfyui").set("comfyui.network", "online"));
    assert!(!online.contains(&"--offline".to_string()), "{online:?}");
    // the person's own arguments come last
    let extra = args(&Selection::new("comfyui").set("comfyui.extra_args", "--preview-method auto"));
    assert_eq!(&extra[extra.len() - 2..], ["--preview-method", "auto"]);
    // --cpu and --lowvram together are an error ComfyUI would give itself: the form says so first
    let registry = shipped_registry(&[]);
    let form = |selection: &Selection| {
        quickai_core::form::FormModel::build(&registry, selection, &on).expect("a form")
    };
    let clash = form(
        &Selection::new("comfyui")
            .set("comfyui.gpu", "off")
            .set("comfyui.memory", "low"),
    );
    let error = clash.field("memory").and_then(|f| f.error.clone());
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.contains("about the graphics card")),
        "{error:?}"
    );
    assert!(
        form(&Selection::new("comfyui").set("comfyui.memory", "low"))
            .field("memory")
            .is_some_and(|f| f.error.is_none())
    );
    assert!(form(&Selection::new("comfyui").set("comfyui.gpu", "off"))
        .field("memory")
        .is_some_and(|f| f.error.is_none()));
    // other ports are the port of the attach, the spawn and the plan
    let p = plan_json(&Selection::new("comfyui").set("comfyui.port", 9000u16), &on);
    assert_eq!(p["public_port"], 9000);
    assert_eq!(p["units"][0]["start"][0]["endpoint"]["port"], 9000);
    assert!(args_of(spawn_of(&p))
        .windows(2)
        .any(|w| w == ["--port", "9000"]));
}

#[test]
fn p3_f09_an_amd_card_is_presented_as_the_architecture_pytorch_has_and_the_person_may_say_otherwise(
) {
    let env = |selection: &Selection, targets: &[&str]| {
        env_of(spawn_of(&plan_json(
            selection,
            &facts(LINUX_AMD, targets, "linux"),
        )))
    };
    let base = Selection::new("comfyui");
    // an RX 6700 XT is gfx1031, and PyTorch's wheels have kernels for gfx1030
    assert_eq!(
        env(&base, &["gfx1031"])["HSA_OVERRIDE_GFX_VERSION"],
        "10.3.0"
    );
    assert_eq!(
        env(&base, &["gfx1102"])["HSA_OVERRIDE_GFX_VERSION"],
        "11.0.0"
    );
    // a card that needs nothing, and a machine whose driver does not say, set nothing
    assert!(!env(&base, &["gfx1100"]).contains_key("HSA_OVERRIDE_GFX_VERSION"));
    assert!(!env(&base, &[]).contains_key("HSA_OVERRIDE_GFX_VERSION"));
    // what the person writes is what is set; "none" sets nothing, where this machine would have
    assert_eq!(
        env(
            &base.clone().set("comfyui.gfx_override", "11.0.1"),
            &["gfx1031"]
        )["HSA_OVERRIDE_GFX_VERSION"],
        "11.0.1"
    );
    assert!(!env(
        &base.clone().set("comfyui.gfx_override", "none"),
        &["gfx1031"]
    )
    .contains_key("HSA_OVERRIDE_GFX_VERSION"));
    assert!(
        !env(&base.clone().set("comfyui.gfx_override", ""), &["gfx1031"])
            .contains_key("HSA_OVERRIDE_GFX_VERSION")
    );
    // and the person's own lines of environment cannot set it behind the field's back
    let lines = base.clone().set(
        "comfyui.extra_env",
        "HSA_OVERRIDE_GFX_VERSION=9.0.0\nMIOPEN_FIND_MODE=3\n",
    );
    let e = env(&lines, &["gfx1031"]);
    assert_eq!(e["HSA_OVERRIDE_GFX_VERSION"], "10.3.0");
    assert_eq!(e["MIOPEN_FIND_MODE"], "3");
}

#[test]
fn p3_f14_a_plan_that_makes_no_folders_does_not_say_so() {
    // llama.cpp's plan is what it was: no `mkdir` and no `write` in its JSON (Ollama's, since WP4.16, makes the home and the
    // temporary folder it is given, which T40 holds; it has no `write`)
    let registry = shipped_registry(&[]);
    let mut selection = Selection::new("llamacpp");
    selection.model = Some(quickai_core::plan::ModelInfo {
        id: "m".into(),
        display: "m".into(),
        path: "/m.gguf".into(),
        size_bytes: 1,
    });
    let mut f = facts(&[], &[], "linux");
    f.binary = Some("/bin/x".into());
    let p = plan(&registry, &selection, &f).expect("a plan");
    let text = serde_json::to_string(&p).unwrap();
    assert!(!text.contains("mkdir"), "{text}");
    assert!(!text.contains("\"write\""), "{text}");
}

#[test]
fn p3_p13_the_comfyui_plan_puts_the_bridge_before_it_starts_and_tells_comfyui_to_read_it() {
    let amd = facts(LINUX_AMD, &["gfx1031"], "linux");
    let p = plan_json(&Selection::new("comfyui"), &amd);
    let spawn = spawn_of(&p);
    assert_eq!(
        spawn["write"],
        json!([
            {"asset": "comfyui/extra_model_paths.yaml", "to": "/data/comfyui/extra_model_paths.yaml"},
            {"asset": "comfyui/quickai_bridge", "to": "/data/comfyui/extensions/quickai_bridge"},
            {"asset": "comfyui/quickai_manager", "to": "/data/comfyui/extensions/quickai_manager"},
        ])
    );
    // the file ComfyUI is told to read is the file that is written
    let args = args_of(spawn);
    let at = args
        .iter()
        .position(|a| a == "--extra-model-paths-config")
        .expect("the flag that makes ComfyUI read it");
    assert_eq!(args[at + 1], "/data/comfyui/extra_model_paths.yaml");
    // the bridge is kept with the person's data and not with the models: another models folder moves the models only
    let moved = plan_json(
        &Selection::new("comfyui").set("comfyui.models_dir", "/elsewhere/models"),
        &amd,
    );
    assert_eq!(spawn_of(&moved)["write"], spawn["write"]);
    assert!(args_of(spawn_of(&moved)).contains(&"/elsewhere/models".to_string()));
    // the setting that leaves the page's button to the browser reaches the bridge as a variable; the flag and the
    // files are the same either way, for a flag whose file is not there stops ComfyUI
    assert_eq!(env_of(spawn)["QUICKAI_PAGE_DOWNLOADS"], "install");
    let browser = plan_json(
        &Selection::new("comfyui").set("comfyui.page_downloads", "browser"),
        &amd,
    );
    assert_eq!(
        env_of(spawn_of(&browser))["QUICKAI_PAGE_DOWNLOADS"],
        "browser"
    );
    assert_eq!(spawn_of(&browser)["write"], spawn["write"]);
    assert_eq!(args_of(spawn_of(&browser)), args_of(spawn));
    // and the explanation of the plan says what it will write, and whose it is
    let text =
        render_explain(&plan(&shipped_registry(&[]), &Selection::new("comfyui"), &amd).unwrap());
    assert!(
        text.contains("write /data/comfyui/extensions/quickai_bridge")
            && text.contains("the application's own comfyui/quickai_bridge")
            && text.contains("engine launch.spawn.write"),
        "{text}"
    );
}

#[test]
fn p3_p14_a_document_may_put_only_files_the_application_has_and_names_them_plainly() {
    let doc = |write: &str| {
        format!(
            r#"schema = 1
kind = "engine"
id = "writer"
requires = {{ primitives = ["spawn@1", "probe.http@1"] }}

[engine]
name = "Writer"

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "/bin/writer"
write = [ {write} ]

[launch.ready]
probe = {{ use = "http", path = "/", expect = 200 }}
"#
        )
    };
    let load = |write: &str, assets: Option<&[String]>| {
        let source = quickai_core::config::MemorySource::new(&[(
            "embedded:engines/writer.toml",
            &doc(write),
        )]);
        let mut known = quickai_core::registry::KnownPrimitives::new(common::PROVIDED);
        if let Some(assets) = assets {
            known = known.with_assets(assets);
        }
        let registry = quickai_core::registry::Registry::build(
            quickai_core::config::load(
                &[(
                    quickai_core::config::Layer::Embedded,
                    &source as &dyn quickai_core::config::LayerSource,
                )],
                &quickai_core::config::LoadOptions::default(),
            ),
            &known,
        );
        texts(&registry, Severity::Error).join("\n")
    };
    let has: Vec<String> = [
        "comfyui",
        "comfyui/quickai_bridge",
        "comfyui/extra_model_paths.yaml",
    ]
    .iter()
    .map(|n| n.to_string())
    .collect();
    let right = r#"{ asset = "comfyui/quickai_bridge", to = "${data}/ext/quickai_bridge" }"#;
    assert_eq!(
        load(right, Some(&has)),
        "",
        "a file the application has is accepted"
    );
    assert_eq!(
        load(right, None),
        "",
        "an application that did not say which it has holds nothing to anything"
    );
    // a name the application lacks is an error that says which, and what the nearest is
    let typo = r#"{ asset = "comfyui/quickai_brigde", to = "/x" }"#;
    let errors = load(typo, Some(&has));
    assert!(
        errors.contains("no file called `comfyui/quickai_brigde`")
            && errors.contains("did you mean `comfyui/quickai_bridge`?"),
        "{errors}"
    );
    // a name that is not a plain relative name is refused whatever the application has
    for bad in [
        "../quickai_bridge",
        "/etc/passwd",
        "comfyui//a",
        "comfyui/./a",
        "",
        "c:/a",
        "comfyui\\a",
    ] {
        let entry = format!(
            r#"{{ asset = "{}", to = "/x" }}"#,
            bad.replace('\\', "\\\\")
        );
        let errors = load(&entry, None);
        assert!(
            errors.contains("not the name of one of the application's own files"),
            "{bad:?}: {errors}"
        );
    }
    // where it goes is a template, held to the holes the context has
    let hole = r#"{ asset = "comfyui/quickai_bridge", to = "${nowhere}/x" }"#;
    assert_ne!(
        load(hole, Some(&has)),
        "",
        "a hole that is not in the context is an error"
    );
}
