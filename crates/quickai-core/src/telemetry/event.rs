//! The shapes that are sent: a batch of events with the properties common to all of them (section 6.1 of
//! docs/reports-and-community.md). A crash report is a batch of one `crash.panic` event, sent to its own address.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::measure::hour_utc;

/// The version of the shapes below; the server refuses a batch of a version it does not know.
pub const SCHEMA: u32 = 1;

/// The build that sends: which version, from where, for which computer, in which kind of package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIdentity {
    pub version: String,
    /// `official` for the builds EverybodyAI releases, `source` for any other.
    pub channel: String,
    /// The commit it was built from, short; `unknown` when the build did not say.
    pub commit: String,
    /// The Rust target it was built for (`x86_64-unknown-linux-gnu`).
    pub target: String,
    /// How it was installed: one of `schema::PACKAGES`.
    pub package: String,
}

/// The properties of every event of a batch, sent once with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Common {
    /// The install id: random, made the first time anything is sent.
    pub install: String,
    /// Random for each start of the app, never kept.
    pub session: String,
    pub app: AppIdentity,
    /// `linux`, `windows` or `macos`.
    pub os: String,
    /// The system's version (`24.04`, `11`, `15.1`).
    pub os_version: String,
    /// The processor's architecture (`x86_64`, `aarch64`).
    pub arch: String,
    /// The memory, in the steps of `measure::memory_bucket_gb`.
    pub memory_gb: u32,
    /// The makers of the graphics adapters, as `gpu::detect` names them.
    pub gpus: Vec<String>,
    /// The system's language, without its region (`en`), or `unknown`.
    pub language: String,
    /// The level in force when the batch was made.
    pub level: String,
}

/// One thing that happened: its name (a row of `schema::EVENTS`), the hour it happened in, and its fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    pub time: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

impl Event {
    /// The event `name`, at the hour of `unix_seconds`, with no fields yet.
    pub fn new(name: &str, unix_seconds: u64) -> Self {
        Self {
            event: name.to_string(),
            time: hour_utc(unix_seconds),
            fields: Map::new(),
        }
    }

    /// The event with `key` set to `value`.
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(key.to_string(), value.into());
        self
    }

    /// The level of the event, from the schema; `None` for an event the schema does not have.
    pub fn level(&self) -> Option<super::Level> {
        super::schema::spec(&self.event).map(|spec| spec.level)
    }
}

/// What one request sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub schema: u32,
    /// Random for each batch: a batch sent twice is stored once.
    pub id: String,
    pub common: Common,
    pub events: Vec<Event>,
}

impl Batch {
    pub fn new(id: String, common: Common, events: Vec<Event>) -> Self {
        Self {
            schema: SCHEMA,
            id,
            common,
            events,
        }
    }
}
