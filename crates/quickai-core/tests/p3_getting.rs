//! Group Q of the test plan (work package 4.13): getting models for an engine that runs on its own
//! (ComfyUI) from the window, the way llama.cpp's window does: what a look-up of the hub offers when the
//! files of a model are kept in folders of their own, checked on the answers the hub gave for the model
//! someone asked for (a video model, its text encoder and its VAE, three repositories of three people), and
//! what a document may say to make it so. What the window does with them is tested in the application
//! (`app/workbench.rs`, `prims/hub.rs`, `downloader.rs`).

mod common;

use common::{acme, fixture, owned, registry_of, shipped_registry, texts};
use quickai_core::diag::Severity;
use quickai_core::lookup::candidate::Candidate;
use quickai_core::lookup::huggingface::{
    candidate, parse_repo, parse_search, search_url, search_url_in, Hit,
};
use quickai_core::lookup::present::{Entry, Fetch, Offer, Piece};
use quickai_core::models::{folders, lookups, place_for};
use quickai_core::registry::engine::LookupKind;
use serde_json::Value;

const TURBO: &str = "hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF";
const ENCODER: &str = "city96/umt5-xxl-encoder-gguf";
const QUANTSTACK: &str = "QuantStack/Wan2.2-TI2V-5B-GGUF";
const LTX: &str = "biali/ltxv-gguf";

/// The candidate a recorded repository of ComfyUI's look-up makes: the hit of the recorded searches
/// (the plain list of a pipeline, the words `umt5`) when the repository is on one, and the recorded files.
fn candidate_of(id: &str) -> Candidate {
    let hits: Vec<Hit> = [
        "comfyui/search_text_to_video.json",
        "comfyui/search_umt5.json",
    ]
    .iter()
    .flat_map(|file| parse_search(&fixture(file)).expect("a recorded search"))
    .collect();
    let hit = hits.into_iter().find(|h| h.id == id).unwrap_or(Hit {
        id: id.to_string(),
        downloads: 0,
        likes: 0,
        pipeline: None,
        tags: Vec::new(),
    });
    let repo = parse_repo(&fixture(&format!(
        "comfyui/repos/{}.json",
        id.replace('/', "__")
    )))
    .unwrap_or_else(|e| panic!("{id}: {e}"));
    candidate("https://huggingface.co", &hit, &repo)
}

/// What a button fetches, as one file: `(kind, name it is kept under, folder, size, sha256)`.
fn one_file(offer: &Offer) -> (Option<&str>, &str, Option<&str>, Option<u64>, Option<&str>) {
    let Fetch::Files(download) = &offer.fetch else {
        panic!("files");
    };
    assert_eq!(download.files.len(), 1, "{offer:?}");
    let piece: &Piece = &download.files[0];
    (
        piece.kind.as_deref(),
        &piece.store_as,
        piece.place.as_deref(),
        piece.size,
        piece.sha256.as_deref(),
    )
}

fn labels(entry: &Entry, only: bool) -> Vec<&str> {
    entry
        .offers
        .iter()
        .filter(|o| o.only().is_some() == only && o.bundle.is_none())
        .map(|o| o.label.as_str())
        .collect()
}

// ---------------------------------------------------------------------------------------------- Q01

