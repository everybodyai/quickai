//! Group B of docs/phase3-test-plan.md, the pure half: bundles (work package 4.9). What is fetched
//! together with a model's weights, how the files are named and kept, how a document says it and is
//! checked, and how the planner puts a companion on the command line. The documents are the ones the
//! application ships and the repositories are recordings of the real Hugging Face.

mod common;

use common::{hub_candidate, owned, registry, texts};
use quickai_core::diag::Severity;
use quickai_core::lookup::present::{
    companion_file_name, safe_file_name, Download, Entry, Fetch, Offer,
};
use quickai_core::plan::{plan, HostFacts, ModelInfo, Selection, Start};

fn download(offer: &Offer) -> &Download {
    match &offer.fetch {
        Fetch::Files(d) => d,
        other => panic!("not files: {other:?}"),
    }
}

fn bundle_offers<'a>(entry: &'a Entry, id: &str) -> Vec<&'a Offer> {
    entry
        .offers
        .iter()
        .filter(|o| o.bundle.as_deref() == Some(id))
        .collect()
}

fn names(d: &Download) -> Vec<&str> {
    d.files.iter().map(|f| f.name.as_str()).collect()
}

/// A user document that adds a bundle of weights and a draft model, with the component that attaches it.
const DRAFT: &str = r#"schema = 1
kind = "engine"
id = "llamacpp"

[[models.bundle]]
id = "speculative"
label = "with draft model"
parts = [ { kind = "draft", label = "draft model", match = ["mtp-*.gguf"], prefer = ["Q4_0"] } ]

[components.draft]
bind.arg = ["--model-draft", "${component.path}"]
"#;

#[test]
fn p3_b01_a_repository_is_offered_in_a_bundle_when_it_has_a_file_for_every_part() {
    let r = registry(&[]);
    let rules = owned(&r, "llamacpp", "hub");
    assert_eq!(rules.bundles.len(), 1);
    assert_eq!(rules.bundles[0].id, "vision");

    // a vision model and its projector: the same buttons, once for the weights alone, once with the projector
    let gemma = rules.present(&hub_candidate("ggml-org/gemma-3-4b-it-GGUF"));
    let plain: Vec<&Offer> = gemma.offers.iter().filter(|o| o.bundle.is_none()).collect();
    let bundled = bundle_offers(&gemma, "vision");
    assert_eq!(plain.len(), 3);
    assert_eq!(bundled.len(), 3);
    assert_eq!(
        bundled.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
        ["Q4_K_M · 3.1 GB", "Q8_0 · 4.6 GB", "F16 · 8.0 GB"],
        "each is the weights and 811.8 MB of projector"
    );
    for (alone, with) in plain.iter().zip(&bundled) {
        assert_eq!(alone.recommended, with.recommended);
        assert_eq!(
            with.bytes.unwrap(),
            alone.bytes.unwrap() + 851_251_104,
            "the bundle is the weights and the projector"
        );
        assert_eq!(
            names(download(with))[0],
            names(download(alone))[0],
            "the weights first"
        );
        assert_eq!(names(download(with))[1], "mmproj-model-f16.gguf");
        assert!(
            with.hover.contains("mmproj-model-f16.gguf") && with.hover.contains("in all"),
            "{}",
            with.hover
        );
        assert_eq!(download(with).bundle.as_deref(), Some("vision"));
        assert_eq!(download(alone).bundle, None);
    }
    assert!(bundled[0].recommended && !bundled[1].recommended);

    // two projectors of the same kind: the first of `prefer`, F16, and not the smaller one that comes next
    let qwen = rules.present(&hub_candidate("unsloth/Qwen3.8-27B-GGUF"));
    for offer in bundle_offers(&qwen, "vision") {
        assert_eq!(names(download(offer))[1], "mmproj-F16.gguf");
    }
    // and the person's own preference moves it
    let person = registry(&[(
        "user:engines/llamacpp.toml",
        "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[[models.bundle]]\nid = \"vision\"\nparts = [ { kind = \"mmproj\", match = [\"mmproj*.gguf\"], prefer = [\"BF16\", \"F16\"] } ]\n",
    )]);
    assert!(person.diagnostics.is_empty(), "{:#?}", person.diagnostics);
    let qwen =
        owned(&person, "llamacpp", "hub").present(&hub_candidate("unsloth/Qwen3.8-27B-GGUF"));
    assert_eq!(
        names(download(bundle_offers(&qwen, "vision")[0]))[1],
        "mmproj-BF16.gguf"
    );

    // a repository with no projector has no bundle, and a projector is never offered as a model
    for id in [
        "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
        "mixedbread-ai/mxbai-embed-large-v1",
        "mudler/Laguna-XS-2.1-APEX-GGUF",
    ] {
        let entry = rules.present(&hub_candidate(id));
        assert!(bundle_offers(&entry, "vision").is_empty(), "{id}");
        assert!(!entry.offers.is_empty(), "{id}");
    }
    for offer in &gemma.offers {
        assert!(!names(download(offer))[0].contains("mmproj"));
    }
}

