//! A small HTTP/1.1 server for fakes that must misbehave on purpose: it hands the handler the
//! socket's own pace, so a reply can be written a chunk at a time with pauses, cut off in the
//! middle, or never sent. `tiny_http` (which the other tests use) buffers a streamed body and
//! hides the connection, which is what makes it unsuitable here. One request per connection,
//! always `Connection: close`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct Request {
    pub method: String,
    /// The path with its query, as sent.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }

    /// The path without its query.
    pub fn route(&self) -> &str {
        self.path.split('?').next().unwrap_or(&self.path)
    }
}

pub struct Response<'a> {
    stream: &'a mut TcpStream,
    status: u16,
    headers: Vec<(String, String)>,
    head_sent: bool,
    chunked: bool,
    head_request: bool,
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

impl Response<'_> {
    pub fn status(&mut self, code: u16) -> &mut Self {
        self.status = code;
        self
    }

    pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    fn send_head(&mut self, length: Option<usize>) -> std::io::Result<()> {
        if self.head_sent {
            return Ok(());
        }
        self.head_sent = true;
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        for (k, v) in &self.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        match length {
            Some(n) => head.push_str(&format!("Content-Length: {n}\r\n")),
            None => {
                self.chunked = true;
                head.push_str("Transfer-Encoding: chunked\r\n");
            }
        }
        head.push_str("Connection: close\r\n\r\n");
        self.stream.write_all(head.as_bytes())?;
        self.stream.flush()
    }

    /// A whole reply with a stated length.
    pub fn send(&mut self, body: &[u8]) {
        let _ = self.send_head(Some(body.len()));
        if !self.head_request {
            let _ = self.stream.write_all(body);
        }
        let _ = self.stream.flush();
    }

    pub fn json(&mut self, status: u16, value: &serde_json::Value, content_type: &str) {
        self.status(status).header("Content-Type", content_type);
        self.send(value.to_string().as_bytes());
    }

    /// Starts a reply of no stated length; each `chunk` is sent and flushed at once.
    pub fn begin_stream(&mut self) {
        let _ = self.send_head(None);
    }

    pub fn chunk(&mut self, data: &[u8]) -> std::io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.send_head(None)?;
        self.stream
            .write_all(format!("{:x}\r\n", data.len()).as_bytes())?;
        self.stream.write_all(data)?;
        self.stream.write_all(b"\r\n")?;
        self.stream.flush()
    }

    pub fn end_stream(&mut self) {
        let _ = self.send_head(None);
        let _ = self.stream.write_all(b"0\r\n\r\n");
        let _ = self.stream.flush();
    }

    /// Hangs up: no terminating chunk, no further bytes (an engine that died mid-reply).
    pub fn abort(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    /// Whether the client has gone (a write fails, or a read says end of stream).
    pub fn client_gone(&mut self) -> bool {
        self.stream.set_nonblocking(true).ok();
        let mut b = [0u8; 1];
        let gone = matches!(self.stream.read(&mut b), Ok(0))
            || matches!(self.stream.read(&mut b), Err(e) if e.kind() != std::io::ErrorKind::WouldBlock);
        self.stream.set_nonblocking(false).ok();
        gone
    }
}

type Handler = dyn Fn(&Request, &mut Response) + Send + Sync;

pub struct MiniServer {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl MiniServer {
    /// Listens on `port` (any free one for `None`). `Err` when the port is taken.
    pub fn start(
        port: Option<u16>,
        handler: impl Fn(&Request, &mut Response) + Send + Sync + 'static,
    ) -> std::io::Result<MiniServer> {
        let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let flag = stop.clone();
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = handler.clone();
                        std::thread::spawn(move || serve(stream, &*handler));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
            // the listener is dropped here: the port is free again
        });
        Ok(MiniServer { port, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stops accepting; the port is free a moment later. A reply being written is not interrupted.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for MiniServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve(mut stream: TcpStream, handler: &Handler) {
    // On Windows and macOS a connection takes the listener's non-blocking mode (Linux's does not):
    // the first read of the head would find nothing yet and drop the client.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let mut response = Response {
        stream: &mut stream,
        status: 200,
        headers: Vec::new(),
        head_sent: false,
        chunked: false,
        head_request: request.method == "HEAD",
    };
    handler(&request, &mut response);
    if !response.head_sent {
        // a handler that said nothing gets an empty 200 rather than a hung client
        response.send(b"");
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    // the head, a byte at a time: simple, and a request is small
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => buf.push(byte[0]),
            _ => return None,
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let find = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    if find("expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let mut body = Vec::new();
    if let Some(len) = find("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.resize(len.min(8 * 1024 * 1024), 0);
        stream.read_exact(&mut body).ok()?;
    }
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_reply_with_a_length_a_streamed_one_and_a_dropped_one() {
        let server = MiniServer::start(None, |req, resp| match req.route() {
            "/whole" => {
                resp.header("X-Test", "yes");
                resp.send(b"hello");
            }
            "/stream" => {
                resp.begin_stream();
                for i in 0..3 {
                    resp.chunk(format!("line {i}\n").as_bytes()).unwrap();
                    std::thread::sleep(Duration::from_millis(80));
                }
                resp.end_stream();
            }
            "/cut" => {
                resp.begin_stream();
                let _ = resp.chunk(b"partial\n");
                resp.abort();
            }
            "/echo" => {
                let n = req.body.len();
                resp.send(
                    format!(
                        "{} {} {}",
                        req.method,
                        n,
                        req.header("content-type").unwrap_or("-")
                    )
                    .as_bytes(),
                );
            }
            _ => {
                resp.status(404);
                resp.send(b"nope");
            }
        })
        .unwrap();
        let base = format!("http://127.0.0.1:{}", server.port());

        let r = ureq::get(format!("{base}/whole")).call().unwrap();
        assert_eq!(r.headers().get("x-test").unwrap(), "yes");
        assert_eq!(r.into_body().read_to_string().unwrap(), "hello");

        // chunks arrive as they are sent, not all at the end
        let r = ureq::get(format!("{base}/stream")).call().unwrap();
        let mut body = r.into_body().into_reader();
        let started = Instant::now();
        let mut first = [0u8; 7];
        body.read_exact(&mut first).unwrap();
        assert_eq!(&first, b"line 0\n");
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "the first chunk waited for the whole reply"
        );
        let mut rest = String::new();
        body.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "line 1\nline 2\n");

        // a reply cut off in the middle is an error to the client, not a clean end
        let r = ureq::get(format!("{base}/cut")).call().unwrap();
        let mut text = String::new();
        let outcome = r.into_body().into_reader().read_to_string(&mut text);
        assert!(
            outcome.is_err(),
            "a cut reply was read as complete: {text:?}"
        );

        let r = ureq::post(format!("{base}/echo"))
            .header("Content-Type", "application/json")
            .send(r#"{"a":1}"#)
            .unwrap();
        assert_eq!(
            r.into_body().read_to_string().unwrap(),
            "POST 7 application/json"
        );
        assert!(matches!(
            ureq::get(format!("{base}/other")).call(),
            Err(ureq::Error::StatusCode(404))
        ));
    }

    #[test]
    fn shutdown_frees_the_port() {
        let server = MiniServer::start(None, |_, r| r.send(b"x")).unwrap();
        let port = server.port();
        assert!(MiniServer::start(Some(port), |_, r| r.send(b"x")).is_err());
        server.shutdown();
        crate::testkit::wait_until(Duration::from_secs(3), "the port to be free", || {
            TcpListener::bind(("127.0.0.1", port)).is_ok()
        });
    }
}
