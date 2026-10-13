//! A stand-in for GitHub's release API, serving the recorded release list of the real Ollama
//! repository (`tests/fixtures/ollama/github_releases.json`: four releases, the newest of them a
//! release candidate listed *above* the stable one, as the real feed lists it) or whatever a test
//! gives it, and failing the ways the real one fails: a rate limit, a server error, garbage.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::mini_http::{MiniServer, Request, Response};

#[derive(Clone, Debug)]
pub enum Mode {
    Normal,
    /// 403 with the headers GitHub sends when an address has used its 60 anonymous requests.
    RateLimited {
        reset_epoch: u64,
    },
    /// This status and `{"message": "..."}`.
    Status(u16),
    /// 200 and a body that is not JSON.
    Garbage,
}

struct State {
    releases: Value,
    mode: Mode,
    log: Vec<(String, Vec<(String, String)>)>,
}

pub struct FakeGithub {
    server: MiniServer,
    state: Arc<Mutex<State>>,
}

/// The recorded real release list.
pub fn recorded_releases() -> Value {
    let path = super::fixtures_dir()
        .join("ollama")
        .join("github_releases.json");
    serde_json::from_slice(&std::fs::read(&path).expect("the recorded release list")).unwrap()
}

/// Points every `browser_download_url` of `releases` at `base` instead of github.com, keeping the
/// rest of the path, so a test can serve the files the feed names.
pub fn with_download_base(releases: &Value, base: &str) -> Value {
    let text = releases.to_string().replace("https://github.com", base);
    serde_json::from_str(&text).unwrap()
}

impl FakeGithub {
    pub fn with_releases(releases: Value) -> FakeGithub {
        let state = Arc::new(Mutex::new(State {
            releases,
            mode: Mode::Normal,
            log: Vec::new(),
        }));
        let shared = state.clone();
        let server = MiniServer::start(None, move |req, resp| handle(&shared, req, resp))
            .expect("a free port");
        FakeGithub { server, state }
    }

    /// The recorded Ollama feed.
    pub fn ollama() -> FakeGithub {
        Self::with_releases(recorded_releases())
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.port())
    }

    pub fn mode(&self, mode: Mode) -> &Self {
        self.state.lock().unwrap().mode = mode;
        self
    }

    /// `(path with query, headers)` of every request, in order.
    pub fn requests(&self) -> Vec<(String, Vec<(String, String)>)> {
        self.state.lock().unwrap().log.clone()
    }
}