#[test]
fn p3_q01_the_model_its_text_encoder_and_its_vae_are_each_offered_in_the_folder_they_belong_in() {
    let registry = shipped_registry(&[]);
    let rules = owned(&registry, "comfyui", "hub");

    // the model: weights in `diffusion_models`, the quantizations the preferences like, smallest first,
    // the closest to the original in bold; the LoRAs beside them are not weights and not offered
    let turbo = rules.present(&candidate_of(TURBO));
    assert_eq!(turbo.model_type.as_deref(), Some("video"));
    assert_eq!(
        labels(&turbo, false)
            .iter()
            .map(|l| l.split(' ').next().unwrap())
            .collect::<Vec<_>>(),
        ["Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"]
    );
    assert!(
        labels(&turbo, true).is_empty(),
        "no VAE and no text encoder in it"
    );
    assert!(
        turbo.offers.iter().all(|o| o.bundle.is_none()),
        "no bundle: the repository lacks both"
    );
    let q8 = turbo
        .offers
        .iter()
        .find(|o| o.label.starts_with("Q8_0"))
        .unwrap();
    assert!(q8.recommended && turbo.offers.iter().filter(|o| o.recommended).count() == 1);
    assert_eq!(
        one_file(q8),
        (
            None,
            "Wan2_2-TI2V-5B-Turbo-Q8_0.gguf",
            Some("diffusion_models"),
            Some(5_404_990_176),
            Some("b4d7d1888d5ec2c29046cc443239faad99614cd3d356c61d8192fcac5f4c98fc")
        )
    );
    let Fetch::Files(download) = &q8.fetch else {
        panic!()
    };
    assert_eq!(
        download.files[0].url,
        "https://huggingface.co/hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF/resolve/main/Wan2_2-TI2V-5B-Turbo-Q8_0.gguf"
    );
    assert!(
        q8.hover.contains("checked against its SHA-256")
            && q8.hover.contains("kept in the diffusion_models folder"),
        "{}",
        q8.hover
    );

    // the text encoder: a repository of nothing else is offered by its files, in `text_encoders`, under the
    // name each has, and is a section of its own
    let encoder = rules.present(&candidate_of(ENCODER));
    assert_eq!(encoder.model_type.as_deref(), Some("text_encoder"));
    assert!(
        labels(&encoder, false).is_empty(),
        "nothing in it is weights"
    );
    assert_eq!(
        labels(&encoder, true)
            .iter()
            .map(|l| l.split(' ').next().unwrap())
            .collect::<Vec<_>>(),
        ["Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"],
        "the F16 and F32 files are not in the four the preferences like"
    );
    let q8 = encoder
        .offers
        .iter()
        .find(|o| o.label.starts_with("Q8_0"))
        .unwrap();
    assert!(q8.recommended);
    assert_eq!(
        one_file(q8),
        (
            Some("text_encoder"),
            "umt5-xxl-encoder-Q8_0.gguf",
            Some("text_encoders"),
            Some(6_043_068_256),
            Some("2521d4de0bf9e1cc6549866463ceae85e4ec3239bc6063f7488810be39033bbc")
        ),
        "under its own name, not `city96--umt5-xxl-encoder-gguf--...`"
    );
    assert_eq!(q8.only().map(|p| p.label.as_str()), Some("text encoder"));
    assert!(q8.bundle.is_none());

    // the VAE: in the repository of the model, a button of a row of its own that fetches it and nothing
    // else, into `vae`, from the folder of the repository it is in
    let quantstack = rules.present(&candidate_of(QUANTSTACK));
    assert_eq!(quantstack.model_type.as_deref(), Some("video"));
    assert_eq!(
        labels(&quantstack, false)
            .iter()
            .map(|l| l.split(' ').next().unwrap())
            .collect::<Vec<_>>(),
        ["Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"]
    );
    let vae: Vec<&Offer> = quantstack
        .offers
        .iter()
        .filter(|o| o.only().is_some())
        .collect();
    assert_eq!(vae.len(), 1, "{:?}", quantstack.offers);
    assert_eq!(
        one_file(vae[0]),
        (
            Some("vae"),
            "Wan2.2_VAE.safetensors",
            Some("vae"),
            Some(1_409_400_960),
            Some("e40321bd36b9709991dae2530eb4ac303dd168276980d3e9bc4b6e2b75fed156")
        )
    );
    assert!(vae[0].label.starts_with("Wan2.2_VAE"), "{}", vae[0].label);
    let Fetch::Files(download) = &vae[0].fetch else {
        panic!()
    };
    assert_eq!(
        download.files[0].url,
        "https://huggingface.co/QuantStack/Wan2.2-TI2V-5B-GGUF/resolve/main/VAE/Wan2.2_VAE.safetensors",
        "from the folder it is in, in the repository"
    );
    assert_eq!(download.files[0].name, "Wan2.2_VAE.safetensors");
    assert_eq!(download.bundle, None);
    assert!(
        vae[0].hover.contains("VAE alone, without a model")
            && vae[0].hover.contains("kept in the vae folder"),
        "{}",
        vae[0].hover
    );

    // all of it counts as the sizes the person reads on the buttons
    assert_eq!(vae[0].bytes, Some(1_409_400_960));
    assert_eq!(vae[0].class.as_deref(), Some("small"));
}

