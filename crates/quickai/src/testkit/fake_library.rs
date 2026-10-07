//! A fake of the library at ollama.com: it serves the pages of the site's search that were recorded
//! (`tests/fixtures/ollama/library/`) by what is asked for, and keeps what it was asked. A fake of a
//! site is only as good as its likeness to it, so the pages are the site's own, trimmed.
//!
//! `/search?c=local` is the popular page, `?q=gpt&c=local` and `?q=embed&c=local` the pages of those
//! words, and any other words find nothing (the page of a search with no results).

use std::sync::{Arc, Mutex};

use super::mini_http::{MiniServer, Request};

/// One request the fake received.
#[derive(Clone, Debug)]
pub struct Asked {
    /// The path with its query, as sent.
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Asked {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// How the fake answers.
#[derive(Clone, Debug, Default)]
pub enum Behaviour {
    /// The recorded pages.
    #[default]
    Normal,
    /// The same status for every request, with a body that says so.
    Error(u16),
    /// A page that is not a page of the search.
    Garbage,
}

pub struct FakeLibrary {
    server: MiniServer,
    asked: Arc<Mutex<Vec<Asked>>>,
    behaviour: Arc<Mutex<Behaviour>>,
}

/// A page recorded from the site.
pub fn recorded_page(file: &str) -> Vec<u8> {
    std::fs::read(super::fixtures_dir().join("ollama/library").join(file))
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

impl FakeLibrary {
    pub fn start() -> FakeLibrary {
        let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
        let behaviour: Arc<Mutex<Behaviour>> = Arc::default();
        let (log, mode) = (asked.clone(), behaviour.clone());
        let server = MiniServer::start(None, move |req: &Request, resp| {
            log.lock().unwrap().push(Asked {
                path: req.path.clone(),
                headers: req.headers.clone(),
            });
            // what the real site does with every page
            resp.header("Set-Cookie", "aid=fake; Path=/; Max-Age=31536000; HttpOnly");
            let behaviour = mode.lock().unwrap().clone();
            match behaviour {
                Behaviour::Error(code) => {
                    resp.status(code);
                    resp.send(format!("error {code}").as_bytes());
                }
                Behaviour::Garbage => {
                    resp.header("Content-Type", "text/html; charset=utf-8");
                    resp.send(b"<html><body><h1>Down for maintenance</h1></body></html>");
                }
                Behaviour::Normal => {
                    resp.header("Content-Type", "text/html; charset=utf-8");
                    let file = match req.path.as_str() {
                        "/search?c=local" => "popular_local.html",
                        "/search?q=gpt&c=local" => "gpt_local.html",
                        "/search?q=embed&c=local" => "embed_local.html",
                        _ => "none_local.html",
                    };
                    resp.send(&recorded_page(file));
                }
            }
        })
        .expect("a port for the fake library");
        FakeLibrary {
            server,
            asked,
            behaviour,
        }
    }

    /// `http://127.0.0.1:PORT`: what a document gives as `site`.
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.port())
    }

    pub fn requests(&self) -> Vec<Asked> {
        self.asked.lock().unwrap().clone()
    }

    /// The paths asked for, in order.
    pub fn paths(&self) -> Vec<String> {
        self.requests().into_iter().map(|r| r.path).collect()
    }

    pub fn behave(&self, behaviour: Behaviour) {
        *self.behaviour.lock().unwrap() = behaviour;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_serves_the_recorded_pages_by_what_is_asked_and_says_what_it_was_asked() {
        let fake = FakeLibrary::start();
        let get = |path: &str| {
            ureq::get(format!("{}{path}", fake.base()))
                .call()
                .unwrap()
                .into_body()
                .read_to_string()
                .unwrap()
        };
        assert!(get("/search?c=local").contains("clef-flash"));
        assert!(get("/search?q=gpt&c=local").contains("gpt-oss"));
        assert!(get("/search?q=embed&c=local").contains("nomic-embed-text"));
        assert!(get("/search?q=zzzz&c=local").contains("No models found"));
        assert_eq!(
            fake.paths(),
            [
                "/search?c=local",
                "/search?q=gpt&c=local",
                "/search?q=embed&c=local",
                "/search?q=zzzz&c=local"
            ]
        );
        fake.behave(Behaviour::Garbage);
        assert!(get("/search?c=local").contains("maintenance"));
        fake.behave(Behaviour::Error(503));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let reply = agent.get(format!("{}/x", fake.base())).call().unwrap();
        assert_eq!(reply.status().as_u16(), 503);
    }
}