#[test]
fn p3_b02_a_bundle_the_person_defines_is_offered_beside_the_shipped_one() {
    let r = registry(&[("user:engines/llamacpp.toml", DRAFT)]);
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
    let rules = owned(&r, "llamacpp", "hub");
    assert_eq!(
        rules
            .bundles
            .iter()
            .map(|b| b.id.as_str())
            .collect::<Vec<_>>(),
        ["vision", "speculative"],
        "the shipped bundle stays, the new one is added"
    );
    // the repository has a draft model and two projectors: both bundles, as two rows
    let qwen = rules.present(&hub_candidate("unsloth/Qwen3.8-27B-GGUF"));
    assert_eq!(bundle_offers(&qwen, "vision").len(), 4);
    let drafts = bundle_offers(&qwen, "speculative");
    assert_eq!(drafts.len(), 4);
    assert_eq!(names(download(drafts[0]))[1], "mtp-Qwen3.8-27B-Q4_0.gguf");
    assert_eq!(download(drafts[0]).files[1].kind.as_deref(), Some("draft"));
    assert!(
        drafts[0]
            .hover
            .contains("the draft model mtp-Qwen3.8-27B-Q4_0.gguf"),
        "{}",
        drafts[0].hover
    );
    // a draft model is not weights, so it is not a button of its own
    assert!(qwen
        .offers
        .iter()
        .all(|o| !names(download(o))[0].starts_with("mtp-")));
    // a repository without one has only what it had
    let gemma = rules.present(&hub_candidate("ggml-org/gemma-3-4b-it-GGUF"));
    assert!(bundle_offers(&gemma, "speculative").is_empty());
    assert_eq!(bundle_offers(&gemma, "vision").len(), 3);
}

#[test]
fn p3_b03_what_is_fetched_is_kept_under_names_that_cannot_clash_or_climb() {
    let r = registry(&[]);
    let rules = owned(&r, "llamacpp", "hub");
    let gemma = rules.present(&hub_candidate("ggml-org/gemma-3-4b-it-GGUF"));
    let with = bundle_offers(&gemma, "vision")[0];
    let d = download(with);
    // the weights keep the name they have always had
    assert_eq!(d.files[0].store_as, "gemma-3-4b-it-Q4_K_M.gguf");
    assert_eq!(
        d.files[0].url,
        "https://huggingface.co/ggml-org/gemma-3-4b-it-GGUF/resolve/main/gemma-3-4b-it-Q4_K_M.gguf"
    );
    assert_eq!(d.files[0].kind, None);
    // the projector carries its repository: `mmproj-F16.gguf` is in nearly every repository
    assert_eq!(
        d.files[1].store_as,
        "ggml-org--gemma-3-4b-it-GGUF--mmproj-model-f16.gguf"
    );
    assert_eq!(d.files[1].kind.as_deref(), Some("mmproj"));
    assert_eq!(d.files[1].size, Some(851_251_104));
    assert_eq!(d.source, "huggingface:ggml-org/gemma-3-4b-it-GGUF");
    assert_eq!(d.total(), Some(2_489_757_856 + 851_251_104));
    assert_eq!(d.weights().unwrap().name, "gemma-3-4b-it-Q4_K_M.gguf");
    // two quantizations of one repository share the projector: the same name, so it is fetched once
    let other = download(bundle_offers(&gemma, "vision")[1]);
    assert_eq!(other.files[1].store_as, d.files[1].store_as);
    // and another repository's projector of the same name is another file
    assert_eq!(
        companion_file_name("unsloth/gemma-3-4b-it-GGUF", "mmproj-model-f16.gguf"),
        "unsloth--gemma-3-4b-it-GGUF--mmproj-model-f16.gguf"
    );
    // a name that tries to leave the folder, or to hide, cannot
    for (raw, kept) in [
        ("../../etc/passwd", "passwd"),
        ("dir/sub/model.gguf", "model.gguf"),
        ("C:\\Users\\x\\model.gguf", "model.gguf"),
        ("..hidden.gguf", "hidden.gguf"),
        ("a b?c#d.gguf", "abcd.gguf"),
        ("", "download.gguf"),
        ("....", "download.gguf"),
    ] {
        assert_eq!(safe_file_name(raw), kept, "{raw:?}");
    }
    assert_eq!(companion_file_name("a/b", "x/../../y.gguf"), "a--b--y.gguf");
    // a size that is not known makes the total not known
    let mut unsure = d.clone();
    unsure.files[1].size = None;
    assert_eq!(unsure.total(), None);
}

