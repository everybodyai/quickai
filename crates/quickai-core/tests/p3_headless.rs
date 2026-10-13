//! Group O of docs/phase3-test-plan.md, the pure half: what the headless program (`quickai-run`, WP4.18,
//! docs/headless.md) asks of the core. The window and that program both fetch what a configuration names,
//! and they must not be able to fetch two different things for the same words, so the choice is the core's.
//! The models are made up (`acme/...`), as in group J, so that no test depends on what the list holds.

mod common;

use common::registry;
use quickai_core::lookup::present::Fetch;
use quickai_core::models::{catalog_fetch, recommended};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const REV: &str = "0123456789abcdef0123456789abcdef01234567";

fn catalog() -> String {
    format!(
        r#"schema = 1
kind = "catalog"
id = "t"
title = "Test"
verified = "2026-10-12"

[[model]]
id = "alpha-4b"
name = "Alpha 4B"
by = "Acme"
type = "chat"
note = "A small model that sees pictures."
license = "Apache-2.0"
params = "4B"
input = ["text", "image"]

[model.run.llamacpp]
repo = "acme/Alpha-4B-GGUF"
rev = "{REV}"
files = [
  {{ file = "Alpha-4B-Q8_0.gguf", size = 4300000000, sha256 = "{B}" }},
  {{ file = "Alpha-4B-Q4_K_M.gguf", size = 2500000000, sha256 = "{A}", best = true }},
]
[model.run.llamacpp.companions]
mmproj = {{ file = "mmproj-Alpha-4B-F16.gguf", size = 800000000, sha256 = "{C}" }}

[model.run.ollama]
ref = "alpha:4b"
size = 3300000000
"#
    )
}

/// O50: a configuration names a file of a model of the list, and what is fetched for those words is the
/// weights of that file with the projector the list pairs with them, before the weights alone; a file the
/// list has not, a model it has not, and a model that an engine pulls by name are nothing to fetch as files.
#[test]
fn p3_o50_the_words_of_a_configuration_name_one_download_and_the_pairing_comes_first() {
    let r = registry(&[("user:catalog/t.toml", &catalog())]);
    let llama = r.engine("llamacpp").expect("llama.cpp");
    let entries = recommended(&r, llama);

    let Some(Fetch::Files(download)) = catalog_fetch(&entries, "alpha-4b", "Alpha-4B-Q4_K_M.gguf")
    else {
        panic!("the list has this file for llama.cpp");
    };
    let names: Vec<&str> = download.files.iter().map(|f| f.store_as.as_str()).collect();
    assert_eq!(
        names,
        [
            "Alpha-4B-Q4_K_M.gguf",
            "acme--Alpha-4B-GGUF--mmproj-Alpha-4B-F16.gguf"
        ],
        "the weights, then the projector the list pairs with them (kept as owner--repo--name): one download, as the window's button gives"
    );
    assert_eq!(download.bundle.as_deref(), Some("vision"));

    // the other quantization is another download, and a file the list has not is none
    let Some(Fetch::Files(other)) = catalog_fetch(&entries, "alpha-4b", "Alpha-4B-Q8_0.gguf")
    else {
        panic!("the list has the other file too");
    };
    assert_eq!(other.weights().unwrap().store_as, "Alpha-4B-Q8_0.gguf");
    assert!(catalog_fetch(&entries, "alpha-4b", "Alpha-4B-Q2_K.gguf").is_none());
    assert!(catalog_fetch(&entries, "no-such-model", "Alpha-4B-Q4_K_M.gguf").is_none());
    // the projector alone is not a model to name
    assert!(catalog_fetch(&entries, "alpha-4b", "mmproj-Alpha-4B-F16.gguf").is_none());

    // for Ollama the same model is a name that the daemon pulls, which no file is
    let ollama = r.engine("ollama").expect("Ollama");
    let for_ollama = recommended(&r, ollama);
    assert!(
        for_ollama.iter().any(|e| e.key == "catalog:alpha-4b"
            && e.offers
                .iter()
                .all(|o| matches!(&o.fetch, Fetch::Reference(name) if name == "alpha:4b"))),
        "Ollama is offered the name"
    );
    assert!(catalog_fetch(&for_ollama, "alpha-4b", "Alpha-4B-Q4_K_M.gguf").is_none());
}

