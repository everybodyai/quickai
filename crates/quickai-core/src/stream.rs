//! Reading a streamed reply: newline-delimited JSON and server-sent events from bytes that arrive
//! in arbitrary pieces, and what the events of a model pull add up to (docs/phase3-test-plan.md,
//! group H; docs/engine-facts.md, O18).
//!
//! Pure: bytes in, values out. The splitters never decode half a line (a multi-byte character cut
//! by the end of a chunk waits for the rest of its line), and look only at the bytes that are new,
//! so a very long line is read in time proportional to its length.

use std::collections::HashSet;

use serde_json::Value;

/// A line longer than this is not read: it is reported as an error and skipped up to its end. No
/// engine sends one, and a stream that never ends a line must not grow memory without limit.
const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

/// A line of an NDJSON stream that was not JSON. The lines around it are unaffected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineError {
    /// Counted from 1, over the whole stream (blank lines count).
    pub line: usize,
    pub message: String,
}

/// Splits bytes into lines and reads each as JSON. A line ends at `\n` or `\r\n`; a blank line is
/// skipped; a multi-byte character cut by the end of a chunk is completed by the next one.
#[derive(Debug, Default)]
pub struct NdjsonSplitter {
    pending: Vec<u8>,
    lines_seen: usize,
    /// The current line was longer than `MAX_LINE_BYTES`: everything up to its end is dropped.
    skipping: bool,
}

impl NdjsonSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a piece of the stream; returns every line it completed, in order.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Value, LineError>> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            match rest.iter().position(|b| *b == b'\n') {
                Some(end) => {
                    self.take(&rest[..end]);
                    out.extend(self.line_done());
                    rest = &rest[end + 1..];
                }
                None => {
                    self.take(rest);
                    break;
                }
            }
        }
        out
    }

    /// The stream ended: a last line with no newline counts.
    pub fn finish(&mut self) -> Vec<Result<Value, LineError>> {
        if self.pending.is_empty() && !self.skipping {
            return Vec::new();
        }
        self.line_done().into_iter().collect()
    }

    fn take(&mut self, bytes: &[u8]) {
        if self.skipping {
            return;
        }
        if self.pending.len() + bytes.len() > MAX_LINE_BYTES {
            self.pending = Vec::new();
            self.skipping = true;
            return;
        }
        self.pending.extend_from_slice(bytes);
    }

    /// A line is complete: what to make of it, if anything.
    fn line_done(&mut self) -> Option<Result<Value, LineError>> {
        self.lines_seen += 1;
        let line = self.lines_seen;
        if std::mem::take(&mut self.skipping) {
            return Some(Err(LineError {
                line,
                message: format!("the line is longer than {MAX_LINE_BYTES} bytes and was skipped"),
            }));
        }
        let mut bytes = std::mem::take(&mut self.pending);
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        Some(serde_json::from_slice(&bytes).map_err(|e| LineError {
            line,
            message: e.to_string(),
        }))
    }
}