#[test]
fn p3_b04_a_bundle_that_is_wrong_is_refused_with_where_and_what_to_write() {
    let patch = |body: &str| {
        registry(&[(
            "user:engines/llamacpp.toml",
            &format!("schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n{body}"),
        )])
    };
    let one_error = |body: &str, words: &[&str]| {
        let r = patch(body);
        let errors = texts(&r, Severity::Error);
        assert_eq!(errors.len(), 1, "{body}: {errors:?}");
        for word in words {
            assert!(
                errors[0].contains(word),
                "`{word}` is not in: {}",
                errors[0]
            );
        }
        // the application's own document stays in force, bundle and all
        let kept = owned(&r, "llamacpp", "hub");
        assert_eq!(kept.bundles.len(), 1, "{body}");
    };
    // a kind the engine cannot attach says which kinds it can
    one_error(
        "[[models.bundle]]\nid = \"x\"\nparts = [ { kind = \"lora\", match = [\"*.gguf\"] } ]\n",
        &[
            "no component called `lora`",
            "the components it has: mmproj",
            "bundle[1].parts[0].kind",
        ],
    );
    one_error(
        "[[models.bundle]]\nid = \"x\"\n",
        &["has no parts", "parts = ["],
    );
    one_error(
        "[[models.bundle]]\nid = \"x\"\nparts = [ { kind = \"mmproj\" } ]\n",
        &["a part says which files it is", "match"],
    );
    one_error(
        "[[models.bundle]]\nid = \"x\"\nparts = [ { kind = \"mmproj\", match = [\"a\", \" \"] } ]\n",
        &["empty entry in `match`"],
    );
    one_error(
        "[[models.bundle]]\nid = \"x\"\nparts = [ { kind = \"mmproj\", match = [\"a\"], prefer = [\"\"] } ]\n",
        &["empty entry in `prefer`"],
    );
    one_error(
        "[[models.bundle]]\nid = \"two words\"\nparts = [ { kind = \"mmproj\", match = [\"a\"] } ]\n",
        &["not a name for a bundle"],
    );
    // two bundles of one document with one name (no other layer to merge them): the second is refused
    let twice = common::registry_of(&[(
        "embedded:engines/acme.toml",
        &common::acme(
            "[[models.bundle]]\nid = \"a\"\nparts = [ { kind = \"mmproj\", match = [\"a\"] } ]\n[[models.bundle]]\nid = \"a\"\nparts = [ { kind = \"mmproj\", match = [\"b\"] } ]\n",
        ),
    )]);
    let errors = texts(&twice, Severity::Error);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("two bundles are called `a`"),
        "{}",
        errors[0]
    );
    assert!(twice.engine("acme").is_none());
}

