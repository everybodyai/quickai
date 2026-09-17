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
//!
//! Besides the plain popularity lookup (`spawn_lookup`), `spawn_keyword_search`
//! lets the user search by name: it runs an exact-phrase search first, then
//! a follow-up pass per significant word in the phrase to pick up "similar"
//! repos a strict phrase match would miss (e.g. a different word order or an
//! extra suffix), merging everything into one deduplicated result stream.

use std::collections::HashSet;
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
const KEYWORD_REPOS_TO_INSPECT: usize = 20;
const SIMILAR_REPOS_TO_INSPECT: usize = 20;
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

// Skipped when deriving per-word "similar" search terms from a keyword —
// searching on these alone tends to return an unrelated flood rather than
// anything actually related to what was typed.
const STOPWORDS: &[&str] = &["gguf", "model", "models", "the", "and", "for", "with"];

// Preferred quantizations, best-first: mainstream llama.cpp-supported
// K-quants that balance size against quality. Falls back to the smallest
// remaining single-file .gguf if a repo has none of these.
const PREFERRED_QUANTS: &[&str] = &[
    "Q4_K_M", "Q4_K_S", "Q5_K_M", "Q4_0", "Q8_0", "Q6_K", "Q3_K_M",
];

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

/// Percent-encodes a query-string value per RFC 3986 (unreserved characters
/// pass through, everything else becomes `%XX`) — hand-rolled since nothing
/// else in this crate needs a URL-encoding dependency.
fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Same GGUF-filtered, downloads-sorted search as `SEARCH_URL`, but scoped
/// to repos whose id/name matches `query`.
fn search_url(query: &str) -> String {
    format!(
        "https://huggingface.co/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&search={}",
        encode_query(query)
    )
}

/// Splits a keyword phrase into individual significant words to try as a
/// broader "similar models" follow-up once the exact phrase has been
/// searched — e.g. "qwen coder" also tries "qwen" and "coder" alone, in
/// case the exact phrase is too narrow to match repos that phrase it
/// differently.
fn similar_terms(keyword: &str) -> Vec<String> {
    let mut terms: Vec<String> = keyword
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| w.len() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .collect();
    terms.sort();
    terms.dedup();
    terms
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

    candidates
        .into_iter()
        .min_by_key(|s| s.size.unwrap_or(u64::MAX))
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

    let mut seen = HashSet::new();
    inspect_and_send(hits, REPOS_TO_INSPECT, &mut seen, &send);
    send(LookupEvent::Done);
}

/// Inspects each hit's file listing and, if a usable `.gguf` file is found,
/// sends a `Found` event — shared by the plain popularity lookup and the
/// keyword/similar searches. `seen` dedups by repo id across multiple
/// search passes (e.g. the exact-phrase pass and the per-word "similar"
/// passes) so the same repo is never listed twice; `limit` caps how many
/// *new* repos this call inspects.
fn inspect_and_send(
    hits: Vec<SearchHit>,
    limit: usize,
    seen: &mut HashSet<String>,
    send: &impl Fn(LookupEvent),
) {
    for hit in hits
        .into_iter()
        .filter(|h| seen.insert(h.id.clone()))
        .take(limit)
    {
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
}

/// Starts a keyword search in a background thread: an exact-phrase search
/// first, then a follow-up pass per significant word in `keyword` to widen
/// the net to "similar" repos a strict phrase match would miss. Results
/// stream back the same way as `spawn_lookup` — `Found` events (deduplicated
/// across every pass) followed by a final `Done`.
pub fn spawn_keyword_search(
    keyword: String,
    wake: impl Fn() + Send + Sync + 'static,
) -> Receiver<LookupEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || run_keyword_search(keyword, tx, wake));
    rx
}

