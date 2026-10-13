//! Group R of the test plan: the release feed of an engine whose files are chosen by exact name
//! (work package 3.2), against the recorded release list of the real Ollama repository: four
//! releases of seventeen files each, the newest of them a release candidate that GitHub lists
//! *above* the stable release.

use std::time::{Duration, Instant};

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::plan::HostFacts;
use quickai_core::registry::{Engine, Registry};

use crate::prims::feed::{spawn_lookup, Build, BuildLookupEvent, LookupSpec};
use crate::testkit::fake_github::{recorded_releases, FakeGithub, Mode};

/// What the Ollama document says about its releases, in the schema this build has: exact names
/// by platform.
const DOC: &str = r#"schema = 1
kind = "engine"
id = "ollama_feed"
requires = { primitives = ["spawn@1"] }

[engine]
name = "Ollama"

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "x"

[acquire]
root = "${data}/ollama"
feed = { use = "github_release", repo = "ollama/ollama", per_page = 10, show = 5, prereleases = false }
assets = { by = "names", rules = [
  { when = "os == 'linux' and arch == 'x86_64'", name = "ollama-linux-amd64.tar.zst" },
  { when = "os == 'linux' and arch == 'aarch64'", name = "ollama-linux-arm64.tar.zst" },
  { when = "os == 'windows' and arch == 'x86_64'", name = "ollama-windows-amd64.zip" },
  { when = "os == 'windows' and arch == 'aarch64'", name = "ollama-windows-arm64.zip" },
  { when = "os == 'macos'", name = "ollama-darwin.tgz" },
] }
install = { use = "archive", binary = { name = "ollama" } }
"#;

fn registry(doc: &str) -> Registry {
    let source = MemorySource::new(&[("embedded:engines/ollama_feed.toml", doc)]);
    Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &crate::prims::known(),
    )
}

fn engine(doc: &str) -> Engine {
    let registry = registry(doc);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    registry.engine("ollama_feed").unwrap().clone()
}

fn facts(os: &str, arch: &str) -> HostFacts {
    HostFacts {
        os: os.into(),
        arch: arch.into(),
        ..Default::default()
    }
}

/// Runs a lookup to its end: the status lines, the builds, and whether it finished.
fn lookup(spec: LookupSpec) -> (Vec<String>, Vec<Build>, bool) {
    let rx = spawn_lookup(spec, || {});
    let (mut statuses, mut builds, mut done) = (Vec::new(), Vec::new(), false);
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(BuildLookupEvent::Status(s)) => statuses.push(s),
            Ok(BuildLookupEvent::Found(b)) => builds.push(b),
            Ok(BuildLookupEvent::Failed) => {}
            Ok(BuildLookupEvent::Done) => {
                done = true;
                break;
            }
            Err(_) => {}
        }
    }
    (statuses, builds, done)
}

fn spec_for(fake: &FakeGithub, doc: &str, os: &str, arch: &str) -> LookupSpec {
    LookupSpec {
        engine: engine(doc),
        facts: facts(os, arch),
        api_base: fake.base(),
        token: None,
    }
}

fn asset_names(builds: &[Build]) -> Vec<String> {
    builds
        .iter()
        .map(|b| format!("{} {}", b.tag, b.asset_name))
        .collect()
}

#[test]
fn p3_r01_the_real_feed_offers_one_build_per_stable_release_for_each_supported_platform() {
    let fake = FakeGithub::ollama();
    let table = [
        ("linux", "x86_64", Some("ollama-linux-amd64.tar.zst")),
        ("linux", "aarch64", Some("ollama-linux-arm64.tar.zst")),
        ("windows", "x86_64", Some("ollama-windows-amd64.zip")),
        ("windows", "aarch64", Some("ollama-windows-arm64.zip")),
        ("macos", "x86_64", Some("ollama-darwin.tgz")),
        ("macos", "aarch64", Some("ollama-darwin.tgz")),
        // platforms the document has no words for get nothing, not a build for another one
        ("freebsd", "x86_64", None),
        ("linux", "riscv64", None),
    ];
    for (os, arch, want) in table {
        let (statuses, builds, done) = lookup(spec_for(&fake, DOC, os, arch));
        assert!(done, "{os}/{arch}: {statuses:?}");
        let expected: Vec<String> = match want {
            // the candidate is skipped; the stable ones follow in the feed's order
            Some(name) => ["v0.35.1", "v0.35.0", "v0.34.4"]
                .iter()
                .map(|t| format!("{t} {name}"))
                .collect(),
            None => Vec::new(),
        };
        assert_eq!(asset_names(&builds), expected, "{os}/{arch}");
    }
}