#[test]
fn p3_b05_a_bundle_of_the_person_patches_the_shipped_one_by_id() {
    // the same id as the shipped bundle: the parts are replaced as a list, the label stays
    let r = registry(&[(
        "user:engines/llamacpp.toml",
        "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[[models.bundle]]\nid = \"vision\"\nparts = [ { kind = \"mmproj\", label = \"projection\", match = [\"*proj*\"] } ]\n",
    )]);
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
    let rules = owned(&r, "llamacpp", "hub");
    assert_eq!(rules.bundles.len(), 1);
    let b = &rules.bundles[0];
    assert_eq!(
        b.label, "with projector",
        "what the person did not say stays"
    );
    assert_eq!(b.parts.len(), 1);
    assert_eq!(
        (b.parts[0].label.as_str(), b.parts[0].matches.as_slice()),
        ("projection", &["*proj*".to_string()][..])
    );
    assert!(
        b.parts[0].prefer.is_empty(),
        "a list is replaced as a whole"
    );
    // the label of a bundle and of a part default to their names
    let defaults = registry(&[(
        "user:engines/llamacpp.toml",
        "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[[models.bundle]]\nid = \"both\"\nparts = [ { kind = \"mmproj\", match = [\"x\"] } ]\n",
    )]);
    let both = owned(&defaults, "llamacpp", "hub")
        .bundles
        .into_iter()
        .find(|b| b.id == "both")
        .unwrap();
    assert_eq!(
        (both.label.as_str(), both.parts[0].label.as_str()),
        ("both", "mmproj")
    );

    // an engine with no components cannot have a bundle
    let ollama = registry(&[(
        "user:engines/ollama.toml",
        "schema = 1\nkind = \"engine\"\nid = \"ollama\"\n[[models.bundle]]\nid = \"x\"\nparts = [ { kind = \"mmproj\", match = [\"x\"] } ]\n",
    )]);
    let errors = texts(&ollama, Severity::Error);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("needs a [components.<kind>] with bind.arg"),
        "{}",
        errors[0]
    );
}

#[test]
fn p3_b06_a_component_is_checked_where_it_is_written() {
    let patch = |body: &str| {
        registry(&[(
            "user:engines/llamacpp.toml",
            &format!("schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n{body}"),
        )])
    };
    for (body, words) in [
        (
            "[components.\"Bad Kind\"]\nbind.arg = [\"--x\", \"${component.path}\"]\n",
            &["not a name for a kind of component"][..],
        ),
        (
            "[components.draft]\nbind.arg = [\"--model-draft\", \"${component.nope}\"]\n",
            &["nope"],
        ),
        (
            "[components.draft]\nbind.arg = [\"--model-draft\", \"${value}\"]\n",
            &["value"],
        ),
        (
            "[components.draft]\norder = 5\n",
            &["does not say how it reaches the program", "bind.arg"],
        ),
    ] {
        let errors = texts(&patch(body), Severity::Error);
        assert_eq!(errors.len(), 1, "{body}: {errors:?}");
        for word in words {
            assert!(
                errors[0].contains(word),
                "`{word}` is not in: {}",
                errors[0]
            );
        }
    }
    // what is right is compiled with its order, 80 unless it says
    let r = patch("[components.draft]\nbind.arg = [\"--model-draft\", \"${component.path}\"]\n[components.vocoder]\norder = 5\nbind.arg = [\"--voc\", \"${component.path}\"]\n");
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
    let engine = r.engine("llamacpp").unwrap();
    let order = |kind: &str| {
        engine
            .components
            .iter()
            .find(|c| c.kind == kind)
            .map(|c| c.order)
    };
    assert_eq!(
        (order("mmproj"), order("draft"), order("vocoder")),
        (Some(80), Some(80), Some(5))
    );
}

fn facts() -> HostFacts {
    HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        ram_bytes: 32 * 1024 * 1024 * 1024,
        cpus: 8,
        binary: Some("/opt/quickai/llama-server".into()),
        ..Default::default()
    }
}

fn selection() -> Selection {
    Selection::new("llamacpp")
        .with_model(ModelInfo {
            id: "gemma-3-4b-it-Q4_K_M.gguf".into(),
            display: "gemma-3-4b-it-Q4_K_M.gguf".into(),
            path: "/models/gemma-3-4b-it-Q4_K_M.gguf".into(),
            size_bytes: 2_489_757_856,
        })
        .set("llamacpp.port", 8080u16)
        .set("llamacpp.ctx_size", 4096u32)
        .set("llamacpp.n_gpu_layers", 999u32)
}

