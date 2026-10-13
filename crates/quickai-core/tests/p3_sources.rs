//! Group Y of docs/phase3-test-plan.md, the pure half: a configuration that can be run on any computer
//! (WP4.19, docs/headless.md). A configuration never carries a model: it says where each file of one can be
//! had from, what whoever runs it has to give, and which program goes with the engine; what belongs to the
//! computer that runs it is said in an environment file or on the command line, and a template of that
//! file is made from the documents. The fetching and the programs are the runtime's.
//!
//! The models are made up (`acme/...`, `Comfy-Org/...` for the shape of a real set) and the fifth engine is
//! only a document, so that nothing here depends on what the application ships beyond its shape.

mod common;

use std::collections::BTreeMap;

use quickai_core::diag::Diagnostic;
use quickai_core::profile::check::{
    open_text, review, EngineHere, FileHere, Inventory, ModelsHere, Opened,
};
use quickai_core::profile::environment::{template, Environment, FileHint, Secret, Where};
use quickai_core::profile::{Configuration, ModelSource, NeedKind, Token};
use quickai_core::registry::Registry;
use quickai_core::value::Value;

const VERSION: &str = "0.1.0";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn person(name: &str) -> String {
    format!("/home/u/.config/quickai/profiles/{name}.toml")
}

fn document(engine: &str, body: &str) -> String {
    format!(
        "schema = 1\nkind = \"profile\"\nid = \"given\"\nname = \"Given\"\nengine = \"{engine}\"\n{body}"
    )
}

fn user_registry(text: &str) -> Registry {
    let file = person("given");
    common::shipped_registry(&[(file.as_str(), text)])
}

fn errors(r: &Registry) -> Vec<String> {
    r.diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(Diagnostic::to_string)
        .collect()
}

fn warnings(r: &Registry) -> Vec<String> {
    r.diagnostics
        .iter()
        .filter(|d| d.severity == quickai_core::diag::Severity::Warning)
        .map(Diagnostic::to_string)
        .collect()
}

/// The configuration the registry read from `text`; panics with the diagnostics when it did not.
fn read(text: &str) -> Configuration {
    let r = user_registry(text);
    r.profile("given")
        .unwrap_or_else(|| panic!("not read: {:?}\n{text}", errors(&r)))
        .configuration
        .clone()
}

/// The errors that kept `text` from being used (and the proof that it was not).
fn refused(text: &str) -> Vec<String> {
    let r = user_registry(text);
    assert!(
        r.profile("given").is_none(),
        "it was used, and should not have been:\n{text}"
    );
    let found = errors(&r);
    assert!(!found.is_empty(), "no error for\n{text}");
    found
}

fn open(text: &str) -> Opened {
    open_text(&common::shipped_registry(&[]), text, VERSION)
        .unwrap_or_else(|n| panic!("{}: {:?}", n.id, n.holes))
}

// ---------------------------------------------------------------------------------------------- Y01-Y05

/// A set of files of the shape a video model has: three kinds, two repositories, a subfolder each, one by
/// address. None of them is in the file; each says where it is and where it goes.
const WAN: &str = r#"
[meta]
note = "Wan 2.2 5B for ComfyUI."

[[model]]
role = "diffusion"
file = "wan2.2_ti2v_5B_fp16.safetensors"
from = "huggingface:Comfy-Org/Wan_2.2_ComfyUI_Repackaged"
path = "split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors"
revision = "0123456789abcdef0123456789abcdef01234567"
into = "diffusion_models"
size = 9999999999
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[model]]
role = "text_encoder"
file = "umt5_xxl_fp8_e4m3fn_scaled.safetensors"
from = "huggingface:Comfy-Org/Wan_2.1_ComfyUI_repackaged"
path = "split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"

[[model]]
role = "vae"
file = "wan2.2_vae.safetensors"
url = "https://mirror.example/wan/wan2.2_vae.safetensors"
into = "vae"
"#;

/// Y01: a file says where each file of a model can be had from, and what it leaves out it leaves out: the
/// place and a name in it, the path in the repository, the revision, the folder it goes in, or an address;
/// it reads back as it was written.
#[test]
fn p3_y01_a_file_model_says_where_it_can_be_had_from_and_reads_back_as_written() {
    let c = read(&document("comfyui", WAN));
    assert_eq!(c.models.len(), 3);
    let diffusion = &c.models[0];
    assert_eq!(diffusion.role, "diffusion");
    assert_eq!(
        diffusion.source,
        ModelSource::File {
            name: "wan2.2_ti2v_5B_fp16.safetensors".into()
        }
    );
    assert_eq!(
        diffusion.from.as_deref(),
        Some("huggingface:Comfy-Org/Wan_2.2_ComfyUI_Repackaged")
    );
    assert_eq!(
        diffusion.path.as_deref(),
        Some("split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors")
    );
    assert_eq!(diffusion.revision.as_deref(), Some(COMMIT));
    assert_eq!(diffusion.into.as_deref(), Some("diffusion_models"));
    assert_eq!(diffusion.size, Some(9_999_999_999));
    assert_eq!(diffusion.sha256.as_deref(), Some(SHA));
    // the second names its place and its path and nothing else; the third an address and a folder
    let encoder = &c.models[1];
    assert!(encoder.revision.is_none() && encoder.into.is_none() && encoder.url.is_none());
    let vae = &c.models[2];
    assert_eq!(
        vae.url.as_deref(),
        Some("https://mirror.example/wan/wan2.2_vae.safetensors")
    );
    assert!(vae.from.is_none() && vae.path.is_none());

    // and written, it is the same configuration
    let r = user_registry(&document("comfyui", WAN));
    let text = c.to_toml(r.engine_map(), VERSION, false);
    for line in [
        "path = \"split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors\"",
        &format!("revision = \"{COMMIT}\""),
        "into = \"diffusion_models\"",
        "url = \"https://mirror.example/wan/wan2.2_vae.safetensors\"",
    ] {
        assert!(text.contains(line), "{line} in\n{text}");
    }
    let again = read(&text);
    assert_eq!(again.models, c.models);
    assert_eq!(again.needs, c.needs);
    assert_eq!(again.runtime, c.runtime);
}

/// Y02: what a place may not be is refused with the key it is in, and the document is not used; what is
/// refused is the kind of thing that climbs out of a folder, signs in, or reaches a computer that is not
/// the person's.
#[test]
fn p3_y02_what_a_place_may_not_be_is_refused_with_the_key_it_is_in() {
    let model = |lines: &str| {
        document(
            "comfyui",
            &format!("[[model]]\nfile = \"x.safetensors\"\n{lines}"),
        )
    };
    let cases: &[(&str, &str)] = &[
        // a path is about the repository `from` names
        (
            "path = \"a/b.safetensors\"\n",
            "`path` says where in the repository",
        ),
        (
            "revision = \"main\"\n",
            "`revision` says where in the repository",
        ),
        // two places
        (
            "from = \"huggingface:a/b\"\nurl = \"https://h.example/x\"\n",
            "once: `from`",
        ),
        // a path that climbs, or is not one
        (
            "from = \"huggingface:a/b\"\npath = \"../x\"\n",
            "`path` is names joined",
        ),
        (
            "from = \"huggingface:a/b\"\npath = \"/etc/passwd\"\n",
            "`path` is names joined",
        ),
        (
            "from = \"huggingface:a/b\"\npath = \"a//b\"\n",
            "`path` is names joined",
        ),
        (
            "from = \"huggingface:a/b\"\npath = \"a\\\\b\"\n",
            "`path` is names joined",
        ),
        (
            "from = \"huggingface:a/b\"\npath = \"C:/x\"\n",
            "`path` is names joined",
        ),
        (
            "from = \"huggingface:a/b\"\npath = \"a/b/c/d/e/f/g/h/i\"\n",
            "`path` is names joined",
        ),
        // a revision that is not a name
        (
            "from = \"huggingface:a/b\"\nrevision = \"a/b\"\n",
            "`revision` is a branch",
        ),
        (
            "from = \"huggingface:a/b\"\nrevision = \"a b\"\n",
            "`revision` is a branch",
        ),
        (
            "from = \"huggingface:a/b\"\nrevision = \"..\"\n",
            "`revision` is a branch",
        ),
        // an address that is not one a file may name
        (
            "url = \"ftp://h.example/x\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"https://user:pw@h.example/x\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"http://h.example/x\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"https://h.example/x y\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"https://h.example/x#frag\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"https:///x\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"https://h.example:0/x\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"file:///etc/passwd\"\n",
            "is not an address a configuration may name",
        ),
        (
            "url = \"javascript:alert(1)\"\n",
            "is not an address a configuration may name",
        ),
        // a folder is one name
        ("into = \"../x\"\n", "`into` is the name of one folder"),
        ("into = \"a/b\"\n", "`into` is the name of one folder"),
        ("into = \"Vae\"\n", "`into` is the name of one folder"),
        ("into = \"\"\n", "`into` is the name of one folder"),
    ];
    for (lines, why) in cases {
        let found = refused(&model(lines));
        assert!(
            found.iter().any(|e| e.contains(why)),
            "{lines:?}: wanted `{why}` in {found:?}"
        );
    }
    // an address that is too long
    let long = format!("url = \"https://h.example/{}\"\n", "a".repeat(2000));
    assert!(refused(&model(&long))
        .iter()
        .any(|e| e.contains("is not an address a configuration may name")));
    // the keys of a place go with a file, not with an entry of the list or a name an engine pulls
    for lines in [
        "[[model]]\ncatalog = \"x\"\nfile = \"y.gguf\"\nurl = \"https://h.example/y\"\n",
        "[[model]]\ncatalog = \"x\"\nfile = \"y.gguf\"\ninto = \"vae\"\n",
        "[[model]]\ncatalog = \"x\"\nfile = \"y.gguf\"\nfrom = \"huggingface:a/b\"\npath = \"c\"\n",
    ] {
        let found = refused(&document("comfyui", lines));
        assert!(
            found
                .iter()
                .any(|e| e.contains("goes with a `file` that is no entry")),
            "{found:?}"
        );
    }
    let found = refused(&document(
        "ollama",
        "[[model]]\nollama = \"qwen3:8b\"\nurl = \"https://h.example/x\"\n",
    ));
    assert!(
        found.iter().any(|e| e.contains("goes with a `file`")),
        "{found:?}"
    );

    // and what is allowed is allowed: an address with a query (a mirror's), this computer's own over http
    for lines in [
        "url = \"https://huggingface.co/a/b/resolve/main/x.safetensors?download=true\"\n",
        "url = \"http://127.0.0.1:9/x.safetensors\"\n",
        "url = \"http://localhost:8000/x.safetensors\"\n",
        "from = \"huggingface:a/b\"\npath = \"x.safetensors\"\nrevision = \"v1.0_rc-2\"\n",
        "into = \"diffusion_models\"\n",
    ] {
        let c = read(&model(lines));
        assert_eq!(c.models.len(), 1, "{lines}");
    }
}