#[test]
fn p3_q01b_a_repository_that_holds_the_model_and_both_files_still_has_the_bundle_and_the_rows_by_themselves(
) {
    let registry = shipped_registry(&[]);
    let rules = owned(&registry, "comfyui", "hub");
    let ltx = rules.present(&candidate_of(LTX));
    assert_eq!(ltx.model_type.as_deref(), Some("video"));
    // the bundle the recommended list has is on offer for a look-up too, and its extra files keep the
    // names they have always had (`<owner>--<repo>--<name>`), in their folders
    let bundled: Vec<&Offer> = ltx
        .offers
        .iter()
        .filter(|o| o.bundle.as_deref() == Some("video"))
        .collect();
    assert!(
        !bundled.is_empty(),
        "{:?}",
        ltx.offers.iter().map(|o| &o.label).collect::<Vec<_>>()
    );
    let Fetch::Files(download) = &bundled[0].fetch else {
        panic!()
    };
    let kinds: Vec<(&str, Option<&str>, &str)> = download
        .files
        .iter()
        .map(|p| {
            (
                p.kind.as_deref().unwrap_or("weights"),
                p.place.as_deref(),
                p.store_as.as_str(),
            )
        })
        .collect();
    assert_eq!(kinds[0].0, "weights");
    assert_eq!(kinds[1].0, "text_encoder");
    assert_eq!(kinds[1].1, Some("text_encoders"));
    assert!(kinds[1].2.starts_with("biali--ltxv-gguf--"), "{kinds:?}");
    assert_eq!(kinds[2].0, "vae");
    assert!(kinds[2].2.starts_with("biali--ltxv-gguf--"), "{kinds:?}");
    // and the same files are each offered by themselves, under their own names
    let alone: Vec<&Offer> = ltx.offers.iter().filter(|o| o.only().is_some()).collect();
    assert!(!alone.is_empty());
    for offer in &alone {
        let (_, name, _, _, _) = one_file(offer);
        assert!(!name.contains("--"), "{name}");
    }
    // the weights are not the files that go with them
    for offer in ltx
        .offers
        .iter()
        .filter(|o| o.bundle.is_none() && o.only().is_none())
    {
        let (_, name, _, _, _) = one_file(offer);
        assert!(
            name.starts_with("ltx-video-2b") || name.starts_with("ltxv"),
            "{name}"
        );
    }
}

#[test]
fn p3_q01c_a_file_a_pattern_finds_is_offered_by_itself_only_when_it_is_a_model_and_not_a_shard() {
    let registry = shipped_registry(&[]);
    let rules = owned(&registry, "comfyui", "hub");
    let body = r#"{"siblings":[
      {"rfilename":"README.md","size":10},
      {"rfilename":"text_encoder/README.md","size":11},
      {"rfilename":"text_encoder/config.json","size":12},
      {"rfilename":"text_encoder/model-Q8_0.gguf","size":13},
      {"rfilename":"umt5-xxl-encoder-Q8_0-00001-of-00002.gguf","size":14},
      {"rfilename":"vae/diffusion_pytorch_model.bin","size":15},
      {"rfilename":"vae/ae.safetensors","size":16},
      {"rfilename":"vae/ae.SafeTensors.html","size":17},
      {"rfilename":"VAE/other_vae.safetensors","size":18}
    ]}"#;
    let hit = Hit {
        id: "a/b".into(),
        downloads: 1,
        likes: 0,
        pipeline: None,
        tags: vec![],
    };
    let entry = rules.present(&candidate(
        "https://huggingface.co",
        &hit,
        &parse_repo(body).unwrap(),
    ));
    let mut got: Vec<(String, String)> = entry
        .offers
        .iter()
        .filter_map(|o| o.only().map(|p| (p.kind.clone().unwrap(), p.name.clone())))
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            ("text_encoder".to_string(), "model-Q8_0.gguf".to_string()),
            ("vae".to_string(), "ae.safetensors".to_string()),
            ("vae".to_string(), "other_vae.safetensors".to_string()),
        ],
        "a note, a configuration, a pickle, a page, and a shard of a file are not what the person asked for"
    );
    // the three that are, are the only things there are to fetch
    assert_eq!(entry.offers.len(), 3);
}