fn handle(shared: &Arc<Mutex<State>>, req: &Request, resp: &mut Response) {
    let (releases, mode) = {
        let mut state = shared.lock().unwrap();
        state.log.push((req.path.clone(), req.headers.clone()));
        (state.releases.clone(), state.mode.clone())
    };
    match mode {
        Mode::RateLimited { reset_epoch } => {
            resp.header("x-ratelimit-limit", "60")
                .header("x-ratelimit-remaining", "0")
                .header("x-ratelimit-used", "60")
                .header("x-ratelimit-resource", "core")
                .header("x-ratelimit-reset", &reset_epoch.to_string());
            return resp.json(
                403,
                &json!({
                    "message": "API rate limit exceeded for 203.0.113.9. (But here's the good news: Authenticated requests get a higher rate limit. Check out the documentation for more details.)",
                    "documentation_url": "https://docs.github.com/rest/overview/resources-in-the-rest-api#rate-limiting"
                }),
                "application/json; charset=utf-8",
            );
        }
        Mode::Status(status) => {
            return resp.json(
                status,
                &json!({"message": "Server Error"}),
                "application/json; charset=utf-8",
            );
        }
        Mode::Garbage => {
            resp.status(200).header("Content-Type", "application/json");
            return resp.send(b"this is not json");
        }
        Mode::Normal => {}
    }

    let route = req.route();
    let mut parts = route.trim_start_matches('/').split('/');
    let (head, _owner, _repo, kind, tag) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.nth(1),
    );
    if head != Some("repos") || kind != Some("releases") {
        return resp.json(
            404,
            &json!({"message": "Not Found"}),
            "application/json; charset=utf-8",
        );
    }
    let all = releases.as_array().cloned().unwrap_or_default();
    match tag {
        Some(tag) => match all.iter().find(|r| r["tag_name"] == tag) {
            Some(r) => resp.json(200, r, "application/json; charset=utf-8"),
            None => resp.json(
                404,
                &json!({"message": "Not Found"}),
                "application/json; charset=utf-8",
            ),
        },
        None => {
            let per_page = req
                .path
                .split_once('?')
                .and_then(|(_, q)| q.split('&').find_map(|kv| kv.strip_prefix("per_page=")))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(30);
            let page: Vec<Value> = all.into_iter().take(per_page).collect();
            resp.json(200, &Value::Array(page), "application/json; charset=utf-8");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::http::get;

    #[test]
    fn it_serves_the_recorded_feed_newest_first_with_the_candidate_above_the_stable_release() {
        let fake = FakeGithub::ollama();
        let r = get(&format!(
            "{}/repos/ollama/ollama/releases?per_page=10",
            fake.base()
        ));
        assert_eq!(r.status, 200);
        let list = r.json();
        let list = list.as_array().unwrap();
        let tags: Vec<&str> = list
            .iter()
            .map(|r| r["tag_name"].as_str().unwrap())
            .collect();
        assert_eq!(tags, ["v0.40.0-rc5", "v0.35.1", "v0.35.0", "v0.34.4"]);
        assert_eq!(list[0]["prerelease"], true);
        assert_eq!(list[1]["prerelease"], false);
        assert_eq!(list[1]["assets"].as_array().unwrap().len(), 17);
        assert!(list[1]["assets"][0]["digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        // per_page is honoured, and one release can be asked for by tag
        let r = get(&format!(
            "{}/repos/ollama/ollama/releases?per_page=2",
            fake.base()
        ));
        assert_eq!(r.json().as_array().unwrap().len(), 2);
        let r = get(&format!(
            "{}/repos/ollama/ollama/releases/tags/v0.35.0",
            fake.base()
        ));
        assert_eq!(r.status, 200);
        assert_eq!(r.json()["tag_name"], "v0.35.0");
        assert_eq!(
            get(&format!(
                "{}/repos/ollama/ollama/releases/tags/v9",
                fake.base()
            ))
            .status,
            404
        );
        assert_eq!(fake.requests().len(), 4);
    }

    #[test]
    fn download_urls_can_be_pointed_at_a_local_file_server() {
        let moved = with_download_base(&recorded_releases(), "http://127.0.0.1:9");
        let url = moved[1]["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == "ollama-linux-amd64.tar.zst")
            .unwrap()["browser_download_url"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            url,
            "http://127.0.0.1:9/ollama/ollama/releases/download/v0.35.1/ollama-linux-amd64.tar.zst"
        );
    }

    #[test]
    fn it_fails_the_ways_the_real_api_fails() {
        let fake = FakeGithub::ollama();
        fake.mode(Mode::RateLimited {
            reset_epoch: 1_791_299_136,
        });
        let r = get(&format!("{}/repos/ollama/ollama/releases", fake.base()));
        assert_eq!(r.status, 403);
        assert_eq!(r.header("x-ratelimit-remaining"), Some("0"));
        assert_eq!(r.header("x-ratelimit-reset"), Some("1791299136"));
        assert!(r.body.contains("rate limit exceeded"));
        fake.mode(Mode::Status(502));
        assert_eq!(
            get(&format!("{}/repos/x/y/releases", fake.base())).status,
            502
        );
        fake.mode(Mode::Garbage);
        assert_eq!(
            get(&format!("{}/repos/x/y/releases", fake.base())).body,
            "this is not json"
        );
    }
}
