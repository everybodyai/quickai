//! An opt-in, best-effort-transparent HTTP reverse proxy that sits in front
//! of llama-server when the user turns on "Show live conversation traffic"
//! in ⚙ Settings. Off (the default), llama-server binds the public port
//! directly and none of this runs — a client talks straight to llama-server
//! with no extra hop.
//!
//! Two concerns are deliberately kept separate:
//!   - *Relaying* bytes client <-> llama-server must always be correct. Every
//!     request/response is forwarded byte-for-byte via `tiny_http`/`ureq`,
//!     both mature HTTP/1.1 implementations, so keep-alive, chunked
//!     transfer, and SSE streaming all just work.
//!   - *Sniffing* those bytes to show something readable in the 📡 Live
//!     window is best-effort only. A parse miss (e.g. a future llama.cpp
//!     response shape this doesn't recognise) just leaves an entry blank —
//!     it can never corrupt or block the actual relay.

use std::io::{self, Read};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::Sender;
use serde_json::Value;

use crate::events::{Event, TrafficEvent};
use crate::hardware::format_bytes;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Requests larger than this are still relayed in full, just not sniffed —
/// keeps a huge upload from being buffered twice in memory.
const MAX_REQUEST_BODY: u64 = 8 * 1024 * 1024;
/// Sniffing gives up (but relaying continues normally) past this much
/// unparsed buffered response data, so a malformed/never-ending stream can't
/// grow memory unbounded.
const MAX_SNIFF_BUFFER: usize = 4 * 1024 * 1024;

pub struct ProxyHandle {
    server: Arc<tiny_http::Server>,
}

impl ProxyHandle {
    /// Unblocks the accept loop so it exits and drops the server, freeing
    /// the public port. Any already-in-flight requests are left to finish
    /// or error out on their own.
    pub fn stop(&self) {
        self.server.unblock();
    }
}

/// Binds `public_port` (what clients connect to) and forwards every
/// connection to `upstream_port` (where llama-server actually listens), on
/// a background thread per connection. Binding happens synchronously, so a
/// "port already in use" error surfaces immediately, before llama-server
/// itself is spawned.
pub fn spawn<F>(
    public_port: u16,
    upstream_port: u16,
    evt_tx: Sender<Event>,
    wake: Arc<F>,
) -> io::Result<ProxyHandle>
where
    F: Fn() + Send + Sync + 'static,
{
    let server = tiny_http::Server::http(("127.0.0.1", public_port))
        .map_err(|e| io::Error::new(io::ErrorKind::AddrInUse, e.to_string()))?;
    let server = Arc::new(server);
    let accept_server = server.clone();
    std::thread::spawn(move || loop {
        match accept_server.recv() {
            Ok(req) => {
                let evt_tx = evt_tx.clone();
                let wake = wake.clone();
                std::thread::spawn(move || handle_request(req, upstream_port, evt_tx, wake));
            }
            // Either `ProxyHandle::stop()` unblocked us, or a fatal I/O
            // error — either way, stop accepting new connections.
            Err(_) => return,
        }
    });
    Ok(ProxyHandle { server })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Chat,
    Completion,
    Other,
}

fn classify(path: &str) -> Kind {
    let p = path.split('?').next().unwrap_or(path);
    if p.ends_with("/chat/completions") {
        Kind::Chat
    } else if p.ends_with("/completions") || p == "/completion" {
        Kind::Completion
    } else {
        Kind::Other
    }
}

