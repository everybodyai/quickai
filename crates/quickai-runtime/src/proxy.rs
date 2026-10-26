//! An opt-in, best-effort-transparent HTTP reverse proxy that sits in front
//! of an engine when the user turns on "Show live conversation traffic"
//! in Settings. Off (the default), the engine binds the public port
//! directly and none of this runs — a client talks straight to the engine
//! with no extra hop.
//!
//! Two concerns are deliberately kept separate:
//!   - *Relaying* bytes client <-> engine must always be correct. Every
//!     request/response is forwarded byte-for-byte via `tiny_http`/`ureq`,
//!     both mature HTTP/1.1 implementations, so keep-alive, chunked
//!     transfer, and SSE streaming all just work.
//!   - *Sniffing* those bytes to show something readable in the Live traffic
//!     window is best-effort only. A parse miss (e.g. a future response
//!     shape this doesn't recognise) just leaves an entry blank —
//!     it can never corrupt or block the actual relay.
//!
//! What counts as a conversation, and where its text is, is not in this file: it is the
//! vocabulary's (`decode` blocks of the API definitions, read by `quickai_core::decode`), handed in
//! with the run plan. A path no decoder claims is relayed and not shown.

use std::io::{self, Read};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::Sender;
use quickai_core::decode::{ApiDecoder, Attachment, AttachmentKind, Reading};
use quickai_core::stream::{NdjsonSplitter, SseEvent, SseSplitter};
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
/// connection to `upstream_port` (where the engine actually listens), on
/// a background thread per connection. Binding happens synchronously, so a
/// "port already in use" error surfaces immediately, before the engine
/// itself is spawned. `decoders` say which paths are conversations and how
/// to read them.
pub fn spawn<F>(
    public_port: u16,
    upstream_port: u16,
    decoders: Vec<ApiDecoder>,
    evt_tx: Sender<Event>,
    wake: Arc<F>,
) -> io::Result<ProxyHandle>
where
    F: Fn() + Send + Sync + 'static,
{
    let decoders = Arc::new(decoders);
    // A relay shows the client what the engine said: an answer with an error status (a prompt too
    // long, a model that is not there) is passed on as it is, and so is a redirect.
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .build(),
    );
    let server = tiny_http::Server::http(("127.0.0.1", public_port))
        .map_err(|e| io::Error::new(io::ErrorKind::AddrInUse, e.to_string()))?;
    let server = Arc::new(server);
    let accept_server = server.clone();
    std::thread::spawn(move || loop {
        match accept_server.recv() {
            Ok(req) => {
                let evt_tx = evt_tx.clone();
                let wake = wake.clone();
                let decoders = decoders.clone();
                let agent = agent.clone();
                std::thread::spawn(move || {
                    handle_request(req, upstream_port, &agent, &decoders, evt_tx, wake)
                });
            }
            // Either `ProxyHandle::stop()` unblocked us, or a fatal I/O
            // error — either way, stop accepting new connections.
            Err(_) => return,
        }
    });
    Ok(ProxyHandle { server })
}

