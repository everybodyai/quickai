//! The community configurations of quickai.everybodyai.dev, as the application reaches them
//! (docs/reports-and-community.md, section 11.9): the list, one configuration checked against the
//! list's SHA-256, and a draft of one to share.
//!
//! Nothing here runs until the person asks (Community..., Get..., Share with the community...), and no
//! request carries the install id or anything about the computer: the list is filtered on the
//! computer, and the server counts a download per configuration only. A file that is fetched is then
//! reviewed as any file from outside is, before anything is taken from it.

use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use quickai_core::profile::{valid_community_id, MAX_FILE_BYTES};

/// The address the community's API is under; `QUICKAI_COMMUNITY_URL` replaces it (for a test, or a server of
/// one's own).
pub const DEFAULT_ENDPOINT: &str = "https://api.everybodyai.dev";

/// How long a request may take.
pub const TIMEOUT: Duration = Duration::from_secs(20);

/// The most the list may be, in bytes.
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

/// Where the community is: `QUICKAI_COMMUNITY_URL`, or `api.everybodyai.dev`.
pub fn endpoint_from_environment() -> String {
    std::env::var("QUICKAI_COMMUNITY_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
}

/// The list of the published configurations, as `GET /v1/community/index` gives it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Index {
    /// The number of the rules the identities were computed by (`profile::canonical::CANON`).
    pub canon: u32,
    /// The address of the community's pages.
    pub site: String,
    pub configurations: Vec<Shared>,
    /// Versions taken out of the list for safety: a configuration that came from one shows why.
    pub withdrawn: Vec<Withdrawn>,
    pub categories: Vec<Category>,
}