fn handle_request<F>(
    mut req: tiny_http::Request,
    upstream_port: u16,
    evt_tx: Sender<Event>,
    wake: Arc<F>,
) where
    F: Fn() + Send + Sync + 'static,
{
    let method = req.method().as_str().to_string();
    let path = req.url().to_string();
    let session = req
        .remote_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let kind = classify(&path);
    let interesting = kind != Kind::Other;

    let mut body = Vec::new();
    let _ = req
        .as_reader()
        .take(MAX_REQUEST_BODY)
        .read_to_end(&mut body);

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    if interesting {
        let parsed: Option<Value> = serde_json::from_slice(&body).ok();
        let ContentSummary { text, attachments } = parsed
            .as_ref()
            .map(|v| extract_content(kind, v))
            .unwrap_or_default();
        send_traffic(
            &evt_tx,
            &wake,
            TrafficEvent::Request {
                id,
                session,
                method: method.clone(),
                path: path.clone(),
                prompt: text,
                attachments,
            },
        );
    }

    let uri = format!("http://127.0.0.1:{upstream_port}{path}");
    let mut builder = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(&uri);
    for h in req.headers() {
        let name = h.field.to_string();
        if name.eq_ignore_ascii_case("host") || name.eq_ignore_ascii_case("connection") {
            continue;
        }
        builder = builder.header(name, h.value.as_str());
    }
    let upstream_req = match builder.body(body) {
        Ok(r) => r,
        Err(_) => {
            let _ = req.respond(tiny_http::Response::empty(tiny_http::StatusCode(400)));
            return;
        }
    };

    let resp = match ureq::run(upstream_req) {
        Ok(r) => r,
        Err(e) => {
            if interesting {
                send_traffic(
                    &evt_tx,
                    &wake,
                    TrafficEvent::ResponseError {
                        id,
                        message: e.to_string(),
                    },
                );
            }
            let _ = req.respond(
                tiny_http::Response::from_string(format!("proxy: upstream error: {e}"))
                    .with_status_code(tiny_http::StatusCode(502)),
            );
            return;
        }
    };

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let is_sse = content_type.contains("text/event-stream");
    let content_length = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok());

    let mut out_headers = Vec::new();
    for (name, value) in resp.headers() {
        if name.as_str().eq_ignore_ascii_case("transfer-encoding")
            || name.as_str().eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let Ok(v) = value.to_str() {
            if let Ok(header) =
                tiny_http::Header::from_bytes(name.as_str().as_bytes(), v.as_bytes())
            {
                out_headers.push(header);
            }
        }
    }

    let body_reader = resp.into_body().into_reader();
    // SSE bodies have no known length up front (chunked either way); for
    // everything else, forward the real Content-Length when we have one.
    let data_length = if is_sse { None } else { content_length };

    if interesting {
        let sniffer = SniffingReader {
            inner: body_reader,
            id,
            kind,
            is_sse,
            buf: Vec::new(),
            done_sent: false,
            evt_tx,
            wake,
        };
        let response = tiny_http::Response::new(
            tiny_http::StatusCode(status),
            out_headers,
            sniffer,
            data_length,
            None,
        );
        let _ = req.respond(response);
    } else {
        let response = tiny_http::Response::new(
            tiny_http::StatusCode(status),
            out_headers,
            body_reader,
            data_length,
            None,
        );
        let _ = req.respond(response);
    }
}

fn send_traffic<F>(evt_tx: &Sender<Event>, wake: &Arc<F>, event: TrafficEvent)
where
    F: Fn() + Send + Sync + 'static,
{
    let _ = evt_tx.send(Event::Traffic(event));
    wake();
}

/// Wraps the upstream response body: every byte read through here is passed
/// on unchanged (so the real client always gets exactly what llama-server
/// sent), while also being fed to a best-effort parser that emits
/// `TrafficEvent`s for the 📡 Live window.
struct SniffingReader<R, F: Fn() + Send + Sync + 'static> {
    inner: R,
    id: u64,
    kind: Kind,
    is_sse: bool,
    /// SSE: unconsumed tail waiting for a full `\n\n`-terminated frame.
    /// Non-SSE: the whole body so far, parsed once at EOF.
    buf: Vec<u8>,
    done_sent: bool,
    evt_tx: Sender<Event>,
    wake: Arc<F>,
}

impl<R: Read, F: Fn() + Send + Sync + 'static> Read for SniffingReader<R, F> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(out)?;
        if n == 0 {
            self.finish();
        } else {
            self.feed(&out[..n]);
        }
        Ok(n)
    }
}

