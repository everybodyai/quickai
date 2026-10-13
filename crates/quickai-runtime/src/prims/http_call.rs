//! `http_call@1`: one HTTP request to the engine the unit started or attached to, with a JSON
//! body built from a document's table and the reply read as JSON, or as a stream of events
//! (docs/architecture.md, section 8; docs/phase3-test-plan.md, group H). The rules (plan decision C30):
//!
//! - a call goes to the unit's own endpoint and nowhere else: the host is this machine and a
//!   path is a plain path (it starts with `/` and holds no `//`, `..`, whitespace or scheme);
//!   a redirect is never followed;
//! - an error reply is `Http` with the engine's own sentence (`{"error": "..."}`) when it has one;
//! - in a stream, an event with an `error` member is a failure even though the HTTP status was 200
//!   (a real pull that cannot reach its registry answers exactly so);
//! - events are handed over as they arrive, a stream that goes quiet for longer than the timeout
//!   has failed, one that ends short is an error and not a quiet end, and cancelling closes the
//!   connection so that the engine stops working on it.
//!
//! The client is small on purpose and written here: the engine is a program on this machine that
//! speaks plain HTTP/1.1, and what the calls need (an idle timeout, a cancel that closes the socket,
//! no redirects, the first bytes of a stream the moment they arrive) is what a general client
//! hides.
//!
//! The session starts using it for the warm-up and unload of a daemon (work package 3.4) and the
//! model list and pull use it after that (3.13); until then only the tests call it.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use quickai_core::plan::Endpoint;
use quickai_core::stream::{sentence, NdjsonSplitter, SseSplitter};
use serde_json::Value;

/// How the reply is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    /// One JSON document.
    Json,
    /// One JSON object per line.
    Ndjson,
    /// Server-sent events whose `data:` is JSON.
    Sse,
}

#[derive(Clone, Debug)]
pub struct Call {
    pub method: String,
    pub path: String,
    pub body: Option<Value>,
    pub framing: Framing,
    /// The whole call for `Json`; for a stream, how long it may be silent.
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    /// `Null` for an empty body.
    pub body: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// The call itself is not allowed; nothing was sent.
    Refused(String),
    /// Nothing answered.
    Unreachable(String),
    TimedOut,
    /// The engine answered with an error status; `message` is its own sentence when it gave one.
    Http {
        status: u16,
        message: String,
    },
    /// The reply was not what the call needs: not JSON, ended early, or an event said `error`.
    Failed(String),
    Cancelled,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Refused(why) | CallError::Unreachable(why) | CallError::Failed(why) => {
                f.write_str(why)
            }
            CallError::TimedOut => f.write_str("the engine did not answer in time"),
            CallError::Http { message, .. } => f.write_str(message),
            CallError::Cancelled => f.write_str("cancelled"),
        }
    }
}

/// Asked every few milliseconds by a call that is waiting: has it been called off?
pub trait CancelCheck {
    fn cancelled(&self) -> bool;
}

impl CancelCheck for AtomicBool {
    fn cancelled(&self) -> bool {
        self.load(Ordering::SeqCst)
    }
}

impl CancelCheck for std::sync::Arc<AtomicBool> {
    fn cancelled(&self) -> bool {
        self.load(Ordering::SeqCst)
    }
}

/// A cancel that is a question to ask (has the run been told to stop?) rather than a flag.
pub struct CancelWhen<F: Fn() -> bool>(pub F);

impl<F: Fn() -> bool> CancelCheck for CancelWhen<F> {
    fn cancelled(&self) -> bool {
        (self.0)()
    }
}

/// How often a wait looks up to see whether it was cancelled or is out of time.
const TICK: Duration = Duration::from_millis(40);
const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// One request, one JSON reply. Any status but a success (2xx) is an `Err(Http)`: a redirect is
/// not followed and so is not an answer.
pub fn call(endpoint: &Endpoint, call: &Call) -> Result<Reply, CallError> {
    call_cancellable(endpoint, call, &AtomicBool::new(false))
}

