//! `feed.github_release@1`: the builds an engine's GitHub releases offer (docs/architecture.md,
//! section 8). The engine document names the repository, how many releases to ask for and show,
//! whether prereleases count and which of a release's files is a build for this machine; this module
//! asks GitHub and reports what the document's rule lets through.
//!
//! Two quirks of release feeds that the document's settings are there for:
//! - llama.cpp tags every CI build as a GitHub *prerelease*: there is no separate stable release
//!   with binaries attached, so its document says `prereleases = true`.
//! - A file that merely looks compatible may hold no executable (llama.cpp's `cudart-*` packages
//!   are CUDA runtimes only), which the document's `exclude` words rule out.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use quickai_core::acquire;
use quickai_core::doc::engine::Feed;
use quickai_core::plan::HostFacts;
use quickai_core::registry::Engine;
use serde::Deserialize;

/// Where GitHub's API lives; a test points the lookup somewhere else.
pub const GITHUB_API: &str = "https://api.github.com";

const CALL_TIMEOUT: Duration = Duration::from_secs(15);

/// One build a release offers for this machine.
pub struct Build {
    pub tag: String,
    pub asset_name: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    /// The directory it is installed under (the document's label rule).
    pub label: String,
}

pub enum BuildLookupEvent {
    Found(Build),
    Status(String),
    Done,
}

/// Everything a lookup needs, owned, so the thread that runs it borrows nothing.
#[derive(Clone)]
pub struct LookupSpec {
    pub engine: Engine,
    pub facts: HostFacts,
    pub api_base: String,
}

impl LookupSpec {
    /// A lookup of the engine's own feed; `None` when its document has none.
    pub fn from_engine(engine: &Engine, facts: &HostFacts) -> Option<LookupSpec> {
        engine.doc.acquire.as_ref()?.feed.as_ref()?;
        Some(LookupSpec {
            engine: engine.clone(),
            facts: facts.clone(),
            api_base: GITHUB_API.to_string(),
        })
    }

    /// `github.com/ggml-org/llama.cpp`, as the status line names it.
    pub fn repo_label(&self) -> String {
        acquire::feed_repo_label(&self.engine).unwrap_or_default()
    }
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: Option<u64>,
}

fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    // GitHub's API rejects requests with no User-Agent header at all.
    let resp = ureq::get(url)
        .header("User-Agent", "quickai")
        .config()
        .timeout_global(Some(CALL_TIMEOUT))
        .build()
        .call()
        .map_err(|e| e.to_string())?;
    let mut body = resp.into_body();
    body.read_json::<T>().map_err(|e| e.to_string())
}

/// Starts the lookup in a background thread and returns at once with the receiving end of the
/// channel: a status line, then a `Found` for each build, then `Done`.
pub fn spawn_lookup(
    spec: LookupSpec,
    wake: impl Fn() + Send + Sync + 'static,
) -> Receiver<BuildLookupEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || run(spec, tx, wake));
    rx
}

