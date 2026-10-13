//! Group V of docs/phase3-test-plan.md: what a look-up shows (work package 4.9). A look-up finds
//! candidates; `present` makes of each what the window draws: its type, the buttons that fetch it,
//! the size class of each. The documents are the ones the application ships, and the repositories
//! are recordings of the real Hugging Face (`crates/quickai-runtime/tests/fixtures/huggingface/record.sh`),
//! so a change to a shipped document or a change of the world shows here.

mod common;

use common::{hub_candidate, owned, registry, registry_of, texts};
use quickai_core::diag::Severity;
use quickai_core::lookup::candidate::{Candidate, Origin, RemoteFile};
use quickai_core::lookup::huggingface;
use quickai_core::lookup::ollama_library;
use quickai_core::lookup::present::{Entry, Fetch, Offer};
use quickai_core::lookup::size::parse_size;
use quickai_core::lookup::types::{classify, size_class, Facts};
use quickai_core::lookup::view::arrange;

const GIB: u64 = 1024 * 1024 * 1024;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn labels(entry: &Entry, bundle: Option<&str>) -> Vec<String> {
    entry
        .offers
        .iter()
        .filter(|o| o.bundle.as_deref() == bundle)
        .map(|o| o.label.clone())
        .collect()
}

fn classes(entry: &Entry, bundle: Option<&str>) -> Vec<String> {
    entry
        .offers
        .iter()
        .filter(|o| o.bundle.as_deref() == bundle)
        .map(|o| o.class.clone().unwrap_or_default())
        .collect()
}

fn weights_of(offer: &Offer) -> String {
    match &offer.fetch {
        Fetch::Files(download) => download.weights().expect("weights").name.clone(),
        other => panic!("not files: {other:?}"),
    }
}

/// A candidate that says only what the arguments say, for the table of types.
fn said(name: &str, capabilities: &[&str], pipeline: Option<&str>, tags: &[&str]) -> Candidate {
    Candidate {
        key: format!("test:{name}"),
        name: name.to_string(),
        description: String::new(),
        downloads: None,
        likes: None,
        capabilities: strings(capabilities),
        pipeline: pipeline.map(str::to_string),
        tags: strings(tags),
        origin: Origin::Library {
            name: name.to_string(),
            sizes: Vec::new(),
        },
    }
}

#[test]
fn p3_v01_a_model_is_of_the_first_type_whose_rule_holds_and_else_of_the_default() {
    let r = registry(&[]);
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
    let rules = owned(&r, "llamacpp", "hub");
    let type_of =
        |name: &str, caps: &[&str], pipeline: Option<&str>, tags: &[&str], companions: &[&str]| {
            let (caps, tags, companions) = (strings(caps), strings(tags), strings(companions));
            classify(
                &rules.types,
                &Facts {
                    capabilities: &caps,
                    pipeline,
                    tags: &tags,
                    name,
                    companions: &companions,
                },
            )
            .map(|t| t.id.clone())
        };
    for (name, caps, pipeline, tags, companions, expected) in [
        // what an Ollama card says
        (
            "qwen3",
            &["tools", "thinking"][..],
            None,
            &[][..],
            &[][..],
            "chat",
        ),
        (
            "embeddinggemma-2",
            &["embedding"],
            None,
            &[],
            &[],
            "embedding",
        ),
        ("llama3.2-vision", &["vision"], None, &[], &[], "vision"),
        ("gemma3n", &["audio", "vision"], None, &[], &[], "audio"),
        ("laya", &[], None, &[], &[], "chat"),
        // what the name says
        ("qwen3-coder", &[], None, &[], &[], "code"),
        ("starcoder2", &[], None, &[], &[], "code"),
        ("nomic-embed-text", &[], None, &[], &[], "embedding"),
        ("Qwen2.5-VL-7B", &[], None, &[], &[], "vision"),
        // a short word is a whole word: `vl` is not in `vllm`
        ("someone/vllm-thing", &[], None, &[], &[], "chat"),
        // embedding beats code beats chat: an embedding of code is an embedding
        ("nomic-embed-code", &[], None, &[], &[], "embedding"),
        // vision beats code
        (
            "qwen-vl-coder",
            &[],
            Some("image-text-to-text"),
            &[],
            &[],
            "vision",
        ),
        // what a repository of Hugging Face says
        (
            "a/b",
            &[],
            Some("text-generation"),
            &["conversational"],
            &[],
            "chat",
        ),
        ("a/b", &[], Some("image-text-to-text"), &[], &[], "vision"),
        (
            "a/b",
            &[],
            Some("feature-extraction"),
            &[],
            &[],
            "embedding",
        ),
        (
            "a/b",
            &[],
            Some("automatic-speech-recognition"),
            &[],
            &[],
            "audio",
        ),
        (
            "mudler/locate-anything.cpp-gguf",
            &[],
            Some("object-detection"),
            &[],
            &[],
            "other",
        ),
        (
            "unsloth/MiniMax-H3-GGUF",
            &[],
            Some("image-text-to-video"),
            &[],
            &[],
            "video",
        ),
        // a tag is a tag whatever its case
        (
            "mudler/Laguna",
            &[],
            None,
            &["gguf", "Code", "coder"],
            &[],
            "code",
        ),
        // a projector beside the weights makes a vision model, whatever the tags say
        (
            "unsloth/Qwen3.8-27B-GGUF",
            &[],
            None,
            &["gguf"],
            &["mmproj"],
            "vision",
        ),
        // nothing says what it is: it chats
        ("mudler/Mystery-GGUF", &[], None, &["gguf"], &[], "chat"),
    ] {
        assert_eq!(
            type_of(name, caps, pipeline, tags, companions).as_deref(),
            Some(expected),
            "{name} {caps:?} {pipeline:?} {tags:?} {companions:?}"
        );
    }
    // a vocabulary with no types has no type of anything
    assert_eq!(
        classify(
            &[],
            &Facts {
                name: "x",
                ..Facts::default()
            }
        ),
        None
    );
}

