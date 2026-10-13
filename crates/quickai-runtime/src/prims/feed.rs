//! `feed.github_release@1` and `feed.pypi@1`: the builds an engine's releases offer (docs/architecture.md,
//! section 8). The engine document names the repository, how many releases to ask for and show,
//! whether prereleases count and which of a release's files is a build for this machine; this module
//! asks GitHub and reports what the document's rule lets through.
//!
//! Quirks of release feeds that the document's settings are there for:
//! - llama.cpp tags every CI build as a GitHub *prerelease*: there is no separate stable release
//!   with binaries attached, so its document says `prereleases = true`.
//! - A file that merely looks compatible may hold no executable (llama.cpp's `cudart-*` packages
//!   are CUDA runtimes only), which the document's `exclude` words rule out.
//! - A feed does not list the newest *stable* release first: Ollama's lists a release candidate
//!   above it (decision C29), so what is offered is chosen by the document and never by position.
//! - The release the document was tested with (`tested`) is offered first and marked, with the
//!   digests the document pins for it; a pin that disagrees with the digest the host lists means the
//!   file changed after it was tested (decision C34).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, Sender};
use quickai_core::acquire;
use quickai_core::doc::engine::Feed;
use quickai_core::plan::HostFacts;
use quickai_core::registry::Engine;
use serde::Deserialize;

use super::install::{InstallRequest, Source};

/// Where GitHub's API lives; a document may name another root (a test points the lookup at a fake).
pub const GITHUB_API: &str = "https://api.github.com";

/// Where the package index's JSON API lives; a document may name another root, as for GitHub's.
pub const PYPI_API: &str = "https://pypi.org";

const CALL_TIMEOUT: Duration = Duration::from_secs(15);

/// One build a release offers for this machine.
#[derive(Clone, Debug)]
pub struct Build {
    pub tag: String,
    pub asset_name: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    /// The directory it is installed under (the document's label rule).
    pub label: String,
    /// The sha256 (hex) the feed lists for the file; only for a feed the document says lists them.
    pub sha256: Option<String>,
    /// The sha256 (hex) the document pins for the file, when this is the release it was tested with.
    pub pin: Option<String>,
    /// This is the release the document was tested with.
    pub tested: bool,
    /// The pin and the feed's digest differ: the file changed after it was tested.
    pub conflict: bool,
    /// What can be added to it on this machine (other files of the same release), off until chosen.
    pub overlays: Vec<OverlayOffer>,
}

/// One more archive a build can have unpacked over it.
#[derive(Clone, Debug)]
pub struct OverlayOffer {
    pub id: String,
    pub label: String,
    pub asset_name: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub pin: Option<String>,
}

impl Build {
    /// What installing this build asks of the installer, with the overlays that were chosen (by id).
    pub fn install_request(&self, chosen_overlays: &[&str]) -> InstallRequest {
        let overlay = |o: &OverlayOffer| Source {
            url: o.url.clone(),
            host_digest: o.sha256.clone(),
            pin: o.pin.clone(),
            size: o.size_bytes,
        };
        InstallRequest {
            label: self.label.clone(),
            base: Source {
                url: self.url.clone(),
                host_digest: self.sha256.clone(),
                pin: self.pin.clone(),
                size: self.size_bytes,
            },
            overlays: self
                .overlays
                .iter()
                .filter(|o| chosen_overlays.contains(&o.id.as_str()))
                .map(overlay)
                .collect(),
        }
    }
}

pub enum BuildLookupEvent {
    Found(Build),
    Status(String),
    /// The look-up could not be made: the status line before it says why (the rate limit's sentence,
    /// `Lookup failed: ...`), and an interface must keep it rather than say that nothing was found.
    /// `Done` follows.
    Failed,
    Done,
}

/// Everything a lookup needs, owned, so the thread that runs it borrows nothing.
#[derive(Clone)]
pub struct LookupSpec {
    pub engine: Engine,
    pub facts: HostFacts,
    pub api_base: String,
    /// A login for the host of the list, sent with every request to it: raises the allowance of
    /// requests. Only a program with no window has one to give.
    pub token: Option<String>,
}