#[test]
fn p3_q01d_the_example_workflow_of_the_docs_names_the_files_the_three_buttons_store() {
    let registry = shipped_registry(&[]);
    let rules = owned(&registry, "comfyui", "hub");
    let workflow: Value = serde_json::from_str(include_str!(
        "../../../docs/examples/workflows/wan22-ti2v-5b-turbo-t2v.json"
    ))
    .expect("the workflow is JSON");
    let stored = |id: &str, label: &str| -> String {
        let entry = rules.present(&candidate_of(id));
        let offer = entry
            .offers
            .iter()
            .find(|o| o.label.starts_with(label))
            .unwrap_or_else(|| panic!("{label} in {id}"));
        one_file(offer).1.to_string()
    };
    // the file each button of a look-up stores is the file the workflow loads
    assert_eq!(workflow["1"]["class_type"], "UnetLoaderGGUF");
    assert_eq!(
        workflow["1"]["inputs"]["unet_name"],
        stored(TURBO, "Q8_0").as_str()
    );
    assert_eq!(workflow["2"]["class_type"], "CLIPLoaderGGUF");
    assert_eq!(
        workflow["2"]["inputs"]["clip_name"],
        stored(ENCODER, "Q8_0").as_str()
    );
    assert_eq!(workflow["2"]["inputs"]["type"], "wan");
    assert_eq!(workflow["3"]["class_type"], "VAELoader");
    assert_eq!(
        workflow["3"]["inputs"]["vae_name"],
        stored(QUANTSTACK, "Wan2.2_VAE").as_str()
    );
    // the settings of the model's card (4 steps, a guidance of 1, Euler, the simple scheduler) and tiles that
    // fit a card of 12 GiB (at 256 and 32 the decode ran out of memory twice and took 453 s instead of 183)
    let sampler = &workflow["8"]["inputs"];
    assert_eq!(
        (
            sampler["steps"].as_u64(),
            sampler["cfg"].as_f64(),
            sampler["sampler_name"].as_str(),
            sampler["scheduler"].as_str()
        ),
        (Some(4), Some(1.0), Some("euler"), Some("simple"))
    );
    let decode = &workflow["9"]["inputs"];
    assert!(
        decode["tile_size"].as_u64().unwrap() <= 192
            && decode["temporal_size"].as_u64().unwrap() <= 16,
        "{decode}"
    );
}

// ---------------------------------------------------------------------------------------------- Q02

#[test]
fn p3_q02_the_look_up_of_llamacpp_reads_the_same_answers_as_it_did_and_has_no_file_by_itself() {
    let registry = shipped_registry(&[]);
    let rules = owned(&registry, "llamacpp", "hub");
    assert!(rules.settings.alone.is_empty());
    assert!(rules.places.weights.is_none() && rules.places.kinds.is_empty());
    for id in [TURBO, ENCODER, QUANTSTACK] {
        let entry = rules.present(&candidate_of(id));
        assert!(entry.offers.iter().all(|o| o.only().is_none()), "{id}");
        let Some(first) = entry.offers.first() else {
            panic!("{id} has weights for llama.cpp as it always did");
        };
        let Fetch::Files(download) = &first.fetch else {
            panic!()
        };
        assert_eq!(download.files[0].place, None, "{id}: one folder, as ever");
        assert!(!first.hover.contains("kept in"), "{}", first.hover);
        // the files of a type llama.cpp does not run are where they were
        assert_ne!(entry.model_type.as_deref(), Some("text_encoder"), "{id}");
        assert_ne!(entry.model_type.as_deref(), Some("image"), "{id}");
    }
    // the VAE is not weights and is not offered
    let quantstack = rules.present(&candidate_of(QUANTSTACK));
    assert!(quantstack.offers.iter().all(|o| !o.label.contains("VAE")));
    assert_eq!(
        quantstack.model_type.as_deref(),
        Some("other"),
        "a video model is not one it runs"
    );
}

// ---------------------------------------------------------------------------------------------- Q03

