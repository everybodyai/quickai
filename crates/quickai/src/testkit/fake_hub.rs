//! A fake of Hugging Face's API and file servers: it serves the answers that were recorded
//! (`tests/fixtures/huggingface/`, made by `record.sh`) by what is asked for, can be given small
//! repositories of its own whose files it really serves, and keeps what it was asked.
//!
//! - `/api/models?...&search=words&pipeline_tag=p`: the recorded list of the most downloaded, narrowed
//!   to the repositories whose id has every word and whose pipeline is `p`, in the order it has;
//! - `/api/models/<owner>/<name>?blobs=true`: the recorded answer for the six repositories that were
//!   recorded; for a repository only the list has, one file of a size of its own; for one the fake was
//!   given (`add_repo`), what it was given;
//! - `/<owner>/<name>/resolve/<revision>/<path>`: the file, of the length the repository says (or
//!   2048 bytes), filled with its own name, so a test can tell which file reached the disk.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::fake_library::Asked;
use super::mini_http::{MiniServer, Request};

/// How the fake answers.
#[derive(Clone, Debug, Default)]
pub enum Behaviour {
    /// The recorded answers.
    #[default]
    Normal,
    /// The same status for every request, with a body that says so.
    Error(u16),
    /// An answer that is not JSON.
    Garbage,
    /// The first `n` requests as usual, the rest refused with 429.
    LimitAfter(usize),
    /// Everything about repositories (not the search) is refused with 500.
    RepositoriesFail,
}

struct State {
    /// The hits of the list, as the API sends them.
    list: Vec<Value>,
    /// The answer for a repository.
    repos: BTreeMap<String, Value>,
    /// The length of a file served, by the path in the repository.
    lengths: BTreeMap<String, u64>,
    /// Repositories that are listed and are not there when asked for.
    missing: std::collections::BTreeSet<String>,
    /// Pipelines whose search fails (500).
    broken_pipelines: std::collections::BTreeSet<String>,
    /// Pipelines whose search is answered with a page that is not JSON.
    garbled_pipelines: std::collections::BTreeSet<String>,
}

pub struct FakeHub {
    server: MiniServer,
    asked: Arc<Mutex<Vec<Asked>>>,
    behaviour: Arc<Mutex<Behaviour>>,
    state: Arc<Mutex<State>>,
}