// ---------------------------------------------------------------------------------------------- O60-O63
// Which build to install when a configuration names none and none is installed: the headless program
// has no list to look at and no person to choose from it, so the document's own words decide, and the
// rule is the same for every engine.

use quickai_core::acquire::{pick_build, Offered};

fn offered<'a>(rows: &[(&'a str, &'a str, bool, bool)]) -> Vec<Offered<'a>> {
    rows.iter()
        .map(|(tag, asset_name, tested, conflict)| Offered {
            tag,
            asset_name,
            tested: *tested,
            conflict: *conflict,
            size: None,
        })
        .collect()
}

/// O60: an engine whose document has a tested release gets that release, whatever is newer, and a build of
/// it whose pin and the feed's digest disagree is not taken.
#[test]
fn p3_o60_the_release_a_document_was_tested_with_is_the_one_when_nothing_is_named() {
    let r = common::shipped_registry(&[]);
    let ollama = r.engine("ollama").expect("Ollama");
    let tested = quickai_core::acquire::tested_tag(ollama).expect("a tested release");
    let rows = [
        ("v9.9.9", "ollama-linux-amd64.tar.zst", false, false),
        (tested, "ollama-linux-amd64.tar.zst", true, false),
        ("v9.9.8", "ollama-linux-amd64.tar.zst", false, false),
    ];
    assert_eq!(pick_build(ollama, &offered(&rows), &[]), Some(1));

    let changed = [
        ("v9.9.9", "ollama-linux-amd64.tar.zst", false, false),
        (tested, "ollama-linux-amd64.tar.zst", true, true),
    ];
    assert_eq!(
        pick_build(ollama, &offered(&changed), &[]),
        Some(0),
        "a file that changed after it was tested is not the tested one"
    );
}

/// O61: with no tested release the newest release that has a usable build is taken, and among its builds
/// the card decides: a build for a card this computer lacks is never taken, the document's order of
/// backends ranks the rest, and a computer whose cards could not be told gets the build that names no
/// backend before the ones that name one.
#[test]
fn p3_o61_the_newest_release_and_a_backend_the_card_can_run() {
    let r = common::shipped_registry(&[]);
    let llama = r.engine("llamacpp").expect("llama.cpp");
    assert!(quickai_core::acquire::tested_tag(llama).is_none());
    let rows = [
        ("b12", "llama-b12-bin-ubuntu-x64.tar.gz", false, false), // names no backend
        (
            "b12",
            "llama-b12-bin-ubuntu-rocm-7.0-x64.tar.gz",
            false,
            false,
        ),
        (
            "b12",
            "llama-b12-bin-ubuntu-vulkan-x64.tar.gz",
            false,
            false,
        ),
        (
            "b12",
            "llama-b12-bin-ubuntu-cuda-12.4-x64.tar.gz",
            false,
            false,
        ),
        (
            "b11",
            "llama-b11-bin-ubuntu-vulkan-x64.tar.gz",
            false,
            false,
        ),
    ];
    let rows = offered(&rows);
    let name = |vendors: &[&str]| pick_build(llama, &rows, vendors).map(|i| rows[i].asset_name);
    assert_eq!(
        name(&["nvidia"]),
        Some("llama-b12-bin-ubuntu-cuda-12.4-x64.tar.gz"),
        "the document lists CUDA first, and a card of NVIDIA's can run it"
    );
    assert_eq!(
        name(&["amd"]),
        Some("llama-b12-bin-ubuntu-vulkan-x64.tar.gz"),
        "Vulkan is listed before ROCm; a CUDA build is not for this card"
    );
    assert_eq!(
        name(&["intel"]),
        Some("llama-b12-bin-ubuntu-vulkan-x64.tar.gz")
    );
    assert_eq!(
        name(&[]),
        Some("llama-b12-bin-ubuntu-x64.tar.gz"),
        "no card found: the build that needs none, and not one for a vendor that may be absent"
    );
    // when the newest release has nothing a card can run, the next one is looked at
    let only_cuda = offered(&[
        (
            "b13",
            "llama-b13-bin-ubuntu-cuda-12.4-x64.tar.gz",
            false,
            false,
        ),
        (
            "b12",
            "llama-b12-bin-ubuntu-vulkan-x64.tar.gz",
            false,
            false,
        ),
    ]);
    assert_eq!(pick_build(llama, &only_cuda, &["amd"]), Some(1));
    // a build whose pin and digest disagree is skipped
    let changed = offered(&[
        ("b12", "llama-b12-bin-ubuntu-vulkan-x64.tar.gz", false, true),
        (
            "b11",
            "llama-b11-bin-ubuntu-vulkan-x64.tar.gz",
            false,
            false,
        ),
    ]);
    assert_eq!(pick_build(llama, &changed, &["amd"]), Some(1));
}