#[test]
fn p3_r02_of_the_seventeen_files_of_a_release_only_the_five_base_builds_are_ever_offered() {
    let releases = recorded_releases();
    let all: Vec<String> = releases[1]["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(all.len(), 17);
    let engine = engine(DOC);
    let mut offered = std::collections::BTreeSet::new();
    for (os, arch) in [
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("windows", "x86_64"),
        ("windows", "aarch64"),
        ("macos", "x86_64"),
        ("macos", "aarch64"),
        ("linux", "riscv64"),
        ("freebsd", "x86_64"),
    ] {
        let names: Vec<&str> = all.iter().map(String::as_str).collect();
        for i in quickai_core::acquire::matching_assets(&engine, &names, &facts(os, arch)) {
            offered.insert(all[i].clone());
        }
    }
    assert_eq!(
        offered.into_iter().collect::<Vec<_>>(),
        [
            "ollama-darwin.tgz",
            "ollama-linux-amd64.tar.zst",
            "ollama-linux-arm64.tar.zst",
            "ollama-windows-amd64.zip",
            "ollama-windows-arm64.zip",
        ],
        "the ROCm and MLX variants, the Jetson builds, the installers and the scripts are never a base build"
    );
}

#[test]
fn p3_r03_the_release_candidate_listed_first_is_skipped_unless_the_document_counts_prereleases() {
    let fake = FakeGithub::ollama();
    let (_, without, _) = lookup(spec_for(&fake, DOC, "linux", "x86_64"));
    assert_eq!(
        without[0].tag, "v0.35.1",
        "the candidate above the stable release was offered"
    );
    let with_candidates = DOC.replace("prereleases = false", "prereleases = true");
    let (_, with, _) = lookup(spec_for(&fake, &with_candidates, "linux", "x86_64"));
    assert_eq!(with[0].tag, "v0.40.0-rc5");
    assert_eq!(with[1].tag, "v0.35.1");
}

#[test]
fn p3_r07_a_lookup_asks_once_and_building_the_spec_asks_nothing() {
    let fake = FakeGithub::ollama();
    let spec = spec_for(&fake, DOC, "linux", "x86_64");
    assert!(
        fake.requests().is_empty(),
        "something was asked before the click"
    );
    let (_, builds, done) = lookup(spec);
    assert!(done && !builds.is_empty());
    let asked = fake.requests();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, "/repos/ollama/ollama/releases?per_page=10");
}

#[test]
fn p3_r05_a_rate_limit_is_reported_as_one_with_the_time_it_resets() {
    let fake = FakeGithub::ollama();
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 15 * 60;
    fake.mode(Mode::RateLimited { reset_epoch: reset });
    let (statuses, builds, done) = lookup(spec_for(&fake, DOC, "linux", "x86_64"));
    assert!(done && builds.is_empty());
    let last = statuses.last().unwrap().to_lowercase();
    assert!(last.contains("rate limit"), "the status says: {last}");
    assert!(
        last.contains("minute"),
        "the status does not say when it resets: {last}"
    );
    assert!(
        !last.contains("http status"),
        "the status is the library's own text: {last}"
    );

    // any other 403, and a server error, keep the general message
    fake.mode(Mode::Status(502));
    let (statuses, _, _) = lookup(spec_for(&fake, DOC, "linux", "x86_64"));
    assert!(statuses.last().unwrap().starts_with("Lookup failed: "));
}