/// One server-sent event: the `event:` name when there was one, and the `data:` lines joined by
/// newlines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Splits bytes into server-sent events (a blank line ends one; `:` lines are comments; `\r\n`
/// and `\n` both end a line). Only `event` and `data` fields are kept; an event with no `data`
/// is not an event.
#[derive(Debug, Default)]
pub struct SseSplitter {
    pending: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            match rest.iter().position(|b| *b == b'\n') {
                Some(end) => {
                    self.pending.extend_from_slice(&rest[..end]);
                    let line = std::mem::take(&mut self.pending);
                    out.extend(self.line(&line));
                    rest = &rest[end + 1..];
                }
                None => {
                    self.pending.extend_from_slice(rest);
                    break;
                }
            }
        }
        out
    }

    /// The stream ended: an event with no closing blank line counts.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            out.extend(self.line(&line));
        }
        out.extend(self.dispatch());
        out
    }

    fn line(&mut self, raw: &[u8]) -> Option<SseEvent> {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        if raw.is_empty() {
            return self.dispatch();
        }
        if raw[0] == b':' {
            return None; // a comment
        }
        let text = String::from_utf8_lossy(raw);
        let (field, value) = match text.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (text.as_ref(), ""),
        };
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data.push(value.to_string()),
            _ => {} // `id`, `retry` and whatever else a server adds are not ours
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = self.event.take();
        if self.data.is_empty() {
            return None;
        }
        Some(SseEvent {
            event,
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

/// Where the numbers of a pull's events are, as JSON pointers (the engine document's
/// `progress = { ... }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressMap {
    /// What tells one layer from another (`/digest`).
    pub key: String,
    pub completed: String,
    pub total: String,
    pub status: String,
    /// An object that has this is a failure, whatever the HTTP status was (`/error`).
    pub error: String,
    /// The `status` text that ends the pull (`success`).
    pub done_status: String,
}

impl ProgressMap {
    /// Ollama's `/api/pull`.
    pub fn ollama_pull() -> Self {
        ProgressMap {
            key: "/digest".into(),
            completed: "/completed".into(),
            total: "/total".into(),
            status: "/status".into(),
            error: "/error".into(),
            done_status: "success".into(),
        }
    }
}

/// What one event of a pull means to the interface.
#[derive(Clone, Debug, PartialEq)]
pub enum PullUpdate {
    /// The engine's own words (`pulling manifest`, `verifying sha256 digest`) for the log.
    Status(String),
    /// How far along the whole pull is, from 0 to 1: bytes done over bytes expected, over every
    /// layer seen so far. Never goes down, and stays below 1 until the pull has finished.
    Progress(f32),
    Failed(String),
    Finished,
}

/// The most a pull shows before the engine says `success`: a layer that has all its bytes may not
/// have been checked and written yet.
const BEFORE_SUCCESS: f32 = 0.999;

/// Adds up the events of a pull. A real pull (the recording in `tests/fixtures/ollama/recorded`)
/// repeats a layer's `total` many times before any `completed`, never reports `completed` for a
/// small layer at all, and ends the big one a little short of its total: only `success` means
/// done.
#[derive(Debug)]
pub struct PullTracker {
    map: ProgressMap,
    /// Each layer by its key: the most bytes it has said it has, and the most it has said it needs.
    layers: Vec<(String, u64, u64)>,
    /// The highest fraction shown so far: the bar never goes back, however a late layer changes the sum.
    shown: Option<f32>,
    said: HashSet<String>,
    finished: bool,
    failed: bool,
}

impl PullTracker {
    pub fn new(map: ProgressMap) -> Self {
        PullTracker {
            map,
            layers: Vec::new(),
            shown: None,
            said: HashSet::new(),
            finished: false,
            failed: false,
        }
    }

    /// One event in, whatever it means out (possibly nothing: an event that changes no number
    /// and has no new words).
    pub fn feed(&mut self, event: &Value) -> Vec<PullUpdate> {
        if self.finished || self.failed {
            return Vec::new();
        }
        // an error comes inside an answer that said 200: a status check never sees it
        if let Some(error) = event.pointer(&self.map.error).filter(|e| !e.is_null()) {
            self.failed = true;
            return vec![PullUpdate::Failed(sentence(error))];
        }
        let mut updates = Vec::new();
        let status = event
            .pointer(&self.map.status)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        // each of the engine's lines once: a layer is announced many times before a byte moves
        if let Some(text) = status {
            if self.said.insert(text.to_string()) {
                updates.push(PullUpdate::Status(text.to_string()));
            }
        }

        let key = event.pointer(&self.map.key).and_then(Value::as_str);
        let number = |pointer: &str| {
            event.pointer(pointer).and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            })
        };
        if let (Some(key), Some(total)) = (key, number(&self.map.total)) {
            let completed = number(&self.map.completed);
            self.note_layer(key, completed.unwrap_or(0), total);
        }

        let done = status == Some(self.map.done_status.as_str());
        if done {
            self.finished = true;
        }
        if let Some(fraction) = self.advance() {
            updates.push(PullUpdate::Progress(fraction));
        }
        if done {
            updates.push(PullUpdate::Finished);
        }
        updates
    }

    /// The progress so far, `None` before any layer has said how big it is.
    pub fn fraction(&self) -> Option<f32> {
        self.shown
    }

    fn note_layer(&mut self, key: &str, completed: u64, total: u64) {
        match self.layers.iter_mut().find(|(k, _, _)| k == key) {
            Some((_, done, needs)) => {
                *done = (*done).max(completed);
                *needs = (*needs).max(total);
            }
            None => self.layers.push((key.to_string(), completed, total)),
        }
    }

    /// Works the fraction out from the layers; `Some` when it is a number the interface has not
    /// been given yet.
    fn advance(&mut self) -> Option<f32> {
        let needed: u64 = self.layers.iter().map(|(_, _, needs)| *needs).sum();
        let computed = if self.finished {
            // success: everything the engine named is there
            1.0
        } else if needed == 0 {
            return None;
        } else {
            // a layer is never more than complete, and nothing is done before `success`
            let done: u64 = self
                .layers
                .iter()
                .map(|(_, done, needs)| (*done).min(*needs))
                .sum();
            ((done as f64 / needed as f64) as f32).min(BEFORE_SUCCESS)
        };
        let next = self.shown.map_or(computed, |shown| shown.max(computed));
        if self.shown == Some(next) {
            return None;
        }
        self.shown = Some(next);
        Some(next)
    }
}