fn fixture(rel: &str) -> String {
    std::fs::read_to_string(super::fixtures_dir().join("huggingface").join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The recorded answer for a repository, as a file name has it: the six of the llama.cpp look-up, and
/// the four of ComfyUI's (`huggingface/comfyui/`, which keep the checksums the hub states).
fn recorded_repo(id: &str) -> Option<Value> {
    ["huggingface/repos", "huggingface/comfyui/repos"]
        .iter()
        .find_map(|folder| {
            let path = super::fixtures_dir()
                .join(folder)
                .join(format!("{}.json", id.replace('/', "__")));
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
        })
}

/// The bytes the fake serves for a file: its repository and its path, over and over, as long as
/// it is asked to be (so a test can say what the file hashes to).
pub fn content(repo: &str, file: &str, length: usize) -> Vec<u8> {
    format!("{repo}:{file}|")
        .bytes()
        .cycle()
        .take(length)
        .collect()
}

/// The SHA-256 of `bytes`, lower case hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A repository's answer: its files with the sizes they have.
pub fn repo_json(id: &str, pipeline: Option<&str>, tags: &[&str], files: &[(&str, u64)]) -> Value {
    let mut value = json!({
        "id": id,
        "modelId": id,
        "downloads": 1,
        "likes": 0,
        "tags": tags,
        "gated": false,
        "siblings": files.iter().map(|(name, size)| json!({"rfilename": name, "size": size})).collect::<Vec<_>>(),
    });
    if let Some(pipeline) = pipeline {
        value["pipeline_tag"] = json!(pipeline);
    }
    value
}

impl FakeHub {
    /// The site with what was recorded.
    pub fn start() -> FakeHub {
        let list: Vec<Value> =
            serde_json::from_str(&fixture("popular.json")).expect("the recorded list");
        let state = Arc::new(Mutex::new(State {
            list,
            repos: BTreeMap::new(),
            lengths: BTreeMap::new(),
            missing: std::collections::BTreeSet::new(),
            broken_pipelines: std::collections::BTreeSet::new(),
            garbled_pipelines: std::collections::BTreeSet::new(),
        }));
        let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
        let behaviour: Arc<Mutex<Behaviour>> = Arc::default();
        let (log, mode, data) = (asked.clone(), behaviour.clone(), state.clone());
        let server = MiniServer::start(None, move |req: &Request, resp| {
            let count = {
                let mut log = log.lock().unwrap();
                log.push(Asked {
                    path: req.path.clone(),
                    headers: req.headers.clone(),
                });
                log.len()
            };
            let behaviour = mode.lock().unwrap().clone();
            let path = req.path.split('?').next().unwrap_or("").to_string();
            let is_api_repo = path.starts_with("/api/models/");
            match behaviour {
                Behaviour::Error(code) => {
                    resp.status(code);
                    resp.send(format!("error {code}").as_bytes());
                    return;
                }
                Behaviour::Garbage => {
                    resp.header("Content-Type", "text/html; charset=utf-8");
                    resp.send(b"<html><body><h1>Down for maintenance</h1></body></html>");
                    return;
                }
                Behaviour::LimitAfter(n) if count > n => {
                    resp.status(429);
                    resp.header("Retry-After", "60");
                    resp.send(b"rate limited");
                    return;
                }
                Behaviour::RepositoriesFail if is_api_repo => {
                    resp.status(500);
                    resp.send(b"boom");
                    return;
                }
                _ => {}
            }
            let data = data.lock().unwrap();
            if path == "/api/models" {
                let query = query_of(&req.path);
                let words: Vec<String> = query
                    .get("search")
                    .map(|s| {
                        s.to_lowercase()
                            .split_whitespace()
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let pipeline = query.get("pipeline_tag").cloned();
                if pipeline
                    .as_ref()
                    .is_some_and(|p| data.broken_pipelines.contains(p))
                {
                    resp.status(500);
                    resp.send(b"boom");
                    return;
                }
                if pipeline
                    .as_ref()
                    .is_some_and(|p| data.garbled_pipelines.contains(p))
                {
                    resp.header("Content-Type", "text/html; charset=utf-8");
                    resp.send(b"<html><body><h1>Down for maintenance</h1></body></html>");
                    return;
                }

                let hits: Vec<&Value> = data
                    .list
                    .iter()
                    .filter(|hit| {
                        let id = hit["id"].as_str().unwrap_or("").to_lowercase();
                        words.iter().all(|w| id.contains(w))
                    })
                    .filter(|hit| {
                        pipeline
                            .as_deref()
                            .is_none_or(|p| hit["pipeline_tag"].as_str() == Some(p))
                    })
                    .take(50)
                    .collect();
                resp.json(200, &json!(hits), "application/json");
            } else if let Some(id) = path.strip_prefix("/api/models/") {
                let id = id.to_string();
                let answer = data
                    .repos
                    .get(&id)
                    .filter(|_| !data.missing.contains(&id))
                    .cloned()
                    .or_else(|| recorded_repo(&id).filter(|_| !data.missing.contains(&id)))
                    .or_else(|| {
                        // a repository only the list has: one file of a size of its own
                        data.list
                            .iter()
                            .filter(|_| !data.missing.contains(&id))
                            .find(|h| h["id"] == id.as_str())
                            .map(|hit| {
                                let name = id.rsplit('/').next().unwrap_or(&id);
                                repo_json(
                                    &id,
                                    hit["pipeline_tag"].as_str(),
                                    &[],
                                    &[(&format!("{name}-Q4_K_M.gguf"), 1_000_000)],
                                )
                            })
                    });
                match answer {
                    Some(value) => resp.json(200, &value, "application/json"),
                    None => {
                        resp.status(404);
                        resp.send(b"{\"error\":\"Repository not found\"}");
                    }
                }
            } else if let Some(rest) = path.strip_prefix('/') {
                // <owner>/<name>/resolve/<revision>/<path...>
                let parts: Vec<&str> = rest.splitn(5, '/').collect();
                if parts.len() == 5 && parts[2] == "resolve" {
                    let file = parts[4];
                    let length = data.lengths.get(file).copied().unwrap_or(2048) as usize;
                    let body = content(&format!("{}/{}", parts[0], parts[1]), file, length);
                    resp.header("Content-Type", "application/octet-stream");
                    resp.send(&body);
                } else {
                    resp.status(404);
                    resp.send(b"not found");
                }
            }
        })
        .expect("a port for the fake hub");
        FakeHub {
            server,
            asked,
            behaviour,
            state,
        }
    }

    /// `http://127.0.0.1:PORT`: what a document gives as `site`.
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.port())
    }

    pub fn requests(&self) -> Vec<Asked> {
        self.asked.lock().unwrap().clone()
    }

    /// The paths asked for, with their queries, in order.
    pub fn paths(&self) -> Vec<String> {
        self.requests().into_iter().map(|r| r.path).collect()
    }

    /// The files asked for (a path of `<owner>/<name>/resolve/main/<file>` as `<file>`), in order.
    pub fn downloads(&self) -> Vec<String> {
        self.paths()
            .into_iter()
            .filter_map(|p| {
                let parts: Vec<String> = p
                    .trim_start_matches('/')
                    .splitn(5, '/')
                    .map(str::to_string)
                    .collect();
                (parts.len() == 5 && parts[2] == "resolve")
                    .then(|| format!("{}/{}:{}", parts[0], parts[1], parts[4]))
            })
            .collect()
    }

    pub fn behave(&self, behaviour: Behaviour) {
        *self.behaviour.lock().unwrap() = behaviour;
    }

    /// Makes `ids` (recorded or added) the whole list, in this order.
    pub fn only(&self, ids: &[&str]) {
        let mut state = self.state.lock().unwrap();
        let mut list = Vec::new();
        for id in ids {
            let hit = state
                .list
                .iter()
                .find(|h| h["id"] == *id)
                .cloned()
                .unwrap_or_else(|| json!({"id": id, "downloads": 1, "likes": 0, "tags": ["gguf"]}));
            list.push(hit);
        }
        state.list = list;
    }

    /// A repository of the fake's own, added to the end of the list. The files it names are served
    /// with the lengths it gives.
    pub fn add_repo(&self, id: &str, pipeline: Option<&str>, tags: &[&str], files: &[(&str, u64)]) {
        let mut state = self.state.lock().unwrap();
        let mut hit = json!({"id": id, "downloads": 1, "likes": 0, "tags": tags});
        if let Some(pipeline) = pipeline {
            hit["pipeline_tag"] = json!(pipeline);
        }
        state.list.retain(|h| h["id"] != id);
        state.list.push(hit);
        for (name, size) in files {
            state.lengths.insert((*name).to_string(), *size);
        }
        state
            .repos
            .insert(id.to_string(), repo_json(id, pipeline, tags, files));
    }

    /// A repository of the fake's own whose files carry, in the answer, the SHA-256 of the bytes the fake
    /// serves for them, as the hub states it for the files it keeps in LFS: a download of one can be
    /// checked. The files are small (they are hashed here).
    pub fn add_checked_repo(
        &self,
        id: &str,
        pipeline: Option<&str>,
        tags: &[&str],
        files: &[(&str, u64)],
    ) {
        self.add_repo(id, pipeline, tags, files);
        let mut state = self.state.lock().unwrap();
        let answer = state.repos.get_mut(id).expect("the repository just added");
        for sibling in answer["siblings"].as_array_mut().unwrap() {
            let name = sibling["rfilename"].as_str().unwrap().to_string();
            let size = sibling["size"].as_u64().unwrap();
            assert!(size <= 16 << 20, "{name} is too big to be hashed here");
            sibling["lfs"] = json!({
                "sha256": sha256_hex(&content(id, &name, size as usize)),
                "size": size,
            });
        }
    }

    /// Makes the hub state, for a file of a repository added with `add_checked_repo`, a checksum that is
    /// not the one of the bytes it serves: a file that is not what the hub says it is.
    pub fn corrupt(&self, id: &str, file: &str) {
        let mut state = self.state.lock().unwrap();
        let answer = state
            .repos
            .get_mut(id)
            .expect("a repository the fake was given");
        let sibling = answer["siblings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|s| s["rfilename"] == file)
            .expect("a file of it");
        sibling["lfs"]["sha256"] = json!("0".repeat(64));
    }

    /// Makes the search of one pipeline (`text-to-video`) fail with 500, and the others go on.
    pub fn break_pipeline(&self, pipeline: &str) {
        self.state
            .lock()
            .unwrap()
            .broken_pipelines
            .insert(pipeline.to_string());
    }

    /// Makes the search of one pipeline answer with a page that is not JSON, and the others go on.
    pub fn garble_pipeline(&self, pipeline: &str) {
        self.state
            .lock()
            .unwrap()
            .garbled_pipelines
            .insert(pipeline.to_string());
    }

    /// Makes a repository one that is listed and not there when it is asked for (404).
    pub fn lose(&self, id: &str) {
        self.state.lock().unwrap().missing.insert(id.to_string());
    }

    /// Makes a repository one that needs a login.
    pub fn gate(&self, id: &str) {
        let mut state = self.state.lock().unwrap();
        let mut answer = state
            .repos
            .get(id)
            .cloned()
            .or_else(|| recorded_repo(id))
            .unwrap_or_else(|| repo_json(id, None, &[], &[("m-Q4_K_M.gguf", 10)]));
        answer["gated"] = json!("manual");
        state.repos.insert(id.to_string(), answer);
    }
}

/// The query of a request path as a map.
fn query_of(path: &str) -> BTreeMap<String, String> {
    let Some((_, query)) = path.split_once('?') else {
        return BTreeMap::new();
    };
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), decode(v)))
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> (u16, String) {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut reply = agent.get(url).call().unwrap();
        let status = reply.status().as_u16();
        (status, reply.body_mut().read_to_string().unwrap())
    }

    #[test]
    fn the_fake_serves_what_was_recorded_by_what_is_asked_and_says_what_it_was_asked() {
        let hub = FakeHub::start();
        let (status, list) = get(&format!(
            "{}/api/models?filter=gguf&sort=downloads&direction=-1&limit=50",
            hub.base()
        ));
        assert_eq!(status, 200);
        let hits: Vec<Value> = serde_json::from_str(&list).unwrap();
        assert_eq!(hits.len(), 50);
        assert_eq!(hits[0]["id"], "mudler/locate-anything.cpp-gguf");
        // words narrow the list to the repositories that have them all
        let (_, found) = get(&format!("{}/api/models?search=qwen%20coder", hub.base()));
        let ids: Vec<String> = serde_json::from_str::<Vec<Value>>(&found)
            .unwrap()
            .iter()
            .map(|h| h["id"].as_str().unwrap().to_string())
            .collect();
        assert!(
            !ids.is_empty()
                && ids.iter().all(
                    |i| i.to_lowercase().contains("qwen") && i.to_lowercase().contains("coder")
                ),
            "{ids:?}"
        );
        // a recorded repository, and one only the list has
        let (_, gemma) = get(&format!(
            "{}/api/models/ggml-org/gemma-3-4b-it-GGUF?blobs=true",
            hub.base()
        ));
        assert!(gemma.contains("mmproj-model-f16.gguf"));
        let (_, other) = get(&format!(
            "{}/api/models/ornith-ai/Ornith-1.5-9B-GGUF?blobs=true",
            hub.base()
        ));
        assert!(other.contains("Ornith-1.5-9B-GGUF-Q4_K_M.gguf"), "{other}");
        assert_eq!(
            get(&format!("{}/api/models/no/such?blobs=true", hub.base())).0,
            404
        );
        assert_eq!(hub.paths().len(), 5);
    }

    #[test]
    fn a_repository_of_the_fakes_own_is_listed_and_its_files_are_served_as_long_as_it_says() {
        let hub = FakeHub::start();
        hub.add_repo(
            "me/tiny-vl-GGUF",
            Some("image-text-to-text"),
            &["gguf"],
            &[("tiny-Q4_K_M.gguf", 3000), ("mmproj-tiny-f16.gguf", 1000)],
        );
        hub.only(&["me/tiny-vl-GGUF"]);
        let (_, list) = get(&format!("{}/api/models", hub.base()));
        assert_eq!(serde_json::from_str::<Vec<Value>>(&list).unwrap().len(), 1);
        let (status, weights) = get(&format!(
            "{}/me/tiny-vl-GGUF/resolve/main/tiny-Q4_K_M.gguf",
            hub.base()
        ));
        assert_eq!((status, weights.len()), (200, 3000));
        assert!(weights.starts_with("me/tiny-vl-GGUF:tiny-Q4_K_M.gguf|"));
        let (_, projector) = get(&format!(
            "{}/me/tiny-vl-GGUF/resolve/main/mmproj-tiny-f16.gguf",
            hub.base()
        ));
        assert_eq!(projector.len(), 1000);
        assert_eq!(
            hub.downloads(),
            [
                "me/tiny-vl-GGUF:tiny-Q4_K_M.gguf",
                "me/tiny-vl-GGUF:mmproj-tiny-f16.gguf"
            ]
        );
        hub.gate("me/tiny-vl-GGUF");
        let (_, answer) = get(&format!(
            "{}/api/models/me/tiny-vl-GGUF?blobs=true",
            hub.base()
        ));
        assert!(answer.contains("\"gated\":\"manual\""), "{answer}");
    }

    #[test]
    fn the_fake_can_fail() {
        let hub = FakeHub::start();
        hub.behave(Behaviour::Error(503));
        assert_eq!(get(&format!("{}/api/models", hub.base())).0, 503);
        hub.behave(Behaviour::Garbage);
        assert!(get(&format!("{}/api/models", hub.base()))
            .1
            .contains("maintenance"));
        hub.behave(Behaviour::RepositoriesFail);
        assert_eq!(get(&format!("{}/api/models", hub.base())).0, 200);
        assert_eq!(
            get(&format!(
                "{}/api/models/ggml-org/gemma-3-4b-it-GGUF",
                hub.base()
            ))
            .0,
            500
        );
        let hub = FakeHub::start();
        hub.behave(Behaviour::LimitAfter(1));
        assert_eq!(get(&format!("{}/api/models", hub.base())).0, 200);
        assert_eq!(get(&format!("{}/api/models", hub.base())).0, 429);
    }
}