/// Y03: the model is never in the file, and neither is a path of a computer: no key a model has takes a
/// path to a place on a disk, the size of a file is bounded, and what is written for a model has no key but
/// the ones that say who it is and where it can be had from.
#[test]
fn p3_y03_a_configuration_never_carries_a_model_or_a_path_of_a_computer() {
    for lines in [
        "file = \"/home/alice/models/x.safetensors\"\n",
        "file = \"../x.safetensors\"\n",
        "file = \"C:\\\\models\\\\x.safetensors\"\n",
        "file = \"sub/x.safetensors\"\n",
        "file = \"x.safetensors\"\nfrom = \"huggingface:a/b\"\npath = \"/home/alice/x\"\n",
        "file = \"x.safetensors\"\ninto = \"/home/alice\"\n",
    ] {
        let found = refused(&document("comfyui", &format!("[[model]]\n{lines}")));
        assert!(!found.is_empty(), "{lines}");
    }
    // a file that is more than a configuration is (256 KiB) is not read, whatever it holds
    let big = document(
        "comfyui",
        &format!("[meta]\nnote = \"{}\"\n", "x".repeat(300 * 1024)),
    );
    let refused_big = open_text(&common::shipped_registry(&[]), &big, VERSION);
    assert_eq!(refused_big.err().map(|n| n.id), Some("not_a_configuration"));

    // what is written for a model has these keys and no other, however it was made
    let c = read(&document("comfyui", WAN));
    let r = user_registry(&document("comfyui", WAN));
    let text = c.to_toml(r.engine_map(), VERSION, false);
    let table: toml::Table = toml::from_str(&text).unwrap();
    let allowed = [
        "role", "name", "file", "catalog", "size", "sha256", "from", "path", "revision", "url",
        "into",
    ];
    for model in table["model"].as_array().unwrap() {
        for key in model.as_table().unwrap().keys() {
            assert!(allowed.contains(&key.as_str()), "{key} in\n{text}");
        }
    }
    assert!(
        !text.contains("/home/") && !text.contains("/mnt/"),
        "{text}"
    );
}

/// Y04: the note that a model is not here says where it can be had from, the repository and the path in
/// it, or the host and the path of the address, and not the address's query, which may be a secret.
#[test]
fn p3_y04_the_note_that_a_model_is_to_be_got_says_where_it_can_be_had_from() {
    let r = common::shipped_registry(&[]);
    let comfy_without_files = |engine: &str| {
        let mut inventory = Inventory {
            app_version: VERSION.into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            ram_bytes: 64 << 30,
            ..Inventory::default()
        };
        inventory.engines.insert(
            engine.to_string(),
            EngineHere {
                installed: true,
                models: ModelsHere::Files(Vec::<FileHere>::new()),
                ..EngineHere::default()
            },
        );
        inventory
    };
    let opened = open(&document("comfyui", WAN));
    let report = review(
        &r,
        &opened.configuration,
        &opened.notes,
        &comfy_without_files("comfyui"),
        false,
    );
    let said: Vec<String> = report
        .of("model_to_get")
        .iter()
        .map(|n| n.text(r.vocabulary()))
        .collect();
    // the note is about the model the engine loads, which is the first
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains(
            "huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors"
        ),
        "{}",
        said[0]
    );

    // the query of an address is not said
    let opened = open(&document(
        "comfyui",
        "[[model]]\nfile = \"x.safetensors\"\nurl = \"https://files.example/x.safetensors?token=SECRET123\"\n",
    ));
    let report = review(
        &r,
        &opened.configuration,
        &opened.notes,
        &comfy_without_files("comfyui"),
        false,
    );
    let said = report.of("model_to_get")[0].text(r.vocabulary());
    assert!(said.contains("files.example/x.safetensors"), "{said}");
    assert!(
        !said.contains("SECRET123") && !said.contains("token="),
        "{said}"
    );
}

/// Y06: the worked examples of the docs are configurations the application reads whole: the files of Wan 2.2 with
/// the repositories they come from, the commits that were checked, their sizes and checksums (read from the hub and
/// held against the files of a computer that has them); and a chat with a pinned Open WebUI that needs a key.
#[test]
fn p3_y06_the_worked_examples_are_configurations_the_application_reads_whole() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/profiles");
    let read_example = |name: &str| -> (Registry, String) {
        let text =
            std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let file = format!("/home/u/.config/quickai/profiles/{name}");
        (
            common::shipped_registry(&[(file.as_str(), text.as_str())]),
            text,
        )
    };

    let (r, _) = read_example("wan22-ti2v-turbo.toml");
    assert_eq!(errors(&r), Vec::<String>::new());
    assert_eq!(warnings(&r), Vec::<String>::new());
    let c = &r.profile("wan22-ti2v-turbo").expect("read").configuration;
    assert_eq!(c.engine, "comfyui");
    type Row<'a> = (&'a str, &'a str, Option<&'a str>, Option<&'a str>, u64);
    let rows: Vec<Row> = c
        .models
        .iter()
        .map(|m| {
            (
                m.role.as_str(),
                m.from.as_deref().unwrap(),
                m.path.as_deref(),
                m.into.as_deref(),
                m.size.unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "diffusion",
                "huggingface:hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF",
                None,
                Some("diffusion_models"),
                5_404_990_176
            ),
            (
                "text_encoder",
                "huggingface:city96/umt5-xxl-encoder-gguf",
                None,
                Some("text_encoders"),
                6_043_068_256
            ),
            (
                "vae",
                "huggingface:QuantStack/Wan2.2-TI2V-5B-GGUF",
                Some("VAE/Wan2.2_VAE.safetensors"),
                Some("vae"),
                1_409_400_960
            ),
        ]
    );
    for model in &c.models {
        assert_eq!(
            model.revision.as_ref().map(String::len),
            Some(40),
            "a commit: {model:?}"
        );
        assert_eq!(model.sha256.as_ref().map(String::len), Some(64));
    }
    // and it is the same file when it is written again
    let text = c.to_toml(r.engine_map(), VERSION, false);
    let file = "/home/u/.config/quickai/profiles/again.toml";
    let again = common::shipped_registry(&[(file, text.as_str())]);
    assert_eq!(
        again
            .profile("wan22-ti2v-turbo")
            .unwrap()
            .configuration
            .models,
        c.models
    );

    let (r, _) = read_example("chat-with-open-webui.toml");
    assert_eq!(errors(&r), Vec::<String>::new());
    assert_eq!(warnings(&r), Vec::<String>::new());
    let c = &r
        .profile("chat-with-open-webui")
        .expect("read")
        .configuration;
    assert_eq!(c.engine, "ollama");
    assert_eq!(
        c.runtime.companions["openwebui"].build.as_deref(),
        Some("0.11.4")
    );
    assert_eq!(c.values["openwebui.enabled"], Value::Bool(true));
    assert_eq!(c.needs.len(), 1);
    assert!(c.needs[0].secret, "the key that signs logins is a secret");
}

