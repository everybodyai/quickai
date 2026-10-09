//! The pure half of `lookup.huggingface_gguf@1`: the addresses of Hugging Face's API and the reading
//! of what it answers (docs/lookup-and-bundles.md, section 5).
//!
//! Two kinds of request make a look-up: the search of GGUF repositories (sorted by downloads,
//! optionally for some words), and, for each repository worth looking into, its files with their
//! sizes (the list does not give sizes). Everything the site says is read as untrusted: an id must
//! be an id, a path must stay inside the repository, a count must be a count, and an answer of any
//! shape is read without a panic.

use serde::Deserialize;
use serde_json::Value;

use super::candidate::{Candidate, Origin, RemoteFile};

/// Where the hub is, unless the document says another place.
pub const SITE: &str = "https://huggingface.co";

/// What one search is asked to return: more than the look-up will look into, so that repositories
/// that turn out to have nothing to fetch do not leave it short.
const SEARCH_LIMIT: usize = 50;
const MAX_HITS: usize = 100;
const MAX_ID_CHARS: usize = 200;
const MAX_FILES: usize = 1000;
const MAX_PATH_CHARS: usize = 300;
const MAX_TAGS: usize = 64;
const MAX_TAG_CHARS: usize = 100;
const MAX_QUERY_CHARS: usize = 100;

/// Skipped when deriving per-word "similar" search terms from a keyword: searching on these alone
/// returns an unrelated flood rather than anything related to what was typed.
const STOPWORDS: &[&str] = &["gguf", "model", "models", "the", "and", "for", "with"];

/// Why an answer gave nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The answer is not what the API sends: not JSON, or not the list or the repository it should be.
    Unreadable,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Unreadable => {
                write!(f, "the answer is not one this version of QuickAI can read (the site may have changed)")
            }
        }
    }
}

/// One repository of the search: enough to decide whether to look into it.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
    pub pipeline: Option<String>,
    pub tags: Vec<String>,
}

/// What the files of a repository say.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub files: Vec<RemoteFile>,
    /// A login is needed to fetch from it.
    pub gated: bool,
    /// Its own pipeline and tags, which say more than the list's when the list's are empty.
    pub pipeline: Option<String>,
    pub tags: Vec<String>,
}

/// `{site}/api/models?filter=gguf&sort=downloads&direction=-1&limit=50`, for `query` as well
/// `&search=...`. The words are what was typed up to a `:` (a tag is not a word of a name), with one
/// space between, and at most 100 characters.
pub fn search_url(site: &str, query: Option<&str>) -> String {
    search_url_in(site, query, None)
}

/// The same search restricted to the repositories of one pipeline (`text-to-video`), which is how the
/// plain list of an engine that runs pictures and video is made: the most downloaded GGUF repository
/// of a language model is no help to it. A pipeline is what the hub calls one: lower case letters,
/// digits and `-`; anything else is left out of the address, which makes the search unrestricted.
pub fn search_url_in(site: &str, query: Option<&str>, pipeline: Option<&str>) -> String {
    let mut url = format!(
        "{}/api/models?filter=gguf&sort=downloads&direction=-1&limit={SEARCH_LIMIT}",
        site.trim().trim_end_matches('/')
    );
    if let Some(pipeline) = pipeline.filter(|p| valid_pipeline(p)) {
        url.push_str("&pipeline_tag=");
        url.push_str(pipeline);
    }
    match query.map(search_words).filter(|w| !w.is_empty()) {
        Some(words) => format!("{url}&search={}", percent_encode(&words)),
        None => url,
    }
}