#[test]
fn p3_q03_the_checksum_the_hub_states_for_a_file_is_read_and_a_word_that_is_none_is_not() {
    let sha = "ab".repeat(32);
    let body = format!(
        r#"{{"siblings":[
          {{"rfilename":"a.gguf","size":1,"lfs":{{"sha256":"{sha}","size":1,"pointerSize":134}}}},
          {{"rfilename":"upper.gguf","size":2,"lfs":{{"sha256":"{}"}}}},
          {{"rfilename":"short.gguf","size":3,"lfs":{{"sha256":"abc"}}}},
          {{"rfilename":"words.gguf","size":4,"lfs":{{"sha256":"{}"}}}},
          {{"rfilename":"number.gguf","size":5,"lfs":{{"sha256":5}}}},
          {{"rfilename":"odd.gguf","size":6,"lfs":5}},
          {{"rfilename":"null.gguf","size":7,"lfs":null}},
          {{"rfilename":"none.gguf","size":8}}
        ]}}"#,
        sha.to_uppercase(),
        "z".repeat(64)
    );
    let repo = parse_repo(&body).unwrap();
    let got: Vec<(&str, Option<&str>)> = repo
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("a.gguf", Some(sha.as_str())),
            ("upper.gguf", Some(sha.as_str())),
            ("short.gguf", None),
            ("words.gguf", None),
            ("number.gguf", None),
            ("odd.gguf", None),
            ("null.gguf", None),
            ("none.gguf", None),
        ],
        "a hash is 64 hexadecimal digits, in any case, kept lower case; whatever else costs the file its hash and not its place"
    );
    // it reaches the piece a button fetches, which a download checks as the bytes arrive (llama.cpp shows
    // four buttons, so a repository of three files shows them all)
    let registry = shipped_registry(&[]);
    let rules = owned(&registry, "llamacpp", "hub");
    let hit = Hit {
        id: "a/b".into(),
        downloads: 0,
        likes: 0,
        pipeline: None,
        tags: vec![],
    };
    let three = parse_repo(&format!(
        r#"{{"siblings":[
          {{"rfilename":"a.gguf","size":1,"lfs":{{"sha256":"{sha}"}}}},
          {{"rfilename":"short.gguf","size":3,"lfs":{{"sha256":"abc"}}}},
          {{"rfilename":"none.gguf","size":8}}
        ]}}"#
    ))
    .unwrap();
    let entry = rules.present(&candidate("https://huggingface.co", &hit, &three));
    let hashed: Vec<(&str, bool)> = entry
        .offers
        .iter()
        .map(|o| match &o.fetch {
            Fetch::Files(d) => (d.files[0].name.as_str(), d.files[0].sha256.is_some()),
            _ => panic!(),
        })
        .collect();
    assert!(
        hashed.contains(&("a.gguf", true)) && hashed.contains(&("none.gguf", false)),
        "{hashed:?}"
    );
    let with = entry
        .offers
        .iter()
        .find(|o| o.hover.contains("a.gguf"))
        .unwrap();
    let without = entry
        .offers
        .iter()
        .find(|o| o.hover.contains("none.gguf"))
        .unwrap();
    assert!(
        with.hover.contains("checked against its SHA-256"),
        "{}",
        with.hover
    );
    assert!(!without.hover.contains("SHA-256"), "{}", without.hover);
}

// ---------------------------------------------------------------------------------------------- Q04

#[test]
fn p3_q04_the_plain_list_of_a_pipeline_is_a_search_that_says_so_and_a_pipeline_is_what_the_hub_calls_one(
) {
    let site = "https://huggingface.co";
    assert_eq!(
        search_url_in(site, None, Some("text-to-video")),
        "https://huggingface.co/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&pipeline_tag=text-to-video"
    );
    assert_eq!(
        search_url_in(site, Some(" wan2.2  ti2v "), Some("text-to-video")),
        "https://huggingface.co/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&pipeline_tag=text-to-video&search=wan2.2%20ti2v"
    );
    // without one it is the search it always was
    assert_eq!(
        search_url_in(site, Some("qwen"), None),
        search_url(site, Some("qwen"))
    );
    // a word that is not a pipeline is not put in an address
    for bad in [
        "",
        "Text-To-Video",
        "text to video",
        "a/b",
        "a&b=c",
        "x?y",
        &"a".repeat(65),
    ] {
        assert_eq!(
            search_url_in(site, None, Some(bad)),
            search_url(site, None),
            "{bad:?}"
        );
    }
}