#[test]
fn p3_v02_a_size_falls_in_the_class_it_is_below_and_a_button_carries_its_own_class() {
    let r = registry(&[]);
    let rules = owned(&r, "llamacpp", "hub");
    let class = |bytes: u64| size_class(&rules.sizes, bytes).map(|c| c.id.as_str());
    assert_eq!(
        rules
            .sizes
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["small", "medium", "large"]
    );
    assert_eq!(class(0), Some("small"));
    assert_eq!(class(4 * GIB - 1), Some("small"));
    assert_eq!(class(4 * GIB), Some("medium"));
    assert_eq!(class(15 * GIB - 1), Some("medium"));
    assert_eq!(class(15 * GIB), Some("large"));
    assert_eq!(class(u64::MAX), Some("large"));
    assert_eq!(parse_size("4GiB"), Some(4 * GIB));

    // the weights and the weights with a projector of the same repository can be in different classes
    let gemma = rules.present(&hub_candidate("ggml-org/gemma-3-4b-it-GGUF"));
    assert_eq!(
        classes(&gemma, None),
        ["small", "small", "medium"],
        "2.3, 3.8 and 7.2 GB"
    );
    assert_eq!(
        classes(&gemma, Some("vision")),
        ["small", "medium", "medium"],
        "3.1, 4.6 and 8.0 GB"
    );
    // and a repository's own buttons straddle a bound: 15.0 GB (16 056 478 688 bytes) is under 15 GiB, 15.3 GB (16 464 440 224) is not
    let qwen = rules.present(&hub_candidate("unsloth/Qwen3.8-27B-GGUF"));
    assert_eq!(
        labels(&qwen, None),
        [
            "UD-Q4_K_S · 14.3 GB",
            "Q4_0 · 15.0 GB",
            "UD-Q4_K_M · 15.3 GB",
            "UD-Q5_K_M · 18.4 GB"
        ]
    );
    assert_eq!(classes(&qwen, None), ["medium", "medium", "large", "large"]);
}