#[test]
fn p3_r06_a_document_can_name_another_api_root_which_is_how_a_test_points_a_feed_at_a_fake() {
    let fake = FakeGithub::ollama();
    let doc = DOC.replace(
        "prereleases = false }",
        &format!("prereleases = false, api = \"{}\" }}", fake.base()),
    );
    let registry = registry(&doc);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let engine = registry.engine("ollama_feed").unwrap();
    let spec = LookupSpec::from_engine(engine, &facts("linux", "x86_64")).expect("a feed");
    assert_eq!(
        spec.api_base,
        fake.base(),
        "the document's api root was not used"
    );
    let (_, builds, done) = lookup(spec);
    assert!(done && builds.len() == 3);
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn p3_r06b_an_api_root_that_is_not_http_is_refused_when_the_document_loads() {
    for bad in [
        "file:///etc",
        "ftp://example.com",
        "javascript:alert(1)",
        "",
    ] {
        let doc = DOC.replace(
            "prereleases = false }",
            &format!("prereleases = false, api = \"{bad}\" }}"),
        );
        let registry = registry(&doc);
        assert!(
            registry.diagnostics.iter().any(|d| d.path.contains("api")),
            "{bad:?} was accepted as an api root: {:#?}",
            registry.diagnostics
        );
    }
}

// ---- digests, the tested release and overlays ---------------------------------------------------------

/// The release the document was tested with is v0.35.1; these are its pins, as the example writes them.
const PINS: &str = r#"
[acquire.pins]
"ollama-linux-amd64.tar.zst" = "sha256:9fcd79ac4575b2bd31b992eee18b1000c8ad126b451627c8f8cd091714cfbb10"
"ollama-linux-arm64.tar.zst" = "sha256:b1b1c85d25136b256d3740f6ecd2f7c0105e8ca71642fd6ff2fc199f3cefd69d"
"ollama-linux-amd64-rocm.tar.zst" = "sha256:786e1ba2ed7877b48aa948315ab704da2ea54a4b636e0ca18c3d328c84ff22cf"
"ollama-windows-amd64.zip" = "sha256:dc50b9ca7f9023c86525012632cd1615b093d0407987444a7f62ecab617e8e93"
"ollama-windows-arm64.zip" = "sha256:1aced40f0654eae8ef1ba07fa101be4b48225308288b0f54c5681ccf1e4199db"
"ollama-windows-amd64-rocm.zip" = "sha256:00e1d52a38479aa88954e01c969228fa98a07f433e639a6ba9fcd80b8d0993ca"
"ollama-darwin.tgz" = "sha256:3137dbf28948ee844e0fb3e584d9b5de6879d73d9f0cb7eff3ad64930601d307"
"#;

/// The document with what WP3.2 adds: the feed lists digests, one release is the tested one with its
/// pins, and the ROCm archive is an overlay on the two platforms that have one.
fn full_doc(tested: &str) -> String {
    let with_keys = DOC
        .replace(
            "prereleases = false }",
            "prereleases = false, digest = true }",
        )
        .replace(
            "install = {",
            &format!(
                "tested = {{ tag = \"{tested}\" }}
overlays = [
  {{ id = \"rocm\", label = \"AMD ROCm (adds about 1 GB)\", rules = [
    {{ when = \"os == 'linux' and arch == 'x86_64'\", name = \"ollama-linux-amd64-rocm.tar.zst\" }},
    {{ when = \"os == 'windows' and arch == 'x86_64'\", name = \"ollama-windows-amd64-rocm.zip\" }},
  ] }},
]
install = {{"
            ),
        );
    format!("{with_keys}{PINS}")
}

fn asset_of(release: &str, name: &str) -> serde_json::Value {
    recorded_releases()
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["tag_name"] == release)
        .unwrap()["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == name)
        .unwrap_or_else(|| panic!("{release} has no {name}"))
        .clone()
}

#[test]
fn p3_r04_every_offered_build_carries_the_sha256_and_size_the_api_gave_for_its_file() {
    let fake = FakeGithub::ollama();
    let (_, builds, _) = lookup(spec_for(&fake, &full_doc("v0.35.1"), "linux", "x86_64"));
    assert_eq!(builds.len(), 3);
    for build in &builds {
        let asset = asset_of(&build.tag, "ollama-linux-amd64.tar.zst");
        let digest = asset["digest"]
            .as_str()
            .unwrap()
            .trim_start_matches("sha256:");
        assert_eq!(build.sha256.as_deref(), Some(digest), "{}", build.tag);
        assert_eq!(build.size_bytes, asset["size"].as_u64(), "{}", build.tag);
        assert_eq!(build.url, asset["browser_download_url"].as_str().unwrap());
    }

    // a feed the document does not say lists digests carries none (llama.cpp's)
    let (_, plain, _) = lookup(spec_for(&fake, DOC, "linux", "x86_64"));
    assert!(plain.iter().all(|b| b.sha256.is_none() && b.pin.is_none()));

    // a file the host lists no digest for has none, and the others still do
    let mut releases = recorded_releases();
    for asset in releases[1]["assets"].as_array_mut().unwrap() {
        if asset["name"] == "ollama-linux-amd64.tar.zst" {
            asset.as_object_mut().unwrap().remove("digest");
        }
    }
    let fake = FakeGithub::with_releases(releases);
    let (_, builds, _) = lookup(spec_for(&fake, &full_doc("v0.35.1"), "linux", "x86_64"));
    assert_eq!(builds[0].tag, "v0.35.1");
    assert_eq!(builds[0].sha256, None, "a digest was made up");
    assert!(builds[1].sha256.is_some());
}

#[test]
fn p3_r10_the_tested_release_is_marked_and_first_with_the_documents_pins_and_a_pin_that_disagrees_is_a_conflict(
) {
    let fake = FakeGithub::ollama();
    let (_, builds, _) = lookup(spec_for(&fake, &full_doc("v0.35.1"), "linux", "x86_64"));
    let tags: Vec<&str> = builds.iter().map(|b| b.tag.as_str()).collect();
    assert_eq!(tags, ["v0.35.1", "v0.35.0", "v0.34.4"]);
    assert!(builds[0].tested && !builds[1].tested && !builds[2].tested);
    assert_eq!(
        builds[0].pin.as_deref(),
        Some("9fcd79ac4575b2bd31b992eee18b1000c8ad126b451627c8f8cd091714cfbb10")
    );
    assert!(
        builds[1].pin.is_none() && builds[2].pin.is_none(),
        "a pin is for the release it was recorded from, not for every release with a file of that name"
    );
    assert!(
        !builds[0].conflict,
        "the pins are the digests the host lists for v0.35.1"
    );
    // what the installer is asked: both digests, which agree
    let request = builds[0].install_request(&[]);
    assert_eq!(request.base.host_digest, request.base.pin);

    // the tested release goes first wherever the feed has it
    let (_, builds, _) = lookup(spec_for(&fake, &full_doc("v0.34.4"), "linux", "x86_64"));
    let tags: Vec<&str> = builds.iter().map(|b| b.tag.as_str()).collect();
    assert_eq!(
        tags,
        ["v0.34.4", "v0.35.1", "v0.35.0"],
        "the tested one is not first"
    );
    assert!(builds[0].tested);
    // and these pins are v0.35.1's, so they disagree with v0.34.4's files: the file changed
    assert!(
        builds[0].conflict,
        "a pin that disagrees with the feed was not flagged"
    );

    // a release the feed does not list at all offers nothing flagged and nothing is invented
    let (_, builds, _) = lookup(spec_for(&fake, &full_doc("v9.9.9"), "linux", "x86_64"));
    assert!(builds.iter().all(|b| !b.tested && b.pin.is_none()));

    // a pin that is changed in the document disagrees with the host's digest
    let wrong = full_doc("v0.35.1").replace("9fcd79ac4575", "000000000000");
    let (_, builds, _) = lookup(spec_for(&fake, &wrong, "linux", "x86_64"));
    assert!(builds[0].tested && builds[0].conflict);
}

#[test]
fn p3_r11_an_overlay_is_offered_only_where_there_is_one_never_as_a_build_and_is_off_until_chosen() {
    let fake = FakeGithub::ollama();
    let doc = full_doc("v0.35.1");
    let (_, builds, _) = lookup(spec_for(&fake, &doc, "linux", "x86_64"));
    for build in &builds {
        assert!(
            !build.asset_name.contains("rocm"),
            "an overlay was offered as a build: {}",
            build.asset_name
        );
        let rocm = build
            .overlays
            .iter()
            .find(|o| o.id == "rocm")
            .expect("a ROCm overlay");
        assert_eq!(rocm.asset_name, "ollama-linux-amd64-rocm.tar.zst");
        assert_eq!(rocm.label, "AMD ROCm (adds about 1 GB)");
        let asset = asset_of(&build.tag, "ollama-linux-amd64-rocm.tar.zst");
        assert_eq!(rocm.url, asset["browser_download_url"].as_str().unwrap());
        assert_eq!(rocm.size_bytes, asset["size"].as_u64());
    }
    // the digests follow the same rule as the build's: the pin only on the tested release
    let rocm = |b: &Build| b.overlays[0].clone();
    assert!(rocm(&builds[0]).pin.is_some() && rocm(&builds[1]).pin.is_none());
    assert_eq!(
        rocm(&builds[0]).pin.as_deref(),
        rocm(&builds[0]).sha256.as_deref(),
        "the pinned digest of the overlay is the one the host lists"
    );

    // off until chosen: nothing is fetched for it unless it is asked for
    let off = builds[0].install_request(&[]);
    assert!(off.overlays.is_empty());
    let on = builds[0].install_request(&["rocm"]);
    assert_eq!(on.overlays.len(), 1);
    assert!(on.overlays[0]
        .url
        .ends_with("ollama-linux-amd64-rocm.tar.zst"));
    assert!(builds[0].install_request(&["nonsense"]).overlays.is_empty());

    // Windows on x86-64 has one; Linux on ARM, a Mac and a machine with no rule have none
    let (_, windows, _) = lookup(spec_for(&fake, &doc, "windows", "x86_64"));
    assert_eq!(
        windows[0].overlays[0].asset_name,
        "ollama-windows-amd64-rocm.zip"
    );
    for (os, arch) in [
        ("linux", "aarch64"),
        ("macos", "aarch64"),
        ("windows", "aarch64"),
    ] {
        let (_, builds, _) = lookup(spec_for(&fake, &doc, os, arch));
        assert!(!builds.is_empty());
        assert!(
            builds.iter().all(|b| b.overlays.is_empty()),
            "{os}/{arch} was offered an overlay"
        );
    }
    // a release that has no such file offers no overlay (the document says what exists, the release what is there)
    let mut releases = recorded_releases();
    releases[1]["assets"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["name"] != "ollama-linux-amd64-rocm.tar.zst");
    let fake = FakeGithub::with_releases(releases);
    let (_, builds, _) = lookup(spec_for(&fake, &doc, "linux", "x86_64"));
    assert!(builds[0].overlays.is_empty(), "{:?}", builds[0].overlays);
    assert!(!builds[1].overlays.is_empty());
}

#[test]
fn the_documents_new_keys_are_checked_where_they_are_written() {
    let bad = |doc: String| registry(&doc).diagnostics;
    // a pin that is not a digest
    let diagnostics = bad(full_doc("v0.35.1").replace(
        "sha256:3137dbf28948ee844e0fb3e584d9b5de6879d73d9f0cb7eff3ad64930601d307",
        "sha256:12345",
    ));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.path.contains("pins") && d.message.contains("not a sha256 digest")),
        "{diagnostics:#?}"
    );
    // an overlay whose condition does not hold together, and one with no rules
    let diagnostics = bad(full_doc("v0.35.1").replace(
        "os == 'windows' and arch == 'x86_64'\", name = \"ollama-windows-amd64-rocm.zip\"",
        "os == \", name = \"x\"",
    ));
    assert!(
        diagnostics.iter().any(|d| d.path.contains("overlays")),
        "{diagnostics:#?}"
    );
    // an overlay with no rule is offered nowhere: said at load
    let diagnostics = bad(full_doc("v0.35.1").replace(
        "label = \"AMD ROCm (adds about 1 GB)\", rules = [",
        "label = \"x\", rules = [] }, { id = \"other\", rules = [",
    ));
    assert!(
        diagnostics.iter().any(|d| d.message.contains("no rules")),
        "{diagnostics:#?}"
    );
    let diagnostics = bad(full_doc("v0.35.1").replace("tag = \"v0.35.1\"", "tag = \"\""));
    assert!(
        diagnostics.iter().any(|d| d.path.contains("tested")),
        "{diagnostics:#?}"
    );
    // and the document as the example writes it is accepted
    assert!(bad(full_doc("v0.35.1")).is_empty());
}

/// The real GitHub, with the embedded Ollama document: the one check that its rule still matches
/// how the project names its files. It needs the network, and it needs GitHub's anonymous allowance
/// (60 requests an hour): when that is used up the test says so and passes, since the answer is
/// "unknown", not "broken".
#[test]
fn p3_r08_the_embedded_ollama_document_finds_real_builds() {
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let engine = registry
        .engine("ollama")
        .expect("the embedded Ollama document");
    let machine = crate::launch::MachineFacts::default();
    let facts = crate::launch::host_facts(std::path::Path::new("ollama"), machine, None);
    let spec = LookupSpec::from_engine(engine, &facts).expect("the embedded document has a feed");
    let (statuses, found, done) = lookup(spec);
    assert!(done);
    if found.is_empty() && statuses.last().is_some_and(|s| rate_limited(s)) {
        eprintln!("GitHub's anonymous allowance is used up: not a verdict ({statuses:?})");
        return;
    }
    assert!(
        !found.is_empty(),
        "no Ollama build for this platform: {statuses:?}"
    );
    for b in &found {
        assert!(
            !b.tag.contains("rc"),
            "a release candidate was offered: {}",
            b.tag
        );
        assert!(b.url.contains(&b.tag));
    }
}

fn rate_limited(status: &str) -> bool {
    let s = status.to_lowercase();
    s.contains("rate limit") || s.contains("403") || s.contains("429")
}

#[test]
fn p3_r09_a_rate_limited_answer_is_recognised_by_the_helper_the_real_network_tests_use() {
    assert!(rate_limited("Lookup failed: http status: 403"));
    assert!(rate_limited("Lookup failed: http status: 429"));
    assert!(rate_limited("GitHub's rate limit is used up"));
    assert!(!rate_limited("Lookup failed: http status: 500"));
    assert!(!rate_limited(
        "Checking github.com/ollama/ollama releases..."
    ));
}