/// Y70: the examples of MiniMax H3 are configurations the application reads whole: the files of the model from one repository of the hub at
/// one commit, each in the folder of its kind with its size and checksum (read from the hub's answer for the file at that commit,
/// 2026-10-13); the models folder as a need, because tens of GB are not for a system disk; no pin and no custom node; and the territory of
/// the licence said where whoever opens the file reads it. There are two: the lean one with the build for first and last frame (5 files), and
/// the whole one with the build for reference pictures as well (7 files, 65 GB).
#[test]
fn p3_y70_the_h3_examples_are_configurations_the_application_reads_whole() {
    // role, file, folder (which is also the folder of the repository the file is in), size
    const FILES: [(&str, &str, &str, u64); 7] = [
        (
            "diffusion",
            "minimax_h3_fl2va_pruned_int8_convrot.safetensors",
            "diffusion_models",
            20_970_379_616,
        ),
        (
            "diffusion",
            "minimax_h3_ref2va_pruned_int8_convrot.safetensors",
            "diffusion_models",
            20_970_379_616,
        ),
        (
            "text_encoder",
            "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
            "text_encoders",
            15_687_142_551,
        ),
        (
            "vae",
            "minimax_h3_video_vae_int8_convrot.safetensors",
            "vae",
            2_811_065_184,
        ),
        (
            "vae",
            "minimax_h3_audio_vae_fp32.safetensors",
            "vae",
            605_254_808,
        ),
        (
            "lora",
            "minimax_h3_fl2v_turbo_8step_v1.0_comfyui_bf16.safetensors",
            "loras",
            1_956_193_000,
        ),
        (
            "lora",
            "minimax_h3_ref2v_turbo_4step_v0.1_comfyui_bf16.safetensors",
            "loras",
            1_956_193_000,
        ),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/profiles");
    // (the example, which of FILES it holds, what it sums to and what its need says)
    let examples: [(&str, &[usize], u64, &str); 2] = [
        (
            "minimax-h3-fl2va",
            &[0, 2, 3, 4, 5],
            42_030_035_159,
            "42 GB",
        ),
        (
            "minimax-h3",
            &[0, 1, 2, 3, 4, 5, 6],
            64_956_607_775,
            "65 GB",
        ),
    ];
    for (id, held, total, said) in examples {
        let text = std::fs::read_to_string(dir.join(format!("{id}.toml")))
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        let file = person(id);
        let r = common::shipped_registry(&[(file.as_str(), text.as_str())]);
        assert_eq!(errors(&r), Vec::<String>::new(), "{id}");
        assert_eq!(warnings(&r), Vec::<String>::new(), "{id}");
        let c = &r.profile(id).expect("read").configuration;
        assert_eq!(c.engine, "comfyui", "{id}");
        let rows: Vec<(&str, &str, String, &str, u64)> = c
            .models
            .iter()
            .map(|m| {
                let ModelSource::File { name } = &m.source else {
                    panic!("{id}: a file, not a name or an entry of the list: {m:?}")
                };
                (
                    m.role.as_str(),
                    name.as_str(),
                    m.path.clone().unwrap(),
                    m.into.as_deref().unwrap(),
                    m.size.unwrap(),
                )
            })
            .collect();
        let wanted: Vec<(&str, &str, String, &str, u64)> = held
            .iter()
            .map(|&i| {
                let (role, name, folder, size) = FILES[i];
                (role, name, format!("{folder}/{name}"), folder, size)
            })
            .collect();
        assert_eq!(rows, wanted, "{id}");
        assert_eq!(
            c.models.iter().map(|m| m.size.unwrap()).sum::<u64>(),
            total,
            "{id}: the sizes the hub gave"
        );
        for model in &c.models {
            assert_eq!(
                model.from.as_deref(),
                Some("huggingface:Comfy-Org/MiniMax-H3"),
                "{id}: {model:?}"
            );
            assert_eq!(
                model.revision.as_deref(),
                Some("e5eb578a89295337b8ff433a035929ce0279e0b6"),
                "{id}: one commit for all, so the files belong together for ever: {model:?}"
            );
            assert_eq!(
                model.sha256.as_ref().map(String::len),
                Some(64),
                "{id}: {model:?}"
            );
        }
        // the folder of the models, which a person says, and what the computer has to have, which it is looked at for
        // (WP4.20: the compiler and the headers of Python are what the first clip of a real run died of)
        assert_eq!(c.needs.len(), 6, "{id}");
        assert_eq!(
            c.needs[0].kind,
            NeedKind::Setting {
                key: "comfyui.models_dir".into()
            },
            "{id}"
        );
        assert!(
            c.needs[0].why.as_deref().unwrap().contains(said),
            "{id}: the need says how much: {:?}",
            c.needs[0].why
        );
        let system: Vec<&str> = c.needs[1..]
            .iter()
            .map(|n| match &n.kind {
                NeedKind::System { id } => id.as_str(),
                other => panic!("{id}: {other:?}"),
            })
            .collect();
        assert_eq!(
            system,
            [
                "python_venv",
                "nvidia_driver",
                "c_compiler",
                "python_headers",
                "triton"
            ],
            "{id}"
        );
        assert!(
            c.needs[1..]
                .iter()
                .all(|n| n.why.as_deref().is_some_and(|w| w.len() > 40)),
            "{id}: each says why"
        );
        assert_eq!(c.runtime.build, None, "{id}: no pin: the build the document was tested with is the one, as for the Wan example");
        assert_eq!(c.values["comfyui.port"], Value::Int(8188), "{id}");
        let note = c.meta.note.as_deref().unwrap();
        for place in ["EU", "UK", "South Korea", "USA"] {
            assert!(note.contains(place), "{id}: the licence's territory is in the note, which is what a window shows: {note}");
        }
        // and it is the same file when it is written again
        let again_text = c.to_toml(r.engine_map(), VERSION, false);
        let again_file = "/home/u/.config/quickai/profiles/again.toml";
        let again = common::shipped_registry(&[(again_file, again_text.as_str())]);
        assert_eq!(
            again.profile(id).unwrap().configuration.models,
            c.models,
            "{id}"
        );
        assert_eq!(
            again.profile(id).unwrap().configuration.needs,
            c.needs,
            "{id}"
        );
    }
}

/// The files of one release of Ollama, as the recorded list of the real releases has them
/// (`crates/quickai-runtime/tests/fixtures/ollama/github_releases.json`).
const OLLAMA_RELEASE_FILES: [&str; 17] = [
    "install.ps1",
    "install.sh",
    "ollama-darwin.tgz",
    "Ollama-darwin.zip",
    "ollama-linux-amd64-mlx.tar.zst",
    "ollama-linux-amd64-rocm.tar.zst",
    "ollama-linux-amd64.tar.zst",
    "ollama-linux-arm64-jetpack5.tar.zst",
    "ollama-linux-arm64-jetpack6.tar.zst",
    "ollama-linux-arm64.tar.zst",
    "ollama-windows-amd64-mlx.zip",
    "ollama-windows-amd64-rocm.zip",
    "ollama-windows-amd64.zip",
    "ollama-windows-arm64.zip",
    "Ollama.dmg",
    "OllamaSetup.exe",
    "sha256sum.txt",
];

/// Y68: a build pinned in an example is named the way the list of builds names it, the release and the file it comes
/// in (`v0.35.1-ollama-linux-amd64`), not by the release alone: a bare release is not in that list, and a program run
/// with it stops with the names that are. The first versions of two examples pinned `v0.35.1` and `v0.40.0`; no test
/// said so, and the run for real did (docs/headless.md, section 11).
#[test]
fn p3_y68_a_build_pinned_in_an_example_is_named_the_way_the_list_of_builds_names_it() {
    use quickai_core::acquire::{builds_from_source, install_label, matching_assets};
    use quickai_core::plan::HostFacts;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/profiles");
    let mut pinned = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let id = path.file_stem().unwrap().to_str().unwrap().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        let file = person(&id);
        let r = common::shipped_registry(&[(file.as_str(), text.as_str())]);
        // a sketch that is not a configuration of this version has no pin to hold
        let Some(entry) = r.profile(&id) else {
            continue;
        };
        let c = &entry.configuration;
        let Some(pin) = c.runtime.build.as_deref() else {
            continue;
        };
        let engine = r
            .engine(&c.engine)
            .unwrap_or_else(|| panic!("{id}: no engine {}", c.engine));
        if builds_from_source(engine) {
            // a program built from a release is labelled by the release alone
            pinned.push((id, pin.to_string()));
            continue;
        }
        assert_eq!(
            c.engine, "ollama",
            "{id}: pins a build of a program whose release files this test does not have: add them above"
        );
        let facts = HostFacts {
            os: "linux".into(),
            arch: "x86_64".into(),
            ..Default::default()
        };
        let found = matching_assets(engine, &OLLAMA_RELEASE_FILES, &facts);
        assert_eq!(found.len(), 1, "one file is the build for this computer");
        let asset = OLLAMA_RELEASE_FILES[found[0]];
        // what follows the release in a label: the file it comes in, as the document's rule writes it
        let ending = install_label(engine, "", asset);
        let release = pin.strip_suffix(ending.as_str()).unwrap_or_else(|| {
            panic!(
                "{id}: `{pin}` is not named as the list of builds names a build (the release and the file, like `v0.35.1{ending}`)"
            )
        });
        assert!(
            release.starts_with('v') && release.len() > 1,
            "{id}: `{pin}` has no release in it"
        );
        assert_eq!(install_label(engine, release, asset), pin, "{id}");
        pinned.push((id, pin.to_string()));
    }
    pinned.sort();
    let ids: Vec<&str> = pinned.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        ["chat-with-open-webui", "vision-chat"],
        "the examples that pin a build: {pinned:?}"
    );
}