fn handle_request<F>(
    mut req: tiny_http::Request,
    upstream_port: u16,
    agent: &ureq::Agent,
    decoders: &[ApiDecoder],
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
    let decoder = decoders.iter().find(|d| d.matches(&path));

    let mut body = Vec::new();
    let _ = req
        .as_reader()
        .take(MAX_REQUEST_BODY)
        .read_to_end(&mut body);

    // A request is shown only when its API is one the vocabulary says how to read and it is a
    // conversation: a load or an unload goes through the same path and says no prompt.
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let sniff = decoder.and_then(|decoder| {
        let parsed: Option<Value> = serde_json::from_slice(&body).ok();
        decoder
            .request(parsed.as_ref())
            .map(|summary| (decoder, summary))
    });
    if let Some((_, summary)) = &sniff {
        send_traffic(
            &evt_tx,
            &wake,
            TrafficEvent::Request {
                id,
                session,
                method: method.clone(),
                path: path.clone(),
                prompt: summary.text.clone(),
                attachments: summary
                    .attachments
                    .iter()
                    .map(describe_attachment)
                    .collect(),
            },
        );
    }
    let interesting = sniff.is_some();
    let decoder = sniff.map(|(decoder, _)| decoder.clone());

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

    let resp = match agent.run(upstream_req) {
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

    // A body of unknown length is a reply that may be streaming (a chat, as it is written): it goes to
    // the client as it arrives. tiny_http's own encoder holds a chunked reply back until a kilobyte of
    // it has been read, which is the whole of a short one.
    if data_length.is_none() {
        match decoder {
            Some(decoder) => relay_as_it_arrives(
                req,
                status,
                &out_headers,
                SniffingReader {
                    inner: body_reader,
                    id,
                    reading: decoder.reading(&content_type),
                    decoder,
                    status,
                    sse: SseSplitter::new(),
                    ndjson: NdjsonSplitter::new(),
                    whole: Vec::new(),
                    fed: 0,
                    ended: false,
                    evt_tx,
                    wake,
                },
            ),
            None => relay_as_it_arrives(req, status, &out_headers, body_reader),
        }
        return;
    }

    if let Some(decoder) = decoder {
        let sniffer = SniffingReader {
            inner: body_reader,
            id,
            reading: decoder.reading(&content_type),
            decoder,
            status,
            sse: SseSplitter::new(),
            ndjson: NdjsonSplitter::new(),
            whole: Vec::new(),
            fed: 0,
            ended: false,
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

/// Writes a reply whose length is not known up front the way it comes: the status line and the headers,
/// then each piece the engine sends as a chunk of its own, flushed at once. A client older than HTTP/1.1
/// is given the bytes as they are and the end of the connection to say that they are over. When the
/// engine breaks off mid-reply the terminating chunk is not sent, so the reply is not mistaken for a
/// whole one (tiny_http has no way to reset the connection, so a client waits for its own timeout).
fn relay_as_it_arrives(
    req: tiny_http::Request,
    status: u16,
    headers: &[tiny_http::Header],
    mut body: impl Read,
) {
    use std::io::Write;
    let chunked = *req.http_version() >= tiny_http::HTTPVersion(1, 1);
    let head_only = *req.method() == tiny_http::Method::Head;
    let mut out = req.into_writer();
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\n",
        tiny_http::StatusCode(status).default_reason_phrase()
    );
    for header in headers {
        head.push_str(&format!("{}: {}\r\n", header.field, header.value));
    }
    head.push_str(if chunked {
        "Transfer-Encoding: chunked\r\n\r\n"
    } else {
        "Connection: close\r\n\r\n"
    });
    if out
        .write_all(head.as_bytes())
        .and_then(|_| out.flush())
        .is_err()
        || head_only
    {
        return;
    }
    let mut buf = [0u8; 8192];
    loop {
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return,
        };
        let sent = if chunked {
            write!(out, "{n:x}\r\n")
                .and_then(|_| out.write_all(&buf[..n]))
                .and_then(|_| out.write_all(b"\r\n"))
        } else {
            out.write_all(&buf[..n])
        };
        // a client that has gone away is not an error of the engine's
        if sent.and_then(|_| out.flush()).is_err() {
            return;
        }
    }
    if chunked {
        let _ = out.write_all(b"0\r\n\r\n").and_then(|_| out.flush());
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
/// on unchanged (so the real client always gets exactly what the engine
/// sent), while also being fed to a best-effort reader that emits
/// `TrafficEvent`s for the Live traffic window.
struct SniffingReader<R, F: Fn() + Send + Sync + 'static> {
    inner: R,
    id: u64,
    decoder: ApiDecoder,
    /// How this reply is read, from its content type and the API's framings.
    reading: Reading,
    status: u16,
    sse: SseSplitter,
    ndjson: NdjsonSplitter,
    /// A reply that comes whole: the body so far, read once at the end.
    whole: Vec<u8>,
    /// How much of the reply has been looked at; past `MAX_SNIFF_BUFFER` it is not.
    fed: usize,
    /// The conversation has been closed (done or failed): nothing more is said about it.
    ended: bool,
    evt_tx: Sender<Event>,
    wake: Arc<F>,
}

impl<R: Read, F: Fn() + Send + Sync + 'static> Read for SniffingReader<R, F> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(out) {
            Ok(0) => {
                self.finish();
                Ok(0)
            }
            Ok(n) => {
                self.feed(&out[..n]);
                Ok(n)
            }
            Err(e) => {
                // the engine went away mid-reply: what arrived stays, and the entry is closed
                self.fail(&format!("the reply was cut off: {e}"));
                Err(e)
            }
        }
    }
}

