//! Which build of a program to install, and the request that installs it (docs/headless.md, section 4).
//!
//! The window shows a list of builds and a person picks one. A program with no window has a label a
//! configuration named, or nothing: it asks the engine's feed what there is (the same look-up the window's
//! list is made from), and chooses by the rule of `quickai_core::acquire::pick_build`, which is the
//! document's words and the same for every engine. What is chosen is said in the plan before anything is
//! fetched.

use std::time::Duration;

use quickai_core::acquire::{self, Offered};
use quickai_core::doc::engine::Feed;
use quickai_core::plan::HostFacts;
use quickai_core::registry::Engine;

use crate::gpu::GpuVendor;
use crate::prims::feed::{self, Build, BuildLookupEvent, LookupSpec};
use crate::prims::install::InstallRequest;

/// A build that was chosen.
#[derive(Debug)]
pub struct Chosen {
    /// The directory it is installed under.
    pub label: String,
    pub request: InstallRequest,
    /// What it downloads, when the feed says.
    pub bytes: Option<u64>,
    /// How the download is checked, in words: against the document's pin, against the feed's digest, or not.
    pub check: &'static str,
}

fn how_checked(pin: &Option<String>, digest: &Option<String>) -> &'static str {
    match (pin, digest) {
        (Some(_), _) => "checked against the sha256 the document pins",
        (None, Some(_)) => "checked against the digest the feed lists",
        (None, None) => "not verified: nothing lists a checksum for it",
    }
}

/// The builds the feed lists, newest first with the tested release at the front. `wide` asks for as many
/// releases as the host gives, for a build a configuration names that is no longer among the newest.
fn lookup(
    engine: &Engine,
    facts: &HostFacts,
    wide: bool,
    token: Option<&str>,
) -> Result<Vec<Build>, String> {
    let mut engine = engine.clone();
    if wide {
        if let Some(Feed::GithubRelease { per_page, show, .. }) =
            engine.doc.acquire.as_mut().and_then(|a| a.feed.as_mut())
        {
            *per_page = 100;
            *show = 100;
        }
    }
    let spec = LookupSpec::from_engine(&engine, facts)
        .ok_or_else(|| "its document has no list of builds to choose from".to_string())?
        .with_token(token.map(str::to_string));
    let repo = spec.repo_label();
    let events = feed::spawn_lookup(spec, || {});
    let (mut builds, mut said, mut failed) = (Vec::new(), String::new(), false);
    loop {
        match events.recv_timeout(Duration::from_secs(120)) {
            Ok(BuildLookupEvent::Found(build)) => builds.push(build),
            Ok(BuildLookupEvent::Status(text)) => said = text,
            Ok(BuildLookupEvent::Failed) => failed = true,
            Ok(BuildLookupEvent::Done) => break,
            Err(_) => {
                failed = true;
                said = format!("{repo} did not answer");
                break;
            }
        }
    }
    if failed {
        Err(if said.is_empty() {
            format!("the list of builds at {repo} could not be read")
        } else if said.contains("rate limit") && token.is_none() {
            format!("{said} Give a GitHub token with --github-token, the variable GITHUB_TOKEN, or `github_token` of an environment file to raise it.")
        } else {
            said
        })
    } else {
        Ok(builds)
    }
}

/// The build to install: the one `wanted` names, else the one the engine's document and this computer's
/// cards choose. `Err` is a sentence that says what to do.
pub fn choose(
    engine: &Engine,
    facts: &HostFacts,
    vendors: &[GpuVendor],
    wanted: Option<&str>,
    token: Option<&str>,
) -> Result<Chosen, String> {
    // an engine whose build is the source of a release is installed from that release's address
    // and needs nothing asked of any list: the label is the release
    if acquire::builds_from_source(engine) {
        if let Some(label) = wanted.or_else(|| acquire::tested_tag(engine)) {
            if let Some(request) = feed::request_for_label(engine, facts, label) {
                let pinned = acquire::tested_tag(engine) == Some(label);
                return Ok(Chosen {
                    label: request.label.clone(),
                    bytes: request.base.size,
                    check: if pinned {
                        "checked against the sha256 the document pins"
                    } else {
                        "not verified: only the release the document was tested with has a checksum"
                    },
                    request,
                });
            }
        }
    }

    let narrow = lookup(engine, facts, false, token)?;
    let mut builds = narrow;
    if let Some(label) = wanted {
        if !builds.iter().any(|b| b.label == label) {
            // it may be older than the newest few: ask for as many as the host gives
            builds = lookup(engine, facts, true, token)?;
        }
        let build = builds
            .iter()
            .find(|b| b.label == label)
            .ok_or_else(|| {
                let some: Vec<&str> = builds.iter().map(|b| b.label.as_str()).take(6).collect();
                format!(
                    "the list of builds has none called `{label}` (it has {}{}); name one of them with --build, or leave the build out of the configuration",
                    some.join(", "),
                    if builds.len() > 6 { ", ..." } else { "" }
                )
            })?;
        return Ok(chosen(build));
    }

    match pick_build(engine, &builds, vendors) {
        Some(at) => Ok(chosen(&builds[at])),
        None if builds.is_empty() => Err(
            "the list of builds is empty for this computer's system; name a build with --build".to_string(),
        ),
        None => Err(format!(
            "none of the {} builds listed can run on this computer ({}); name one with --build, or say what the computer has with --gpu",
            builds.len(),
            if vendors.is_empty() {
                "no graphics card was found".to_string()
            } else {
                format!(
                    "it has {}",
                    vendors.iter().map(|v| v.label()).collect::<Vec<_>>().join(", ")
                )
            }
        )),
    }
}

/// The build of `builds` that the rule of `acquire::pick_build` takes for a computer with the graphics cards `vendors`,
/// by its place in the list: what this program installs, and what the window's Install takes.
pub fn pick_build<'a>(
    engine: &Engine,
    builds: impl IntoIterator<Item = &'a Build>,
    vendors: &[GpuVendor],
) -> Option<usize> {
    let offered: Vec<Offered<'_>> = builds
        .into_iter()
        .map(|b| Offered {
            tag: &b.tag,
            asset_name: &b.asset_name,
            tested: b.tested,
            conflict: b.conflict,
            size: b.size_bytes,
        })
        .collect();
    let keys: Vec<&str> = vendors.iter().map(GpuVendor::key).collect();
    acquire::pick_build(engine, &offered, &keys)
}

fn chosen(build: &Build) -> Chosen {
    Chosen {
        label: build.label.clone(),
        request: build.install_request(&[]),
        bytes: build.size_bytes,
        check: how_checked(&build.pin, &build.sha256),
    }
}