/// `call`, which gives up with `Cancelled` (and closes the connection) when `cancel` says so.
pub fn call_cancellable(
    endpoint: &Endpoint,
    call: &Call,
    cancel: &dyn CancelCheck,
) -> Result<Reply, CallError> {
    let deadline = Instant::now() + call.timeout;
    let mut wire = Wire::open(endpoint, call, Some(deadline), cancel)?;
    let status = wire.head.status;
    let mut body = Vec::new();
    while let Some(piece) = wire.next_piece()? {
        if body.len() + piece.len() > MAX_BODY_BYTES {
            return Err(CallError::Failed("the reply is too large".into()));
        }
        body.extend_from_slice(&piece);
    }
    if !(200..300).contains(&status) {
        return Err(CallError::Http {
            status,
            message: engine_sentence(status, &body),
        });
    }
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Reply {
            status,
            body: Value::Null,
        });
    }
    serde_json::from_slice(&body)
        .map(|body| Reply { status, body })
        .map_err(|e| CallError::Failed(format!("the reply is not JSON: {e}")))
}

/// One request, a stream of events handed to `on_event` as they arrive. Returns when the stream
/// ends, `cancel` is set, or it fails.
pub fn stream(
    endpoint: &Endpoint,
    call: &Call,
    cancel: &dyn CancelCheck,
    on_event: &mut dyn FnMut(Value),
) -> Result<(), CallError> {
    let mut wire = Wire::open(endpoint, call, None, cancel)?;
    let status = wire.head.status;
    if !(200..300).contains(&status) {
        let mut body = Vec::new();
        while let Some(piece) = wire.next_piece()? {
            body.extend_from_slice(&piece);
            if body.len() > 1024 * 1024 {
                break;
            }
        }
        return Err(CallError::Http {
            status,
            message: engine_sentence(status, &body),
        });
    }

    // an event with an `error` member is a failure, whatever the HTTP status was
    let deliver = |value: Value, on_event: &mut dyn FnMut(Value)| -> Result<(), CallError> {
        if let Some(error) = value.get("error").filter(|e| !e.is_null()) {
            return Err(CallError::Failed(sentence(error)));
        }
        on_event(value);
        Ok(())
    };

    let mut lines = NdjsonSplitter::new();
    let mut events = SseSplitter::new();
    // reads what arrived; `true` when the stream said it is over (`[DONE]`)
    let mut handle = |bytes: &[u8], finish: bool| -> Result<bool, CallError> {
        match call.framing {
            Framing::Ndjson | Framing::Json => {
                let mut parsed = lines.push(bytes);
                if finish {
                    parsed.extend(lines.finish());
                }
                for line in parsed {
                    match line {
                        Ok(value) => deliver(value, on_event)?,
                        Err(e) => {
                            return Err(CallError::Failed(format!(
                                "line {} of the reply is not JSON: {}",
                                e.line, e.message
                            )))
                        }
                    }
                }
            }
            Framing::Sse => {
                let mut parsed = events.push(bytes);
                if finish {
                    parsed.extend(events.finish());
                }
                for event in parsed {
                    if event.data.trim() == "[DONE]" {
                        return Ok(true);
                    }
                    match serde_json::from_str::<Value>(&event.data) {
                        Ok(value) => deliver(value, on_event)?,
                        Err(e) => {
                            return Err(CallError::Failed(format!(
                                "an event of the reply is not JSON: {e}"
                            )))
                        }
                    }
                }
            }
        }
        Ok(false)
    };
    while let Some(piece) = wire.next_piece()? {
        if handle(&piece, false)? {
            return Ok(());
        }
    }
    handle(&[], true)?;
    Ok(())
}

/// The engine's own words for what went wrong, or the status when it said none.
fn engine_sentence(status: u16, body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").cloned())
        .filter(|e| !e.is_null())
        .map(|e| sentence(&e))
        .unwrap_or_else(|| format!("http status: {status}"))
}

/// Whether `path` is a plain path on the engine: nothing that could aim the request elsewhere.
fn path_is_plain(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains("//")
        && !path.contains("://")
        && !path.split('/').any(|part| part == "..")
        && path.chars().all(|c| c.is_ascii_graphic())
}

/// The address of the engine: only this machine, whatever the document says.
fn loopback(endpoint: &Endpoint) -> Result<SocketAddr, CallError> {
    let port = endpoint
        .port
        .ok_or_else(|| CallError::Refused("the engine has no port to call".into()))?;
    let ip: IpAddr = if endpoint.host.eq_ignore_ascii_case("localhost") {
        IpAddr::from([127, 0, 0, 1])
    } else {
        endpoint
            .host
            .trim_matches(['[', ']'])
            .parse()
            .map_err(|_| CallError::Refused(format!("`{}` is not an address", endpoint.host)))?
    };
    if !ip.is_loopback() {
        return Err(CallError::Refused(format!(
            "a call goes only to the engine on this machine, not to {ip}"
        )));
    }
    Ok(SocketAddr::new(ip, port))
}