#[test]
fn p3_q04b_the_look_up_of_comfyui_says_its_pipelines_what_it_offers_by_itself_and_where_a_file_may_go(
) {
    let registry = shipped_registry(&[]);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let engine = registry.engine("comfyui").unwrap();
    let hub = &lookups(engine)[0];
    let LookupKind::HuggingFace {
        popular,
        pipelines,
        words,
        read,
        ..
    } = &hub.kind
    else {
        panic!("the hub")
    };
    assert_eq!(
        pipelines,
        &[
            "text-to-video",
            "image-to-video",
            "text-to-image",
            "image-to-image"
        ]
    );
    assert_eq!(
        words,
        &["encoder"],
        "a text encoder carries no pipeline: it is searched for by name"
    );
    assert_eq!(*popular, 6);
    assert_eq!(read.alone, ["text_encoder", "vae"]);
    assert_eq!(read.types, ["video", "image", "text_encoder"]);
    assert_eq!(read.prefer[0], "Q8_0");
    // llama.cpp's has no pipelines and nothing by itself, as before
    let llama = registry.engine("llamacpp").unwrap();
    let LookupKind::HuggingFace {
        pipelines,
        words,
        read,
        ..
    } = &lookups(llama)[0].kind
    else {
        panic!("the hub")
    };
    assert!(pipelines.is_empty() && words.is_empty() && read.alone.is_empty());

    // what a document may say
    let with = |lookup: &str, bundle: bool| {
        let bundle = if bundle {
            "[[models.bundle]]\nid = \"b\"\nparts = [ { kind = \"mmproj\", match = [\"*proj*\"] } ]\n"
        } else {
            ""
        };
        let doc = acme(&format!(
            "[models]\n[[models.lookup]]\nid = \"hub\"\nuse = \"huggingface_gguf\"\n{lookup}\n{bundle}"
        ));
        registry_of(&[("embedded:engines/acme.toml", &doc)])
    };
    let errors = |lookup: &str, bundle: bool| texts(&with(lookup, bundle), Severity::Error);
    assert!(errors(
        "pipelines = [\"text-to-video\", \"image-to-video\"]\nalone = [\"mmproj\"]",
        true
    )
    .is_empty());
    let built = with("pipelines = [\"a\", \"a\", \"b\"]", false);
    let LookupKind::HuggingFace { pipelines, .. } = &lookups(built.engine("acme").unwrap())[0].kind
    else {
        panic!()
    };
    assert_eq!(pipelines, &["a", "b"], "once each");
    for (lookup, expect) in [
        (
            "pipelines = [\"Text To Video\"]",
            "is not a pipeline of the hub",
        ),
        ("pipelines = [5]", "is not a pipeline of the hub"),
        (
            "pipelines = \"text-to-video\"",
            "is not a list of at most eight pipelines",
        ),
        (
            "pipelines = [\"a\",\"b\",\"c\",\"d\",\"e\",\"f\",\"g\",\"h\",\"i\"]",
            "at most eight",
        ),
    ] {
        let found = errors(lookup, false);
        assert!(
            found.iter().any(|e| e.contains(expect)),
            "{lookup}: {found:?}"
        );
    }
    // the words the plain list is also searched for
    let built = with(
        "words = [\"encoder\", \"text encoder\", \"  encoder \"]",
        false,
    );
    let LookupKind::HuggingFace { words, .. } = &lookups(built.engine("acme").unwrap())[0].kind
    else {
        panic!()
    };
    assert_eq!(words, &["encoder", "text encoder"], "once each");
    for (lookup, expect) in [
        ("words = [\"\"]", "is not a few plain words"),
        ("words = [\"text  encoder\"]", "is not a few plain words"),
        ("words = [\"a:b\"]", "is not a few plain words"),
        ("words = [5]", "is not a few plain words"),
        ("words = [\"a\",\"b\",\"c\",\"d\",\"e\"]", "at most four"),
        (
            "words = \"encoder\"",
            "is not a list of at most four searches",
        ),
    ] {
        let found = errors(lookup, false);
        assert!(
            found.iter().any(|e| e.contains(expect)),
            "{lookup}: {found:?}"
        );
    }
    // a kind offered by itself is one a part of a bundle finds
    let found = errors("alone = [\"vae\"]", true);
    assert!(
        found
            .iter()
            .any(|e| e.contains("finds a file of the kind `vae`")
                && e.contains("the kinds its bundles find: mmproj")),
        "{found:?}"
    );
    let found = errors("alone = [\"vae\"]", false);
    assert!(
        found
            .iter()
            .any(|e| e.contains("a [[models.bundle]] part finds")),
        "{found:?}"
    );
    let found = errors("alone = \"vae\"", true);
    assert!(
        found.iter().any(|e| e.contains("is not a list")),
        "{found:?}"
    );
    // the folders a file by hand may go in are plain names
    let doc = acme(
        "[models]\n[models.acquire]\nuse = \"file\"\ndir = \"${data}/m\"\nplace = \"a\"\nplaces = [\"b\", \"Not A Folder\", \"../c\", \"b\"]\n",
    );
    let found = texts(
        &registry_of(&[("embedded:engines/acme.toml", &doc)]),
        Severity::Error,
    );
    assert_eq!(
        found
            .iter()
            .filter(|e| e.contains("is not the name of a folder"))
            .count(),
        2,
        "{found:?}"
    );
    // and a key the look-up of Ollama does not take is a warning, as for any key
    let ollama = acme(
        "[models]\n[[models.lookup]]\nid = \"lib\"\nuse = \"ollama_library\"\nalone = [\"x\"]\n",
    );
    let found = texts(
        &registry_of(&[("embedded:engines/acme.toml", &ollama)]),
        Severity::Warning,
    );
    assert!(
        found
            .iter()
            .any(|w| w.contains("`alone` is not a key of the look-up")),
        "{found:?}"
    );
}

