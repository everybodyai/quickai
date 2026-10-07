//! A client for tests: every status is an answer, never an error, and the body of an error answer
//! is kept (`ureq` by default turns a 4xx or 5xx into an error and drops what the server said).

use serde_json::Value;

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.body))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into()
}

fn read(response: ureq::http::Response<ureq::Body>) -> Reply {
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = response.into_body().read_to_string().unwrap_or_default();
    Reply {
        status,
        headers,
        body,
    }
}

pub fn get(url: &str) -> Reply {
    read(
        agent()
            .get(url)
            .header("User-Agent", "quickai-test")
            .call()
            .unwrap_or_else(|e| panic!("GET {url}: {e}")),
    )
}

pub fn post_json(url: &str, body: &Value) -> Reply {
    read(
        agent()
            .post(url)
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .unwrap_or_else(|e| panic!("POST {url}: {e}")),
    )
}

pub fn delete_json(url: &str, body: &Value) -> Reply {
    read(
        agent()
            .delete(url)
            .header("Content-Type", "application/json")
            .force_send_body()
            .send(body.to_string())
            .unwrap_or_else(|e| panic!("DELETE {url}: {e}")),
    )
}

/// `Err` when nothing answered (connection refused, reset, timed out).
pub fn try_get(url: &str) -> Result<Reply, String> {
    agent()
        .get(url)
        .header("User-Agent", "quickai-test")
        .call()
        .map(read)
        .map_err(|e| e.to_string())
}

/// Any method with an optional raw body (sent as JSON); how a recorded request is replayed.
pub fn send(method: &str, url: &str, body: Option<&str>) -> Reply {
    match (method, body) {
        ("GET", _) => get(url),
        ("POST", Some(b)) => read(
            agent()
                .post(url)
                .header("Content-Type", "application/json")
                .send(b.to_string())
                .unwrap_or_else(|e| panic!("POST {url}: {e}")),
        ),
        ("DELETE", Some(b)) => read(
            agent()
                .delete(url)
                .header("Content-Type", "application/json")
                .force_send_body()
                .send(b.to_string())
                .unwrap_or_else(|e| panic!("DELETE {url}: {e}")),
        ),
        (m, b) => panic!("send does not do {m} with body {b:?}"),
    }
}
