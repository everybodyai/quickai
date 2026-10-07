//! A server of files for the installer tests: whatever bytes are registered under a path, with the
//! length stated as a real server states it, and a count of what was asked.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct FileServer {
    base: String,
    hits: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    stop: Arc<AtomicBool>,
}

/// A reader that gives a file out slowly, so a test can act while a download is under way.
struct Slow {
    data: Vec<u8>,
    at: usize,
    chunk: usize,
    pause: Duration,
}

impl Read for Slow {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.at >= self.data.len() {
            return Ok(0);
        }
        std::thread::sleep(self.pause);
        let n = self.chunk.min(out.len()).min(self.data.len() - self.at);
        out[..n].copy_from_slice(&self.data[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

impl FileServer {
    pub fn start() -> FileServer {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let files: Arc<Mutex<BTreeMap<String, Vec<u8>>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (h, p, f, s) = (hits.clone(), paths.clone(), files.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                h.fetch_add(1, Ordering::Relaxed);
                let url = request.url().to_string();
                p.lock().unwrap().push(url.clone());
                let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
                let body = f.lock().unwrap().get(path).cloned();
                match body {
                    None => {
                        let _ = request.respond(
                            tiny_http::Response::from_string("not found").with_status_code(404),
                        );
                    }
                    Some(data) => {
                        // `?kbps=N` hands the file out at about N kilobytes a second
                        let kbps = query
                            .split('&')
                            .find_map(|kv| kv.strip_prefix("kbps="))
                            .and_then(|v| v.parse::<usize>().ok());
                        let len = data.len();
                        match kbps {
                            Some(kbps) => {
                                let chunk = 4096;
                                let pause = Duration::from_micros(
                                    (chunk as u64 * 1_000_000) / (kbps as u64 * 1024).max(1),
                                );
                                let reader = Slow {
                                    data,
                                    at: 0,
                                    chunk,
                                    pause,
                                };
                                let _ = request.respond(tiny_http::Response::new(
                                    tiny_http::StatusCode(200),
                                    vec![],
                                    reader,
                                    Some(len),
                                    None,
                                ));
                            }
                            None => {
                                let _ = request.respond(
                                    tiny_http::Response::from_data(data)
                                        .with_chunked_threshold(usize::MAX),
                                );
                            }
                        }
                    }
                }
            }
        });
        FileServer {
            base: format!("http://127.0.0.1:{port}"),
            hits,
            paths,
            files,
            stop,
        }
    }

    /// Serves `bytes` at `path` (`/ollama-linux-amd64.tar.zst`).
    pub fn add(&self, path: &str, bytes: Vec<u8>) -> &Self {
        self.files.lock().unwrap().insert(path.to_string(), bytes);
        self
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn requests(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn paths_asked(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
}

impl Drop for FileServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> (u16, Vec<u8>, Option<u64>) {
        let resp = ureq::get(url).call();
        match resp {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = r.into_body();
                let len = body.content_length();
                (
                    status,
                    body.into_with_config()
                        .limit(u64::MAX)
                        .read_to_vec()
                        .unwrap(),
                    len,
                )
            }
            Err(ureq::Error::StatusCode(code)) => (code, Vec::new(), None),
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn it_serves_what_was_added_with_its_length_and_counts_requests() {
        let server = FileServer::start();
        server.add("/a.bin", vec![7u8; 100_000]);
        let (status, body, len) = get(&server.url("/a.bin"));
        assert_eq!((status, body.len(), len), (200, 100_000, Some(100_000)));
        assert_eq!(get(&server.url("/missing")).0, 404);
        assert_eq!(server.requests(), 2);
        assert_eq!(server.paths_asked(), ["/a.bin", "/missing"]);
    }

    #[test]
    fn a_slow_file_takes_about_as_long_as_its_rate_says() {
        let server = FileServer::start();
        server.add("/s.bin", vec![1u8; 64 * 1024]);
        let started = std::time::Instant::now();
        let (status, body, _) = get(&server.url("/s.bin?kbps=256"));
        let took = started.elapsed();
        assert_eq!((status, body.len()), (200, 64 * 1024));
        assert!(
            took >= Duration::from_millis(150),
            "64 KB at 256 KB/s should take about 250 ms, took {took:?}"
        );
    }
}