#[test]
fn p3_v03_a_repository_offers_the_quantizations_the_preferences_like_and_nothing_that_is_not_weights(
) {
    let r = registry(&[]);
    let rules = owned(&r, "llamacpp", "hub");
    let qwen = rules.present(&hub_candidate("unsloth/Qwen3.8-27B-GGUF"));
    assert_eq!(qwen.name, "unsloth/Qwen3.8-27B-GGUF");
    assert_eq!(
        qwen.model_type.as_deref(),
        Some("vision"),
        "it has no pipeline tag, but it has a projector"
    );
    // four buttons, smallest first: the ones that come first in `prefer`, not the 24 there are
    assert_eq!(
        qwen.offers
            .iter()
            .filter(|o| o.bundle.is_none())
            .map(weights_of)
            .collect::<Vec<_>>(),
        [
            "Qwen3.8-27B-UD-Q4_K_S.gguf",
            "Qwen3.8-27B-Q4_0.gguf",
            "Qwen3.8-27B-UD-Q4_K_M.gguf",
            "Qwen3.8-27B-UD-Q5_K_M.gguf",
        ]
    );
    // the best of the preferences is the one marked
    let recommended: Vec<&str> = qwen
        .offers
        .iter()
        .filter(|o| o.bundle.is_none() && o.recommended)
        .map(|o| o.label.split(' ').next().unwrap())
        .collect();
    assert_eq!(recommended, ["UD-Q4_K_M"], "UD-Q4_K_M is a Q4_K_M");
    // and the button says why it is bold, where the others do not
    for offer in qwen.offers.iter().filter(|o| o.bundle.is_none()) {
        assert_eq!(
            offer.hover.contains("the one this look-up prefers"),
            offer.recommended,
            "{}",
            offer.hover
        );
    }

    // the whole list: 24 weights; the two shards, the draft model, the matrix and the two projectors are
    // not among them
    let mut everything = rules.settings.clone();
    everything.show_variants = 64;
    let all = quickai_core::lookup::present::present(
        &hub_candidate("unsloth/Qwen3.8-27B-GGUF"),
        &quickai_core::lookup::present::Rules {
            settings: &everything,
            ..rules.rules()
        },
    );
    let plain: Vec<String> = all
        .offers
        .iter()
        .filter(|o| o.bundle.is_none())
        .map(weights_of)
        .collect();
    assert_eq!(plain.len(), 24, "{plain:#?}");
    for left_out in ["BF16", "mtp", "imatrix", "mmproj"] {
        assert!(
            !plain.iter().any(|f| f.contains(left_out)),
            "{left_out} is offered as weights: {plain:#?}"
        );
    }
    // smallest first
    let sizes: Vec<u64> = all
        .offers
        .iter()
        .filter(|o| o.bundle.is_none())
        .map(|o| o.bytes.unwrap())
        .collect();
    assert!(sizes.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn p3_v03b_a_file_is_skipped_for_what_it_is_and_not_for_what_its_repository_is_called() {
    let r = registry(&[]);
    let rules = owned(&r, "llamacpp", "hub");
    let files = |list: &[(&str, u64)]| Candidate {
        origin: Origin::Hub {
            site: "https://hub.example".into(),
            repo: "x/Qwen3.8-27B-iMatrix-NVFP4-MTP-GGUF".into(),
            revision: "main".into(),
            files: list
                .iter()
                .map(|(p, s)| RemoteFile::new(p, Some(*s)))
                .collect(),
        },
        ..said("x/Qwen3.8-27B-iMatrix-NVFP4-MTP-GGUF", &[], None, &["gguf"])
    };
    let entry = rules.present(&files(&[
        // weights of a model that says `MTP` and `iMatrix` in its own name: offered
        ("Qwen3.8-27B-iMatrix-NVFP4-MTP-Q4_K_M.gguf", 100),
        ("Qwen3.8-27B-iMatrix-NVFP4-MTP-Q8_0.gguf", 200),
        // what is not weights: a shard, a draft model by folder and by name, the matrix, a projector
        ("Qwen3.8-27B-MTP-BF16-00001-of-00002.gguf", 300),
        ("MTP/head.gguf", 10),
        ("mtp-head-Q4_0.gguf", 10),
        ("imatrix_unsloth.gguf", 10),
        ("mmproj-F16.gguf", 10),
    ]));
    assert_eq!(
        entry
            .offers
            .iter()
            .filter(|o| o.bundle.is_none())
            .map(weights_of)
            .collect::<Vec<_>>(),
        [
            "Qwen3.8-27B-iMatrix-NVFP4-MTP-Q4_K_M.gguf",
            "Qwen3.8-27B-iMatrix-NVFP4-MTP-Q8_0.gguf"
        ]
    );
}

#[test]
fn p3_v04_a_file_that_names_no_quantization_is_told_by_what_is_not_the_same_in_the_names() {
    let r = registry(&[]);
    let rules = owned(&r, "llamacpp", "hub");
    // seven files that name none: what they begin with in common is taken off
    let laguna = rules.present(&hub_candidate("mudler/Laguna-XS-2.1-APEX-GGUF"));
    assert_eq!(
        laguna.model_type.as_deref(),
        Some("code"),
        "its tags say `code`, its pipeline nothing"
    );
    assert_eq!(
        labels(&laguna, None),
        [
            "I-Mini · 11.9 GB",
            "Compact · 14.7 GB",
            "I-Compact · 14.7 GB",
            "Quality · 20.3 GB"
        ],
        "the four smallest, nothing being preferred"
    );
    assert!(laguna.offers.iter().all(|o| !o.recommended));

    // a repository with one file: its name as it is, and a subfolder is not part of what is kept
    let embedding = rules.present(&hub_candidate("mixedbread-ai/mxbai-embed-large-v1"));
    assert_eq!(embedding.model_type.as_deref(), Some("embedding"));
    assert_eq!(labels(&embedding, None), ["F16 · 638.6 MB"]);
    match &embedding.offers[0].fetch {
        Fetch::Files(download) => {
            let weights = download.weights().unwrap();
            assert_eq!(weights.store_as, "mxbai-embed-large-v1-f16.gguf");
            assert_eq!(
                weights.url,
                "https://huggingface.co/mixedbread-ai/mxbai-embed-large-v1/resolve/main/gguf/mxbai-embed-large-v1-f16.gguf"
            );
        }
        other => panic!("{other:?}"),
    }

    // two files that say the same quantization are told by their names
    let twice = Candidate {
        origin: Origin::Hub {
            site: "https://hub.example".into(),
            repo: "a/b".into(),
            revision: "main".into(),
            files: vec![
                RemoteFile::new("x/alpha-Q4_K_M.gguf", Some(5)),
                RemoteFile::new("y/beta-Q4_K_M.gguf", Some(6)),
            ],
        },
        ..said("a/b", &[], None, &[])
    };
    assert_eq!(
        labels(&rules.present(&twice), None),
        ["alpha-Q4_K_M · 5 B", "beta-Q4_K_M · 6 B"]
    );
    // a repository of nothing but a projector, or of no GGUF at all, has nothing to offer
    for files in [
        vec![RemoteFile::new("mmproj-f16.gguf", Some(1))],
        vec![
            RemoteFile::new("model.safetensors", Some(1)),
            RemoteFile::new("README.md", None),
        ],
        vec![RemoteFile::new("model-00001-of-00002.gguf", Some(1))],
        Vec::new(),
    ] {
        let none = Candidate {
            origin: Origin::Hub {
                site: "https://hub.example".into(),
                repo: "a/b".into(),
                revision: "main".into(),
                files,
            },
            ..said("a/b", &[], None, &[])
        };
        assert!(rules.present(&none).offers.is_empty());
    }
}

#[test]
fn p3_v05_a_card_of_the_library_has_a_button_for_every_size_and_says_how_big_it_is_for_a_guess() {
    let r = registry(&[]);
    let rules = owned(&r, "ollama", "library");
    assert_eq!(rules.settings.bytes_per_parameter, 0.6);
    assert_eq!(rules.settings.show_variants, 16);
    let page = ollama_library::parse_search(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../quickai-runtime/tests/fixtures/ollama/library/popular_local.html"),
        )
        .unwrap(),
    )
    .unwrap();
    let entry = |name: &str| {
        let model = page
            .models
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name}"));
        rules.present(&model.candidate())
    };
    let tev1 = entry("tev1");
    assert_eq!(labels(&tev1, None), ["0.8b · ~457.8 MB", "4b · ~2.2 GB"]);
    assert_eq!(classes(&tev1, None), ["small", "small"]);
    assert!(tev1
        .offers
        .iter()
        .all(|o| o.estimated && o.bundle.is_none() && !o.recommended));
    assert_eq!(
        tev1.offers
            .iter()
            .map(|o| o.fetch.clone())
            .collect::<Vec<_>>(),
        [
            Fetch::Reference("tev1:0.8b".into()),
            Fetch::Reference("tev1:4b".into())
        ]
    );
    assert!(
        tev1.offers[1].hover.contains("Pull tev1:4b") && tev1.offers[1].hover.contains("2.2 GB")
    );
    // 27 billion parameters: just over 15 GiB at 0.6 bytes each
    let big = entry("qwen3.8");
    let last = big.offers.last().unwrap();
    assert!(last.label.starts_with("27b · ~15.1 GB"), "{}", last.label);
    assert_eq!(last.class.as_deref(), Some("large"));
    // a card that shows no size is pulled by its name
    let laya = entry("laya");
    assert_eq!(labels(&laya, None), ["latest"]);
    assert_eq!(laya.offers[0].fetch, Fetch::Reference("laya".into()));
    assert_eq!(
        (laya.offers[0].bytes, laya.offers[0].class.clone()),
        (None, None)
    );
    // what the card says it can do decides its type, with no repository to ask
    assert_eq!(
        entry("embeddinggemma-2").model_type.as_deref(),
        Some("embedding")
    );
    assert_eq!(laya.model_type.as_deref(), Some("chat"));

    // the buttons are the smallest first, and no more than `show_variants`
    let many = Candidate {
        origin: Origin::Library {
            name: "m".into(),
            sizes: ["70b", "1b", "e2b", "8x7b", "weird", "22m", "3b"]
                .map(String::from)
                .to_vec(),
        },
        ..said("m", &[], None, &[])
    };
    let shown = rules.present(&many);
    assert_eq!(
        labels(&shown, None)
            .iter()
            .map(|l| l.split(' ').next().unwrap().to_string())
            .collect::<Vec<_>>(),
        ["22m", "1b", "e2b", "3b", "8x7b", "70b", "weird"],
        "a size that is not a count is last, where the card put it"
    );
    let mut few = rules.settings.clone();
    few.show_variants = 3;
    let capped = quickai_core::lookup::present::present(
        &many,
        &quickai_core::lookup::present::Rules {
            settings: &few,
            ..rules.rules()
        },
    );
    assert_eq!(capped.offers.len(), 3);
}

