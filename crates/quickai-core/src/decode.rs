//! Reading an API's traffic (work package 3.6, docs/phase3-test-plan.md group D): what the proxy
//! makes of a request and of the reply, from the `decode` block of the API's definition in the
//! vocabulary (docs/examples/vocab/core.toml).
//!
//! Pure: JSON values in, text out. The proxy relays every byte untouched and asks here only what
//! to show; a shape this module does not recognise is a blank entry, never a failed relay.
//! Splitting a stream into events is `stream`'s; this knows where the text is inside one.

use serde::Serialize;
use serde_json::Value;

use crate::stream::sentence;

/// How an API's replies come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    /// Server-sent events, `data: {json}` and a final `data: [DONE]`.
    Sse,
    /// One JSON object per line.
    Ndjson,
    /// One JSON body.
    Json,
}

impl Framing {
    pub fn parse(name: &str) -> Option<Framing> {
        match name {
            "sse" => Some(Framing::Sse),
            "ndjson" => Some(Framing::Ndjson),
            "json" => Some(Framing::Json),
            _ => None,
        }
    }
}

/// How one reply is to be read: decided by its content type among the framings the API has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Event by event.
    Sse,
    /// Line by line.
    Ndjson,
    /// When it is all there.
    Whole,
    /// Not something this API says how to read.
    Skip,
}

/// Where the text of a message is, in a request of a chat API.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChatShape {
    /// The list of messages.
    pub messages: String,
    /// Within a message: who says it.
    pub role: String,
    /// Within a message: what is said, a text or a list of typed parts.
    pub content: String,
    /// Messages of these roles are not what the person asked (the system's).
    pub skip_roles: Vec<String>,
    /// Within a part of a list: its type and its text.
    pub parts: Option<PartsShape>,
    /// Within a message: base64 images (Ollama's native chat).
    pub images: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PartsShape {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
}

/// One API as the proxy reads it. Plain data: the plan carries it to the session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApiDecoder {
    pub id: String,
    /// The paths of the API, exactly (a query is not part of a path).
    pub paths: Vec<String>,
    pub framing: Vec<Framing>,
    /// A request without a chat in it: where its prompt is.
    pub prompt: Option<String>,
    /// In an event of a stream: where the new text is (the first that gives a text wins).
    pub delta: Vec<String>,
    /// In a reply that comes whole: where the text is.
    pub final_text: Vec<String>,
    /// In a reply or an event: where the engine says what went wrong.
    pub error: Vec<String>,
    pub chat: Option<ChatShape>,
}

/// What a request said, for the entry in the Live tab.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestSummary {
    pub text: Option<String>,
    pub attachments: Vec<Attachment>,
}

/// Something that came with the text, described and not kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub kind: AttachmentKind,
    /// From a data URL, or from the first bytes of a base64 image.
    pub mime: Option<String>,
    pub name: Option<String>,
    /// About how big, from the length of the base64.
    pub approx_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Audio,
    File,
    /// A part type this module has no word for.
    Other(String),
}

impl ApiDecoder {
    /// True for a request to one of this API's paths (a query string is not looked at).
    pub fn matches(&self, path: &str) -> bool {
        let path = path.split('?').next().unwrap_or(path);
        self.paths.iter().any(|p| p == path)
    }

    /// How to read a reply with this content type: the streaming framings are chosen by the type
    /// the engine says it is sending, and a whole JSON body is what is left.
    pub fn reading(&self, content_type: &str) -> Reading {
        let content_type = content_type.to_ascii_lowercase();
        let has = |f: Framing| self.framing.contains(&f);
        if content_type.contains("text/event-stream") && has(Framing::Sse) {
            Reading::Sse
        } else if (content_type.contains("ndjson") || content_type.contains("jsonl"))
            && has(Framing::Ndjson)
        {
            Reading::Ndjson
        } else if has(Framing::Json) {
            Reading::Whole
        } else {
            Reading::Skip
        }
    }