/// An `error` member as a sentence: the text itself, or the `message` of an object (OpenAI's
/// shape), or what JSON there is.
pub fn sentence(error: &Value) -> String {
    match error {
        Value::String(text) => text.clone(),
        Value::Object(map) => map
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| error.to_string(), str::to_string),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_line_longer_than_the_limit_is_dropped_up_to_its_end_and_the_next_line_is_read() {
        let mut s = NdjsonSplitter::new();
        let chunk = vec![b'x'; 8 * 1024 * 1024];
        let mut out = Vec::new();
        out.extend(s.push(b"{\"a\":1}\n"));
        for _ in 0..9 {
            out.extend(s.push(&chunk));
        }
        out.extend(s.push(b"\n{\"a\":2}\n"));
        assert_eq!(out.len(), 3, "{out:?}");
        assert_eq!(out[0], Ok(json!({"a": 1})));
        assert!(
            matches!(&out[1], Err(LineError { line: 2, message }) if message.contains("skipped"))
        );
        assert_eq!(out[2], Ok(json!({"a": 2})));
    }

    #[test]
    fn an_sse_event_with_only_a_name_is_not_an_event() {
        let mut s = SseSplitter::new();
        let events = s.push(b"event: ping\n\nid: 7\nretry: 100\n\ndata: x\n\n");
        assert_eq!(
            events,
            [SseEvent {
                event: None,
                data: "x".into()
            }]
        );
        // a field with no colon is that field with no value
        assert_eq!(
            s.push(b"data\n\n"),
            [SseEvent {
                event: None,
                data: String::new()
            }]
        );
    }

    #[test]
    fn an_error_member_is_a_sentence_whatever_shape_it_has() {
        assert_eq!(sentence(&json!("model not found")), "model not found");
        assert_eq!(
            sentence(&json!({"message": "rate limited", "type": "x"})),
            "rate limited"
        );
        assert_eq!(sentence(&json!({"code": 7})), "{\"code\":7}");
    }

    #[test]
    fn what_follows_an_error_or_the_end_of_a_pull_is_ignored() {
        let mut t = PullTracker::new(ProgressMap::ollama_pull());
        assert_eq!(
            t.feed(&json!({"error": "boom"})),
            [PullUpdate::Failed("boom".into())]
        );
        assert!(t.feed(&json!({"status": "pulling manifest"})).is_empty());
        let mut t = PullTracker::new(ProgressMap::ollama_pull());
        let updates = t.feed(&json!({"status": "success"}));
        assert_eq!(updates.last(), Some(&PullUpdate::Finished));
        assert!(t.feed(&json!({"status": "pulling manifest"})).is_empty());
    }
}