fn run_keyword_search(
    keyword: String,
    tx: Sender<LookupEvent>,
    wake: impl Fn() + Send + Sync + 'static,
) {
    let send = |e: LookupEvent| {
        let _ = tx.send(e);
        wake();
    };
    let keyword = keyword.trim();
    let mut seen = HashSet::new();

    send(LookupEvent::Status(format!(
        "Searching Hugging Face for \"{keyword}\"..."
    )));
    match get_json::<Vec<SearchHit>>(&search_url(keyword)) {
        Ok(hits) => inspect_and_send(hits, KEYWORD_REPOS_TO_INSPECT, &mut seen, &send),
        Err(e) => send(LookupEvent::Status(format!("Search failed: {e}"))),
    }

    let terms: Vec<String> = similar_terms(keyword)
        .into_iter()
        .filter(|t| !t.eq_ignore_ascii_case(keyword))
        .collect();
    if !terms.is_empty() {
        send(LookupEvent::Status("Looking for similar models...".into()));
        for term in terms {
            match get_json::<Vec<SearchHit>>(&search_url(&term)) {
                Ok(hits) => inspect_and_send(hits, SIMILAR_REPOS_TO_INSPECT, &mut seen, &send),
                Err(e) => send(LookupEvent::Status(format!(
                    "Skipped similar search \"{term}\": {e}"
                ))),
            }
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

        assert!(
            !found.is_empty(),
            "expected at least one compatible model to be found"
        );
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
            Sibling {
                rfilename: "model-BF16-00001-of-00002.gguf".into(),
                size: Some(1),
            },
            Sibling {
                rfilename: "MTP/mtp-model-Q4_0.gguf".into(),
                size: Some(1),
            },
            Sibling {
                rfilename: "model-Q4_K_M.gguf".into(),
                size: Some(2),
            },
            Sibling {
                rfilename: "model-Q8_0.gguf".into(),
                size: Some(3),
            },
        ];
        let picked = pick_file(&siblings).expect("expected a pick");
        assert_eq!(picked.rfilename, "model-Q4_K_M.gguf");
    }

    #[test]
    fn pick_file_falls_back_to_smallest_when_no_preferred_quant() {
        let siblings = vec![
            Sibling {
                rfilename: "model-IQ2_XXS.gguf".into(),
                size: Some(500),
            },
            Sibling {
                rfilename: "model-IQ1_S.gguf".into(),
                size: Some(200),
            },
        ];
        let picked = pick_file(&siblings).expect("expected a pick");
        assert_eq!(picked.rfilename, "model-IQ1_S.gguf");
    }

    #[test]
    fn encode_query_escapes_reserved_characters() {
        assert_eq!(encode_query("qwen coder"), "qwen%20coder");
        assert_eq!(encode_query("Bonsai-1.7B_v2~1"), "Bonsai-1.7B_v2~1");
        assert_eq!(encode_query("a/b?c"), "a%2Fb%3Fc");
    }

    #[test]
    fn search_url_embeds_encoded_query_and_stays_gguf_filtered() {
        let url = search_url("qwen coder");
        assert!(url.contains("filter=gguf"));
        assert!(url.contains("search=qwen%20coder"));
    }

    #[test]
    fn similar_terms_splits_dedups_and_drops_short_or_stop_words() {
        let terms = similar_terms("Qwen Coder gguf the model");
        assert_eq!(terms, vec!["coder", "qwen"]);
    }

    #[test]
    fn similar_terms_of_single_word_is_just_that_word() {
        // `run_keyword_search` is what filters out the term matching the
        // full keyword (since that's already covered by the exact-phrase
        // pass) — `similar_terms` itself just extracts significant words.
        assert_eq!(similar_terms("qwen"), vec!["qwen"]);
    }

    /// Real search against Hugging Face's API, matching this repo's
    /// convention of testing against real network/process resources rather
    /// than mocks. "qwen" is common enough across GGUF repos that both the
    /// exact-phrase pass and at least one per-word "similar" pass should
    /// turn up results (though for a single-word keyword like this one,
    /// `similar_terms` skips the redundant pass over the same word).
    #[test]
    fn keyword_search_finds_at_least_one_compatible_model() {
        let rx = spawn_keyword_search("qwen".to_string(), || {});
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

        assert!(
            !found.is_empty(),
            "expected at least one compatible model to be found"
        );
        let mut ids: Vec<&str> = found.iter().map(|m| m.repo_id.as_str()).collect();
        ids.sort();
        let unique = ids.len();
        ids.dedup();
        assert_eq!(
            unique,
            ids.len(),
            "expected no duplicate repos across passes"
        );
    }
}