impl<R, F: Fn() + Send + Sync + 'static> SniffingReader<R, F> {
    fn feed(&mut self, chunk: &[u8]) {
        if self.ended || self.fed >= MAX_SNIFF_BUFFER {
            return; // gave up sniffing this response; relay is unaffected
        }
        self.fed += chunk.len();
        match self.reading {
            Reading::Sse => {
                for event in self.sse.push(chunk) {
                    self.sse_event(event);
                }
            }
            Reading::Ndjson => {
                // a line that is not JSON is skipped: the lines around it are still read
                for value in self.ndjson.push(chunk).into_iter().flatten() {
                    self.event(&value);
                }
            }
            Reading::Whole => self.whole.extend_from_slice(chunk),
            Reading::Skip => {}
        }
    }

    fn sse_event(&mut self, event: SseEvent) {
        if event.data.trim() == "[DONE]" {
            self.done(None);
        } else if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
            self.event(&value);
        }
    }

    /// One event of a stream: an error ends the conversation, text is shown as it comes.
    fn event(&mut self, value: &Value) {
        if self.ended {
            return;
        }
        if let Some(message) = self.decoder.error(value) {
            self.fail(&message);
        } else if let Some(delta) = self.decoder.delta(value).filter(|d| !d.is_empty()) {
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

    fn finish(&mut self) {
        if self.ended {
            return;
        }
        match self.reading {
            Reading::Sse => {
                for event in self.sse.finish() {
                    self.sse_event(event);
                }
                self.done(None);
            }
            Reading::Ndjson => {
                for value in self.ndjson.finish().into_iter().flatten() {
                    self.event(&value);
                }
                self.done(None);
            }
            Reading::Whole => {
                let body = serde_json::from_slice::<Value>(&self.whole).ok();
                match body.as_ref().and_then(|v| self.decoder.error(v)) {
                    Some(message) => self.fail(&message),
                    None if self.status >= 400 => {
                        self.fail(&format!("the engine answered with status {}", self.status))
                    }
                    None => {
                        let full_text = body.and_then(|v| self.decoder.final_text(&v));
                        self.done(full_text);
                    }
                }
            }
            Reading::Skip => self.done(None),
        }
    }

    fn done(&mut self, full_text: Option<String>) {
        if !self.ended {
            self.ended = true;
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

    fn fail(&mut self, message: &str) {
        if !self.ended {
            self.ended = true;
            send_traffic(
                &self.evt_tx,
                &self.wake,
                TrafficEvent::ResponseError {
                    id: self.id,
                    message: message.to_string(),
                },
            );
        }
    }
}

/// One thing that came with the text, in a few words for the Live tab. A type, a file name or a
/// size is said when it is known, and an unknown kind still gets a label: that something was sent
/// is worth showing.
fn describe_attachment(attachment: &Attachment) -> String {
    let size = attachment
        .approx_bytes
        .map(|b| format!("~{}", format_bytes(b)));
    let detail = |parts: &[Option<String>]| {
        let parts: Vec<&str> = parts.iter().flatten().map(String::as_str).collect();
        (!parts.is_empty()).then(|| format!(" ({})", parts.join(", ")))
    };
    match &attachment.kind {
        AttachmentKind::Image => format!(
            "🖼 image{}",
            detail(&[attachment.mime.clone(), size]).unwrap_or_default()
        ),
        AttachmentKind::Audio => "🔊 audio".to_string(),
        AttachmentKind::File => match (&attachment.name, &attachment.mime) {
            (Some(name), _) => format!("📄 {name}{}", detail(&[size]).unwrap_or_default()),
            (None, mime) => format!(
                "📄 file{}",
                detail(&[mime.clone(), size]).unwrap_or_default()
            ),
        },
        AttachmentKind::Other(kind) => format!("📎 {kind}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::decoders;

    /// The decoder the embedded vocabulary has for `path`, as the proxy picks it.
    fn api_of(path: &str) -> Option<String> {
        decoders()
            .iter()
            .find(|d| d.matches(path))
            .map(|d| d.id.clone())
    }

    #[test]
    fn the_embedded_vocabulary_claims_the_paths_llamacpp_always_had_and_ollamas_own() {
        for (path, api) in [
            ("/v1/chat/completions", "openai.chat"),
            ("/v1/chat/completions?stream=true", "openai.chat"),
            ("/chat/completions", "openai.chat"),
            ("/v1/completions", "openai.completions"),
            ("/completions", "openai.completions"),
            ("/completion", "llamacpp.completion"),
            ("/api/chat", "ollama.chat"),
            ("/api/generate", "ollama.generate"),
        ] {
            assert_eq!(api_of(path).as_deref(), Some(api), "{path}");
        }
        for path in [
            "/health",
            "/",
            "/v1/embeddings",
            "/props",
            "/v1/models",
            "/api/tags",
            "/api/pull",
        ] {
            assert_eq!(api_of(path), None, "{path} is relayed and not shown");
        }
    }

    fn summary_of(path: &str, body: &str) -> Option<(Option<String>, Vec<String>)> {
        let decoders = decoders();
        let decoder = decoders.iter().find(|d| d.matches(path)).unwrap();
        let body: Value = serde_json::from_str(body).unwrap();
        decoder.request(Some(&body)).map(|s| {
            (
                s.text,
                s.attachments.iter().map(describe_attachment).collect(),
            )
        })
    }

    #[test]
    fn a_request_is_described_as_it_always_was() {
        let chat =
            r#"{"messages":[{"role":"system","content":"sys"},{"role":"user","content":"hello"}]}"#;
        assert_eq!(
            summary_of("/v1/chat/completions", chat),
            Some((Some("hello".to_string()), vec![]))
        );
        assert_eq!(
            summary_of("/completion", r#"{"prompt":"once upon a time"}"#),
            Some((Some("once upon a time".to_string()), vec![]))
        );
        let vision = r#"{"messages":[{"role":"user","content":[
            {"type":"text","text":"what is this?"},
            {"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8gd29ybGQ="}}
        ]}]}"#;
        assert_eq!(
            summary_of("/v1/chat/completions", vision),
            Some((
                Some("what is this?".to_string()),
                vec!["🖼 image (image/png, ~12 B)".to_string()]
            ))
        );
        let file = r#"{"messages":[{"role":"user","content":[
            {"type":"text","text":"summarize this"},
            {"type":"file","file":{"filename":"report.pdf","file_data":"data:application/pdf;base64,aGVsbG8="}},
            {"type":"input_audio","input_audio":{"data":"x"}},
            {"type":"hologram"}
        ]}]}"#;
        assert_eq!(
            summary_of("/v1/chat/completions", file),
            Some((
                Some("summarize this".to_string()),
                vec![
                    "📄 report.pdf (~6 B)".to_string(),
                    "🔊 audio".to_string(),
                    "📎 hologram".to_string()
                ]
            ))
        );
    }

    #[test]
    fn a_file_without_a_name_and_an_image_without_a_data_url_are_still_labelled() {
        let part = |kind, mime: Option<&str>, name: Option<&str>, bytes: Option<u64>| {
            describe_attachment(&Attachment {
                kind,
                mime: mime.map(str::to_string),
                name: name.map(str::to_string),
                approx_bytes: bytes,
            })
        };
        assert_eq!(part(AttachmentKind::Image, None, None, None), "🖼 image");
        assert_eq!(
            part(AttachmentKind::Image, None, None, Some(3)),
            "🖼 image (~3 B)"
        );
        assert_eq!(part(AttachmentKind::File, None, None, None), "📄 file");
        assert_eq!(
            part(AttachmentKind::File, Some("application/pdf"), None, Some(6)),
            "📄 file (application/pdf, ~6 B)"
        );
        assert_eq!(
            part(AttachmentKind::File, None, Some("a.txt"), None),
            "📄 a.txt"
        );
    }

    #[test]
    fn the_streaming_shapes_of_llamacpp_are_read_by_the_api_each_belongs_to() {
        let decoders = decoders();
        let by = |id: &str| decoders.iter().find(|d| d.id == id).unwrap();
        let chat: Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"hi"}}]}"#).unwrap();
        assert_eq!(by("openai.chat").delta(&chat).as_deref(), Some("hi"));
        let completions: Value = serde_json::from_str(r#"{"choices":[{"text":"hi"}]}"#).unwrap();
        assert_eq!(
            by("openai.completions").delta(&completions).as_deref(),
            Some("hi")
        );
        let legacy: Value = serde_json::from_str(r#"{"content":"hi","stop":false}"#).unwrap();
        assert_eq!(
            by("llamacpp.completion").delta(&legacy).as_deref(),
            Some("hi")
        );
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
            // the whole request is read before the answer, as a server does: a socket closed with
            // bytes of it unread is reset on Windows, and the reset can beat the answer to the proxy
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let head_end = request.windows(4).position(|w| w == b"\r\n\r\n");
                if let Some(end) = head_end {
                    let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
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
                let port = crate::testkit::free_port();
                spawn(
                    port,
                    upstream_port,
                    decoders(),
                    evt_tx.clone(),
                    wake.clone(),
                )
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