impl LookupSpec {
    /// A lookup of the engine's own feed; `None` when its document has none. The API is GitHub's
    /// unless the document names another root.
    pub fn from_engine(engine: &Engine, facts: &HostFacts) -> Option<LookupSpec> {
        let feed = engine.doc.acquire.as_ref()?.feed.as_ref()?;
        let api_base = match feed {
            Feed::GithubRelease { api, .. } => {
                api.clone().unwrap_or_else(|| GITHUB_API.to_string())
            }
            Feed::Pypi { api, .. } => api.clone().unwrap_or_else(|| PYPI_API.to_string()),
        };
        Some(LookupSpec {
            engine: engine.clone(),
            facts: facts.clone(),
            api_base,
            token: None,
        })
    }

    /// The same look-up, signed in.
    pub fn with_token(mut self, token: Option<String>) -> LookupSpec {
        self.token = token.filter(|t| !t.trim().is_empty());
        self
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
    /// `sha256:<hex>`, on releases made since GitHub began to list one.
    #[serde(default)]
    digest: Option<String>,
}

/// Why a lookup did not get a release list.
enum LookupError {
    /// GitHub's allowance for this address is used up; `reset` is when it comes back.
    RateLimited {
        reset: Option<u64>,
    },
    Other(String),
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn get_releases(url: &str, token: Option<&str>) -> Result<Vec<GhRelease>, LookupError> {
    get_json(url, token)
}

fn get_json<T: serde::de::DeserializeOwned>(
    url: &str,
    token: Option<&str>,
) -> Result<T, LookupError> {
    // a status is an answer here: the headers of a 403 say whether it is the rate limit
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(CALL_TIMEOUT))
        .build()
        .into();
    // GitHub's API rejects requests with no User-Agent header at all.
    let mut request = agent.get(url).header("User-Agent", "quickai");
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let resp = request
        .call()
        .map_err(|e| LookupError::Other(e.to_string()))?;
    let status = resp.status().as_u16();
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    if status == 403 || status == 429 {
        let used_up = header("x-ratelimit-remaining").as_deref() == Some("0");
        let retry_after = header("retry-after").and_then(|v| v.trim().parse::<u64>().ok());
        if used_up || status == 429 || retry_after.is_some() {
            let reset = header("x-ratelimit-reset")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .or_else(|| retry_after.map(|seconds| now_epoch() + seconds));
            return Err(LookupError::RateLimited { reset });
        }
    }
    if !(200..300).contains(&status) {
        return Err(LookupError::Other(format!("http status: {status}")));
    }
    resp.into_body()
        .read_json::<T>()
        .map_err(|e| LookupError::Other(e.to_string()))
}

/// The sentence for a used-up allowance, with when it comes back.
fn rate_limit_message(reset: Option<u64>, now: u64) -> String {
    let start =
        "GitHub's rate limit is used up for this network (60 requests an hour without signing in)";
    match reset {
        Some(reset) if reset > now => {
            let minutes = (reset - now).div_ceil(60);
            format!(
                "{start}; it resets in about {minutes} minute{}.",
                if minutes == 1 { "" } else { "s" }
            )
        }
        Some(_) => format!("{start}; it should be over by now, so try again."),
        None => format!("{start}; try again in a while."),
    }
}

/// `sha256:<hex>` as the bare lower-case hex.
fn bare_digest(digest: &str) -> String {
    digest
        .trim()
        .trim_start_matches("sha256:")
        .to_ascii_lowercase()
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

/// The build that is release `tag`'s source (`assets` of `by = "source"`): the archive GitHub makes of
/// the tag, which no file of the release lists a digest for, so only the pin of the tested release
/// can check it.
fn source_build(spec: &LookupSpec, tag: &str, tested_tag: Option<&str>) -> Option<Build> {
    let name = acquire::source_archive_name(tag);
    let url = acquire::source_archive_url(&spec.engine, tag)?;
    let tested = tested_tag == Some(tag);
    Some(Build {
        tag: tag.to_string(),
        label: acquire::install_label(&spec.engine, tag, &name),
        pin: tested
            .then(|| acquire::pin_for(&spec.engine, &name))
            .flatten(),
        asset_name: name,
        url,
        size_bytes: None,
        sha256: None,
        tested,
        conflict: false,
        overlays: Vec::new(),
    })
}

/// What going on with the install of the build `label` asks of the installer, worked out from the engine's
/// document alone (a build from source is labelled by its release): the source of that release, with the
/// pin of the release the document was tested with if it is that one. `None` for an engine whose builds are
/// not made from source, and for a label that no release of the document would give.
pub fn request_for_label(
    engine: &Engine,
    facts: &HostFacts,
    label: &str,
) -> Option<InstallRequest> {
    if !acquire::builds_from_source(engine) {
        return None;
    }
    // the address of a wheel is not made up from its version: what an install that stopped was made from
    // is kept in its own record, and the index is asked only for a release that is looked up
    if matches!(
        engine.doc.acquire.as_ref().and_then(|a| a.assets.as_ref()),
        Some(quickai_core::doc::engine::Assets::Wheel { .. })
    ) {
        return None;
    }
    let spec = LookupSpec::from_engine(engine, facts)?;
    let build = source_build(&spec, label, acquire::tested_tag(engine))?;
    (build.label == label).then(|| build.install_request(&[]))
}

fn run(spec: LookupSpec, tx: Sender<BuildLookupEvent>, wake: impl Fn() + Send + Sync + 'static) {
    let send = |e: BuildLookupEvent| {
        let _ = tx.send(e);
        wake();
    };
    if matches!(
        spec.engine
            .doc
            .acquire
            .as_ref()
            .and_then(|a| a.feed.as_ref()),
        Some(Feed::Pypi { .. })
    ) {
        run_pypi(&spec, &send);
        return;
    }
    let Some(Feed::GithubRelease {
        repo,
        per_page,
        show,
        prereleases,
        digest,
        ..
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
        send(BuildLookupEvent::Failed);
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
    let releases = match get_releases(&url, spec.token.as_deref()) {
        Ok(r) => r,
        Err(LookupError::RateLimited { reset }) => {
            send(BuildLookupEvent::Status(rate_limit_message(
                reset,
                now_epoch(),
            )));
            send(BuildLookupEvent::Failed);
            send(BuildLookupEvent::Done);
            return;
        }
        Err(LookupError::Other(e)) => {
            send(BuildLookupEvent::Status(format!("Lookup failed: {e}")));
            send(BuildLookupEvent::Failed);
            send(BuildLookupEvent::Done);
            return;
        }
    };

    let tested_tag = acquire::tested_tag(&spec.engine);
    let overlay_choices = acquire::overlay_choices(&spec.engine, &spec.facts);
    let from_source = acquire::builds_from_source(&spec.engine);
    // one group of builds per release, in the feed's order; the tested release goes first
    let mut groups: Vec<(bool, Vec<Build>)> = Vec::new();
    for release in releases {
        if release.draft || (release.prerelease && !prereleases) {
            continue;
        }
        if from_source {
            // the build is the release's source: every release has one, and it is the same on every
            // platform
            if let Some(build) = source_build(&spec, &release.tag_name, tested_tag) {
                groups.push((build.tested, vec![build]));
            }
            continue;
        }
        let names: Vec<&str> = release.assets.iter().map(|a| a.name.as_str()).collect();
        let picked = acquire::matching_assets(&spec.engine, &names, &spec.facts);
        if picked.is_empty() {
            continue;
        }
        let tested = tested_tag == Some(release.tag_name.as_str());
        let host_digest = |asset: &GhAsset| {
            digest
                .then(|| asset.digest.as_deref().map(bare_digest))
                .flatten()
        };
        let pin_of = |name: &str| {
            tested
                .then(|| acquire::pin_for(&spec.engine, name))
                .flatten()
        };
        let overlays: Vec<OverlayOffer> = overlay_choices
            .iter()
            .filter_map(|choice| {
                let asset = release
                    .assets
                    .iter()
                    .find(|a| a.name == choice.asset_name)?;
                Some(OverlayOffer {
                    id: choice.id.clone(),
                    label: choice.label.clone(),
                    asset_name: asset.name.clone(),
                    url: asset.browser_download_url.clone(),
                    size_bytes: asset.size,
                    sha256: host_digest(asset),
                    pin: pin_of(&asset.name),
                })
            })
            .collect();
        let builds: Vec<Build> = picked
            .into_iter()
            .map(|i| {
                let asset = &release.assets[i];
                let (sha256, pin) = (host_digest(asset), pin_of(&asset.name));
                Build {
                    tag: release.tag_name.clone(),
                    asset_name: asset.name.clone(),
                    url: asset.browser_download_url.clone(),
                    size_bytes: asset.size,
                    label: acquire::install_label(&spec.engine, &release.tag_name, &asset.name),
                    conflict: matches!((&sha256, &pin), (Some(a), Some(b)) if a != b),
                    sha256,
                    pin,
                    tested,
                    overlays: overlays.clone(),
                }
            })
            .collect();
        groups.push((tested, builds));
    }
    // The release the document was tested with is offered even when the feed's first page has moved on
    // from it: its address and its pin are the document's, and nothing is asked of the feed for them.
    if from_source && !groups.iter().any(|(tested, _)| *tested) {
        if let Some(build) = tested_tag.and_then(|tag| source_build(&spec, tag, Some(tag))) {
            groups.push((true, vec![build]));
        }
    }
    // the tested release first, whatever its place in the feed, and the rest as the feed has them
    let (tested_groups, others): (Vec<_>, Vec<_>) = groups.into_iter().partition(|(t, _)| *t);
    for (_, builds) in tested_groups.into_iter().chain(others).take(*show) {
        for build in builds {
            send(BuildLookupEvent::Found(build));
        }
    }
    send(BuildLookupEvent::Done);
}

// ---- the package index ----------------------------------------------------------------------------------

/// A file of a release, as the index lists it.
#[derive(Clone, Debug, Deserialize)]
struct PypiFile {
    filename: String,
    url: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    digests: PypiDigests,
    #[serde(default)]
    packagetype: String,
    #[serde(default)]
    yanked: bool,
    #[serde(default)]
    upload_time_iso_8601: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PypiDigests {
    #[serde(default)]
    sha256: Option<String>,
}

/// `GET /pypi/<package>/json`: every release, with its files.
#[derive(Deserialize)]
struct PypiPackage {
    #[serde(default)]
    releases: std::collections::BTreeMap<String, Vec<PypiFile>>,
}

/// `GET /pypi/<package>/<version>/json`: one release, with its files.
#[derive(Deserialize)]
struct PypiRelease {
    #[serde(default)]
    urls: Vec<PypiFile>,
}

impl PypiFile {
    /// The wheel that is for every platform (`py3-none-any`), which is the one an application installed from
    /// the index is made of; a file that was withdrawn is not offered.
    fn is_pure_wheel(&self) -> bool {
        !self.yanked
            && self.packagetype == "bdist_wheel"
            && self.filename.ends_with("-none-any.whl")
    }
}

/// Whether a version is a pre-release in the index's sense (`1.0a1`, `1.0b2`, `1.0rc1`, `1.0.dev3`).
fn is_prerelease(version: &str) -> bool {
    let v = version.to_ascii_lowercase();
    if v.contains("dev") {
        return true;
    }
    let b = v.as_bytes();
    (1..b.len().saturating_sub(1)).any(|i| {
        b[i - 1].is_ascii_digit()
            && ((matches!(b[i], b'a' | b'b') && b[i + 1].is_ascii_digit())
                || (b[i] == b'r'
                    && b[i + 1] == b'c'
                    && b.get(i + 2).is_some_and(u8::is_ascii_digit)))
    })
}

/// The build that is `file` of release `version`: its address and its sha256 are the index's, and the pin is
/// the document's for the release it was tested with.
fn pypi_build(
    spec: &LookupSpec,
    version: &str,
    file: &PypiFile,
    tested_tag: Option<&str>,
) -> Build {
    let tested = tested_tag == Some(version);
    let sha256 = file.digests.sha256.as_deref().map(bare_digest);
    let pin = tested
        .then(|| acquire::pin_for(&spec.engine, &file.filename))
        .flatten();
    Build {
        tag: version.to_string(),
        asset_name: file.filename.clone(),
        url: file.url.clone(),
        size_bytes: file.size,
        label: acquire::install_label(&spec.engine, version, &file.filename),
        conflict: matches!((&sha256, &pin), (Some(a), Some(b)) if a != b),
        sha256,
        pin,
        tested,
        overlays: Vec::new(),
    }
}

/// The builds of a package on the index: the newest releases that have a wheel, the one the document was
/// tested with first (asked for by itself when the list has moved past it), each with the sha256 the index
/// lists for its file.
fn run_pypi(spec: &LookupSpec, send: &dyn Fn(BuildLookupEvent)) {
    let Some(Feed::Pypi {
        package,
        show,
        prereleases,
        ..
    }) = spec
        .engine
        .doc
        .acquire
        .as_ref()
        .and_then(|a| a.feed.as_ref())
    else {
        return;
    };
    let fail = |text: String| {
        send(BuildLookupEvent::Status(text));
        send(BuildLookupEvent::Failed);
        send(BuildLookupEvent::Done);
    };
    send(BuildLookupEvent::Status(format!(
        "Checking {} releases...",
        spec.repo_label()
    )));
    let listed: PypiPackage =
        match get_json(&format!("{}/pypi/{package}/json", spec.api_base), None) {
            Ok(listed) => listed,
            Err(LookupError::RateLimited { reset }) => {
                return fail(rate_limit_message(reset, now_epoch()));
            }
            Err(LookupError::Other(e)) => return fail(format!("Lookup failed: {e}")),
        };
    let tested_tag = acquire::tested_tag(&spec.engine);
    // the newest first, by when the wheel was uploaded: a version number is not a reliably sortable thing
    let mut found: Vec<(String, PypiFile)> = listed
        .releases
        .into_iter()
        .filter(|(version, _)| *prereleases || !is_prerelease(version))
        .filter_map(|(version, files)| {
            let wheel = files.into_iter().find(PypiFile::is_pure_wheel)?;
            Some((version, wheel))
        })
        .collect();
    found.sort_by(|a, b| b.1.upload_time_iso_8601.cmp(&a.1.upload_time_iso_8601));
    let mut builds: Vec<Build> = found
        .iter()
        .map(|(version, file)| pypi_build(spec, version, file, tested_tag))
        .collect();
    // the tested release is offered even when the newest ones have moved past it
    if let Some(tag) = tested_tag {
        let shown = builds.iter().take(*show).any(|b| b.tag == tag);
        if !shown {
            builds.retain(|b| b.tag != tag);
            let one: Result<PypiRelease, LookupError> = get_json(
                &format!("{}/pypi/{package}/{tag}/json", spec.api_base),
                None,
            );
            if let Ok(one) = one {
                if let Some(file) = one.urls.iter().find(|f| f.is_pure_wheel()) {
                    builds.insert(0, pypi_build(spec, tag, file, Some(tag)));
                }
            }
        }
    }
    // the tested release first, and the rest newest first
    let (tested, others): (Vec<_>, Vec<_>) = builds.into_iter().partition(|b| b.tested);
    for build in tested.into_iter().chain(others).take(*show) {
        send(BuildLookupEvent::Found(build));
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

    fn spec_for(fake: &FakeGithub, text: &str) -> LookupSpec {
        LookupSpec {
            engine: engine(text),
            facts: facts(),
            api_base: fake.base.clone(),
            token: None,
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
            token: None,
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
        let (statuses, found, done) = lookup(spec);
        assert!(done);
        // GitHub lets an address ask 60 times an hour without signing in. When that is used up (a
        // test run, or another project's on the same machine) the answer is "unknown", not "the
        // document no longer matches the project's file names".
        if found.is_empty()
            && statuses.last().is_some_and(|s| {
                let s = s.to_lowercase();
                s.contains("rate limit") || s.contains("403") || s.contains("429")
            })
        {
            eprintln!("GitHub's anonymous allowance is used up: not a verdict ({statuses:?})");
            return;
        }
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

    #[test]
    fn a_version_is_a_pre_release_when_the_index_would_say_so() {
        for pre in [
            "1.0a1",
            "1.0b2",
            "1.0rc1",
            "0.12.0rc10",
            "1.0.dev3",
            "2.0.0a0",
            "1.0.0b1",
            "3.1.0RC2",
        ] {
            assert!(is_prerelease(pre), "{pre}");
        }
        for stable in [
            "0.11.4",
            "1.0",
            "1.0.post1",
            "2026.2",
            "10.0.1",
            "1.2.3.4",
            "0.0.0",
            "1.0.0+local.5",
        ] {
            assert!(!is_prerelease(stable), "{stable}");
        }
    }

    #[test]
    fn only_a_wheel_for_every_platform_that_was_not_withdrawn_is_a_build() {
        let file = |name: &str, kind: &str, yanked: bool| PypiFile {
            filename: name.into(),
            url: format!("https://x/{name}"),
            size: None,
            digests: PypiDigests::default(),
            packagetype: kind.into(),
            yanked,
            upload_time_iso_8601: None,
        };
        assert!(file("a-1-py3-none-any.whl", "bdist_wheel", false).is_pure_wheel());
        assert!(file("a-1-py2.py3-none-any.whl", "bdist_wheel", false).is_pure_wheel());
        assert!(!file("a-1-py3-none-any.whl", "bdist_wheel", true).is_pure_wheel());
        assert!(!file(
            "a-1-cp312-cp312-manylinux_2_28_x86_64.whl",
            "bdist_wheel",
            false
        )
        .is_pure_wheel());
        assert!(!file("a-1.tar.gz", "sdist", false).is_pure_wheel());
        assert!(!file("a-1-py3-none-any.whl", "sdist", false).is_pure_wheel());
    }
}