impl<R, F: Fn() + Send + Sync + 'static> SniffingReader<R, F> {
    fn feed(&mut self, chunk: &[u8]) {
        if self.buf.len() >= MAX_SNIFF_BUFFER {
            return; // gave up sniffing this response; relay is unaffected
        }
        self.buf.extend_from_slice(chunk);
        if self.is_sse {
            while let Some((frame_end, next_start)) = find_frame_boundary(&self.buf) {
                let frame = self.buf[..frame_end].to_vec();
                self.buf.drain(..next_start);
                if self.handle_sse_frame(&frame) {
                    return; // saw `[DONE]`; nothing more to do
                }
            }
        }
    }

    /// Returns `true` if this frame was a `[DONE]` sentinel.
    fn handle_sse_frame(&mut self, frame: &[u8]) -> bool {
        for line in frame.split(|&b| b == b'\n') {
            let line = strip_trailing_cr(line);
            let Some(rest) = strip_prefix(line, b"data:") else {
                continue;
            };
            let rest = trim_leading_space(rest);
            if rest == b"[DONE]" {
                self.send_done(None);
                return true;
            }
            if let Ok(json) = serde_json::from_slice::<Value>(rest) {
                if let Some(delta) = extract_delta(&json) {
                    if !delta.is_empty() {
                        send_traffic(
                            &self.evt_tx,
                            &self.wake,
                            TrafficEvent::ResponseDelta {
                                id: self.id,
                                text: delta,
                            },
                        );
                    }
                }
            }
        }
        false
    }

    fn finish(&mut self) {
        if self.done_sent {
            return;
        }
        self.done_sent = true;
        if self.is_sse {
            self.send_done(None);
        } else {
            let full_text = serde_json::from_slice::<Value>(&self.buf)
                .ok()
                .and_then(|v| extract_full_text(self.kind, &v));
            self.send_done(full_text);
        }
    }

    fn send_done(&mut self, full_text: Option<String>) {
        send_traffic(
            &self.evt_tx,
            &self.wake,
            TrafficEvent::ResponseDone {
                id: self.id,
                full_text,
            },
        );
    }
}

/// Finds the end of the first complete SSE event in `buf` (terminated by a
/// blank line, i.e. `\n\n` or `\r\n\r\n`). Returns `(frame_end, next_start)`
/// where `frame_end` excludes the terminator and `next_start` is where the
/// next frame begins.
fn find_frame_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let pos = buf.windows(2).position(|w| w == b"\n\n")?;
    Some((pos, pos + 2))
}

fn strip_trailing_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn strip_prefix<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

fn trim_leading_space(line: &[u8]) -> &[u8] {
    line.strip_prefix(b" ").unwrap_or(line)
}

/// The interesting parts of one chat/completion request: the free text (for
/// the 📡 Live tab's prompt preview) and a description of anything else
/// that came along with it.
#[derive(Default)]
struct ContentSummary {
    text: Option<String>,
    attachments: Vec<String>,
}

fn extract_content(kind: Kind, body: &Value) -> ContentSummary {
    match kind {
        Kind::Chat => body
            .get("messages")
            .and_then(|m| m.as_array())
            .and_then(|messages| {
                messages
                    .iter()
                    .rev()
                    .find(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
            })
            .map(|msg| summarize_content(msg.get("content")))
            .unwrap_or_default(),
        Kind::Completion => ContentSummary {
            text: body
                .get("prompt")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            attachments: Vec::new(),
        },
        Kind::Other => ContentSummary::default(),
    }
}

/// A message's `content` is either a plain string (the common case) or,
/// for clients sending images/documents alongside text, an array of typed
/// parts (the OpenAI-style vision/file shape). Text parts are concatenated
/// for the prompt preview; every other part type is turned into a short,
/// human-readable attachment description rather than dropped silently.
fn summarize_content(content: Option<&Value>) -> ContentSummary {
    match content {
        Some(Value::String(s)) => ContentSummary {
            text: Some(s.clone()),
            attachments: Vec::new(),
        },
        Some(Value::Array(parts)) => {
            let mut text_parts = Vec::new();
            let mut attachments = Vec::new();
            for part in parts {
                match part.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some(other) => attachments.push(describe_attachment(other, part)),
                    None => {}
                }
            }
            ContentSummary {
                text: (!text_parts.is_empty()).then(|| text_parts.join(" ")),
                attachments,
            }
        }
        _ => ContentSummary::default(),
    }
}