fn args_of(r: &quickai_core::registry::Registry, selection: &Selection) -> Vec<(String, String)> {
    let plan = plan(r, selection, &facts()).unwrap_or_else(|e| panic!("{e:#?}"));
    match &plan.units[0].start[0] {
        Start::Spawn(spawn) => spawn
            .args
            .iter()
            .map(|a| (a.value.clone(), a.origin.clone()))
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn values(args: &[(String, String)]) -> Vec<&str> {
    args.iter().map(|(v, _)| v.as_str()).collect()
}

#[test]
fn p3_b07_an_attached_companion_is_on_the_command_line_before_the_persons_own_arguments() {
    let r = registry(&[]);
    let without = args_of(&r, &selection());
    assert_eq!(
        values(&without),
        [
            "--model",
            "/models/gemma-3-4b-it-Q4_K_M.gguf",
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "--ctx-size",
            "4096",
            "--n-gpu-layers",
            "999"
        ]
    );
    // a model with a projector: its arguments come after the fields and before `extra_args`
    let with = args_of(
        &r,
        &selection()
            .with_component("mmproj", "/models/proj.gguf")
            .set("llamacpp.extra_args", "--flash-attn on"),
    );
    assert_eq!(
        values(&with),
        [
            "--model",
            "/models/gemma-3-4b-it-Q4_K_M.gguf",
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "--ctx-size",
            "4096",
            "--n-gpu-layers",
            "999",
            "--mmproj",
            "/models/proj.gguf",
            "--flash-attn",
            "on"
        ]
    );
    // and the plan says where each came from
    let origins: Vec<&str> = with.iter().map(|(_, o)| o.as_str()).collect();
    assert_eq!(&origins[10..12], ["component mmproj", "component mmproj"]);
    assert!(origins[12].starts_with("field llamacpp.extra_args"));
    // what an engine that attaches nothing of that kind is told is nothing: a selection may hold a
    // kind the document of another engine has
    let lora = args_of(&r, &selection().with_component("lora", "/models/x.gguf"));
    assert_eq!(lora, without);
    // two files of a kind are two sets of arguments, in the order they were attached
    let two = args_of(
        &r,
        &selection()
            .with_component("mmproj", "/a.gguf")
            .with_component("mmproj", "/b.gguf"),
    );
    assert_eq!(
        &values(&two)[10..],
        ["--mmproj", "/a.gguf", "--mmproj", "/b.gguf"]
    );
    // Ollama's document has no components, so what is attached to its selection is nobody's business
    let ollama_plain = plan(&r, &Selection::new("ollama"), &facts());
    let ollama_with = plan(
        &r,
        &Selection::new("ollama").with_component("mmproj", "/x"),
        &facts(),
    );
    assert_eq!(
        ollama_plain.map(|p| p.units[0].start.len()).ok(),
        ollama_with.map(|p| p.units[0].start.len()).ok()
    );
}

#[test]
fn p3_b08_the_order_of_a_component_is_the_documents_to_say() {
    // a component of order 5 stands before the fields; those of 20 (the context size's own) after it, a
    // field first and then the components in the order of their kinds; then the GPU layers
    let r = registry(&[(
        "user:engines/llamacpp.toml",
        "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[components.aaa]\norder = 20\nbind.arg = [\"--aaa\", \"${component.path}\"]\n[components.mmproj]\norder = 20\nbind.arg = [\"--mmproj\", \"${component.path}\"]\n[components.draft]\norder = 5\nbind.arg = [\"--model-draft\", \"${component.path}\"]\n",
    )]);
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
    let args = args_of(
        &r,
        &selection()
            .with_component("mmproj", "/p.gguf")
            .with_component("aaa", "/a.gguf")
            .with_component("draft", "/d.gguf"),
    );
    assert_eq!(
        &values(&args)[6..],
        [
            "--model-draft",
            "/d.gguf",
            "--ctx-size",
            "4096",
            "--aaa",
            "/a.gguf",
            "--mmproj",
            "/p.gguf",
            "--n-gpu-layers",
            "999",
        ]
    );
}
