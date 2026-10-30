//! A fake of EverybodyAI's telemetry API (`api.everybodyai.dev/v1`, docs/reports-and-community.md section 9): it
//! records every request and answers with the statuses a test lines up (202 when none is lined up).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A request the fake received.
#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    pub path: String,
    /// The `QuickAI-Install` header.
    pub install: Option<String>,
    pub user_agent: Option<String>,
    pub body: String,
}

/// An answer to give: a status, a `Retry-After`, and a body.
#[derive(Clone, Debug)]
pub struct Reply {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: String,
}

impl Reply {
    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            retry_after: None,
            body: String::new(),
        }
    }
}

pub struct FakeApi {
    pub url: String,
    received: Arc<Mutex<Vec<Received>>>,
    replies: Arc<Mutex<VecDeque<Reply>>>,
    stop: Arc<AtomicBool>,
}

impl FakeApi {
    pub fn start() -> FakeApi {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let received: Arc<Mutex<Vec<Received>>> = Arc::default();
        let replies: Arc<Mutex<VecDeque<Reply>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (r, q, s) = (received.clone(), replies.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                let header = |name: &str| {
                    request
                        .headers()
                        .iter()
                        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
                        .map(|h| h.value.as_str().to_string())
                };
                let (install, user_agent) = (header("QuickAI-Install"), header("user-agent"));
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                r.lock().unwrap().push(Received {
                    method: request.method().as_str().to_string(),
                    path: request.url().to_string(),
                    install,
                    user_agent,
                    body,
                });
                let reply = q.lock().unwrap().pop_front().unwrap_or(Reply::status(202));
                let mut response =
                    tiny_http::Response::from_string(reply.body).with_status_code(reply.status);
                if let Some(after) = reply.retry_after {
                    response.add_header(
                        tiny_http::Header::from_bytes("Retry-After", after.to_string()).unwrap(),
                    );
                }
                let _ = request.respond(response);
            }
        });
        FakeApi {
            url: format!("http://127.0.0.1:{port}"),
            received,
            replies,
            stop,
        }
    }

    /// Lines up the answer to the next request.
    pub fn reply(&self, reply: Reply) {
        self.replies.lock().unwrap().push_back(reply);
    }

    /// The requests so far.
    pub fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }
}

impl Drop for FakeApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
