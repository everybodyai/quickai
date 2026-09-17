//! "Lookup" — searches Hugging Face for GGUF models compatible with the
//! llama.cpp build this app bundles, so the user doesn't have to go
//! hunting for a download URL by hand. Two-step API dance: a search
//! listing the top repos tagged "gguf" (sorted by downloads, a rough
//! proxy for "best"), then per-repo a file listing to pick out one
//! reasonably-sized quantization to offer.
//!
//! Results stream back in download-sorted order as a flat list; the GUI
//! (app.rs) is what buckets them into a "top 10" plus High/Medium/Small
//! size sections, so this module just needs to gather a pool wide enough
//! for those buckets to have something to work with.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use serde::Deserialize;

pub struct RemoteModel {
    pub repo_id: String,
    pub filename: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    pub downloads: u64,
}

pub enum LookupEvent {
    Found(RemoteModel),
    Status(String),
    Done,
}

const SEARCH_URL: &str =
    "https://huggingface.co/api/models?filter=gguf&sort=downloads&direction=-1&limit=50";
// Wide enough that the GUI's High/Medium/Small size buckets (see app.rs)
// each have a decent chance of finding something, not just the top-10
// most-downloaded repos (which tend to cluster around one size class).
const REPOS_TO_INSPECT: usize = 30;
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

// Preferred quantizations, best-first: mainstream llama.cpp-supported
// K-quants that balance size against quality. Falls back to the smallest
// remaining single-file .gguf if a repo has none of these.
const PREFERRED_QUANTS: &[&str] = &["Q4_K_M", "Q4_K_S", "Q5_K_M", "Q4_0", "Q8_0", "Q6_K", "Q3_K_M"];

#[derive(Deserialize)]
struct SearchHit {
    id: String,
    #[serde(default)]
    downloads: u64,
}

#[derive(Deserialize)]
struct Sibling {
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
struct RepoFiles {
    siblings: Vec<Sibling>,
}

fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let resp = ureq::get(url)
        .config()
        .timeout_global(Some(CALL_TIMEOUT))
        .build()
        .call()
        .map_err(|e| e.to_string())?;
    let mut body = resp.into_body();
    body.read_json::<T>().map_err(|e| e.to_string())
}

/// Picks one file out of a repo's listing to offer: skips sharded
/// multi-part files (`-00001-of-00005.gguf`) and auxiliary MTP draft-model
/// files, then prefers a known-good quant name, else falls back to the
/// smallest remaining `.gguf`.
fn pick_file(siblings: &[Sibling]) -> Option<&Sibling> {
    let candidates: Vec<&Sibling> = siblings
        .iter()
        .filter(|s| {
            let name = s.rfilename.to_ascii_lowercase();
            name.ends_with(".gguf") && !name.contains("-of-") && !name.contains("mtp")
        })
        .collect();

    for quant in PREFERRED_QUANTS {
        if let Some(hit) = candidates
            .iter()
            .find(|s| s.rfilename.to_ascii_uppercase().contains(quant))
        {
            return Some(hit);
        }
    }

    candidates.into_iter().min_by_key(|s| s.size.unwrap_or(u64::MAX))
}

/// Starts the search in a background thread and returns immediately with
/// the receiving end of the channel; results stream in as `Found` events
/// (one repo can take a moment to inspect) followed by a final `Done`.
pub fn spawn_lookup(wake: impl Fn() + Send + Sync + 'static) -> Receiver<LookupEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || run(tx, wake));
    rx
}

fn run(tx: Sender<LookupEvent>, wake: impl Fn() + Send + Sync + 'static) {
    let send = |e: LookupEvent| {
        let _ = tx.send(e);
        wake();
    };

    send(LookupEvent::Status(
        "Searching Hugging Face for GGUF models...".into(),
    ));
    let hits: Vec<SearchHit> = match get_json(SEARCH_URL) {
        Ok(h) => h,
        Err(e) => {
            send(LookupEvent::Status(format!("Search failed: {e}")));
            send(LookupEvent::Done);
            return;
        }
    };

    for hit in hits.into_iter().take(REPOS_TO_INSPECT) {
        let files_url = format!("https://huggingface.co/api/models/{}?blobs=true", hit.id);
        let files: RepoFiles = match get_json(&files_url) {
            Ok(f) => f,
            Err(e) => {
                send(LookupEvent::Status(format!("Skipped {}: {e}", hit.id)));
                continue;
            }
        };
        if let Some(sib) = pick_file(&files.siblings) {
            send(LookupEvent::Found(RemoteModel {
                repo_id: hit.id.clone(),
                filename: sib.rfilename.clone(),
                url: format!(
                    "https://huggingface.co/{}/resolve/main/{}",
                    hit.id, sib.rfilename
                ),
                size_bytes: sib.size,
                downloads: hit.downloads,
            }));
        }
    }
    send(LookupEvent::Done);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Real search against Hugging Face's API, matching this repo's
    /// convention of testing against real network/process resources
    /// rather than mocks.
    #[test]
    fn finds_at_least_one_compatible_model() {
        let rx = spawn_lookup(|| {});
        // Inspecting REPOS_TO_INSPECT repos sequentially against a live API
        // takes noticeably longer than it used to now that it's 30 instead
        // of 10, so give this real-network test more room.
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut found = Vec::new();
        loop {
            if Instant::now() >= deadline {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(300)) {
                Ok(LookupEvent::Found(m)) => found.push(m),
                Ok(LookupEvent::Done) => break,
                Ok(LookupEvent::Status(_)) => {}
                Err(_) => continue,
            }
        }

        assert!(!found.is_empty(), "expected at least one compatible model to be found");
        for m in &found {
            assert!(m.filename.to_ascii_lowercase().ends_with(".gguf"));
            assert!(!m.filename.to_ascii_lowercase().contains("-of-"));
            assert!(m.url.starts_with("https://huggingface.co/"));
            assert!(m.url.ends_with(&m.filename));
        }
    }

    #[test]
    fn pick_file_prefers_known_quant_and_skips_shards_and_mtp() {
        let siblings = vec![
            Sibling { rfilename: "model-BF16-00001-of-00002.gguf".into(), size: Some(1) },
            Sibling { rfilename: "MTP/mtp-model-Q4_0.gguf".into(), size: Some(1) },
            Sibling { rfilename: "model-Q4_K_M.gguf".into(), size: Some(2) },
            Sibling { rfilename: "model-Q8_0.gguf".into(), size: Some(3) },
        ];
        let picked = pick_file(&siblings).expect("expected a pick");
        assert_eq!(picked.rfilename, "model-Q4_K_M.gguf");
    }

    #[test]
    fn pick_file_falls_back_to_smallest_when_no_preferred_quant() {
        let siblings = vec![
            Sibling { rfilename: "model-IQ2_XXS.gguf".into(), size: Some(500) },
            Sibling { rfilename: "model-IQ1_S.gguf".into(), size: Some(200) },
        ];
        let picked = pick_file(&siblings).expect("expected a pick");
        assert_eq!(picked.rfilename, "model-IQ1_S.gguf");
    }
}