/// How the body of a reply is delimited.
#[derive(Clone, Copy)]
enum Body {
    /// Exactly this many more bytes.
    Length(u64),
    Chunked(ChunkState),
    /// Until the server closes the connection.
    UntilClose,
    None,
}

#[derive(Clone, Copy)]
enum ChunkState {
    /// Expecting a chunk-size line.
    Size,
    /// This many bytes of the chunk to come, then its CRLF.
    Data(u64),
    /// The CRLF that ends a chunk's data.
    DataEnd,
    /// After the last chunk: trailers up to a blank line.
    Trailer,
    Done,
}

struct Head {
    status: u16,
}

/// A connection with a request sent and the head of its reply read.
struct Wire<'a> {
    stream: TcpStream,
    cancel: &'a dyn CancelCheck,
    /// Bytes read from the socket and not yet used.
    buffer: Vec<u8>,
    head: Head,
    body: Body,
    /// Until when the whole call may take (`call`), or `None` for the idle limit of a stream.
    deadline: Option<Instant>,
    idle: Duration,
    last_data: Instant,
}

impl<'a> Wire<'a> {
    /// Connects, sends the request and reads the head of the reply. Nothing is sent when the call is
    /// refused.
    fn open(
        endpoint: &Endpoint,
        call: &Call,
        deadline: Option<Instant>,
        cancel: &'a dyn CancelCheck,
    ) -> Result<Wire<'a>, CallError> {
        let address = loopback(endpoint)?;
        let method = call.method.to_ascii_uppercase();
        if !["GET", "POST", "PUT", "DELETE", "PATCH"].contains(&method.as_str()) {
            return Err(CallError::Refused(format!(
                "`{}` is not a method a call uses",
                call.method
            )));
        }
        if !path_is_plain(&call.path) {
            return Err(CallError::Refused(format!(
                "`{}` is not a plain path on the engine",
                call.path
            )));
        }
        let started = Instant::now();
        let budget = |now: Instant| match deadline {
            Some(deadline) => deadline.saturating_duration_since(now),
            None => call.timeout,
        };
        let stream =
            TcpStream::connect_timeout(&address, budget(started).max(TICK)).map_err(|e| {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    CallError::TimedOut
                } else {
                    CallError::Unreachable(format!("could not connect to {address}: {e}"))
                }
            })?;
        stream
            .set_read_timeout(Some(TICK))
            .and_then(|()| stream.set_write_timeout(Some(budget(Instant::now()).max(TICK))))
            .map_err(|e| CallError::Unreachable(e.to_string()))?;
        let _ = stream.set_nodelay(true);

        let body = call.body.as_ref().map(Value::to_string);
        let mut request = format!(
            "{method} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: quickai\r\nAccept: {}\r\nConnection: close\r\n",
            call.path,
            address,
            if call.framing == Framing::Sse {
                "text/event-stream"
            } else {
                "application/json"
            }
        );
        if let Some(body) = &body {
            request.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        request.push_str("\r\n");
        let mut sending = &stream;
        sending
            .write_all(request.as_bytes())
            .and_then(|()| match &body {
                Some(body) => sending.write_all(body.as_bytes()),
                None => Ok(()),
            })
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                    CallError::TimedOut
                }
                _ => CallError::Unreachable(format!("could not send to {address}: {e}")),
            })?;

        let mut wire = Wire {
            stream,
            cancel,
            buffer: Vec::new(),
            head: Head { status: 0 },
            body: Body::None,
            deadline,
            idle: call.timeout,
            last_data: Instant::now(),
        };
        wire.read_head(&method)?;
        Ok(wire)
    }

    /// Waits for more bytes: `Ok(false)` when the other side has closed the connection. Gives up
    /// when cancelled, when the call is out of time, or when a stream has been silent too long.
    fn fill(&mut self) -> Result<bool, CallError> {
        let mut chunk = [0u8; 16 * 1024];
        loop {
            if self.cancel.cancelled() {
                return Err(CallError::Cancelled);
            }
            let now = Instant::now();
            let out_of_time = match self.deadline {
                Some(deadline) => now >= deadline,
                None => now.duration_since(self.last_data) >= self.idle,
            };
            if out_of_time {
                return Err(CallError::TimedOut);
            }
            match self.stream.read(&mut chunk) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.buffer.extend_from_slice(&chunk[..n]);
                    self.last_data = Instant::now();
                    return Ok(true);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(CallError::Failed(format!("the connection broke: {e}"))),
            }
        }
    }

    fn read_head(&mut self, method: &str) -> Result<(), CallError> {
        let end = loop {
            if let Some(at) = find(&self.buffer, b"\r\n\r\n") {
                break at;
            }
            if self.buffer.len() > MAX_HEAD_BYTES {
                return Err(CallError::Failed("the reply's head is too long".into()));
            }
            if !self.fill()? {
                return Err(CallError::Failed(
                    "the connection closed before the engine answered".into(),
                ));
            }
        };
        let head = String::from_utf8_lossy(&self.buffer[..end]).into_owned();
        self.buffer.drain(..end + 4);
        let mut lines = head.split("\r\n");
        let status_line = lines.next().unwrap_or_default();
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .filter(|_| status_line.starts_with("HTTP/"))
            .ok_or_else(|| CallError::Failed(format!("not an HTTP reply: `{status_line}`")))?;
        let header = |name: &str| {
            lines
                .clone()
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_string())
        };
        let chunked =
            header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
        let length = header("content-length").and_then(|v| v.parse::<u64>().ok());
        self.head = Head { status };
        self.body = if method == "HEAD" || status == 204 || status == 304 || status < 200 {
            Body::None
        } else if chunked {
            Body::Chunked(ChunkState::Size)
        } else if let Some(length) = length {
            Body::Length(length)
        } else {
            Body::UntilClose
        };
        Ok(())
    }

    /// The next piece of the body as it arrives, `None` at its end. A body that ends short of what
    /// the reply announced is an error.
    fn next_piece(&mut self) -> Result<Option<Vec<u8>>, CallError> {
        let early = || CallError::Failed("the reply ended before it was complete".into());
        loop {
            match self.body {
                Body::None => return Ok(None),
                Body::UntilClose => {
                    if !self.buffer.is_empty() {
                        return Ok(Some(std::mem::take(&mut self.buffer)));
                    }
                    if !self.fill()? {
                        self.body = Body::None;
                        return Ok(None);
                    }
                }
                Body::Length(left) => {
                    if left == 0 {
                        self.body = Body::None;
                        return Ok(None);
                    }
                    if self.buffer.is_empty() && !self.fill()? {
                        return Err(early());
                    }
                    let take = (self.buffer.len() as u64).min(left) as usize;
                    self.body = Body::Length(left - take as u64);
                    return Ok(Some(self.buffer.drain(..take).collect()));
                }
                Body::Chunked(state) => match state {
                    ChunkState::Size => {
                        let Some(at) = find(&self.buffer, b"\r\n") else {
                            if self.buffer.len() > 1024 {
                                return Err(CallError::Failed("a chunk size is too long".into()));
                            }
                            if !self.fill()? {
                                return Err(early());
                            }
                            continue;
                        };
                        let line = String::from_utf8_lossy(&self.buffer[..at]).into_owned();
                        self.buffer.drain(..at + 2);
                        let size = line.split(';').next().unwrap_or_default().trim();
                        let size = u64::from_str_radix(size, 16).map_err(|_| {
                            CallError::Failed(format!("`{line}` is not a chunk size"))
                        })?;
                        self.body = Body::Chunked(if size == 0 {
                            ChunkState::Trailer
                        } else {
                            ChunkState::Data(size)
                        });
                    }
                    ChunkState::Data(left) => {
                        if self.buffer.is_empty() && !self.fill()? {
                            return Err(early());
                        }
                        let take = (self.buffer.len() as u64).min(left) as usize;
                        let piece: Vec<u8> = self.buffer.drain(..take).collect();
                        let left = left - take as u64;
                        self.body = Body::Chunked(if left == 0 {
                            ChunkState::DataEnd
                        } else {
                            ChunkState::Data(left)
                        });
                        return Ok(Some(piece));
                    }
                    ChunkState::DataEnd => {
                        if self.buffer.len() < 2 {
                            if !self.fill()? {
                                return Err(early());
                            }
                            continue;
                        }
                        self.buffer.drain(..2);
                        self.body = Body::Chunked(ChunkState::Size);
                    }
                    ChunkState::Trailer => {
                        // up to the blank line that ends the reply; trailers are not used
                        let Some(at) = find(&self.buffer, b"\r\n") else {
                            if !self.fill()? {
                                // some servers close right after the last chunk
                                self.body = Body::Chunked(ChunkState::Done);
                            }
                            continue;
                        };
                        let blank = at == 0;
                        self.buffer.drain(..at + 2);
                        if blank {
                            self.body = Body::Chunked(ChunkState::Done);
                        }
                    }
                    ChunkState::Done => {
                        self.body = Body::None;
                        return Ok(None);
                    }
                },
            }
        }
    }
}