/// Describes one non-text content part by its declared `type`, pulling out
/// a filename or a data URL's MIME type/approximate size when present.
/// Best-effort only — an unrecognised shape still gets a generic label
/// rather than being dropped, since even "some kind of attachment was
/// sent" is more useful than nothing in the 📡 Live tab.
fn describe_attachment(kind: &str, part: &Value) -> String {
    match kind {
        "image_url" => describe_url(
            "🖼 image",
            part.pointer("/image_url/url").and_then(|u| u.as_str()),
        ),
        "input_audio" => "🔊 audio".to_string(),
        "file" | "input_file" | "document" => {
            let name = part
                .pointer("/file/filename")
                .or_else(|| part.pointer("/file/file_name"))
                .or_else(|| part.get("filename"))
                .and_then(|n| n.as_str());
            let data_url = part
                .pointer("/file/file_data")
                .or_else(|| part.pointer("/file/url"))
                .or_else(|| part.pointer("/source/data"))
                .and_then(|u| u.as_str());
            match (name, data_url.and_then(parse_data_url)) {
                (Some(n), Some((_, bytes))) => format!("📄 {n} (~{})", format_bytes(bytes)),
                (Some(n), None) => format!("📄 {n}"),
                (None, Some((mime, bytes))) => {
                    format!("📄 file ({mime}, ~{})", format_bytes(bytes))
                }
                (None, None) => "📄 file".to_string(),
            }
        }
        other => format!("📎 {other}"),
    }
}

fn describe_url(label: &str, url: Option<&str>) -> String {
    match url.and_then(parse_data_url) {
        Some((mime, bytes)) => format!("{label} ({mime}, ~{})", format_bytes(bytes)),
        None => label.to_string(),
    }
}

/// Parses a `data:<mime>;base64,<payload>` URL into its declared MIME type
/// and an approximate decoded size (base64 expands data by ~4/3) — a rough
/// "how big was this" label, not an exact byte count.
fn parse_data_url(url: &str) -> Option<(String, u64)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta
        .split(';')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown type");
    let approx_bytes = payload.len() as u64 * 3 / 4;
    Some((mime.to_string(), approx_bytes))
}