/// Whether `text` can be a pipeline tag of the hub (`text-to-video`).
pub fn valid_pipeline(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The words of what was typed: up to a `:`, one space between, no more than 100 characters.
pub fn search_words(query: &str) -> String {
    let before_tag = query.split(':').next().unwrap_or("");
    let words = before_tag.split_whitespace().collect::<Vec<_>>().join(" ");
    words
        .chars()
        .take(MAX_QUERY_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// `{site}/api/models/{id}?blobs=true`: the repository with the size of each file.
pub fn repo_url(site: &str, id: &str) -> String {
    format!(
        "{}/api/models/{}?blobs=true",
        site.trim().trim_end_matches('/'),
        encode_path(id)
    )
}

/// `{site}/{repo}/resolve/{revision}/{path}`: where a file is fetched from.
pub fn file_url(site: &str, repo: &str, revision: &str, path: &str) -> String {
    format!(
        "{}/{}/resolve/{}/{}",
        site.trim().trim_end_matches('/'),
        encode_path(repo),
        encode_path(revision),
        encode_path(path)
    )
}

/// Percent-encodes a query value (unreserved characters pass through, the rest become `%XX`).
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Percent-encodes each component of a path and keeps the slashes between them.
fn encode_path(path: &str) -> String {
    path.split('/')
        .map(percent_encode)
        .collect::<Vec<_>>()
        .join("/")
}

/// The words to try as a broader "similar models" follow-up once the whole phrase has been
/// searched: `qwen coder` also tries `qwen` and `coder`, in case the phrase is too narrow to match
/// repositories that word it differently. Short words and stop words are not tried.
pub fn similar_terms(keyword: &str) -> Vec<String> {
    let mut terms: Vec<String> = keyword
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| w.len() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

/// Whether `id` is what a repository is called: `owner/name` (or a bare name), of letters, digits,
/// `.`, `-` and `_`.
pub fn valid_repo_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('/').collect();
    id.chars().count() <= MAX_ID_CHARS
        && (1..=2).contains(&parts.len())
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        })
}

/// Whether `path` stays inside a repository: relative, no `..`, no backslash, no control character.
fn valid_file_path(path: &str) -> bool {
    !path.is_empty()
        && path.chars().count() <= MAX_PATH_CHARS
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

fn clean_tags(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .filter(|t| {
                    !t.is_empty()
                        && t.chars().count() <= MAX_TAG_CHARS
                        && !t.chars().any(char::is_control)
                })
                .map(str::to_string)
                .take(MAX_TAGS)
                .collect()
        })
        .unwrap_or_default()
}

fn text_of(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| {
            !t.is_empty() && t.chars().count() <= MAX_TAG_CHARS && !t.chars().any(char::is_control)
        })
        .map(str::to_lowercase)
}

fn count_of(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(0)
}

/// The repositories of a search answer, in the order given. Entries that are not repositories (no
/// valid id) are left out; an answer that is not a list is `Unreadable`.
pub fn parse_search(body: &str) -> Result<Vec<Hit>, ReadError> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(body) else {
        return Err(ReadError::Unreadable);
    };
    let mut seen = std::collections::BTreeSet::new();
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = item
                .get("id")
                .or_else(|| item.get("modelId"))?
                .as_str()?
                .to_string();
            if !valid_repo_id(&id) || !seen.insert(id.clone()) {
                return None;
            }
            Some(Hit {
                id,
                downloads: count_of(item.get("downloads")),
                likes: count_of(item.get("likes")),
                pipeline: text_of(item.get("pipeline_tag")),
                tags: clean_tags(item.get("tags")),
            })
        })
        .take(MAX_HITS)
        .collect())
}

#[derive(Deserialize)]
struct Sibling {
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
    /// Present for a file the hub keeps in LFS (every large one), with the hash of its bytes. Read
    /// loosely: a shape that is not the usual one costs the file its hash and nothing else.
    #[serde(default)]
    lfs: Option<Value>,
}

/// A SHA-256 as the hub writes it: 64 hexadecimal digits. Anything else is no hash (a download is
/// checked against a hash that is one, and not against a word).
fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The files of a repository from its answer. A repository with no file list is `Unreadable`; a
/// file whose path is not one inside the repository is left out.
pub fn parse_repo(body: &str) -> Result<Repo, ReadError> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Err(ReadError::Unreadable);
    };
    let Some(siblings) = value.get("siblings").and_then(Value::as_array) else {
        return Err(ReadError::Unreadable);
    };
    let files = siblings
        .iter()
        .filter_map(|s| serde_json::from_value::<Sibling>(s.clone()).ok())
        .filter(|s| valid_file_path(&s.rfilename))
        .map(|s| {
            let file = RemoteFile::new(&s.rfilename, s.size);
            let hash = s
                .lfs
                .as_ref()
                .and_then(|lfs| lfs.get("sha256"))
                .and_then(Value::as_str)
                .filter(|h| valid_sha256(h));
            match hash {
                Some(hash) => file.with_sha256(hash),
                None => file,
            }
        })
        .take(MAX_FILES)
        .collect();
    let gated = match value.get("gated") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty() && s != "false",
        _ => false,
    };
    Ok(Repo {
        files,
        gated,
        pipeline: text_of(value.get("pipeline_tag")),
        tags: clean_tags(value.get("tags")),
    })
}