/// One published configuration, at its current version.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Shared {
    pub id: String,
    pub version: u32,
    pub title: String,
    pub summary: String,
    /// The GitHub login of whoever shared it; `None` for a former member.
    pub author: Option<String>,
    pub engine: String,
    pub engine_name: String,
    pub build: Option<String>,
    /// What people call its models.
    pub models: Vec<String>,
    /// Ids of `Index::categories`.
    pub categories: Vec<String>,
    pub made_on: Option<MadeOn>,
    /// It sets its own command-line arguments.
    pub custom_args: bool,
    /// It sets lines of environment.
    pub custom_env: bool,
    /// It has a value that is asked about on import (free text, wider access).
    pub needs_review: bool,
    /// The mean of its stars, when it has ratings.
    pub stars: Option<f64>,
    pub ratings: u64,
    pub downloads: u64,
    /// How many people shared the same configuration.
    pub sharers: u64,
    /// The SHA-256 of the file, in hex: what is fetched is held to it.
    pub sha256: String,
    pub identity: String,
    pub family: String,
    /// The identities of its earlier versions.
    pub older: Vec<Older>,
    /// The day it was published (`2026-10-30`).
    pub published: String,
    /// Its page.
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MadeOn {
    pub os: Option<String>,
    pub arch: Option<String>,
    pub gpu: Vec<String>,
    pub memory_gb: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Older {
    pub version: u32,
    pub identity: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Withdrawn {
    pub id: String,
    pub version: u32,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Category {
    pub id: String,
    pub name: String,
}

impl Index {
    /// Reads the list, keeping only the entries that can be fetched: a valid id, a version and a SHA-256.
    pub fn parse(text: &str) -> Result<Index, String> {
        let mut index: Index = serde_json::from_str(text)
            .map_err(|e| format!("The community's list could not be read ({e})."))?;
        index
            .configurations
            .retain(|c| valid_community_id(&c.id) && c.version > 0 && is_sha256(&c.sha256));
        Ok(index)
    }

    /// The configuration whose current or earlier version has this identity, and that version.
    pub fn shared_as(&self, identity: &str) -> Option<(&Shared, u32)> {
        self.configurations.iter().find_map(|c| {
            if c.identity == identity {
                Some((c, c.version))
            } else {
                c.older
                    .iter()
                    .find(|o| o.identity == identity)
                    .map(|o| (c, o.version))
            }
        })
    }

    /// Why this version was taken out of the list, when it was.
    pub fn withdrawn(&self, id: &str, version: u32) -> Option<&Withdrawn> {
        self.withdrawn
            .iter()
            .find(|w| w.id == id && w.version == version)
    }

    /// What people call a category.
    pub fn category_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.categories
            .iter()
            .find(|c| c.id == id)
            .map_or(id, |c| c.name.as_str())
    }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The SHA-256 of `bytes`, in lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn user_agent() -> String {
    format!("quickai/{}", env!("CARGO_PKG_VERSION"))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn url(endpoint: &str, path: &str) -> String {
    format!("{}/v1/community{path}", endpoint.trim_end_matches('/'))
}

fn unreachable(e: impl std::fmt::Display) -> String {
    format!("The community could not be reached ({e}).")
}

/// The `message` of an error body, or what the status says.
fn message_of(status: u16, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["message"].as_str().map(str::to_string))
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| match status {
            404 => "The community has no such configuration.".to_string(),
            429 => "The community has had many requests from your network. Try again in a minute."
                .to_string(),
            _ => format!("The community answered {status}."),
        })
}

/// `GET /v1/community/index`: the published configurations.
pub fn fetch_index(endpoint: &str, timeout: Duration) -> Result<Index, String> {
    let response = agent(timeout)
        .get(&url(endpoint, "/index"))
        .header("user-agent", user_agent())
        .call()
        .map_err(unreachable)?;
    let status = response.status().as_u16();
    let text = response
        .into_body()
        .with_config()
        .limit(MAX_INDEX_BYTES)
        .read_to_string()
        .map_err(unreachable)?;
    if !(200..300).contains(&status) {
        return Err(message_of(status, &text));
    }
    Index::parse(&text)
}

/// `GET /v1/community/configs/<id>/<version>`: the file of `shared`, held to the SHA-256 of the list. A file that
/// is not the one the list names is refused, and nothing of it is used.
pub fn fetch_configuration(
    endpoint: &str,
    shared: &Shared,
    timeout: Duration,
) -> Result<String, String> {
    if !valid_community_id(&shared.id) || shared.version == 0 {
        return Err("The list names a configuration that cannot be fetched.".to_string());
    }
    let path = format!("/configs/{}/{}", shared.id, shared.version);
    let response = agent(timeout)
        .get(&url(endpoint, &path))
        .header("user-agent", user_agent())
        .call()
        .map_err(unreachable)?;
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .with_config()
        .limit(MAX_FILE_BYTES as u64 + 1)
        .read_to_vec()
        .map_err(unreachable)?;
    if !(200..300).contains(&status) {
        return Err(message_of(status, &String::from_utf8_lossy(&bytes)));
    }
    if !sha256_hex(&bytes).eq_ignore_ascii_case(&shared.sha256) {
        return Err(format!(
            "The file of \"{}\" is not the one the community's list names. Nothing was taken from it.",
            shared.title
        ));
    }
    String::from_utf8(bytes).map_err(|_| "The file is not text.".to_string())
}

/// What the community said to a draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Draft {
    /// Taken: the person finishes it at this address, signed in with GitHub, before it expires.
    Claim { url: String, expires: String },
    /// The same configuration is shared already (published, or waiting for review).
    Already {
        id: String,
        version: u32,
        title: String,
        url: String,
        pending: bool,
        message: String,
    },
    /// Not taken, with why.
    Refused {
        message: String,
        problems: Vec<String>,
    },
}

/// `POST /v1/community/drafts`: the text of a configuration the person wants to share, as they saw it. The
/// request carries nothing else.
pub fn upload_draft(endpoint: &str, text: &str, timeout: Duration) -> Result<Draft, String> {
    let response = agent(timeout)
        .post(&url(endpoint, "/drafts"))
        .header("content-type", "application/toml; charset=utf-8")
        .header("user-agent", user_agent())
        .send(text)
        .map_err(unreachable)?;
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .map_err(unreachable)?;
    draft_answer(status, &body)
}

/// What an answer to a draft means.
pub fn draft_answer(status: u16, body: &str) -> Result<Draft, String> {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    match status {
        201 => {
            let url = text(&json["claim"]);
            // a server of one's own on this computer answers with its own address
            let local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
            if !url.starts_with("https://") && !local {
                return Err("The community's answer had no address to finish at.".to_string());
            }
            Ok(Draft::Claim {
                url,
                expires: text(&json["expires"]),
            })
        }
        200 if json["already"].is_object() => {
            let already = &json["already"];
            Ok(Draft::Already {
                id: text(&already["id"]),
                version: already["version"].as_u64().unwrap_or(0) as u32,
                title: text(&already["title"]),
                url: text(&already["url"]),
                pending: already["state"].as_str() == Some("pending"),
                message: text(&json["message"]),
            })
        }
        400 | 409 | 413 | 429 => Ok(Draft::Refused {
            message: message_of(status, body),
            problems: json["problems"]
                .as_array()
                .map(|p| {
                    p.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        }),
        _ => Err(message_of(status, body)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::fake_telemetry::{FakeApi, Reply};

    const FILE: &str =
        "schema = 1\nkind = \"profile\"\nid = \"vision-chat\"\nengine = \"ollama\"\n";

    fn index_json(sha: &str) -> String {
        serde_json::json!({
            "canon": 1,
            "site": "https://quickai.everybodyai.dev/configurations",
            "configurations": [{
                "id": "vision-chat", "version": 3, "title": "Vision chat", "summary": "Pictures in.",
                "author": "octocat", "engine": "ollama", "engine_name": "Ollama", "build": null,
                "models": ["Qwen3-VL 8B"], "categories": ["vision"],
                "made_on": { "os": "linux", "memory_gb": 16 },
                "custom_args": false, "custom_env": false, "needs_review": false,
                "stars": 4.5, "ratings": 12, "downloads": 230, "sharers": 2,
                "sha256": sha, "identity": "aa", "family": "ff",
                "older": [{ "version": 2, "identity": "bb" }],
                "published": "2026-10-30",
                "url": "https://quickai.everybodyai.dev/configurations/vision-chat",
                "a_key_of_later": true
            }, {
                "id": "Not An Id", "version": 1, "sha256": sha
            }],
            "withdrawn": [{ "id": "old-one", "version": 2, "reason": "It opened the server to the network." }],
            "categories": [{ "id": "vision", "name": "Pictures in" }]
        })
        .to_string()
    }

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            retry_after: None,
            body: body.to_string(),
        }
    }

    #[test]
    fn the_list_is_read_leniently_and_keeps_only_what_can_be_fetched() {
        let index = Index::parse(&index_json(&sha256_hex(FILE.as_bytes()))).unwrap();
        assert_eq!(index.configurations.len(), 1);
        let shared = &index.configurations[0];
        assert_eq!((shared.id.as_str(), shared.version), ("vision-chat", 3));
        assert_eq!(
            index.shared_as("aa").map(|(c, v)| (c.id.as_str(), v)),
            Some(("vision-chat", 3))
        );
        assert_eq!(index.shared_as("bb").map(|(_, v)| v), Some(2));
        assert!(index.shared_as("cc").is_none());
        assert!(index.withdrawn("old-one", 2).is_some());
        assert_eq!(index.category_name("vision"), "Pictures in");
    }

    #[test]
    fn requests_carry_no_install_id() {
        let api = FakeApi::start();
        api.reply(reply(200, &index_json(&sha256_hex(FILE.as_bytes()))));
        fetch_index(&api.url, TIMEOUT).unwrap();
        let got = api.received();
        assert_eq!(got[0].path, "/v1/community/index");
        assert_eq!(got[0].install, None);
        assert!(got[0]
            .user_agent
            .as_deref()
            .unwrap()
            .starts_with("quickai/"));
    }

    #[test]
    fn a_file_that_is_not_the_one_the_list_names_is_refused() {
        let api = FakeApi::start();
        let index = Index::parse(&index_json(&sha256_hex(FILE.as_bytes()))).unwrap();
        let shared = &index.configurations[0];
        api.reply(reply(200, FILE));
        assert_eq!(
            fetch_configuration(&api.url, shared, TIMEOUT).unwrap(),
            FILE
        );
        api.reply(reply(200, &FILE.replace("ollama", "llamacpp")));
        assert!(fetch_configuration(&api.url, shared, TIMEOUT).is_err());
        api.reply(reply(
            410,
            r#"{"error":"withdrawn","message":"Taken out for safety."}"#,
        ));
        assert_eq!(
            fetch_configuration(&api.url, shared, TIMEOUT).unwrap_err(),
            "Taken out for safety."
        );
        assert_eq!(
            api.received()[0].path,
            "/v1/community/configs/vision-chat/3"
        );
    }

    #[test]
    fn each_answer_to_a_draft_means_one_thing() {
        let api = FakeApi::start();
        api.reply(reply(
            201,
            r#"{"claim":"https://quickai.everybodyai.dev/configurations/share?draft=abc","expires":"2026-10-31T19:00:00Z"}"#,
        ));
        assert!(matches!(
            upload_draft(&api.url, FILE, TIMEOUT).unwrap(),
            Draft::Claim { url, .. } if url.ends_with("draft=abc")
        ));
        assert_eq!(api.received()[0].body, FILE);
        assert_eq!(api.received()[0].install, None);
        api.reply(reply(
            200,
            r#"{"already":{"id":"vision-chat","version":3,"title":"Vision chat","url":"https://quickai.everybodyai.dev/configurations/vision-chat","state":"pending"},"message":"It is shared already."}"#,
        ));
        assert!(matches!(
            upload_draft(&api.url, FILE, TIMEOUT).unwrap(),
            Draft::Already {
                pending: true,
                version: 3,
                ..
            }
        ));
        api.reply(reply(
            400,
            r#"{"error":"refused","message":"It cannot be shared.","problems":["A word looks like a token."]}"#,
        ));
        assert_eq!(
            upload_draft(&api.url, FILE, TIMEOUT).unwrap(),
            Draft::Refused {
                message: "It cannot be shared.".into(),
                problems: vec!["A word looks like a token.".into()],
            }
        );
        api.reply(reply(
            409,
            r#"{"error":"refused_before","message":"It was refused before."}"#,
        ));
        assert!(matches!(
            upload_draft(&api.url, FILE, TIMEOUT).unwrap(),
            Draft::Refused { message, .. } if message == "It was refused before."
        ));
        api.reply(reply(503, ""));
        assert!(upload_draft(&api.url, FILE, TIMEOUT).is_err());
    }
}
