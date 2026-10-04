//! "Look up llama.cpp builds" — lists `llama-server` builds published as
//! assets on llama.cpp's own GitHub releases, filtered down to ones that
//! look runnable on this machine's OS and CPU architecture. The GUI's
//! equivalent of `remote.rs`'s model lookup, but for the server binary
//! itself rather than the model file — see the naming convention this
//! mirrors in `quickai_base`'s `build/common.sh` (`fetch_llama`; its
//! packaging scripts fetch the same releases the same way).
//!
//! Two quirks of that release feed worth knowing:
//! - Every CI build is tagged as a GitHub *prerelease* — there's no
//!   separate "stable" release with binaries attached (the one plain
//!   release GitHub calls "latest" is a changelog-only marker with no
//!   binary assets), so this deliberately does not filter prereleases out.
//! - The archive format differs by OS: Windows assets are `.zip`, but
//!   Linux/macOS assets are `.tar.gz` (see `llama_install`'s two
//!   extraction paths). A `cudart-llama-...` asset is a CUDA-runtime-only
//!   companion package with no `llama-server` inside it at all, so those
//!   are excluded even though their name otherwise looks compatible.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use serde::Deserialize;

pub struct LlamaBuild {
    pub tag: String,
    pub asset_name: String,
    pub url: String,
    pub size_bytes: Option<u64>,
}

impl LlamaBuild {
    /// Filesystem-safe subdirectory name this build gets installed under —
    /// see `paths::llama_bin_install_dir`. Includes the tag so two releases
    /// never collide, and the asset's own name (already filename-safe)
    /// so two different backends of the *same* release don't collide either.
    pub fn install_label(&self) -> String {
        let stem = self
            .asset_name
            .strip_suffix(".tar.gz")
            .or_else(|| self.asset_name.strip_suffix(".zip"))
            .unwrap_or(&self.asset_name);
        format!("{}-{stem}", self.tag)
    }
}

pub enum BuildLookupEvent {
    Found(LlamaBuild),
    Status(String),
    Done,
}

const RELEASES_URL: &str = "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=10";
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
// How many *releases* (not assets) to show builds from, most recent first —
// plenty to pick a working one without listing the entire release history.
const MAX_RELEASES_TO_SHOW: usize = 5;

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
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

fn os_keywords() -> &'static [&'static str] {
    match std::env::consts::OS {
        "windows" => &["win"],
        "macos" => &["macos", "osx"],
        _ => &["ubuntu", "linux"],
    }
}

fn arch_keywords() -> &'static [&'static str] {
    match std::env::consts::ARCH {
        "aarch64" => &["arm64", "aarch64"],
        _ => &["x64", "x86_64", "amd64"],
    }
}

/// Whether `name` looks like a prebuilt archive for the given OS/arch
/// keyword sets — a substring match against both, since llama.cpp's own
/// naming (`llama-<tag>-bin-<os>-<backend>-<arch>.{zip,tar.gz}`) isn't
/// otherwise documented/stable enough to parse more strictly. Split out
/// from `is_compatible_asset` so tests can check the matching logic
/// against every platform's keywords, not just whichever one the test
/// runs on.
fn matches_platform(name: &str, os_keywords: &[&str], arch_keywords: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.ends_with(".zip") || lower.ends_with(".tar.gz"))
        && !lower.contains("cudart")
        && os_keywords.iter().any(|k| lower.contains(k))
        && arch_keywords.iter().any(|k| lower.contains(k))
}

/// Whether `asset_name` looks like a prebuilt archive for *this* machine's
/// OS and CPU architecture.
fn is_compatible_asset(name: &str) -> bool {
    matches_platform(name, os_keywords(), arch_keywords())
}