/// Tries the OpenAI chat-streaming shape, then OpenAI completions, then
/// llama.cpp's own legacy `/completion` shape — whichever matches first.
fn extract_delta(json: &Value) -> Option<String> {
    json.pointer("/choices/0/delta/content")
        .or_else(|| json.pointer("/choices/0/text"))
        .or_else(|| json.get("content"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn extract_full_text(kind: Kind, json: &Value) -> Option<String> {
    let _ = kind;
    json.pointer("/choices/0/message/content")
        .or_else(|| json.pointer("/choices/0/text"))
        .or_else(|| json.get("content"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognizes_chat_and_completion_endpoints() {
        assert_eq!(classify("/v1/chat/completions"), Kind::Chat);
        assert_eq!(classify("/v1/chat/completions?stream=true"), Kind::Chat);
        assert_eq!(classify("/v1/completions"), Kind::Completion);
        assert_eq!(classify("/completion"), Kind::Completion);
        assert_eq!(classify("/health"), Kind::Other);
        assert_eq!(classify("/"), Kind::Other);
    }

    #[test]
    fn extract_content_reads_last_non_system_message() {
        let body: Value = serde_json::from_str(
            r#"{"messages":[{"role":"system","content":"sys"},{"role":"user","content":"hello"}]}"#,
        )
        .unwrap();
        let summary = extract_content(Kind::Chat, &body);
        assert_eq!(summary.text, Some("hello".to_string()));
        assert!(summary.attachments.is_empty());

        let body: Value = serde_json::from_str(r#"{"prompt":"once upon a time"}"#).unwrap();
        assert_eq!(
            extract_content(Kind::Completion, &body).text,
            Some("once upon a time".to_string())
        );
    }

    #[test]
    fn extract_content_pulls_text_and_image_out_of_multipart_content() {
        let body: Value = serde_json::from_str(
            r#"{"messages":[{"role":"user","content":[
                {"type":"text","text":"what is this?"},
                {"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8gd29ybGQ="}}
            ]}]}"#,
        )
        .unwrap();
        let summary = extract_content(Kind::Chat, &body);
        assert_eq!(summary.text, Some("what is this?".to_string()));
        assert_eq!(summary.attachments.len(), 1);
        assert!(
            summary.attachments[0].contains("image/png"),
            "{:?}",
            summary.attachments
        );
    }

    #[test]
    fn extract_content_describes_a_named_file_attachment() {
        let body: Value = serde_json::from_str(
            r#"{"messages":[{"role":"user","content":[
                {"type":"text","text":"summarize this"},
                {"type":"file","file":{"filename":"report.pdf","file_data":"data:application/pdf;base64,aGVsbG8="}}
            ]}]}"#,
        )
        .unwrap();
        let summary = extract_content(Kind::Chat, &body);
        assert_eq!(summary.text, Some("summarize this".to_string()));
        assert_eq!(
            summary.attachments,
            vec!["📄 report.pdf (~6 B)".to_string()]
        );
    }

    #[test]
    fn parse_data_url_extracts_mime_and_approximate_size() {
        let (mime, bytes) = parse_data_url("data:application/pdf;base64,aGVsbG8=").unwrap();
        assert_eq!(mime, "application/pdf");
        assert_eq!(bytes, 6); // 8 base64 chars * 3/4
        assert!(parse_data_url("not a data url").is_none());
    }

    #[test]
    fn extract_delta_tries_known_streaming_shapes() {
        let chat: Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"hi"}}]}"#).unwrap();
        assert_eq!(extract_delta(&chat), Some("hi".to_string()));

        let completions: Value = serde_json::from_str(r#"{"choices":[{"text":"hi"}]}"#).unwrap();
        assert_eq!(extract_delta(&completions), Some("hi".to_string()));

        let legacy: Value = serde_json::from_str(r#"{"content":"hi","stop":false}"#).unwrap();
        assert_eq!(extract_delta(&legacy), Some("hi".to_string()));
    }

    #[test]
    fn find_frame_boundary_splits_on_blank_line() {
        let buf = b"data: {\"a\":1}\n\ndata: {\"a\":2}\n\n";
        let (end, next) = find_frame_boundary(buf).unwrap();
        assert_eq!(&buf[..end], &b"data: {\"a\":1}"[..]);
        assert_eq!(&buf[next..], &b"data: {\"a\":2}\n\n"[..]);
    }

    /// End-to-end: proxy a real request through to a plain TCP echo-ish
    /// stub standing in for llama-server, and confirm the client sees the
    /// right response *and* the expected Traffic events come out the other
    /// side — using the real port-binding/threading path, not mocks.
    #[test]
    fn proxies_a_chat_completion_and_emits_traffic_events() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::Write;
            let (mut stream, _) = upstream.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf); // don't care about the exact request bytes here
            let body = r#"{"choices":[{"message":{"content":"hi there"}}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        });

        let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
        let wake = Arc::new(|| {});
        // A port found by binding 0 can still be held when `spawn` binds it (a child that another
        // test has just forked keeps the listener until it execs): ask for another one.
        let (public_port, handle) = (0..5)
            .find_map(|_| {
                let port = crate::runtime::process::ephemeral_port().unwrap();
                spawn(port, upstream_port, evt_tx.clone(), wake.clone())
                    .ok()
                    .map(|handle| (port, handle))
            })
            .expect("five free ports in a row were all taken");

        let resp = ureq::post(format!(
            "http://127.0.0.1:{public_port}/v1/chat/completions"
        ))
        .header("Content-Type", "application/json")
        .send(r#"{"messages":[{"role":"user","content":"hello"}]}"#)
        .unwrap();
        assert_eq!(resp.status(), 200);
        let text = resp.into_body().read_to_string().unwrap();
        assert!(text.contains("hi there"), "unexpected body: {text}");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut saw_request = false;
        let mut saw_done_with_text = false;
        while std::time::Instant::now() < deadline && !(saw_request && saw_done_with_text) {
            if let Ok(Event::Traffic(t)) =
                evt_rx.recv_timeout(std::time::Duration::from_millis(200))
            {
                match t {
                    TrafficEvent::Request {
                        prompt,
                        attachments,
                        ..
                    } => {
                        assert_eq!(prompt, Some("hello".to_string()));
                        assert!(attachments.is_empty());
                        saw_request = true;
                    }
                    TrafficEvent::ResponseDone { full_text, .. } => {
                        assert_eq!(full_text, Some("hi there".to_string()));
                        saw_done_with_text = true;
                    }
                    _ => {}
                }
            }
        }
        assert!(saw_request, "never saw a Request traffic event");
        assert!(
            saw_done_with_text,
            "never saw a ResponseDone traffic event with parsed text"
        );

        handle.stop();
    }
}