/// What a repository says about itself in its tags, as a sentence: "Quantized from Qwen/Qwen3-4B ·
/// apache-2.0". Empty when the tags say neither.
pub fn describe(tags: &[String]) -> String {
    let base = tags
        .iter()
        .find(|t| t.starts_with("base_model:quantized:"))
        .map(|t| format!("Quantized from {}", &t["base_model:quantized:".len()..]))
        .or_else(|| {
            tags.iter()
                .find(|t| t.starts_with("base_model:"))
                .and_then(|t| t.rsplit(':').next())
                .map(|model| format!("Based on {model}"))
        });
    let license = tags
        .iter()
        .find_map(|t| t.strip_prefix("license:"))
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    [base, license]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The candidate a repository makes, from the hit of the search and what its files said. A
/// repository whose own answer has pipeline or tags uses them when the list's were empty.
pub fn candidate(site: &str, hit: &Hit, repo: &Repo) -> Candidate {
    let pipeline = hit.pipeline.clone().or_else(|| repo.pipeline.clone());
    let mut tags = hit.tags.clone();
    for tag in &repo.tags {
        if !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    tags.truncate(MAX_TAGS);
    Candidate {
        key: format!("hf:{}", hit.id),
        name: hit.id.clone(),
        description: describe(&tags),
        downloads: Some(hit.downloads),
        likes: Some(hit.likes),
        capabilities: Vec::new(),
        pipeline,
        tags,
        origin: Origin::Hub {
            site: site.trim().trim_end_matches('/').to_string(),
            repo: hit.id.clone(),
            revision: "main".to_string(),
            files: repo.files.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_is_for_gguf_by_downloads_and_for_the_words_when_there_are_some() {
        assert_eq!(
            search_url("https://huggingface.co", None),
            "https://huggingface.co/api/models?filter=gguf&sort=downloads&direction=-1&limit=50"
        );
        assert_eq!(
            search_url("https://huggingface.co/", Some("  qwen   coder:7b ")),
            "https://huggingface.co/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&search=qwen%20coder"
        );
        // nothing typed but a tag is the plain search
        assert_eq!(
            search_url("http://127.0.0.1:9", Some(":7b")),
            search_url("http://127.0.0.1:9", None)
        );
        assert_eq!(search_words(&"a ".repeat(200)).chars().count(), 99);
    }

    #[test]
    fn a_repository_and_a_file_have_addresses() {
        assert_eq!(
            repo_url("https://huggingface.co", "ggml-org/gemma-3-4b-it-GGUF"),
            "https://huggingface.co/api/models/ggml-org/gemma-3-4b-it-GGUF?blobs=true"
        );
        assert_eq!(
            file_url("https://huggingface.co", "a/b", "main", "dir/my model.gguf"),
            "https://huggingface.co/a/b/resolve/main/dir/my%20model.gguf"
        );
    }

    #[test]
    fn similar_terms_are_the_significant_words_once_each() {
        assert_eq!(
            similar_terms("Qwen Coder gguf the model"),
            ["coder", "qwen"]
        );
        assert_eq!(similar_terms("qwen"), ["qwen"]);
        assert_eq!(similar_terms("qwen qwen"), ["qwen"]);
        assert!(similar_terms("of an").is_empty());
    }

    #[test]
    fn what_the_search_says_is_read_and_what_is_not_a_repository_is_left_out() {
        let body = r#"[
          {"id":"unsloth/Qwen3-4B-GGUF","downloads":1100000,"likes":340,"pipeline_tag":"Text-Generation","tags":["gguf","Conversational","license:apache-2.0"]},
          {"id":"../../etc/passwd","downloads":9},
          {"id":"unsloth/Qwen3-4B-GGUF","downloads":1},
          {"modelId":"a/b","downloads":"many"},
          {"downloads":5},
          7
        ]"#;
        let hits = parse_search(body).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, "unsloth/Qwen3-4B-GGUF");
        assert_eq!(hits[0].downloads, 1_100_000);
        assert_eq!(hits[0].likes, 340);
        assert_eq!(hits[0].pipeline.as_deref(), Some("text-generation"));
        assert_eq!(
            hits[0].tags,
            ["gguf", "Conversational", "license:apache-2.0"]
        );
        assert_eq!(hits[1].id, "a/b");
        assert_eq!(
            hits[1].downloads, 0,
            "a count that is not a count is nothing"
        );
        assert_eq!(parse_search("[]").unwrap(), []);
        for bad in [
            "",
            "not json",
            "{}",
            "{\"error\":\"Rate limit\"}",
            "null",
            "5",
        ] {
            assert_eq!(parse_search(bad), Err(ReadError::Unreadable), "{bad:?}");
        }
    }

    #[test]
    fn the_files_of_a_repository_are_read_with_their_sizes_and_the_unsafe_ones_left_out() {
        let body = r#"{"siblings":[
          {"rfilename":"gemma-3-4b-it-Q4_K_M.gguf","size":2489757856},
          {"rfilename":"mmproj-model-f16.gguf","size":851251104},
          {"rfilename":"README.md"},
          {"rfilename":"../escape.gguf","size":1},
          {"rfilename":"/abs.gguf","size":1},
          {"rfilename":"dir\\win.gguf","size":1},
          {"rfilename":"a//b.gguf","size":1},
          {"rfilename":""},
          {"size":3}
        ],"gated":false,"pipeline_tag":"image-text-to-text","tags":["Gguf"]}"#;
        let repo = parse_repo(body).unwrap();
        assert_eq!(
            repo.files
                .iter()
                .map(|f| (f.path.as_str(), f.size))
                .collect::<Vec<_>>(),
            [
                ("gemma-3-4b-it-Q4_K_M.gguf", Some(2_489_757_856)),
                ("mmproj-model-f16.gguf", Some(851_251_104)),
                ("README.md", None),
            ]
        );
        assert!(!repo.gated);
        assert_eq!(repo.pipeline.as_deref(), Some("image-text-to-text"));
        assert_eq!(repo.tags, ["Gguf"]);
        for bad in ["", "[]", "{}", "{\"siblings\":3}", "no"] {
            assert_eq!(parse_repo(bad), Err(ReadError::Unreadable), "{bad:?}");
        }
    }

    #[test]
    fn gated_is_a_flag_or_a_word() {
        let gated = |v: &str| {
            parse_repo(&format!(r#"{{"siblings":[],"gated":{v}}}"#))
                .unwrap()
                .gated
        };
        assert!(gated("true"));
        assert!(gated("\"auto\""));
        assert!(gated("\"manual\""));
        assert!(!gated("false"));
        assert!(!gated("\"\""));
        assert!(!gated("null"));
        assert!(!parse_repo(r#"{"siblings":[]}"#).unwrap().gated);
    }

    #[test]
    fn a_repository_describes_itself_by_its_tags() {
        let tags = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            describe(&tags(&[
                "gguf",
                "base_model:Qwen/Qwen3-4B",
                "base_model:quantized:Qwen/Qwen3-4B",
                "license:apache-2.0"
            ])),
            "Quantized from Qwen/Qwen3-4B · apache-2.0"
        );
        assert_eq!(
            describe(&tags(&["base_model:finetune:x/y"])),
            "Based on x/y"
        );
        assert_eq!(describe(&tags(&["license:mit"])), "mit");
        assert_eq!(describe(&tags(&["gguf"])), "");
        assert_eq!(describe(&[]), "");
    }

    #[test]
    fn ids_and_paths_are_held_to_what_they_are() {
        for ok in ["a/b", "unsloth/Qwen3-4B-GGUF", "gpt2", "a.b/c_d-e"] {
            assert!(valid_repo_id(ok), "{ok}");
        }
        for bad in [
            "",
            "a/b/c",
            "/a",
            "a/",
            "..",
            "a/..",
            "a b/c",
            "a/b?x",
            &"x".repeat(300),
        ] {
            assert!(!valid_repo_id(bad), "{bad}");
        }
        assert!(valid_file_path("MTP/mtp-x.gguf"));
        assert!(!valid_file_path("a/../b"));
        assert!(!valid_file_path("a\nb"));
    }

    #[test]
    fn a_candidate_is_what_the_hit_and_the_files_say_together() {
        let hit = Hit {
            id: "ggml-org/gemma-3-4b-it-GGUF".into(),
            downloads: 64_508,
            likes: 12,
            pipeline: None,
            tags: vec!["gguf".into()],
        };
        let repo = Repo {
            files: vec![RemoteFile::new("m-Q4_K_M.gguf", Some(5))],
            gated: false,
            pipeline: Some("image-text-to-text".into()),
            tags: vec!["gguf".into(), "conversational".into()],
        };
        let c = candidate("https://huggingface.co/", &hit, &repo);
        assert_eq!(c.key, "hf:ggml-org/gemma-3-4b-it-GGUF");
        assert_eq!(
            c.pipeline.as_deref(),
            Some("image-text-to-text"),
            "the repository's own when the list had none"
        );
        assert_eq!(c.tags, ["gguf", "conversational"]);
        assert_eq!(c.downloads, Some(64_508));
        match c.origin {
            Origin::Hub {
                site,
                repo,
                revision,
                files,
            } => {
                assert_eq!(site, "https://huggingface.co");
                assert_eq!(repo, "ggml-org/gemma-3-4b-it-GGUF");
                assert_eq!(revision, "main");
                assert_eq!(files.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }
}