/// A short, human-readable label for the GPU backend an asset's filename
/// suggests it was built with — display-only, and also what
/// `gpu::mismatch_warning` keys its vendor-lock check off of. Falls back to
/// "default" for a name that doesn't mention one (bundled CPU/Vulkan
/// releases are often just left unlabeled).
pub fn backend_label(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("cuda") {
        "CUDA"
    } else if lower.contains("vulkan") {
        "Vulkan"
    } else if lower.contains("sycl") {
        "SYCL"
    } else if lower.contains("hip") || lower.contains("rocm") {
        "ROCm/HIP"
    } else if lower.contains("cpu") {
        "CPU"
    } else {
        "default"
    }
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

/// Starts the lookup in a background thread and returns immediately with
/// the receiving end of the channel — same shape as `remote::spawn_lookup`.
pub fn spawn_lookup(wake: impl Fn() + Send + Sync + 'static) -> Receiver<BuildLookupEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || run(tx, wake));
    rx
}

fn run(tx: Sender<BuildLookupEvent>, wake: impl Fn() + Send + Sync + 'static) {
    let send = |e: BuildLookupEvent| {
        let _ = tx.send(e);
        wake();
    };

    send(BuildLookupEvent::Status(
        "Checking github.com/ggml-org/llama.cpp releases...".into(),
    ));
    let releases: Vec<GhRelease> = match get_json(RELEASES_URL) {
        Ok(r) => r,
        Err(e) => {
            send(BuildLookupEvent::Status(format!("Lookup failed: {e}")));
            send(BuildLookupEvent::Done);
            return;
        }
    };

    let mut shown = 0;
    for release in releases {
        if shown >= MAX_RELEASES_TO_SHOW {
            break;
        }
        let matches: Vec<GhAsset> = release
            .assets
            .into_iter()
            .filter(|a| is_compatible_asset(&a.name))
            .collect();
        if matches.is_empty() {
            continue;
        }
        for asset in matches {
            send(BuildLookupEvent::Found(LlamaBuild {
                tag: release.tag_name.clone(),
                asset_name: asset.name,
                url: asset.browser_download_url,
                size_bytes: asset.size,
            }));
        }
        shown += 1;
    }
    send(BuildLookupEvent::Done);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn backend_label_recognises_known_backends() {
        assert_eq!(
            backend_label("llama-b11007-bin-win-cuda-cu12.4-x64.zip"),
            "CUDA"
        );
        assert_eq!(
            backend_label("llama-b11007-bin-win-vulkan-x64.zip"),
            "Vulkan"
        );
        assert_eq!(backend_label("llama-b11007-bin-win-sycl-x64.zip"), "SYCL");
        assert_eq!(
            backend_label("llama-b11007-bin-win-hip-x64.zip"),
            "ROCm/HIP"
        );
        assert_eq!(backend_label("llama-b11007-bin-win-cpu-x64.zip"), "CPU");
        assert_eq!(backend_label("llama-b11007-bin-macos-arm64.zip"), "default");
    }

    #[test]
    fn install_label_combines_tag_and_asset_stem() {
        let build = LlamaBuild {
            tag: "b11007".into(),
            asset_name: "llama-b11007-bin-win-vulkan-x64.zip".into(),
            url: String::new(),
            size_bytes: None,
        };
        assert_eq!(
            build.install_label(),
            "b11007-llama-b11007-bin-win-vulkan-x64"
        );
    }

    #[test]
    fn install_label_strips_tar_gz_suffix_too() {
        let build = LlamaBuild {
            tag: "b11026".into(),
            asset_name: "llama-b11026-bin-ubuntu-vulkan-x64.tar.gz".into(),
            url: String::new(),
            size_bytes: None,
        };
        assert_eq!(
            build.install_label(),
            "b11026-llama-b11026-bin-ubuntu-vulkan-x64"
        );
    }

    /// Real request against GitHub's API, matching this repo's convention
    /// of testing against real network/process resources rather than mocks.
    #[test]
    fn finds_at_least_one_release_with_assets() {
        let rx = spawn_lookup(|| {});
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut found = Vec::new();
        loop {
            if Instant::now() >= deadline {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(300)) {
                Ok(BuildLookupEvent::Found(b)) => found.push(b),
                Ok(BuildLookupEvent::Done) => break,
                Ok(BuildLookupEvent::Status(_)) => {}
                Err(_) => continue,
            }
        }
        // Every recent release has a compatible archive for every desktop
        // OS (Windows as .zip, Linux/macOS as .tar.gz — see the module
        // doc), so this should never come back empty on a supported
        // platform.
        assert!(
            !found.is_empty(),
            "expected at least one compatible llama.cpp build to be found"
        );
        for b in &found {
            let lower = b.asset_name.to_ascii_lowercase();
            assert!(lower.ends_with(".zip") || lower.ends_with(".tar.gz"));
            assert!(!lower.contains("cudart"));
            assert!(b.url.contains(&b.tag));
        }
    }

    #[test]
    fn matches_platform_requires_both_os_and_arch_keywords() {
        let win = &["win"];
        let x64 = &["x64", "x86_64", "amd64"];
        let arm64 = &["arm64", "aarch64"];
        assert!(matches_platform(
            "llama-b1-bin-win-vulkan-x64.zip",
            win,
            x64
        ));
        assert!(!matches_platform(
            "llama-b1-bin-win-vulkan-arm64.zip",
            win,
            x64
        ));
        assert!(!matches_platform("llama-b1-bin-macos-arm64.zip", win, x64));
        assert!(matches_platform(
            "llama-b1-bin-macos-arm64.zip",
            &["macos", "osx"],
            arm64
        ));
        // Linux/macOS assets are .tar.gz, not .zip — both extensions are
        // accepted (see the module doc).
        assert!(matches_platform(
            "llama-b1-bin-ubuntu-vulkan-x64.tar.gz",
            &["ubuntu", "linux"],
            x64
        ));
        assert!(!matches_platform("llama-b1-bin-win-cpu-x64.rar", win, x64));
        // A cudart-* companion package matches every keyword but contains
        // no llama-server, so it's excluded regardless.
        assert!(!matches_platform(
            "cudart-llama-b1-bin-win-cuda-12.4-x64.zip",
            win,
            x64
        ));
    }
}