#[test]
fn p3_v06_the_recorded_answers_of_hugging_face_are_read_into_candidates() {
    let hits = huggingface::parse_search(&common::fixture("popular.json")).unwrap();
    assert_eq!(hits.len(), 50);
    assert_eq!(hits[0].id, "mudler/locate-anything.cpp-gguf");
    assert_eq!(hits[0].pipeline.as_deref(), Some("object-detection"));
    assert!(
        hits.windows(2).all(|w| w[0].downloads >= w[1].downloads),
        "most downloaded first"
    );
    let qwen = hits
        .iter()
        .find(|h| h.id == "unsloth/Qwen3.8-27B-GGUF")
        .unwrap();
    assert_eq!(
        qwen.pipeline, None,
        "the second of the day had no pipeline tag"
    );
    assert_eq!(qwen.downloads, 6_553_039);

    let candidate = hub_candidate("unsloth/Qwen3.8-27B-GGUF");
    assert_eq!(candidate.key, "hf:unsloth/Qwen3.8-27B-GGUF");
    assert_eq!(
        candidate.description,
        "Quantized from Qwen/Qwen3.8-27B · apache-2.0"
    );
    assert_eq!(candidate.downloads, Some(6_553_039));
    assert_eq!(candidate.likes, Some(qwen.likes));
    match &candidate.origin {
        Origin::Hub {
            site,
            repo,
            revision,
            files,
        } => {
            assert_eq!(
                (site.as_str(), repo.as_str(), revision.as_str()),
                ("https://huggingface.co", "unsloth/Qwen3.8-27B-GGUF", "main")
            );
            assert_eq!(
                files.iter().filter(|f| f.path.ends_with(".gguf")).count(),
                30
            );
            assert!(files.iter().all(|f| f.path.ends_with(".md")
                || f.path.ends_with(".json")
                || f.path == ".gitattributes"
                || f.size.is_some()));
        }
        other => panic!("{other:?}"),
    }
    // the search for a word answers in the same shape
    let found = huggingface::parse_search(&common::fixture("search_qwen.json")).unwrap();
    assert!(!found.is_empty() && found.iter().all(|h| huggingface::valid_repo_id(&h.id)));
    // every recorded repository reads as a repository, and none of them needs a login
    for id in [
        "ggml-org/gemma-3-4b-it-GGUF",
        "unsloth/Qwen3.8-27B-GGUF",
        "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
        "mixedbread-ai/mxbai-embed-large-v1",
        "mudler/Laguna-XS-2.1-APEX-GGUF",
        "mudler/locate-anything.cpp-gguf",
    ] {
        let repo = huggingface::parse_repo(&common::fixture(&format!(
            "repos/{}.json",
            id.replace('/', "__")
        )))
        .unwrap();
        assert!(
            !repo.gated && repo.files.iter().any(|f| f.path.ends_with(".gguf")),
            "{id}"
        );
    }
}

