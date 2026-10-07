//! The typed form of a vocabulary document (`kind = "vocab"`): the shared nouns that engines and
//! the interface refer to by id (docs/architecture.md, section 6.2). From work package 3.6 it holds
//! the APIs an engine can serve and how the proxy reads each one; the adapter kinds of Phase 4 join
//! it later. A vocabulary can name things and describe them; it cannot start a program or reach a
//! host.

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct VocabDoc {
    #[serde(default, rename = "modality")]
    pub modalities: Vec<Modality>,
    #[serde(default, rename = "api")]
    pub apis: Vec<ApiDef>,
}

/// A kind of input or output a model can have.
#[derive(Clone, Debug, Deserialize)]
pub struct Modality {
    pub id: String,
    pub label: Option<String>,
}

/// An API a client can call, and how the proxy reads it.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiDef {
    pub id: String,
    /// The paths of the API on the engine (`/v1/chat/completions`), exactly as a request has them.
    #[serde(default)]
    pub paths: Vec<String>,
    /// What a client can do with it: `chat`, `completion`, `embedding`.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// How the Live tab reads the traffic of this API; without it the proxy only relays it.
    pub decode: Option<DecodeSpec>,
    /// Curl examples for the API panel.
    #[serde(default)]
    pub examples: Vec<ApiExample>,
}

/// Where the text is, in the requests and replies of an API (JSON pointers, RFC 6901).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DecodeSpec {
    /// How a reply comes: `sse`, `ndjson`, `json`. The type the engine says it is sending chooses
    /// among them.
    #[serde(default)]
    pub framing: Vec<String>,
    /// Where the prompt is in a request that is not a chat.
    pub prompt: Option<String>,
    /// Where the new text is in an event of a stream; the first pointer that gives a text wins.
    #[serde(default)]
    pub delta: Vec<String>,
    /// Where the text is in a reply that comes whole.
    #[serde(default, rename = "final")]
    pub final_text: Vec<String>,
    /// Where the engine says what went wrong, in a reply or an event.
    #[serde(default)]
    pub error: Vec<String>,
    /// The shape of a chat request.
    pub chat: Option<ChatSpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChatSpec {
    /// The list of messages in the request.
    pub messages: String,
    /// Within a message: who says it.
    pub role: String,
    /// Within a message: what is said.
    pub content: String,
    /// Messages of these roles are not what the person asked.
    #[serde(default)]
    pub skip_roles: Vec<String>,
    /// Within a part of a content that is a list: its type and its text.
    pub parts: Option<PartsSpec>,
    /// Within a message: a list of base64 images.
    pub images: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PartsSpec {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
}

/// One curl example for the API panel.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiExample {
    pub label: String,
    /// Shown only while this holds (`has(model.modalities.input, 'image')`).
    pub when: Option<String>,
    pub curl: String,
}