/// Y07: what the application remembers of where a file was fetched from reaches an export: the place, the
/// path in the repository when it is not at the top and the revision when it is not `main`; they mean something
/// only with the place, and what is not the shape of either is not written.
#[test]
fn p3_y07_an_export_says_where_in_the_repository_a_fetched_file_is() {
    use quickai_core::profile::state::{capture, ModelFacts, WorkingState};
    let r = common::shipped_registry(&[]);
    let mut state = WorkingState {
        engine: "comfyui".into(),
        ..WorkingState::default()
    };
    state.values.insert(
        "comfyui.last_model".into(),
        Value::Str("Wan2.2_VAE.safetensors".into()),
    );
    let facts = |source: Option<&str>, path: Option<&str>, revision: Option<&str>| ModelFacts {
        size: Some(1_409_400_960),
        source: source.map(str::to_string),
        path: path.map(str::to_string),
        revision: revision.map(str::to_string),
        ..ModelFacts::default()
    };
    let first = |got: quickai_core::profile::state::Captured| got.configuration.models[0].clone();
    let pick = first(capture(
        &r,
        &state,
        &BTreeMap::new(),
        Some(&facts(
            Some("huggingface:QuantStack/Wan2.2-TI2V-5B-GGUF"),
            Some("VAE/Wan2.2_VAE.safetensors"),
            Some("57437632ddd08bdcbd1508c866aa22e126ed51d2"),
        )),
    ));
    assert_eq!(
        pick.from.as_deref(),
        Some("huggingface:QuantStack/Wan2.2-TI2V-5B-GGUF")
    );
    assert_eq!(pick.path.as_deref(), Some("VAE/Wan2.2_VAE.safetensors"));
    assert_eq!(
        pick.revision.as_deref(),
        Some("57437632ddd08bdcbd1508c866aa22e126ed51d2")
    );
    // and written, with the keys of a place
    let c = Configuration {
        id: "x".into(),
        name: "x".into(),
        engine: "comfyui".into(),
        models: vec![pick],
        ..Configuration::default()
    };
    let text = c.to_toml(r.engine_map(), VERSION, false);
    assert!(
        text.contains("path = \"VAE/Wan2.2_VAE.safetensors\"")
            && text.contains("revision = \"5743"),
        "{text}"
    );
    // a path with no place, and a path or a revision that is no path or revision, are not written
    for (source, path, revision) in [
        (None, Some("VAE/x"), Some("main")),
        (Some("huggingface:a/b"), Some("../x"), Some("a b")),
    ] {
        let pick = first(capture(
            &r,
            &state,
            &BTreeMap::new(),
            Some(&facts(source, path, revision)),
        ));
        assert_eq!(
            (pick.path, pick.revision),
            (None, None),
            "{source:?} {path:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------- Y10-Y12

/// Y10: what a file needs from whoever runs it is read, written and read again: a login, a setting that
/// belongs to a computer, a variable of a program's environment (for the engine, or for a program that goes
/// with it); a login is always a secret, and so is a variable whose name looks like one.
#[test]
fn p3_y10_what_a_file_needs_from_whoever_runs_it_is_read_and_written() {
    let body = r#"
[[needs]]
token = "huggingface"
why = "The Wan repositories ask you to accept their licence first."

[[needs]]
setting = "ollama.models_dir"

[[needs]]
env = "OLLAMA_NUM_PARALLEL"

[[needs]]
env = "WEBUI_SECRET_KEY"
for = "openwebui"
why = "Signs the logins; keep it the same between runs."

[[needs]]
env = "WEBUI_NAME"
for = "openwebui"
secret = true
"#;
    let c = read(&document("ollama", body));
    assert_eq!(c.needs.len(), 5);
    assert_eq!(c.needs[0].kind, NeedKind::Token(Token::Huggingface));
    assert!(c.needs[0].secret);
    assert_eq!(
        c.needs[0].why.as_deref(),
        Some("The Wan repositories ask you to accept their licence first.")
    );
    assert_eq!(
        c.needs[1].kind,
        NeedKind::Setting {
            key: "ollama.models_dir".into()
        }
    );
    assert!(!c.needs[1].secret);
    // a variable is the engine's unless the file says another program
    assert_eq!(
        c.needs[2].kind,
        NeedKind::Env {
            program: "ollama".into(),
            name: "OLLAMA_NUM_PARALLEL".into()
        }
    );
    assert!(!c.needs[2].secret);
    assert_eq!(
        c.needs[3].kind,
        NeedKind::Env {
            program: "openwebui".into(),
            name: "WEBUI_SECRET_KEY".into()
        }
    );
    assert!(c.needs[3].secret, "a name with SECRET in it is a secret");
    assert!(c.needs[4].secret, "and so is one that says so");
    assert_eq!(c.needs[3].label(), "WEBUI_SECRET_KEY (for openwebui)");
    assert_eq!(c.needs[0].label(), "a Hugging Face token");

    let r = user_registry(&document("ollama", body));
    let text = c.to_toml(r.engine_map(), VERSION, false);
    assert!(
        text.contains("[[needs]]") && text.contains("token = \"huggingface\""),
        "{text}"
    );
    assert_eq!(read(&text).needs, c.needs, "{text}");
}

/// Y11: a need that is not one is refused with its place: it is exactly one of a token, a setting and a
/// variable; a token is of a site this knows; a setting is one of the engine or of what goes with it; a
/// variable has a name and goes to a program that takes lines of environment; and there are not many.
#[test]
fn p3_y11_a_need_that_is_not_one_is_refused() {
    let cases: &[(&str, &str)] = &[
        (
            "[[needs]]\n",
            "exactly one of `token`, `setting`, `env` and `system`",
        ),
        (
            "[[needs]]\ntoken = \"huggingface\"\nenv = \"X\"\n",
            "exactly one of",
        ),
        (
            "[[needs]]\ntoken = \"gitlab\"\n",
            "is not a site this takes a token for",
        ),
        (
            "[[needs]]\nsetting = \"ollama.nothing\"\n",
            "is not a setting of this engine",
        ),
        (
            "[[needs]]\nsetting = \"comfyui.models_dir\"\n",
            "is not a setting of this engine",
        ),
        (
            "[[needs]]\nsetting = \"ollama\"\n",
            "is not a setting of this engine",
        ),
        (
            "[[needs]]\nsetting = \"ollama.last_model\"\n",
            "is not a setting of this engine",
        ),
        (
            "[[needs]]\nenv = \"1BAD\"\n",
            "is not the name of a variable",
        ),
        (
            "[[needs]]\nenv = \"A B\"\n",
            "is not the name of a variable",
        ),
        (
            "[[needs]]\nenv = \"X\"\nfor = \"comfyui\"\n",
            "does not take lines of environment",
        ),
    ];
    for (body, why) in cases {
        let found = refused(&document("ollama", body));
        assert!(
            found
                .iter()
                .any(|e| e.contains(why) || e.contains("takes lines of environment")),
            "{body:?}: wanted `{why}` in {found:?}"
        );
    }
    let many: String = (0..17)
        .map(|i| format!("[[needs]]\nenv = \"V{i}\"\n"))
        .collect();
    assert!(refused(&document("ollama", &many))
        .iter()
        .any(|e| e.contains("at most 16")));
    // a program with no lines of environment cannot be given a variable
    let found = refused(&document("llamacpp", "[[needs]]\nenv = \"X\"\n"));
    assert!(
        found
            .iter()
            .any(|e| e.contains("takes lines of environment")),
        "{found:?}"
    );
}

/// Y12: a program that goes with the engine can be pinned to a build (`[runtime.openwebui]`), the pin is
/// read and written, it is for a program that goes with this engine and no other, and a key in it that is
/// not a build is told as one this version does not know.
#[test]
fn p3_y12_a_program_that_goes_with_the_engine_can_be_pinned_to_a_build() {
    let c = read(&document(
        "ollama",
        "[runtime]\nbuild = \"v0.35.1\"\n\n[runtime.openwebui]\nbuild = \"0.11.4\"\nversion = \"0.11.4\"\n\n[values.openwebui]\nenabled = true\n",
    ));
    assert_eq!(c.runtime.build.as_deref(), Some("v0.35.1"));
    let pin = &c.runtime.companions["openwebui"];
    assert_eq!(pin.build.as_deref(), Some("0.11.4"));
    assert_eq!(pin.version.as_deref(), Some("0.11.4"));
    let r = user_registry(&document(
        "ollama",
        "[runtime]\nbuild = \"v0.35.1\"\n\n[runtime.openwebui]\nbuild = \"0.11.4\"\n",
    ));
    let text = c.to_toml(r.engine_map(), VERSION, false);
    assert!(
        text.contains("[runtime.openwebui]") && text.contains("build = \"0.11.4\""),
        "{text}"
    );
    assert_eq!(read(&text).runtime, c.runtime);

    // a pin for a program that does not go with this engine is not one: it is told, and nothing is pinned
    let text = document("comfyui", "[runtime.openwebui]\nbuild = \"0.11.4\"\n");
    let r = user_registry(&text);
    let c = &r.profile("given").expect("still usable").configuration;
    assert!(c.runtime.companions.is_empty());
    assert!(
        warnings(&r).iter().any(|w| w.contains("runtime.openwebui")),
        "{:?}",
        warnings(&r)
    );
    // a key that is no build is told
    let text = document(
        "ollama",
        "[runtime.openwebui]\nbuild = \"0.11.4\"\ncolour = \"red\"\n",
    );
    let r = user_registry(&text);
    assert!(r.profile("given").is_some());
    assert!(
        warnings(&r)
            .iter()
            .any(|w| w.contains("runtime.openwebui.colour")),
        "{:?}",
        warnings(&r)
    );
    // and a label is a name, not a path
    let found = refused(&document(
        "ollama",
        "[runtime.openwebui]\nbuild = \"../x\"\n",
    ));
    assert!(
        found
            .iter()
            .any(|e| e.contains("is not the label of a build")),
        "{found:?}"
    );
}

// ---------------------------------------------------------------------------------------------- Y20-Y26

const SERVER: &str = r#"
schema = 1

[fetch]
huggingface_token = "hf_THE_TOKEN"
huggingface_endpoint = "https://hf-mirror.example/"
github_token = ""

[values.ollama]
port = 11500
models_dir = "/mnt/big/ollama"
extra_env = "OLLAMA_KEEP_ALIVE=24h"

[values.openwebui]
port = 3100
python = "/usr/bin/python3.12"

[env.openwebui]
WEBUI_SECRET_KEY = "s3cr3t-value"
ENABLE_SIGNUP = false
WEBUI_NAME = "Studio"
EMPTY_ONE = ""

[models]
"wan2.2_vae.safetensors" = "/srv/wan/wan2.2_vae.safetensors"
"umt5.safetensors" = "https://mirror.example/umt5.safetensors"
"x.safetensors" = "huggingface:me/my-copy"
"nothing.safetensors" = ""
"#;

fn env_of(text: &str, engine: &str) -> Result<(Environment, Vec<String>), Vec<String>> {
    let r = common::shipped_registry(&[]);
    Environment::read(text, "environment file server.toml", r.engine_map(), engine)
        .map_err(|problems| problems.iter().map(ToString::to_string).collect())
}

/// Y20: the environment file is read: the logins, a mirror, the settings of the engine and of the program
/// that goes with it (including the ones a configuration cannot carry), the lines of a program's
/// environment, and where to get a file from; a value left empty is not set; where each thing was said is
/// remembered; a key this version does not know is told.
#[test]
fn p3_y20_the_environment_file_is_read_and_says_where_each_thing_was_said() {
    let (env, unknown) = env_of(SERVER, "ollama").expect("read");
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(
        env.fetch.huggingface_token.as_ref().map(Secret::expose),
        Some("hf_THE_TOKEN")
    );
    assert_eq!(
        env.fetch.huggingface_endpoint.as_deref(),
        Some("https://hf-mirror.example"),
        "the slash at the end goes"
    );
    assert!(
        env.fetch.github_token.is_none(),
        "an empty value is not set"
    );
    assert_eq!(env.values["ollama.port"], Value::Int(11500));
    assert_eq!(
        env.values["ollama.models_dir"],
        Value::Str("/mnt/big/ollama".into())
    );
    assert_eq!(env.values["openwebui.port"], Value::Int(3100));
    let lines: Vec<(&str, &str)> = env.env["openwebui"]
        .iter()
        .map(|l| (l.name.as_str(), l.value.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            ("ENABLE_SIGNUP", "false"),
            ("WEBUI_NAME", "Studio"),
            ("WEBUI_SECRET_KEY", "s3cr3t-value")
        ],
        "a switch is text, and an empty one is not set"
    );
    assert_eq!(
        env.models["wan2.2_vae.safetensors"],
        Where::Path("/srv/wan/wan2.2_vae.safetensors".into())
    );
    assert_eq!(
        env.models["umt5.safetensors"],
        Where::Url("https://mirror.example/umt5.safetensors".into())
    );
    assert_eq!(
        env.models["x.safetensors"],
        Where::Place("huggingface:me/my-copy".into())
    );
    assert!(!env.models.contains_key("nothing.safetensors"));
    assert_eq!(
        env.origins["values.ollama.models_dir"],
        "environment file server.toml"
    );

    // a key this version does not know is told, with the file
    let (_, unknown) = env_of("[fetch]\ncolour = \"red\"\n", "ollama").expect("read");
    assert_eq!(unknown.len(), 1, "{unknown:?}");
    assert!(
        unknown[0].contains("server.toml") && unknown[0].contains("fetch.colour"),
        "{unknown:?}"
    );
    // a login left empty, or only spaces, is not one, whichever site it is for
    for blank in ["", "   "] {
        let (env, _) = env_of(
            &format!("[fetch]\nhuggingface_token = \"{blank}\"\ngithub_token = \"{blank}\"\nhuggingface_endpoint = \"{blank}\"\n"),
            "ollama",
        )
        .expect("read");
        assert_eq!(
            env.fetch,
            quickai_core::profile::environment::Fetch::default(),
            "{blank:?}"
        );
    }
    // an empty file, and one with only comments, say nothing
    for text in ["", "# nothing\n", "schema = 1\n[fetch]\n[models]\n"] {
        let (env, _) = env_of(text, "ollama").expect("read");
        assert_eq!(env, Environment::default(), "{text:?}");
    }
}

/// Y21: what is wrong in the file is named with the file and the place, all of it at once: not TOML (with
/// the line), a version this does not read, an engine that is not the configuration's, a setting the engine
/// has not or that is not one a file can give, a value that does not fit its setting, a variable that is not
/// a name or is not one line, a program that takes no lines, an address that is not one.
#[test]
fn p3_y21_what_is_wrong_in_the_file_is_named_with_its_place() {
    let cases: &[(&str, &str)] = &[
        ("[fetch\nx = 1\n", "server.toml:1: not a TOML file"),
        ("schema = 2\n", "schema"),
        (
            "[values.comfyui]\nport = 1\n",
            "`comfyui` is not the engine of this configuration",
        ),
        (
            "[values.ollama]\nnothing = 1\n",
            "Ollama has no setting called `nothing`",
        ),
        (
            "[values.ollama]\nlast_model = \"x\"\n",
            "that is not a setting a file can give",
        ),
        ("[values.ollama]\nport = 99999999\n", "values.ollama.port"),
        ("[values.ollama]\nport = \"eighty\"\n", "values.ollama.port"),
        ("[values.ollama]\ngpu = \"purple\"\n", "values.ollama.gpu"),
        (
            "[env.ollama]\n\"1BAD\" = \"x\"\n",
            "that is not the name of a variable",
        ),
        (
            "[env.ollama]\nA = \"x\\ny\"\n",
            "a variable's value is one line",
        ),
        ("[env.ollama]\nA = [1]\n", "a variable has a text"),
        (
            "[env.nothing]\nA = \"x\"\n",
            "`nothing` is not the engine of this configuration",
        ),
        (
            "[fetch]\nhuggingface_endpoint = \"hf-mirror\"\n",
            "is not the address of a site",
        ),
        (
            "[fetch]\nhuggingface_endpoint = \"https://h.example/some/path\"\n",
            "no path",
        ),
        (
            "[fetch]\nhuggingface_endpoint = \"https://u:p@h.example\"\n",
            "no sign-in",
        ),
        (
            "[models]\n\"a/b.gguf\" = \"/x\"\n",
            "a file is a name, with no folder in it",
        ),
        (
            "[models]\n\"x.gguf\" = \"https://u:p@h.example/x\"\n",
            "is not an address",
        ),
    ];
    for (text, why) in cases {
        let found = env_of(text, "ollama").expect_err(text);
        assert!(
            found.iter().any(|e| e.contains(why)),
            "{text:?}: wanted `{why}` in {found:?}"
        );
        assert!(
            found.iter().all(|e| e.contains("server.toml")),
            "every problem names the file: {found:?}"
        );
    }
    // several problems are all told
    let found = env_of(
        "[values.ollama]\nport = 0\nnothing = 1\n[env.ollama]\n\"1\" = \"x\"\n",
        "ollama",
    )
    .expect_err("three");
    assert!(found.len() >= 3, "{found:?}");
    // llama.cpp takes no lines of environment
    let found = env_of("[env.llamacpp]\nA = \"x\"\n", "llamacpp").expect_err("no lines");
    assert!(
        found
            .iter()
            .any(|e| e.contains("takes no lines of environment")),
        "{found:?}"
    );
    // the settings a configuration cannot carry can be given here: that is what this file is for
    let (env, _) = env_of(
        "[values.ollama]\nmodels_dir = \"/x\"\nbin_override = \"/y\"\n",
        "ollama",
    )
    .expect("the ones that never travel");
    assert_eq!(env.values.len(), 2);
}

/// Y22: the words of a command line: `--set` reads a text as the type of the setting, `--env` takes a
/// line for the engine or for the program named before a colon, `--model-source` a file and where.
#[test]
fn p3_y22_the_words_of_the_command_line_are_read_as_what_they_are_for() {
    let r = common::shipped_registry(&[]);
    let engines = r.engine_map();
    let mut env = Environment::default();
    env.set_text(engines, "ollama", "ollama.port", "11600", "--set")
        .unwrap();
    env.set_text(
        engines,
        "ollama",
        "ollama.use_default_store",
        "yes",
        "--set",
    )
    .unwrap();
    env.set_text(engines, "ollama", "ollama.gpu", "off", "--set")
        .unwrap();
    env.set_text(
        engines,
        "ollama",
        "openwebui.extra_env",
        "A=1\nB=2",
        "--set",
    )
    .unwrap();
    assert_eq!(env.values["ollama.port"], Value::Int(11600));
    assert_eq!(env.values["ollama.use_default_store"], Value::Bool(true));
    assert_eq!(env.values["ollama.gpu"], Value::Str("off".into()));
    assert_eq!(
        env.values["openwebui.extra_env"],
        Value::Str("A=1\nB=2".into())
    );
    for (key, text, why) in [
        ("ollama", "1", "not `<engine>.<field>`"),
        ("nothing.port", "1", "`nothing` is not the engine"),
        (
            "ollama.nothing",
            "1",
            "Ollama has no setting called `nothing`",
        ),
        ("ollama.port", "eighty", "`eighty` is not a whole number"),
        ("ollama.port", "0", "ollama.port"),
        ("ollama.use_default_store", "maybe", "is not true or false"),
        ("ollama.gpu", "purple", "ollama.gpu"),
        ("ollama.last_model", "x", "has no setting called"),
    ] {
        let wrong = env
            .set_text(engines, "ollama", key, text, "--set")
            .expect_err(key);
        assert!(wrong.contains(why), "{key}={text}: {wrong}");
    }

    // a line is the engine's unless a program is named
    env.add_env_line(engines, "ollama", "OLLAMA_FLASH_ATTENTION=1", "--env")
        .unwrap();
    env.add_env_line(engines, "ollama", "openwebui:ENABLE_SIGNUP=false", "--env")
        .unwrap();
    env.add_env_line(engines, "ollama", "openwebui:WEBUI_NAME=a=b", "--env")
        .unwrap();
    assert_eq!(env.env["ollama"][0].name, "OLLAMA_FLASH_ATTENTION");
    assert_eq!(
        env.env["openwebui"][1].value, "a=b",
        "the value is what follows the first `=`"
    );
    for (spec, why) in [
        ("NOEQUALS", "is not `NAME=VALUE`"),
        ("1BAD=x", "is not the name of a variable"),
        ("nothing:A=1", "`nothing` is not the engine"),
        ("A=1\n2", "one line"),
    ] {
        let wrong = env
            .add_env_line(engines, "ollama", spec, "--env")
            .expect_err(spec);
        assert!(wrong.contains(why), "{spec}: {wrong}");
    }
    let wrong = Environment::default()
        .add_env_line(engines, "llamacpp", "A=1", "--env")
        .expect_err("llama.cpp takes no lines");
    assert!(wrong.contains("takes no lines of environment"), "{wrong}");

    // a file and where
    let mut env = Environment::default();
    env.add_model_source("a.gguf=/srv/a.gguf", "--model-source")
        .unwrap();
    env.add_model_source("b.gguf=https://mirror.example/b.gguf", "--model-source")
        .unwrap();
    env.add_model_source("c.gguf=huggingface:me/c", "--model-source")
        .unwrap();
    env.add_model_source("d.gguf=http://10.0.0.5:8000/d.gguf", "--model-source")
        .unwrap();
    assert_eq!(env.models.len(), 4);
    assert_eq!(
        env.models["d.gguf"],
        Where::Url("http://10.0.0.5:8000/d.gguf".into())
    );
    for (spec, why) in [
        ("nothing", "is not `FILE=WHERE`"),
        ("a/b.gguf=/x", "is not a file name"),
        ("a.gguf=", "nothing to get it from"),
        ("a.gguf=https://u:p@h/x", "is not an address"),
    ] {
        let wrong = env
            .add_model_source(spec, "--model-source")
            .expect_err(spec);
        assert!(wrong.contains(why), "{spec}: {wrong}");
    }
}

/// Y23: layers are put together so that what is said last wins, the lines of environment follow one another,
/// and nothing that is shown of them shows a secret: not the structure, not the sentences that say where
/// each thing was said.
#[test]
fn p3_y23_layers_are_put_together_and_a_secret_is_never_shown() {
    let r = common::shipped_registry(&[]);
    let engines = r.engine_map();
    let (file, _) = env_of(SERVER, "ollama").unwrap();
    let mut env = Environment::default();
    env.merge(file);
    let mut flags = Environment::default();
    flags
        .set_text(engines, "ollama", "ollama.port", "12000", "--set")
        .unwrap();
    flags
        .add_env_line(engines, "ollama", "openwebui:WEBUI_NAME=Other", "--env")
        .unwrap();
    flags
        .set_fetch("huggingface_token", "hf_FROM_THE_FLAG", "--hf-token")
        .unwrap();
    flags
        .set_fetch("github_token", "ghp_GITHUB_TOKEN", "GITHUB_TOKEN")
        .unwrap();
    env.merge(flags);
    assert_eq!(
        env.values["ollama.port"],
        Value::Int(12000),
        "the later layer wins"
    );
    assert_eq!(
        env.values["ollama.models_dir"],
        Value::Str("/mnt/big/ollama".into()),
        "and the rest stays"
    );
    assert_eq!(
        env.fetch.huggingface_token.as_ref().map(Secret::expose),
        Some("hf_FROM_THE_FLAG")
    );
    assert_eq!(env.origins["values.ollama.port"], "--set");
    assert_eq!(env.origins["fetch.huggingface_token"], "--hf-token");
    let names: Vec<&str> = env.env["openwebui"]
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "ENABLE_SIGNUP",
            "WEBUI_NAME",
            "WEBUI_SECRET_KEY",
            "WEBUI_NAME"
        ],
        "the lines follow one another (the later of a name wins where they are used)"
    );

    // what is shown
    let told = env.describe(engines);
    let shown: String = told
        .iter()
        .map(|d| {
            format!(
                "{} = {} ({})",
                d.key,
                d.shown.as_deref().unwrap_or("hidden"),
                d.origin
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    for secret in [
        "hf_THE_TOKEN",
        "hf_FROM_THE_FLAG",
        "ghp_GITHUB_TOKEN",
        "s3cr3t-value",
    ] {
        assert!(!shown.contains(secret), "{secret} in\n{shown}");
        assert!(
            !format!("{env:?}").contains(secret),
            "{secret} in the debug form"
        );
    }
    assert!(
        shown.contains("fetch.huggingface_token = hidden (--hf-token)"),
        "{shown}"
    );
    assert!(
        shown.contains("env.openwebui.WEBUI_SECRET_KEY = hidden"),
        "{shown}"
    );
    assert!(
        shown.contains("env.openwebui.WEBUI_NAME = Studio"),
        "a plain one is shown:\n{shown}"
    );
    assert!(
        shown.contains("values.ollama.models_dir = /mnt/big/ollama (environment file server.toml)"),
        "{shown}"
    );
    assert!(
        shown.contains("values.ollama.extra_env = 1 line"),
        "a block of lines is counted, not shown:\n{shown}"
    );
    assert!(
        shown.contains("models.wan2.2_vae.safetensors = /srv/wan/wan2.2_vae.safetensors"),
        "{shown}"
    );
    // a path is told whole: it is what the person wants to check (found by the suites of the program, which
    // saw the first 60 characters of a folder and "...")
    let long = format!("/mnt/{}/models", "a-folder-with-a-long-name/".repeat(5));
    let mut env = Environment::default();
    env.values
        .insert("ollama.models_dir".into(), Value::Str(long.clone()));
    let told = env.describe(engines);
    assert_eq!(told[0].shown.as_deref(), Some(long.as_str()), "{told:?}");
    // a setting whose name says it is a secret is hidden too
    let mut env = Environment::default();
    env.values
        .insert("ollama.api_key".into(), Value::Str("TOPSECRET".into()));
    let shown = format!("{:?}", env.describe(engines));
    assert!(!shown.contains("TOPSECRET"), "{shown}");
}

/// Y24: what is said here is put over the settings the configuration gave: a setting replaces, a line of
/// environment follows the lines the configuration's own text holds, a program that takes no lines is
/// named, and a configuration that gave nothing for the field gets just the lines.
#[test]
fn p3_y24_what_is_said_here_goes_over_what_the_configuration_gave() {
    let r = common::shipped_registry(&[]);
    let engines = r.engine_map();
    let (file, _) = env_of(SERVER, "ollama").unwrap();
    let mut given: BTreeMap<String, Value> = BTreeMap::new();
    given.insert("ollama.port".into(), Value::Int(11434));
    given.insert("ollama.ctx_size".into(), Value::Int(8192));
    given.insert(
        "ollama.extra_env".into(),
        Value::Str("OLLAMA_FLASH_ATTENTION=1\n".into()),
    );
    file.apply_to(engines, &mut given).unwrap();
    assert_eq!(given["ollama.port"], Value::Int(11500));
    assert_eq!(
        given["ollama.ctx_size"],
        Value::Int(8192),
        "what it does not say stays"
    );
    assert_eq!(
        given["ollama.models_dir"],
        Value::Str("/mnt/big/ollama".into())
    );
    // the configuration's own lines, then the ones given here (this file also had a value of its own for
    // the field, which a line of `[values.ollama]` replaced before the lines followed)
    assert_eq!(
        given["ollama.extra_env"].as_str().unwrap(),
        "OLLAMA_KEEP_ALIVE=24h",
        "the file's own text replaces the configuration's, and no line of `[env.ollama]` was given"
    );
    assert_eq!(
        given["openwebui.extra_env"].as_str().unwrap(),
        "ENABLE_SIGNUP=false\nWEBUI_NAME=Studio\nWEBUI_SECRET_KEY=s3cr3t-value",
        "a program the configuration gave nothing for gets just the lines"
    );

    // the lines follow what the configuration holds
    let mut env = Environment::default();
    env.add_env_line(engines, "ollama", "OLLAMA_NUM_PARALLEL=4", "--env")
        .unwrap();
    let mut given: BTreeMap<String, Value> = BTreeMap::new();
    given.insert(
        "ollama.extra_env".into(),
        Value::Str("OLLAMA_FLASH_ATTENTION=1\n".into()),
    );
    env.apply_to(engines, &mut given).unwrap();
    assert_eq!(
        given["ollama.extra_env"].as_str().unwrap(),
        "OLLAMA_FLASH_ATTENTION=1\nOLLAMA_NUM_PARALLEL=4"
    );
    // a program with no field for lines is named
    let mut wrong = Environment::default();
    wrong.env.insert(
        "llamacpp".into(),
        vec![quickai_core::profile::environment::EnvLine {
            name: "A".into(),
            value: "1".into(),
        }],
    );
    let why = wrong
        .apply_to(engines, &mut BTreeMap::new())
        .expect_err("no lines");
    assert!(why.contains("takes no lines of environment"), "{why}");
}

/// Y25: a configuration still needs what nothing gave, and gets it from any one place: a login from a file or
/// a flag, a setting from the file, from the command line or from the configuration itself, a variable from
/// either; an empty value is not given.
#[test]
fn p3_y25_what_a_configuration_still_needs_is_what_nothing_has_given() {
    let body = r#"
[[needs]]
token = "huggingface"
[[needs]]
setting = "ollama.models_dir"
[[needs]]
env = "WEBUI_SECRET_KEY"
for = "openwebui"
"#;
    let c = read(&document("ollama", body));
    let r = common::shipped_registry(&[]);
    let engines = r.engine_map();
    let nothing = BTreeMap::new();
    let unmet = |env: &Environment, effective: &BTreeMap<String, Value>| -> Vec<String> {
        env.unmet(&c, effective).iter().map(|n| n.label()).collect()
    };
    assert_eq!(
        unmet(&Environment::default(), &nothing),
        [
            "a Hugging Face token",
            "ollama.models_dir",
            "WEBUI_SECRET_KEY (for openwebui)"
        ]
    );
    // each is met by the place that has it
    let mut env = Environment::default();
    env.set_fetch("huggingface_token", "hf_x", "HF_TOKEN")
        .unwrap();
    assert_eq!(unmet(&env, &nothing).len(), 2);
    env.set_text(engines, "ollama", "ollama.models_dir", "/mnt/x", "--set")
        .unwrap();
    let mut effective = BTreeMap::new();
    env.apply_to(engines, &mut effective).unwrap();
    assert_eq!(
        unmet(&env, &effective),
        ["WEBUI_SECRET_KEY (for openwebui)"]
    );
    env.add_env_line(engines, "ollama", "openwebui:WEBUI_SECRET_KEY=abc", "--env")
        .unwrap();
    assert!(unmet(&env, &effective).is_empty());
    // a setting the configuration itself gave is given
    let mut effective = BTreeMap::new();
    effective.insert(
        "ollama.models_dir".to_string(),
        Value::Str("/from/the/configuration".into()),
    );
    let mut env = Environment::default();
    env.set_fetch("huggingface_token", "hf_x", "HF_TOKEN")
        .unwrap();
    env.add_env_line(engines, "ollama", "openwebui:WEBUI_SECRET_KEY=abc", "--env")
        .unwrap();
    assert!(unmet(&env, &effective).is_empty());
    // an empty value is not given
    effective.insert("ollama.models_dir".to_string(), Value::Str("  ".into()));
    assert_eq!(unmet(&env, &effective), ["ollama.models_dir"]);
    let mut empty = Environment::default();
    empty
        .set_fetch("huggingface_token", "   ", "HF_TOKEN")
        .unwrap();
    assert!(empty.fetch.huggingface_token.is_none());
    empty.env.insert(
        "openwebui".into(),
        vec![quickai_core::profile::environment::EnvLine {
            name: "WEBUI_SECRET_KEY".into(),
            value: String::new(),
        }],
    );
    assert!(empty
        .unmet(&c, &BTreeMap::new())
        .iter()
        .any(|n| n.label().starts_with("WEBUI_SECRET_KEY")));
    // a GitHub token is a need of its own
    let c = read(&document("ollama", "[[needs]]\ntoken = \"github\"\n"));
    assert_eq!(Environment::default().unmet(&c, &nothing).len(), 1);
    let mut env = Environment::default();
    env.set_fetch("github_token", "ghp_x", "GITHUB_TOKEN")
        .unwrap();
    assert!(env.unmet(&c, &nothing).is_empty());
}

fn hints() -> Vec<FileHint> {
    vec![
        FileHint {
            name: "wan2.2_vae.safetensors".into(),
            source: "huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/split_files/vae/wan2.2_vae.safetensors".into(),
            size: Some(1_400_000_000),
        },
        FileHint {
            name: "umt5.safetensors".into(),
            source: "mirror.example/umt5.safetensors".into(),
            size: None,
        },
    ]
}

/// Y26: the file to fill in is made from the documents: what the configuration needs is a line to complete,
/// the settings that belong to a computer, the lines of a program's environment and the files the
/// configuration fetches are there in comments; it is TOML, and read back as it is it says nothing.
#[test]
fn p3_y26_the_file_to_fill_in_is_made_from_the_documents() {
    let r = common::shipped_registry(&[]);
    let body = r#"
[[needs]]
token = "huggingface"
why = "The Wan repositories want you to accept their licence."
[[needs]]
setting = "comfyui.models_dir"
why = "Somewhere with 40 GB free."
"#;
    let c = read(&document("comfyui", body));
    let text = template(r.engine_map(), &c, &hints());

    // it explains itself
    for line in [
        "# The environment of a computer for the configuration \"Given\" (given).",
        "schema = 1",
        "[fetch]",
        "[values.comfyui]",
        "[env.comfyui]",
        "[models]",
    ] {
        assert!(text.contains(line), "{line} in\n{text}");
    }
    // what is needed is a line to complete, with why; what is not is in a comment
    assert!(
        text.contains(
            "NEEDED by this configuration: The Wan repositories want you to accept their licence."
        ),
        "{text}"
    );
    assert!(text.contains("\nhuggingface_token = \"\"\n"), "{text}");
    assert!(
        text.contains("\n# huggingface_endpoint = \"\"\n")
            && text.contains("\n# github_token = \"\"\n"),
        "{text}"
    );
    assert!(
        text.contains("NEEDED by this configuration: Somewhere with 40 GB free."),
        "{text}"
    );
    assert!(text.contains("\nmodels_dir = \"\"\n"), "{text}");
    // the settings that belong to a computer (a folder, a Python, a launcher, a card) are offered; a port, which travels, is not
    for key in [
        "output_dir",
        "python",
        "bin_override",
        "gfx_override",
        "network",
        "page_downloads",
        "extra_args",
    ] {
        assert!(text.contains(&format!("\n# {key} = ")), "{key} in\n{text}");
    }
    assert!(
        !text.contains("# port =") && !text.contains("# mode ="),
        "{text}"
    );
    // the document's own words are the help
    assert!(
        text.contains("Where the models are kept, in the folders ComfyUI looks in"),
        "{text}"
    );
    // the lines of environment: the names the document keeps for itself are said
    assert!(
        text.contains("\n# NAME = \"value\"\n") && text.contains("HSA_OVERRIDE_GFX_VERSION"),
        "{text}"
    );
    // the files the configuration fetches, with where from and how big
    assert!(
        text.contains("# \"wan2.2_vae.safetensors\" = \"\"      # from huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/split_files/vae/wan2.2_vae.safetensors (1.3 GB)"),
        "{text}"
    );
    assert!(
        text.contains(
            "# \"umt5.safetensors\" = \"\"      # from mirror.example/umt5.safetensors\n"
        ),
        "{text}"
    );

    // it is TOML, and as it stands it says nothing: what is needed is empty, and so not given
    let (env, unknown) =
        Environment::read(&text, "the template", r.engine_map(), "comfyui").expect("read");
    assert_eq!(env, Environment::default());
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(env.unmet(&c, &BTreeMap::new()).len(), 2);
    // filled in, it is read
    let filled = text
        .replace(
            "\nhuggingface_token = \"\"\n",
            "\nhuggingface_token = \"hf_x\"\n",
        )
        .replace("\nmodels_dir = \"\"\n", "\nmodels_dir = \"/mnt/big\"\n")
        .replace(
            "# \"wan2.2_vae.safetensors\" = \"\" ",
            "\"wan2.2_vae.safetensors\" = \"/srv/vae.safetensors\" #",
        );
    let (env, _) =
        Environment::read(&filled, "the template", r.engine_map(), "comfyui").expect("read");
    assert!(env.unmet(&c, &env.values.clone()).is_empty());
    assert_eq!(
        env.models["wan2.2_vae.safetensors"],
        Where::Path("/srv/vae.safetensors".into())
    );

    // a configuration that needs nothing and fetches nothing has a template that says so
    let plain = read(&document("llamacpp", ""));
    let text = template(r.engine_map(), &plain, &[]);
    assert!(!text.contains("NEEDED"), "{text}");
    assert!(
        text.contains("(this configuration fetches no file)"),
        "{text}"
    );
    assert!(
        !text.contains("[env."),
        "llama.cpp takes no lines of environment:\n{text}"
    );
    toml::from_str::<toml::Table>(&text).expect("TOML");
}

const ECHO: &str = r#"schema = 1
kind = "engine"
id = "echo"
requires = { primitives = ["spawn@1", "locate.setting@1"] }

[engine]
name = "Echo"
order = 50

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
id = "cache"
label = "Cache folder"
help = "Where Echo keeps what it learned."
type = "dir"

[[field]]
id = "vars"
label = "Extra environment"
help = "Variables for Echo."
type = "kv_lines"
default = ""
bind.env_lines = { protect = ["ECHO_HOME"] }

[[field]]
id = "bin"
type = "runtime_path"
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
id = "vars"
type = "kv_lines"
default = ""
bind.env_lines = { protect = [] }

[[field]]
id = "bin"
type = "runtime_path"
"#;

/// Y27: nothing in this names an engine: a fifth engine that is only a document, with a program that goes
/// with it, has its folders and its lines of environment in the file to fill in and can be given them, and
/// the code that does this has no word of any engine of the application in it.
#[test]
fn p3_y27_an_engine_that_is_only_a_document_takes_part_and_the_code_names_none() {
    let dir = "/home/u/.config/quickai/engines";
    let (echo, page) = (format!("{dir}/echo.toml"), format!("{dir}/echo-page.toml"));
    let r = common::shipped_registry(&[(echo.as_str(), ECHO), (page.as_str(), ECHO_PAGE)]);
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    let c = Configuration {
        id: "echoes".into(),
        name: "Echoes".into(),
        engine: "echo".into(),
        needs: vec![quickai_core::profile::Need {
            kind: NeedKind::Env {
                program: "echo-page".into(),
                name: "ECHO_PAGE_KEY".into(),
            },
            why: Some("it signs".into()),
            secret: true,
        }],
        ..Configuration::default()
    };
    let text = template(r.engine_map(), &c, &[]);
    assert!(
        text.contains("[values.echo]") && text.contains("# cache = \"\""),
        "{text}"
    );
    assert!(text.contains("Where Echo keeps what it learned."), "{text}");
    assert!(
        text.contains("[env.echo]") && text.contains("[env.echo-page]"),
        "{text}"
    );
    assert!(
        text.contains("ECHO_HOME"),
        "the protected name is said:\n{text}"
    );
    assert!(text.contains("\nECHO_PAGE_KEY = \"\"\n"), "{text}");
    toml::from_str::<toml::Table>(&text).expect("TOML");

    // it can be given them
    let mut env = Environment::default();
    env.set_text(r.engine_map(), "echo", "echo.cache", "/mnt/echo", "--set")
        .unwrap();
    env.add_env_line(
        r.engine_map(),
        "echo",
        "echo-page:ECHO_PAGE_KEY=abc",
        "--env",
    )
    .unwrap();
    let mut values = BTreeMap::new();
    env.apply_to(r.engine_map(), &mut values).unwrap();
    assert_eq!(values["echo.cache"], Value::Str("/mnt/echo".into()));
    assert_eq!(
        values["echo-page.vars"],
        Value::Str("ECHO_PAGE_KEY=abc".into())
    );
    assert!(env.unmet(&c, &values).is_empty());

    // and the code that does it names no engine of the application
    for file in ["mod.rs", "environment.rs"] {
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