fn entry_of(model_type: Option<&str>, offers: &[Option<&str>]) -> Entry {
    Entry {
        key: format!("k:{model_type:?}:{offers:?}"),
        name: "n".into(),
        description: String::new(),
        downloads: None,
        likes: None,
        tags: Vec::new(),
        model_type: model_type.map(str::to_string),
        offers: offers
            .iter()
            .map(|class| Offer {
                bundle: None,
                label: "x".into(),
                hover: String::new(),
                bytes: None,
                estimated: false,
                class: class.map(str::to_string),
                recommended: false,
                fetch: Fetch::Reference("x".into()),
            })
            .collect(),
        card: None,
    }
}

#[test]
fn p3_v07_the_entries_are_laid_out_in_sections_by_type_and_a_size_keeps_the_buttons_in_its_class() {
    let r = registry(&[]);
    let types = owned(&r, "llamacpp", "hub").types;
    let entries = vec![
        entry_of(Some("other"), &[Some("small")]),
        entry_of(Some("vision"), &[Some("small"), Some("medium")]),
        entry_of(Some("chat"), &[Some("large")]),
        entry_of(Some("chat"), &[Some("small"), Some("small"), None]),
        entry_of(Some("embedding"), &[Some("small")]),
        entry_of(Some("no-such-type"), &[Some("small")]),
        entry_of(None, &[Some("medium")]),
        entry_of(Some("code"), &[]),
    ];
    let sections = arrange(&entries, &types, None);
    // the order the types say, not the order the entries came; what has nothing to show is not there
    assert_eq!(
        sections
            .iter()
            .map(|s| (s.type_id.as_str(), s.label.as_str(), s.open))
            .collect::<Vec<_>>(),
        [
            ("chat", "Chat", true),
            ("vision", "Vision", true),
            ("embedding", "Embedding", true),
            ("other", "Other", false),
        ]
    );
    let places = |id: &str| -> Vec<(usize, Vec<usize>)> {
        sections
            .iter()
            .find(|s| s.type_id == id)
            .unwrap()
            .entries
            .iter()
            .map(|e| (e.entry, e.offers.clone()))
            .collect()
    };
    // entries keep the order they came in, each with all its offers
    assert_eq!(places("chat"), [(2, vec![0]), (3, vec![0, 1, 2])]);
    assert_eq!(places("vision"), [(1, vec![0, 1])]);
    // what is of no type there is, or of none, is shown under Other, with what was found of that type
    assert_eq!(places("other"), [(0, vec![0]), (5, vec![0]), (6, vec![0])]);

    // a size keeps the offers of its class, and an entry with none of them is not shown
    let small = arrange(&entries, &types, Some("small"));
    let shown =
        |s: &Vec<quickai_core::lookup::view::Section>, id: &str| -> Vec<(usize, Vec<usize>)> {
            s.iter()
                .find(|x| x.type_id == id)
                .map(|x| {
                    x.entries
                        .iter()
                        .map(|e| (e.entry, e.offers.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };
    assert_eq!(
        shown(&small, "chat"),
        [(3, vec![0, 1])],
        "the large chat model is gone, the third button has no class"
    );
    assert_eq!(shown(&small, "vision"), [(1, vec![0])]);
    assert_eq!(shown(&small, "embedding"), [(4, vec![0])]);
    let large = arrange(&entries, &types, Some("large"));
    assert_eq!(
        large.iter().map(|s| s.type_id.as_str()).collect::<Vec<_>>(),
        ["chat"]
    );
    assert!(arrange(&entries, &types, Some("no-such-class")).is_empty());
    assert!(arrange(&[], &types, None).is_empty());

    // without types there is one section with no heading
    let flat = arrange(&entries, &[], None);
    assert_eq!(flat.len(), 1);
    assert_eq!((flat[0].type_id.as_str(), flat[0].label.as_str()), ("", ""));
    assert_eq!(
        flat[0].entries.len(),
        7,
        "the entry with no offer is not shown"
    );
}

const DOC: &str = "schema = 1\nkind = \"vocab\"\nid = \"{id}\"\n";

fn vocab(id: &str, body: &str) -> String {
    format!("{}{body}", DOC.replace("{id}", id))
}

#[test]
fn p3_v08_the_vocabulary_that_sorts_is_checked_where_it_is_written() {
    let one = |body: &str| registry_of(&[("embedded:vocab/t.toml", &vocab("t", body))]);
    let errors = |body: &str| texts(&one(body), Severity::Error);

    // a vocabulary that is right defines what it says
    let fine = one("[[model_type]]\nid = \"a\"\nmatch = { words = [\"x\"] }\n[[model_type]]\nid = \"b\"\ndefault = true\n[[size_class]]\nid = \"s\"\nbelow = \"1GB\"\n[[size_class]]\nid = \"l\"\n");
    assert!(fine.diagnostics.is_empty(), "{:#?}", fine.diagnostics);
    assert_eq!(fine.vocabulary().model_types().len(), 2);
    assert_eq!(fine.vocabulary().size_classes().len(), 2);
    assert_eq!(
        fine.vocabulary().model_types()[0].label,
        "a",
        "the label defaults to the id"
    );
    assert_eq!(
        fine.vocabulary().size_classes()[0].below,
        Some(1_000_000_000)
    );

    for (body, words) in [
        ("[[model_type]]\nid = \"Bad Id\"\n", &["`Bad Id` is not a valid model type id"][..]),
        ("[[model_type]]\nid = \"a\"\nmatch = { words = [\"x\"] }\n[[model_type]]\nid = \"a\"\n", &["`a` is written twice"]),
        ("[[model_type]]\nid = \"a\"\ndefault = true\n[[model_type]]\nid = \"b\"\ndefault = true\n", &["only one model type is the default"]),
        ("[[model_type]]\nid = \"a\"\nmatch = { words = [\"x\", \" \"] }\n", &["empty entry in `match.words`"]),
        ("[[size_class]]\nid = \"s\"\nbelow = \"lots\"\n", &["`lots` is not a size", "4GiB"]),
        ("[[size_class]]\nid = \"s\"\nbelow = \"0\"\n", &["is not a size"]),
        ("[[size_class]]\nid = \"s\"\nbelow = \"2GB\"\n[[size_class]]\nid = \"m\"\nbelow = \"1GB\"\n", &["ends at or below where `s` does", "smallest to the largest"]),
        ("[[size_class]]\nid = \"s\"\nbelow = \"1GB\"\n[[size_class]]\nid = \"m\"\nbelow = \"1GB\"\n", &["ends at or below where `s` does"]),
        ("[[size_class]]\nid = \"all\"\n[[size_class]]\nid = \"s\"\nbelow = \"1GB\"\n", &["`all` has no `below`", "must be the last"]),
        ("[[size_class]]\nid = \"s\"\nbelow = \"1GB\"\n[[size_class]]\nid = \"s\"\nbelow = \"2GB\"\n", &["`s` is written twice"]),
    ] {
        let found = errors(body);
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        for word in words {
            assert!(found[0].contains(word), "`{word}` is not in: {}", found[0]);
        }
    }
    // a type that nothing can be of is a warning: the document stays
    let useless = one("[[model_type]]\nid = \"a\"\n");
    assert_eq!(useless.vocabulary().model_types().len(), 1);
    let warnings = texts(&useless, Severity::Warning);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("nothing is ever of it"),
        "{}",
        warnings[0]
    );

    // two documents may not define the same type, a size class, or both be the default
    let both = registry_of(&[
        ("embedded:vocab/a.toml", &vocab("a", "[[model_type]]\nid = \"x\"\ndefault = true\n[[size_class]]\nid = \"s\"\nbelow = \"1GB\"\n")),
        ("embedded:vocab/b.toml", &vocab("b", "[[model_type]]\nid = \"x\"\nmatch = { words = [\"y\"] }\n[[model_type]]\nid = \"z\"\ndefault = true\n[[size_class]]\nid = \"s\"\nbelow = \"2GB\"\n")),
    ]);
    let found = texts(&both, Severity::Error);
    assert!(
        found
            .iter()
            .any(|e| e.contains("model type `x` is already defined by the vocabulary `a`")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|e| e.contains("`x` is already the default type")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|e| e.contains("size class `s` is already defined")),
        "{found:?}"
    );

    // the vocabulary the application ships is right, and says what the window needs
    let shipped = registry(&[]);
    assert!(shipped.diagnostics.is_empty(), "{:#?}", shipped.diagnostics);
    let ids: Vec<&str> = shipped
        .vocabulary()
        .model_types()
        .iter()
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "embedding",
            "audio",
            "documents",
            "vision",
            "code",
            "reasoning",
            "translation",
            "video",
            "image",
            "text_encoder",
            "other",
            "chat"
        ],
        "the order the rules are tried in: a document model is not a vision model for being read by one"
    );
    assert_eq!(
        shipped
            .vocabulary()
            .model_types()
            .iter()
            .filter(|t| t.default)
            .count(),
        1
    );

    // a vocabulary of the person's patches one type by `id` and adds another
    let person = registry(&[(
        "user:vocab/core.toml",
        "schema = 1\nkind = \"vocab\"\nid = \"core\"\n\n[[model_type]]\nid = \"code\"\nlabel = \"Programming\"\nmatch = { words = [\"coder\", \"codex\"] }\n\n[[model_type]]\nid = \"legal\"\nlabel = \"Legal\"\nmatch = { words = [\"lawyer\", \"legal\"] }\n\n[[size_class]]\nid = \"small\"\nbelow = \"2GiB\"\n",
    )]);
    assert!(person.diagnostics.is_empty(), "{:#?}", person.diagnostics);
    let types = person.vocabulary().model_types();
    let code = types.iter().find(|t| t.id == "code").unwrap();
    assert_eq!(code.label, "Programming");
    assert_eq!(
        code.rule.words,
        ["coder", "codex"],
        "a list is replaced, not added to"
    );
    assert_eq!(
        types.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        [
            "embedding",
            "audio",
            "documents",
            "vision",
            "code",
            "reasoning",
            "translation",
            "video",
            "image",
            "text_encoder",
            "other",
            "chat",
            "legal"
        ],
        "a type of the person's own is tried after the shipped ones"
    );
    assert_eq!(person.vocabulary().size_classes()[0].below, Some(2 * GIB));
    assert_eq!(
        person.vocabulary().size_classes()[1].below,
        Some(15 * GIB),
        "the others stay"
    );
    let words = strings(&[]);
    let legal = classify(
        types,
        &Facts {
            name: "acme/legal-llama-8b",
            capabilities: &words,
            tags: &words,
            companions: &words,
            pipeline: None,
        },
    );
    assert_eq!(legal.map(|t| t.id.as_str()), Some("legal"));
}

#[test]
fn p3_v09_the_look_up_of_hugging_face_is_what_the_document_declares_and_is_checked() {
    let shipped = registry(&[]);
    let hub = owned(&shipped, "llamacpp", "hub");
    assert_eq!(
        hub.settings.prefer,
        ["Q4_K_M", "Q4_K_S", "Q5_K_M", "Q4_0", "Q8_0", "Q6_K", "Q3_K_M"]
    );
    assert_eq!(
        hub.settings.skip,
        ["*-?????-of-?????*", "mtp/*", "mtp-*", "imatrix*"]
    );
    assert_eq!(hub.settings.show_variants, 4);
    assert_eq!(
        hub.settings.types,
        [
            "chat",
            "reasoning",
            "code",
            "translation",
            "vision",
            "documents",
            "embedding"
        ]
    );
    let engine = shipped.engine("llamacpp").unwrap();
    match &quickai_core::models::lookups(engine)[0].kind {
        quickai_core::registry::engine::LookupKind::HuggingFace {
            site,
            popular,
            keyword,
            similar,
            ..
        } => {
            assert_eq!(site, "https://huggingface.co");
            assert_eq!(
                (*popular, *keyword, *similar),
                (30, 20, 20),
                "what the old code looked into"
            );
        }
        other => panic!("{other:?}"),
    }
    // llama.cpp lists no models by name: its window is the one of files
    assert!(engine.models.as_ref().unwrap().list.is_none());

    let patch = |lookup: &str| {
        registry(&[(
            "user:engines/llamacpp.toml",
            &format!("schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\n[[models.lookup]]\nid = \"hub\"\n{lookup}\n"),
        )])
    };
    // the person's document patches the look-up by id: one key changes and the rest stay
    let moved = patch("site = \"http://127.0.0.1:9\"\nshow_variants = 7\n");
    assert!(moved.diagnostics.is_empty(), "{:#?}", moved.diagnostics);
    let patched = owned(&moved, "llamacpp", "hub");
    assert_eq!(patched.settings.show_variants, 7);
    assert_eq!(patched.settings.prefer, hub.settings.prefer);
    assert_eq!(
        quickai_core::models::lookups(moved.engine("llamacpp").unwrap())[0]
            .kind
            .site(),
        "http://127.0.0.1:9"
    );

    // what is wrong in it is refused with where and what to write, and the shipped document stays
    for (lookup, words) in [
        (
            "popular = 0",
            &["`0` is not a number of repositories", "1 to 50"][..],
        ),
        ("keyword = 51", &["`51` is not a number of repositories"]),
        ("similar = \"many\"", &["is not a number of repositories"]),
        (
            "show_variants = 0",
            &["is not a number of buttons", "1 to 64"],
        ),
        ("show_variants = 65", &["is not a number of buttons"]),
        ("prefer = \"Q4_K_M\"", &["is not a list"]),
        ("skip = [1]", &["is not a word"]),
        ("types = [\"chat\", \"\"]", &["is not a word"]),
        (
            "site = \"ftp://x\"",
            &["not an http or https address", "https://huggingface.co"],
        ),
    ] {
        let r = patch(lookup);
        let errors = texts(&r, Severity::Error);
        assert_eq!(errors.len(), 1, "{lookup}: {errors:?}");
        for word in words {
            assert!(
                errors[0].contains(word),
                "`{word}` is not in: {}",
                errors[0]
            );
        }
        let kept = owned(&r, "llamacpp", "hub");
        assert_eq!(
            kept.settings.show_variants, 4,
            "{lookup}: the document of the application stays"
        );
    }
    // a key it does not take is a warning, and so is a type no vocabulary defines (with the nearest)
    let extra = patch("sort = \"new\"\ntypes = [\"chat\", \"vison\"]");
    assert!(texts(&extra, Severity::Error).is_empty());
    let warnings = texts(&extra, Severity::Warning);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("`sort` is not a key of the look-up `huggingface_gguf`")),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(
            |w| w.contains("`vison` is not a model type any vocabulary defines")
                && w.contains("did you mean `vision`?")
        ),
        "{warnings:?}"
    );
    // a build without the primitive skips the document of the engine and says which
    let lacking = common::registry_with(
        &common::PROVIDED
            .iter()
            .copied()
            .filter(|p| *p != "lookup.huggingface_gguf@1")
            .collect::<Vec<_>>(),
        &[],
    );
    assert!(lacking.engine("llamacpp").is_none());
    assert!(texts(&lacking, Severity::Error)
        .iter()
        .any(|e| e.contains("lookup.huggingface_gguf@1")));
    // and Ollama's library keeps what it had, with the two keys the entries need
    let library = owned(&shipped, "ollama", "library");
    assert_eq!(
        (
            library.settings.bytes_per_parameter,
            library.settings.show_variants
        ),
        (0.6, 16)
    );
    assert!(library.settings.types.is_empty(), "every type is run");
}