    /// What the request was about, or `None` when it was not a conversation: a load or an unload
    /// goes through the same path as a prompt and says none (`keep_alive` and nothing else), and
    /// must not appear as an empty chat. A body that is not JSON is shown as it always was, blank.
    pub fn request(&self, body: Option<&Value>) -> Option<RequestSummary> {
        let Some(body) = body else {
            return Some(RequestSummary::default());
        };
        if let Some(chat) = &self.chat {
            return match body.pointer(&chat.messages) {
                Some(Value::Array(messages)) if messages.is_empty() => None,
                Some(Value::Array(messages)) => Some(
                    messages
                        .iter()
                        .rev()
                        .find(|m| {
                            let role = m.pointer(&chat.role).and_then(Value::as_str);
                            !role.is_some_and(|r| chat.skip_roles.iter().any(|s| s == r))
                        })
                        .map(|message| summarize_message(chat, message))
                        .unwrap_or_default(),
                ),
                _ => Some(RequestSummary::default()),
            };
        }
        let prompt = self.prompt.as_ref()?;
        match body.pointer(prompt) {
            Some(Value::String(text)) if text.is_empty() => None,
            Some(Value::String(text)) => Some(RequestSummary {
                text: Some(text.clone()),
                attachments: Vec::new(),
            }),
            Some(_) => Some(RequestSummary::default()),
            None => None,
        }
    }

    /// The new text of one event of a stream (possibly empty).
    pub fn delta(&self, event: &Value) -> Option<String> {
        first_text(event, &self.delta)
    }

    /// The text of a reply that came whole.
    pub fn final_text(&self, body: &Value) -> Option<String> {
        first_text(body, &self.final_text)
    }

    /// What the engine said went wrong, when this reply or event says so.
    pub fn error(&self, value: &Value) -> Option<String> {
        self.error
            .iter()
            .filter_map(|p| value.pointer(p))
            .find(|v| !v.is_null())
            .map(sentence)
    }
}

fn first_text(value: &Value, pointers: &[String]) -> Option<String> {
    pointers
        .iter()
        .find_map(|p| value.pointer(p).and_then(Value::as_str))
        .map(str::to_string)
}

/// The last message that is not the system's: its text, and what came with it.
fn summarize_message(chat: &ChatShape, message: &Value) -> RequestSummary {
    let mut summary = match message.pointer(&chat.content) {
        Some(Value::String(text)) => RequestSummary {
            text: Some(text.clone()),
            attachments: Vec::new(),
        },
        Some(Value::Array(parts)) => summarize_parts(chat, parts),
        _ => RequestSummary::default(),
    };
    if let Some(Value::Array(images)) = chat.images.as_ref().and_then(|p| message.pointer(p)) {
        for image in images.iter().filter_map(Value::as_str) {
            summary.attachments.push(Attachment {
                kind: AttachmentKind::Image,
                mime: sniff_image(image),
                name: None,
                approx_bytes: Some(image.len() as u64 * 3 / 4),
            });
        }
    }
    summary
}

/// A content that is a list of typed parts (OpenAI's vision and file shape): the text parts are
/// joined for the prompt, every other part is described.
fn summarize_parts(chat: &ChatShape, parts: &[Value]) -> RequestSummary {
    let Some(shape) = &chat.parts else {
        return RequestSummary::default();
    };
    let mut text = Vec::new();
    let mut attachments = Vec::new();
    for part in parts {
        match part.pointer(&shape.kind).and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = part.pointer(&shape.text).and_then(Value::as_str) {
                    text.push(t.to_string());
                }
            }
            Some(other) => attachments.push(describe_part(other, part)),
            None => {}
        }
    }
    RequestSummary {
        text: (!text.is_empty()).then(|| text.join(" ")),
        attachments,
    }
}