fn run(spec: LookupSpec, tx: Sender<BuildLookupEvent>, wake: impl Fn() + Send + Sync + 'static) {
    let send = |e: BuildLookupEvent| {
        let _ = tx.send(e);
        wake();
    };
    let Some(Feed::GithubRelease {
        repo,
        per_page,
        show,
        prereleases,
    }) = spec
        .engine
        .doc
        .acquire
        .as_ref()
        .and_then(|a| a.feed.as_ref())
    else {
        send(BuildLookupEvent::Status(
            "This engine has no list of builds.".into(),
        ));
        send(BuildLookupEvent::Done);
        return;
    };

    send(BuildLookupEvent::Status(format!(
        "Checking {} releases...",
        spec.repo_label()
    )));
    let url = format!(
        "{}/repos/{repo}/releases?per_page={per_page}",
        spec.api_base
    );
    let releases: Vec<GhRelease> = match get_json(&url) {
        Ok(r) => r,
        Err(e) => {
            send(BuildLookupEvent::Status(format!("Lookup failed: {e}")));
            send(BuildLookupEvent::Done);
            return;
        }
    };

    let mut shown = 0;
    for release in releases {
        if shown >= *show {
            break;
        }
        if release.draft || (release.prerelease && !prereleases) {
            continue;
        }
        let names: Vec<&str> = release.assets.iter().map(|a| a.name.as_str()).collect();
        let picked = acquire::matching_assets(&spec.engine, &names, &spec.facts);
        if picked.is_empty() {
            continue;
        }
        for i in picked {
            let asset = &release.assets[i];
            send(BuildLookupEvent::Found(Build {
                tag: release.tag_name.clone(),
                asset_name: asset.name.clone(),
                url: asset.browser_download_url.clone(),
                size_bytes: asset.size,
                label: acquire::install_label(&spec.engine, &release.tag_name, &asset.name),
            }));
        }
        shown += 1;
    }
    send(BuildLookupEvent::Done);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    const ENGINE: &str = r#"schema = 1
kind = "engine"
id = "demo"
requires = { primitives = ["spawn@1"] }
[engine]
name = "Demo"
[launch]
strategies = ["spawn"]
[launch.spawn]
program = "x"
[acquire]
root = "${data}/demo-bin"
feed = { use = "github_release", repo = "owner/demo", per_page = 10, show = 5, prereleases = true }
assets = { by = "keywords", ext = [".zip", ".tar.gz"], exclude = ["cudart"], os = { windows = ["win"], linux = ["ubuntu", "linux"] }, arch = { x86_64 = ["x64"] }, label = "${tag}-${stem}" }
install = { use = "archive", binary = { name = "demo-server" } }
"#;

    fn engine(text: &str) -> Engine {
        let source = MemorySource::new(&[("embedded:engines/demo.toml", text)]);
        let registry = quickai_core::registry::Registry::build(
            load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
            &crate::prims::known(),
        );
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        registry.engine("demo").unwrap().clone()
    }

    fn facts() -> HostFacts {
        HostFacts {
            os: "linux".into(),
            arch: "x86_64".into(),
            ..Default::default()
        }
    }

    /// What the fake was asked: the path with its query, and the headers.
    type Request = (String, Vec<(String, String)>);

    /// A GitHub API of our own: the JSON of a release list, and a record of what was asked.
    struct FakeGithub {
        base: String,
        seen: Arc<Mutex<Vec<Request>>>,
        stop: Arc<std::sync::atomic::AtomicBool>,
    }

    impl FakeGithub {
        fn serve(status: u16, body: String) -> FakeGithub {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let port = server.server_addr().to_ip().unwrap().port();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (log, flag) = (seen.clone(), stop.clone());
            std::thread::spawn(move || {
                while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                        continue;
                    };
                    let headers = request
                        .headers()
                        .iter()
                        .map(|h| (h.field.to_string(), h.value.to_string()))
                        .collect();
                    log.lock()
                        .unwrap()
                        .push((request.url().to_string(), headers));
                    let _ = request.respond(
                        tiny_http::Response::from_string(body.clone())
                            .with_status_code(status)
                            .with_header(
                                tiny_http::Header::from_bytes("Content-Type", "application/json")
                                    .unwrap(),
                            ),
                    );
                }
            });
            FakeGithub {
                base: format!("http://127.0.0.1:{port}"),
                seen,
                stop,
            }
        }
    }

    impl Drop for FakeGithub {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn release(
        tag: &str,
        prerelease: bool,
        draft: bool,
        assets: &[(&str, u64)],
    ) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "prerelease": prerelease,
            "draft": draft,
            "assets": assets.iter().map(|(name, size)| serde_json::json!({
                "name": name,
                "size": size,
                "browser_download_url": format!("https://example.com/{tag}/{name}"),
            })).collect::<Vec<_>>(),
        })
    }

    /// Runs a lookup to its end: the status lines, the builds as `tag name`, and whether it finished.
    fn lookup(spec: LookupSpec) -> (Vec<String>, Vec<Build>, bool) {
        let rx = spawn_lookup(spec, || {});
        let (mut statuses, mut builds, mut done) = (Vec::new(), Vec::new(), false);
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(BuildLookupEvent::Status(s)) => statuses.push(s),
                Ok(BuildLookupEvent::Found(b)) => builds.push(b),
                Ok(BuildLookupEvent::Done) => {
                    done = true;
                    break;
                }
                Err(_) => {}
            }
        }
        (statuses, builds, done)
    }

    fn spec_for(fake: &FakeGithub, text: &str) -> LookupSpec {
        LookupSpec {
            engine: engine(text),
            facts: facts(),
            api_base: fake.base.clone(),
        }
    }

    fn names(builds: &[Build]) -> Vec<String> {
        builds
            .iter()
            .map(|b| format!("{} {}", b.tag, b.asset_name))
            .collect()
    }

    #[test]
    fn builds_for_this_machine_are_reported_in_release_order_with_what_a_row_needs() {
        let body = serde_json::json!([
            release(
                "b3",
                true,
                false,
                &[
                    ("demo-b3-bin-win-x64.zip", 100),
                    ("demo-b3-bin-ubuntu-vulkan-x64.tar.gz", 300),
                    ("cudart-demo-b3-bin-ubuntu-x64.tar.gz", 5),
                    ("demo-b3-bin-ubuntu-arm64.tar.gz", 400),
                ]
            ),
            release("b2", true, false, &[("demo-b2-bin-ubuntu-x64.tar.gz", 250)]),
        ])
        .to_string();
        let fake = FakeGithub::serve(200, body);
        let (statuses, builds, done) = lookup(spec_for(&fake, ENGINE));
        assert!(done);
        assert_eq!(statuses, ["Checking github.com/owner/demo releases..."]);
        assert_eq!(
            names(&builds),
            [
                "b3 demo-b3-bin-ubuntu-vulkan-x64.tar.gz",
                "b2 demo-b2-bin-ubuntu-x64.tar.gz"
            ],
            "this machine's builds only: not Windows, not arm64, not the CUDA runtime package"
        );
        let first = &builds[0];
        assert_eq!(
            first.url,
            "https://example.com/b3/demo-b3-bin-ubuntu-vulkan-x64.tar.gz"
        );
        assert_eq!(first.size_bytes, Some(300));
        assert_eq!(first.label, "b3-demo-b3-bin-ubuntu-vulkan-x64");
    }

    #[test]
    fn the_request_names_the_repository_and_the_count_and_says_who_is_asking() {
        let fake = FakeGithub::serve(200, "[]".to_string());
        let _ = lookup(spec_for(&fake, ENGINE));
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "/repos/owner/demo/releases?per_page=10");
        let agent = seen[0]
            .1
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            agent,
            Some("quickai"),
            "GitHub rejects a request with no User-Agent"
        );
    }

    #[test]
    fn prereleases_count_only_when_the_document_says_so() {
        let body = serde_json::json!([
            release("b2", true, false, &[("demo-b2-bin-ubuntu-x64.tar.gz", 1)]),
            release("v1", false, false, &[("demo-v1-bin-ubuntu-x64.tar.gz", 1)]),
        ])
        .to_string();
        let fake = FakeGithub::serve(200, body);
        let (_, with, _) = lookup(spec_for(&fake, ENGINE));
        assert_eq!(
            names(&with),
            [
                "b2 demo-b2-bin-ubuntu-x64.tar.gz",
                "v1 demo-v1-bin-ubuntu-x64.tar.gz"
            ]
        );
        let stable_only = ENGINE.replace("prereleases = true", "prereleases = false");
        let (_, without, _) = lookup(spec_for(&fake, &stable_only));
        assert_eq!(names(&without), ["v1 demo-v1-bin-ubuntu-x64.tar.gz"]);
    }

    #[test]
    fn a_draft_is_never_offered() {
        let body = serde_json::json!([release(
            "b2",
            false,
            true,
            &[("demo-b2-bin-ubuntu-x64.tar.gz", 1)]
        )])
        .to_string();
        let fake = FakeGithub::serve(200, body);
        let (_, builds, done) = lookup(spec_for(&fake, ENGINE));
        assert!(done && builds.is_empty());
    }

    #[test]
    fn only_so_many_releases_with_a_build_are_shown_and_one_without_does_not_count() {
        let body = serde_json::json!([
            release("b5", true, false, &[("demo-b5-bin-ubuntu-x64.tar.gz", 1)]),
            release("b4", true, false, &[("demo-b4-bin-win-x64.zip", 1)]),
            release("b3", true, false, &[("demo-b3-bin-ubuntu-x64.tar.gz", 1)]),
            release("b2", true, false, &[("demo-b2-bin-ubuntu-x64.tar.gz", 1)]),
        ])
        .to_string();
        let fake = FakeGithub::serve(200, body);
        let two = ENGINE.replace("show = 5", "show = 2");
        let (_, builds, _) = lookup(spec_for(&fake, &two));
        assert_eq!(
            names(&builds),
            [
                "b5 demo-b5-bin-ubuntu-x64.tar.gz",
                "b3 demo-b3-bin-ubuntu-x64.tar.gz"
            ],
            "b4 has nothing for Linux, so it is skipped without using up one of the two"
        );
    }

    #[test]
    fn a_failure_is_a_status_line_and_the_end() {
        let fake = FakeGithub::serve(500, "{\"message\":\"boom\"}".into());
        let (statuses, builds, done) = lookup(spec_for(&fake, ENGINE));
        assert!(done && builds.is_empty());
        assert!(
            statuses
                .last()
                .is_some_and(|s| s.starts_with("Lookup failed: ")),
            "{statuses:?}"
        );

        let fake = FakeGithub::serve(200, "this is not json".into());
        let (statuses, builds, done) = lookup(spec_for(&fake, ENGINE));
        assert!(done && builds.is_empty());
        assert!(
            statuses
                .last()
                .is_some_and(|s| s.starts_with("Lookup failed: ")),
            "{statuses:?}"
        );

        // nothing listening at all
        let (statuses, _, done) = lookup(LookupSpec {
            engine: engine(ENGINE),
            facts: facts(),
            api_base: "http://127.0.0.1:1".into(),
        });
        assert!(done);
        assert!(
            statuses
                .last()
                .is_some_and(|s| s.starts_with("Lookup failed: ")),
            "{statuses:?}"
        );
    }

    #[test]
    fn an_engine_without_a_feed_has_no_lookup() {
        let no_feed = ENGINE
            .lines()
            .filter(|l| !l.starts_with("feed ="))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(LookupSpec::from_engine(&engine(&no_feed), &facts()).is_none());
        assert!(LookupSpec::from_engine(&engine(ENGINE), &facts()).is_some());
    }

    /// The real GitHub API, with the real llama.cpp release feed: the one check that the document's
    /// rule still matches how the project names its files. It needs the network, as the project's
    /// other real-resource tests do.
    #[test]
    fn the_embedded_llamacpp_document_finds_real_builds() {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let engine = registry.engine("llamacpp").expect("the embedded engine");
        let machine = crate::launch::MachineFacts::default();
        let facts = crate::launch::host_facts(std::path::Path::new("llama-server"), machine, None);
        let Some(spec) = LookupSpec::from_engine(engine, &facts) else {
            panic!("the embedded llama.cpp document has no feed");
        };
        let (_, found, done) = lookup(spec);
        assert!(done);
        // Every recent release has a compatible archive for every desktop OS (Windows as .zip,
        // Linux and macOS as .tar.gz), so this should never come back empty on a supported platform.
        assert!(
            !found.is_empty(),
            "expected at least one compatible llama.cpp build"
        );
        for b in &found {
            let lower = b.asset_name.to_ascii_lowercase();
            assert!(lower.ends_with(".zip") || lower.ends_with(".tar.gz"));
            assert!(!lower.contains("cudart"));
            assert!(b.url.contains(&b.tag));
        }
    }
}