/// O62: nothing offered, or nothing this computer can run, is no pick: the program says so instead of
/// installing something that will not work.
#[test]
fn p3_o62_nothing_that_can_run_is_nothing_to_install() {
    let r = common::shipped_registry(&[]);
    let llama = r.engine("llamacpp").expect("llama.cpp");
    assert_eq!(pick_build(llama, &[], &["nvidia"]), None);
    let cuda_only = offered(&[("b1", "llama-b1-bin-win-cuda-cu12.4-x64.zip", false, false)]);
    assert_eq!(pick_build(llama, &cuda_only, &["amd"]), None);
    assert_eq!(pick_build(llama, &cuda_only, &[]), None);
}

/// O63: of builds that rank the same the smaller is taken. Found by the first real run: the list of a real
/// llama.cpp release has an OpenVINO build that the document does not name as a backend, so it "names no
/// backend" as the plain build does, and it comes first in the list; it is 105 MB beside the plain build's
/// 16, and is for Intel's cards.
#[test]
fn p3_o63_of_builds_that_rank_the_same_the_smaller_download_is_taken() {
    let r = common::shipped_registry(&[]);
    let llama = r.engine("llamacpp").expect("llama.cpp");
    // the Linux x64 builds of release b11564, in the order the host lists them, with their sizes
    let release = [
        ("llama-b11564-bin-ubuntu-cuda-12.8-x64.tar.gz", 163),
        ("llama-b11564-bin-ubuntu-cuda-13.4-x64.tar.gz", 145),
        ("llama-b11564-bin-ubuntu-openvino-2026.4.1-x64.tar.gz", 105),
        ("llama-b11564-bin-ubuntu-rocm-10.0-x64.tar.gz", 233),
        ("llama-b11564-bin-ubuntu-sycl-fp16-x64.tar.gz", 53),
        ("llama-b11564-bin-ubuntu-sycl-fp32-x64.tar.gz", 53),
        ("llama-b11564-bin-ubuntu-vulkan-x64.tar.gz", 28),
        ("llama-b11564-bin-ubuntu-x64.tar.gz", 16),
    ];
    let rows: Vec<Offered<'_>> = release
        .iter()
        .map(|(name, mb)| Offered {
            tag: "b11564",
            asset_name: name,
            tested: false,
            conflict: false,
            size: Some(mb << 20),
        })
        .collect();
    let name = |vendors: &[&str]| pick_build(llama, &rows, vendors).map(|i| rows[i].asset_name);
    assert_eq!(
        name(&[]),
        Some("llama-b11564-bin-ubuntu-x64.tar.gz"),
        "no card found: the plain build, and not the OpenVINO one that lists first"
    );
    assert_eq!(
        name(&["amd"]),
        Some("llama-b11564-bin-ubuntu-vulkan-x64.tar.gz")
    );
    assert_eq!(
        name(&["nvidia"]),
        Some("llama-b11564-bin-ubuntu-cuda-13.4-x64.tar.gz"),
        "the two CUDA builds rank the same and the document does not say which: the lighter is taken (and a driver older than CUDA 13 wants `--build`, which docs/headless.md says)"
    );
}