// ---------------------------------------------------------------------------------------------- Q05

#[test]
fn p3_q05_the_vocabulary_sorts_what_comfyui_runs_into_video_pictures_and_text_encoders() {
    let registry = shipped_registry(&[]);
    let comfy = owned(&registry, "comfyui", "hub");
    let llama = owned(&registry, "llamacpp", "hub");
    let candidate_with = |pipeline: Option<&str>, name: &str, files: &[&str]| -> Candidate {
        let body = format!(
            r#"{{"siblings":[{}]}}"#,
            files
                .iter()
                .map(|f| format!(r#"{{"rfilename":"{f}","size":10}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let hit = Hit {
            id: name.to_string(),
            downloads: 1,
            likes: 0,
            pipeline: pipeline.map(str::to_string),
            tags: vec!["gguf".into()],
        };
        candidate("https://huggingface.co", &hit, &parse_repo(&body).unwrap())
    };
    let kind_of = |rules: &common::Owned, c: &Candidate| rules.present(c).model_type.unwrap();
    let picture = candidate_with(Some("text-to-image"), "a/flux-gguf", &["flux-Q4_K_M.gguf"]);
    let edit = candidate_with(
        Some("image-to-image"),
        "a/kontext-gguf",
        &["kontext-Q4_K_M.gguf"],
    );
    let clip = candidate_with(Some("text-to-video"), "a/clip-gguf", &["clip-Q4_K_M.gguf"]);
    let encoder = candidate_with(
        None,
        "a/t5-v1_1-xxl-encoder-gguf",
        &["t5-v1_1-xxl-encoder-Q8_0.gguf"],
    );
    let named_encoder = candidate_with(
        None,
        "a/qwen-image-gguf",
        &["Qwen-Image-Text-Encoder-Q8_0.gguf"],
    );
    let language = candidate_with(
        Some("text-generation"),
        "a/qwen3-gguf",
        &["Qwen3-4B-Q4_K_M.gguf"],
    );
    assert_eq!(kind_of(&comfy, &picture), "image");
    assert_eq!(kind_of(&comfy, &edit), "image");
    assert_eq!(kind_of(&comfy, &clip), "video");
    assert_eq!(kind_of(&comfy, &encoder), "text_encoder");
    assert_eq!(kind_of(&comfy, &named_encoder), "text_encoder");
    // a language model is not what ComfyUI runs: it is listed (folded) as other
    assert_eq!(kind_of(&comfy, &language), "other");
    // llama.cpp does not run any of these, so it lists them where it always did
    for c in [&picture, &edit, &clip] {
        assert_eq!(kind_of(&llama, c), "other");
    }
    assert_eq!(kind_of(&llama, &language), "chat");
    // a text encoder is found by its files and not by the words in its name, so llama.cpp's list is unmoved
    assert_eq!(kind_of(&llama, &encoder), "chat");
    // in the order the sections are shown
    let ids: Vec<&str> =
        quickai_core::lookup::types::in_display_order(registry.vocabulary().model_types())
            .iter()
            .map(|t| t.id.as_str())
            .collect();
    let at = |id: &str| ids.iter().position(|i| *i == id).unwrap();
    assert!(
        at("video") < at("image")
            && at("image") < at("text_encoder")
            && at("text_encoder") < at("other"),
        "{ids:?}"
    );
}

// ---------------------------------------------------------------------------------------------- Q06

#[test]
fn p3_q06_the_folders_of_an_engine_and_the_folder_a_file_belongs_in_come_from_its_document() {
    let registry = shipped_registry(&[]);
    let comfy = registry.engine("comfyui").unwrap();
    assert_eq!(
        folders(comfy),
        [
            "diffusion_models",
            "loras",
            "text_encoders",
            "vae",
            "checkpoints",
            "controlnet",
            "clip_vision",
            "upscale_models",
            "embeddings"
        ],
        "the weights', then each kind of companion's (in the order of the kinds), then the others the document names, once each"
    );
    assert!(folders(registry.engine("llamacpp").unwrap()).is_empty());
    assert!(folders(registry.engine("ollama").unwrap()).is_empty());

    for (name, folder) in [
        ("Wan2.2_VAE.safetensors", Some("vae")),
        ("ltxv_0.9.5_vae_fp16.safetensors", Some("vae")),
        ("umt5-xxl-encoder-Q8_0.gguf", Some("text_encoders")),
        ("UMT5-XXL-Encoder-Q5_K_M.GGUF", Some("text_encoders")),
        ("t5xxl_fp16-q4_0.gguf", Some("text_encoders")),
        ("t5xl_fp16-q8_0.gguf", Some("text_encoders")),
        ("byt5_small_glyphxl_fp32-q8_0.gguf", Some("text_encoders")),
        ("umt5base_fp32-q8_0.gguf", Some("text_encoders")),
        ("flux1-dev-Q5_0.gguf", Some("diffusion_models")),
        ("sd3.5_medium-Q5_K_S.gguf", Some("diffusion_models")),
        ("hidream-i1-full-Q5_K_M.gguf", Some("diffusion_models")),
        ("wan2.1-t2v-14b-Q5_K_M.gguf", Some("diffusion_models")),
        (
            "Qwen2.5-VL-7B-Instruct-Q5_K_M.gguf",
            Some("diffusion_models"),
        ),
        ("t5-v1_1-xxl-encoder-Q8_0.gguf", Some("text_encoders")),
        ("Qwen-Image-Text-Encoder-Q8_0.gguf", Some("text_encoders")),
        ("clip_l.safetensors", Some("diffusion_models")),
        ("Wan2_2-TI2V-5B-Turbo-Q8_0.gguf", Some("diffusion_models")),
        ("flux1-dev-Q5_0.gguf", Some("diffusion_models")),
        ("ltx-video-2b-v0.9-q4_k_m.gguf", Some("diffusion_models")),
    ] {
        assert_eq!(place_for(comfy, name).as_deref(), folder, "{name}");
    }
    // an engine that keeps its files in one folder has no folder to guess
    assert_eq!(
        place_for(
            registry.engine("llamacpp").unwrap(),
            "Wan2.2_VAE.safetensors"
        ),
        None
    );
}

// ---------------------------------------------------------------------------------------------- Q07

#[test]
fn p3_q07_a_button_that_fetches_into_folders_says_which() {
    let registry = shipped_registry(&[]);
    let rules = owned(&registry, "comfyui", "hub");
    let ltx = rules.present(&candidate_of(LTX));
    let bundled = ltx.offers.iter().find(|o| o.bundle.is_some()).unwrap();
    assert!(
        bundled
            .hover
            .contains("It is kept in the diffusion_models, text_encoders and vae folders."),
        "{}",
        bundled.hover
    );
    let single = ltx
        .offers
        .iter()
        .find(|o| o.bundle.is_none() && o.only().is_none())
        .unwrap();
    assert!(
        single
            .hover
            .ends_with("It is kept in the diffusion_models folder."),
        "{}",
        single.hover
    );
    // a sentence ends before another begins, whatever the one before it ended with
    assert!(!single.hover.contains(".."), "{}", single.hover);
    assert!(!single.hover.contains(" ."), "{}", single.hover);
    // the recommended list says it too, as it builds its entries with the same places
    let video = quickai_core::models::recommended(&registry, registry.engine("comfyui").unwrap());
    let ltx_listed = video
        .iter()
        .find(|e| e.name.starts_with("LTX-Video"))
        .expect("the list's video model");
    assert!(
        ltx_listed.offers[0]
            .hover
            .contains("kept in the diffusion_models, text_encoders and vae folders"),
        "{}",
        ltx_listed.offers[0].hover
    );
}