/// One part that is not text, by its declared type, with a file name or a data URL's type and size
/// when it has them. An unknown type still gets a label: that something was sent is worth showing.
fn describe_part(kind: &str, part: &Value) -> Attachment {
    let from_url = |url: Option<&str>| url.and_then(data_url).unwrap_or((None, None));
    match kind {
        "image_url" => {
            let (mime, approx_bytes) =
                from_url(part.pointer("/image_url/url").and_then(Value::as_str));
            Attachment {
                kind: AttachmentKind::Image,
                mime,
                name: None,
                approx_bytes,
            }
        }
        "input_audio" => Attachment {
            kind: AttachmentKind::Audio,
            mime: None,
            name: None,
            approx_bytes: None,
        },
        "file" | "input_file" | "document" => {
            let name = part
                .pointer("/file/filename")
                .or_else(|| part.pointer("/file/file_name"))
                .or_else(|| part.get("filename"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let url = part
                .pointer("/file/file_data")
                .or_else(|| part.pointer("/file/url"))
                .or_else(|| part.pointer("/source/data"))
                .and_then(Value::as_str);
            let (mime, approx_bytes) = from_url(url);
            Attachment {
                kind: AttachmentKind::File,
                mime,
                name,
                approx_bytes,
            }
        }
        other => Attachment {
            kind: AttachmentKind::Other(other.to_string()),
            mime: None,
            name: None,
            approx_bytes: None,
        },
    }
}

/// `data:<mime>;base64,<payload>` as its declared type and about how many bytes the payload
/// decodes to (base64 grows data by a third): a rough label of how big, not a count.
fn data_url(url: &str) -> Option<(Option<String>, Option<u64>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.split(';').next().filter(|s| !s.is_empty());
    Some((
        Some(mime.unwrap_or("unknown type").to_string()),
        Some(payload.len() as u64 * 3 / 4),
    ))
}

/// The type of an image from the first characters of its base64 (the signature of PNG, JPEG, GIF
/// and WebP files), when it has one of those.
fn sniff_image(base64: &str) -> Option<String> {
    [
        ("iVBORw0KGgo", "image/png"),
        ("/9j/", "image/jpeg"),
        ("R0lGOD", "image/gif"),
        ("UklGR", "image/webp"),
    ]
    .iter()
    .find(|(prefix, _)| base64.starts_with(prefix))
    .map(|(_, mime)| mime.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat(parts: bool, images: bool) -> ChatShape {
        ChatShape {
            messages: "/messages".into(),
            role: "/role".into(),
            content: "/content".into(),
            skip_roles: vec!["system".into()],
            parts: parts.then(|| PartsShape {
                kind: "/type".into(),
                text: "/text".into(),
            }),
            images: images.then(|| "/images".into()),
        }
    }

    fn decoder(chat: Option<ChatShape>, prompt: Option<&str>, framing: &[Framing]) -> ApiDecoder {
        ApiDecoder {
            id: "t.chat".into(),
            paths: vec!["/v1/chat".into(), "/chat".into()],
            framing: framing.to_vec(),
            prompt: prompt.map(str::to_string),
            delta: vec!["/choices/0/delta/content".into(), "/message/content".into()],
            final_text: vec!["/choices/0/message/content".into()],
            error: vec!["/error".into()],
            chat,
        }
    }

    #[test]
    fn a_path_matches_exactly_without_its_query() {
        let d = decoder(None, None, &[Framing::Json]);
        assert!(d.matches("/v1/chat"));
        assert!(d.matches("/chat?stream=true"));
        for no in ["/v1/chat/x", "/v1", "/", "/health", "v1/chat", "/CHAT"] {
            assert!(!d.matches(no), "{no}");
        }
    }

    #[test]
    fn the_content_type_and_the_apis_framings_decide_how_a_reply_is_read() {
        let both = decoder(None, None, &[Framing::Sse, Framing::Json]);
        assert_eq!(both.reading("text/event-stream"), Reading::Sse);
        assert_eq!(
            both.reading("text/event-stream; charset=utf-8"),
            Reading::Sse
        );
        assert_eq!(both.reading("application/json"), Reading::Whole);
        assert_eq!(both.reading(""), Reading::Whole);
        // a framing the API does not have is not used
        assert_eq!(both.reading("application/x-ndjson"), Reading::Whole);
        let ollama = decoder(None, None, &[Framing::Ndjson, Framing::Json]);
        assert_eq!(ollama.reading("application/x-ndjson"), Reading::Ndjson);
        assert_eq!(ollama.reading("Application/X-NDJSON"), Reading::Ndjson);
        assert_eq!(ollama.reading("text/event-stream"), Reading::Whole);
        let streams_only = decoder(None, None, &[Framing::Sse]);
        assert_eq!(streams_only.reading("application/json"), Reading::Skip);
    }

    #[test]
    fn a_chat_request_is_the_last_message_that_is_not_the_systems() {
        let d = decoder(Some(chat(true, false)), None, &[Framing::Json]);
        let body = json!({"messages": [
            {"role": "user", "content": "first"},
            {"role": "assistant", "content": "answer"},
            {"role": "user", "content": "second"},
            {"role": "system", "content": "late system"},
        ]});
        let summary = d.request(Some(&body)).unwrap();
        assert_eq!(summary.text.as_deref(), Some("second"));
        assert!(summary.attachments.is_empty());
        // only system messages: still a conversation, with nothing to say
        let only_system = json!({"messages": [{"role": "system", "content": "s"}]});
        assert_eq!(
            d.request(Some(&only_system)),
            Some(RequestSummary::default())
        );
    }

    #[test]
    fn an_empty_list_of_messages_is_a_load_and_not_a_conversation() {
        let d = decoder(Some(chat(false, false)), None, &[Framing::Json]);
        assert_eq!(
            d.request(Some(&json!({"model": "m", "messages": []}))),
            None
        );
        assert!(
            d.request(Some(&json!({"model": "m"}))).is_some(),
            "no list at all is shown as it always was"
        );
        assert!(d.request(None).is_some(), "so is a body that is not JSON");
    }

    #[test]
    fn a_prompt_request_is_its_prompt_and_an_absent_or_empty_one_is_a_load() {
        let d = decoder(None, Some("/prompt"), &[Framing::Json]);
        let said = d
            .request(Some(&json!({"prompt": "once upon a time"})))
            .unwrap();
        assert_eq!(said.text.as_deref(), Some("once upon a time"));
        assert_eq!(
            d.request(Some(&json!({"model": "m", "keep_alive": -1}))),
            None
        );
        assert_eq!(
            d.request(Some(&json!({"model": "m", "prompt": "", "keep_alive": 0}))),
            None
        );
        // a prompt that is not a text (a list of tokens) is a conversation with nothing to say
        assert_eq!(
            d.request(Some(&json!({"prompt": [1, 2, 3]}))),
            Some(RequestSummary::default())
        );
    }

    #[test]
    fn the_parts_of_a_content_are_joined_and_the_rest_described() {
        let d = decoder(Some(chat(true, false)), None, &[Framing::Json]);
        let body = json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "what is"},
            {"type": "text", "text": "this?"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8gd29ybGQ="}},
            {"type": "input_audio", "input_audio": {"data": "x"}},
            {"type": "file", "file": {"filename": "report.pdf", "file_data": "data:application/pdf;base64,aGVsbG8="}},
            {"type": "hologram"},
            {"no_type": true},
        ]}]});
        let summary = d.request(Some(&body)).unwrap();
        assert_eq!(summary.text.as_deref(), Some("what is this?"));
        assert_eq!(
            summary.attachments,
            [
                Attachment {
                    kind: AttachmentKind::Image,
                    mime: Some("image/png".into()),
                    name: None,
                    approx_bytes: Some(12)
                },
                Attachment {
                    kind: AttachmentKind::Audio,
                    mime: None,
                    name: None,
                    approx_bytes: None
                },
                Attachment {
                    kind: AttachmentKind::File,
                    mime: Some("application/pdf".into()),
                    name: Some("report.pdf".into()),
                    approx_bytes: Some(6)
                },
                Attachment {
                    kind: AttachmentKind::Other("hologram".into()),
                    mime: None,
                    name: None,
                    approx_bytes: None
                },
            ]
        );
    }

    #[test]
    fn ollamas_images_are_attachments_with_the_type_their_first_bytes_give() {
        let d = decoder(Some(chat(false, true)), None, &[Framing::Ndjson]);
        let png = "iVBORw0KGgo".repeat(40);
        let body = json!({"messages": [{"role": "user", "content": "what is this", "images": [png, "AAAA", "/9j/4AAQ"]}]});
        let summary = d.request(Some(&body)).unwrap();
        assert_eq!(summary.text.as_deref(), Some("what is this"));
        let kinds: Vec<_> = summary
            .attachments
            .iter()
            .map(|a| (&a.kind, a.mime.as_deref(), a.approx_bytes))
            .collect();
        assert_eq!(
            kinds,
            [
                (&AttachmentKind::Image, Some("image/png"), Some(330)),
                (&AttachmentKind::Image, None, Some(3)),
                (&AttachmentKind::Image, Some("image/jpeg"), Some(6)),
            ]
        );
    }

    #[test]
    fn deltas_final_text_and_errors_are_found_by_their_pointers() {
        let d = decoder(None, None, &[Framing::Sse, Framing::Json]);
        assert_eq!(
            d.delta(&json!({"choices": [{"delta": {"content": "hi"}}]}))
                .as_deref(),
            Some("hi")
        );
        assert_eq!(
            d.delta(&json!({"message": {"content": "yo"}})).as_deref(),
            Some("yo"),
            "the next pointer"
        );
        assert_eq!(
            d.delta(&json!({"choices": [{"delta": {"role": "assistant"}}]})),
            None
        );
        assert_eq!(
            d.delta(&json!({"choices": [{"delta": {"content": null}}]})),
            None
        );
        assert_eq!(
            d.final_text(&json!({"choices": [{"message": {"content": "all"}}]}))
                .as_deref(),
            Some("all")
        );
        // an error is a text, or an object with a message, or what JSON there is
        assert_eq!(
            d.error(&json!({"error": "model 'x' not found"})).as_deref(),
            Some("model 'x' not found")
        );
        assert_eq!(
            d.error(&json!({"error": {"message": "too long", "code": 400}}))
                .as_deref(),
            Some("too long")
        );
        assert_eq!(d.error(&json!({"error": null})), None);
        assert_eq!(d.error(&json!({"choices": []})), None);
    }
}
