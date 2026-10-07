//! The typed form of a vocabulary document (`kind = "vocab"`): the shared nouns that engines and
//! the interface refer to by id (docs/architecture.md, section 6.2). From work package 3.6 it holds
//! the APIs an engine can serve and how the proxy reads each one; from WP4.9 the types of model a
//! look-up sorts what it found into and the classes of size it filters by (docs/lookup-and-bundles.md);
//! the adapter kinds of Phase 4 join it later. A vocabulary can name things and describe them; it
//! cannot start a program or reach a host.

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct VocabDoc {
    #[serde(default, rename = "modality")]
    pub modalities: Vec<Modality>,
    #[serde(default, rename = "api")]
    pub apis: Vec<ApiDef>,
    /// What a look-up sorts models into: the first whose rule holds, in the order written.
    #[serde(default, rename = "model_type")]
    pub model_types: Vec<ModelTypeDef>,
    /// The bands of size a look-up filters by, from the smallest up.
    #[serde(default, rename = "size_class")]
    pub size_classes: Vec<SizeClassDoc>,
}

/// A type of model: Chat, Code, Vision, Embedding, ...
#[derive(Clone, Debug, Deserialize)]
pub struct ModelTypeDef {
    pub id: String,
    /// The heading of its section (default: the id).
    pub label: Option<String>,
    pub help: Option<String>,
    /// Where its section stands among the others: lower first (default: after the ones that say, in
    /// the order written).
    pub order: Option<i64>,
    /// What a model is when no type's rule holds. At most one.
    #[serde(default)]
    pub default: bool,
    /// What makes a model one of this type; any one hit is enough.
    #[serde(default, rename = "match")]
    pub rule: TypeRuleDef,
}

/// What a type's `match` may say, every list in lower or any case.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TypeRuleDef {
    /// What a source says the model can do (an Ollama card's `vision`).
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The pipeline tag of a Hugging Face repository (`image-text-to-text`).
    #[serde(default)]
    pub pipelines: Vec<String>,
    /// Another tag of the repository.
    #[serde(default)]
    pub tags: Vec<String>,
    /// A word of the model's name; four letters or more also match the start of one.
    #[serde(default)]
    pub words: Vec<String>,
    /// A kind of companion file found beside the model (`mmproj`).
    #[serde(default)]
    pub companions: Vec<String>,
}

/// A band of sizes.
#[derive(Clone, Debug, Deserialize)]
pub struct SizeClassDoc {
    pub id: String,
    /// What the filter says (`Small (< 4 GB)`; default: the id).
    pub label: Option<String>,
    pub help: Option<String>,
    /// The size it ends below (`4GiB`, `500MB`); left out, the class takes every size beyond the
    /// last bound and is written last.
    pub below: Option<String>,
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
