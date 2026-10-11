//! The typed form of a `profile` document, which the people who use QuickAI call a
//! **configuration**: what runs and how, in a small file that can be saved, chosen and given to
//! someone else (docs/configurations.md).
//!
//! It is a document of the data tier: it can pick things the registry already knows (an engine, a
//! model, a build, the value of a setting) and cannot name a program, a host or an address to fetch
//! from. What each key may hold is checked against the engines' documents when it is read
//! (`profile::read`); this module is only its shape.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileDoc {
    /// What the dropdown and the list call it, up to 60 characters (default: the id).
    pub name: Option<String>,
    /// For the person who is given the file. Nothing in it changes what runs.
    #[serde(default)]
    pub meta: MetaDoc,
    /// The engine it runs: one a person can choose, not a companion. Required.
    pub engine: String,
    /// The program, as who it is and not where it is.
    pub runtime: Option<RuntimeDoc>,
    /// The models it uses, as who they are and never as a path. The first is the one the engine loads.
    #[serde(default, rename = "model")]
    pub models: Vec<ModelDoc>,
    /// The value of settings, by the id of the engine and then the id of the field. Only what differs
    /// from the default is written. A companion's settings sit in a table of its own.
    #[serde(default)]
    pub values: BTreeMap<String, BTreeMap<String, toml::Value>>,
    /// Settings of the application that change a run.
    pub app: Option<AppDoc>,
}

/// `[meta]`: who made it, when, and for what. Coarse on purpose: nothing that names a person or a
/// computer.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct MetaDoc {
    /// A sentence or two from whoever made the file (`Qwen3-VL 8B on Ollama, with Open WebUI`).
    pub note: Option<String>,
    /// The application that wrote it (`QuickAI 0.1.0`).
    pub made_with: Option<String>,
    /// The kind of computer it was made on.
    pub made_on: Option<MadeOnDoc>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MadeOnDoc {
    /// `linux`, `windows` or `macos`.
    pub os: Option<String>,
    /// `x86_64` or `aarch64`.
    pub arch: Option<String>,
    /// The makers of the graphics cards (`amd`, `nvidia`, `intel`).
    #[serde(default)]
    pub gpu: Vec<String>,
    /// The memory, in whole gigabytes.
    pub memory_gb: Option<u64>,
}

/// `[runtime]`: which program.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RuntimeDoc {
    /// The label of a build in the manager's list (`v0.40.0`, `b11007-vulkan`): the name of the
    /// directory the build is installed in. Left out, the program the computer uses by default.
    pub build: Option<String>,
    /// The version the program said it was, for the notes.
    pub version: Option<String>,
}

/// `[[model]]`: one model, by exactly one of its sources: a name the engine lists (the key is one of
/// the engine's `accepts.refs`: `ollama = "qwen3:8b"`), an entry of the recommended list (`catalog`,
/// and `file` for one of its files), or a file of the models folder (`file`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ModelDoc {
    /// What it is for (`chat`, `embedding`). Default `main`.
    pub role: Option<String>,
    /// What people call it; nothing finds it by this.
    pub name: Option<String>,
    /// The id of an entry of the recommended list.
    pub catalog: Option<String>,
    /// A file name (never a path): the file of a `catalog` entry, or of the models folder.
    pub file: Option<String>,
    /// Its size in bytes, to recognise it by and to say what getting it costs.
    pub size: Option<u64>,
    /// Its SHA-256, lower case hexadecimal.
    pub sha256: Option<String>,
    /// Where it can be got from, as a kind of place and a name (`huggingface:owner/repo`), for a file that
    /// is no entry of the recommended list. It is said to whoever has not got the model and is never fetched
    /// from by itself: the person asks, and looks it up.
    pub from: Option<String>,
    /// The named reference, by the kind the engine accepts (`ollama = "qwen3:8b"`).
    #[serde(flatten)]
    pub refs: BTreeMap<String, toml::Value>,
}

/// `[app]`: settings of the application that are part of a run.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AppDoc {
    /// Whether the live-traffic proxy sits in front of the engine.
    pub traffic_proxy: Option<bool>,
}