impl Drop for Wire<'_> {
    /// Closing the connection is what tells the engine its client has gone.
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_one_that_cannot_aim_the_request_elsewhere() {
        for good in [
            "/",
            "/api/version",
            "/v1/chat/completions",
            "/api/tags?x=1",
            "/a..b",
        ] {
            assert!(path_is_plain(good), "{good}");
        }
        for bad in [
            "",
            "api",
            "//host/x",
            "/a//b",
            "/../x",
            "/a/../b",
            "/a/..",
            "http://h/x",
            "/a b",
            "/a\r\nHost: x",
            "/é",
            "/a\tb",
        ] {
            assert!(!path_is_plain(bad), "{bad:?}");
        }
    }

    #[test]
    fn only_this_machine_can_be_called() {
        let at = |host: &str| {
            loopback(&Endpoint {
                host: host.into(),
                port: Some(1),
            })
        };
        assert_eq!(at("127.0.0.1").unwrap(), "127.0.0.1:1".parse().unwrap());
        assert_eq!(at("localhost").unwrap(), "127.0.0.1:1".parse().unwrap());
        assert_eq!(at("::1").unwrap(), "[::1]:1".parse().unwrap());
        assert_eq!(at("[::1]").unwrap(), "[::1]:1".parse().unwrap());
        for remote in ["203.0.113.7", "0.0.0.0", "example.com", "", "10.0.0.1"] {
            assert!(
                matches!(at(remote), Err(CallError::Refused(_))),
                "{remote:?}"
            );
        }
        assert!(matches!(
            loopback(&Endpoint {
                host: "127.0.0.1".into(),
                port: None
            }),
            Err(CallError::Refused(_))
        ));
    }

    #[test]
    fn the_engines_sentence_is_found_in_the_shapes_engines_use() {
        assert_eq!(
            engine_sentence(404, br#"{"error":"model not found"}"#),
            "model not found"
        );
        assert_eq!(
            engine_sentence(500, br#"{"error":{"message":"out of memory","type":"x"}}"#),
            "out of memory"
        );
        for no_sentence in [
            &b""[..],
            b"404 page not found",
            b"{\"other\":1}",
            b"{\"error\":null}",
        ] {
            assert_eq!(engine_sentence(404, no_sentence), "http status: 404");
        }
    }

    // ---- the wire: replies that are cut, split and strange -------------------------------------------

    /// A server that answers one connection with exactly these bytes, then closes it. `piece`
    /// writes them that many at a time with a pause between, to see that where the network happens
    /// to cut a reply makes no difference.
    fn raw_server(reply: Vec<u8>, piece: usize) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            // the request, up to its blank line (and its body, which these tests do not send)
            let mut seen = Vec::new();
            let mut byte = [0u8; 1];
            while !seen.ends_with(b"\r\n\r\n") && socket.read(&mut byte).unwrap_or(0) == 1 {
                seen.push(byte[0]);
            }
            for part in reply.chunks(piece.max(1)) {
                if socket.write_all(part).is_err() {
                    return;
                }
                if piece < 64 {
                    std::thread::sleep(Duration::from_micros(300));
                }
            }
        });
        port
    }

    fn at(port: u16) -> Endpoint {
        Endpoint {
            host: "127.0.0.1".into(),
            port: Some(port),
        }
    }

    fn json_get(path: &str) -> Call {
        Call {
            method: "GET".into(),
            path: path.into(),
            body: None,
            framing: Framing::Json,
            timeout: Duration::from_secs(5),
        }
    }

    fn lines_call() -> Call {
        Call {
            framing: Framing::Ndjson,
            ..json_get("/api/pull")
        }
    }

    fn is_early(e: &CallError) -> bool {
        matches!(e, CallError::Failed(m) if m.contains("before it was complete"))
    }

    #[test]
    fn a_reply_that_stops_short_is_an_error_wherever_it_stops() {
        let body = b"{\"status\":\"a\"}\n";
        let head = |extra: &str| format!("HTTP/1.1 200 OK\r\n{extra}\r\n").into_bytes();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("in the middle of a chunk's data", {
                let mut r = head("Transfer-Encoding: chunked\r\n");
                r.extend(b"20\r\n");
                r.extend(body);
                r
            }),
            ("in a chunk's size line", {
                let mut r = head("Transfer-Encoding: chunked\r\n");
                r.extend(b"1");
                r
            }),
            ("where a chunk's CRLF should be", {
                let mut r = head("Transfer-Encoding: chunked\r\n");
                r.extend(format!("{:x}\r\n", body.len()).as_bytes());
                r.extend(body);
                r.extend(b"\r");
                r
            }),
            ("before a chunk after the first", {
                let mut r = head("Transfer-Encoding: chunked\r\n");
                r.extend(format!("{:x}\r\n", body.len()).as_bytes());
                r.extend(body);
                r.extend(b"\r\n");
                r
            }),
            ("short of its content length", {
                let mut r = head(&format!("Content-Length: {}\r\n", body.len() + 50));
                r.extend(body);
                r
            }),
        ];
        for (what, reply) in cases {
            for piece in [1, 7, 4096] {
                let port = raw_server(reply.clone(), piece);
                let mut seen = Vec::new();
                let outcome = stream(
                    &at(port),
                    &lines_call(),
                    &AtomicBool::new(false),
                    &mut |e| seen.push(e),
                );
                let error = outcome.expect_err(&format!("{what} ({piece} at a time): a quiet end"));
                assert!(is_early(&error), "{what} ({piece} at a time): {error:?}");
            }
        }
        // the same for a whole reply asked for as JSON
        let mut r = head(&format!("Content-Length: {}\r\n", 100));
        r.extend(b"{\"a\":");
        let error = call(&at(raw_server(r, 4096)), &json_get("/x")).unwrap_err();
        assert!(is_early(&error), "{error:?}");
    }

    #[test]
    fn a_whole_reply_is_the_same_however_the_network_cuts_it() {
        let mut reply = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/x-ndjson\r\n\r\n".to_vec();
        for line in [
            "{\"n\":1}\n",
            "{\"n\":2}\n",
            "{\"n\":3,\"t\":\"héllo ✓\"}\n",
        ] {
            reply.extend(format!("{:x};ext=1\r\n{line}\r\n", line.len()).as_bytes());
        }
        reply.extend(b"0\r\nTrailer: x\r\n\r\n");
        for piece in [1, 2, 3, 5, 11, 4096] {
            let port = raw_server(reply.clone(), piece);
            let mut seen = Vec::new();
            stream(
                &at(port),
                &lines_call(),
                &AtomicBool::new(false),
                &mut |e| seen.push(e),
            )
            .unwrap_or_else(|e| panic!("{piece} at a time: {e:?}"));
            assert_eq!(
                seen,
                [
                    serde_json::json!({"n": 1}),
                    serde_json::json!({"n": 2}),
                    serde_json::json!({"n": 3, "t": "héllo ✓"})
                ],
                "{piece} at a time"
            );
        }
    }

    #[test]
    fn a_body_with_no_length_ends_when_the_connection_does_and_a_head_with_none_has_none() {
        let reply =
            b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}".to_vec();
        let reply_got = call(&at(raw_server(reply, 3)), &json_get("/x")).unwrap();
        assert_eq!(reply_got.body, serde_json::json!({"ok": true}));
        let none = b"HTTP/1.1 204 No Content\r\n\r\n".to_vec();
        assert_eq!(
            call(&at(raw_server(none, 4096)), &json_get("/x"))
                .unwrap()
                .body,
            Value::Null
        );
    }

    #[test]
    fn something_that_does_not_speak_http_is_an_error_not_a_hang() {
        for garbage in [&b"hello there\r\n\r\n"[..], b"SSH-2.0-OpenSSH\r\n\r\n", b""] {
            let outcome = call(&at(raw_server(garbage.to_vec(), 4096)), &json_get("/x"));
            assert!(
                matches!(outcome, Err(CallError::Failed(_))),
                "{garbage:?}: {outcome:?}"
            );
        }
    }
}
