//! The typed form of a `profile` document, which the people who use QuickAI call a
//! **configuration**: what runs and how, in a small file that can be saved, chosen and given to
//! someone else (docs/configurations.md).
//!
//! It is a document of the data tier: it can pick things the registry already knows (an engine, a
//! model, a build, the value of a setting) and cannot name a program. It can say where a model file
//! can be had from (a place and a name in it, or an address), because the model is never in the
//! file: whoever runs the configuration fetches it, and the window only says it. What each key may
//! hold is checked against the engines' documents when it is read (`profile::read`); this module is
//! only its shape.

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
    /// What the file cannot carry and whoever runs it has to say (a token, a setting that belongs to a
    /// computer, a variable of a program's environment).
    #[serde(default)]
    pub needs: Vec<NeedDoc>,
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
    /// The configuration of quickai.everybodyai.dev it was fetched from.
    pub community: Option<CommunityDoc>,
}

/// `[meta] community`: a configuration of quickai.everybodyai.dev, and its version.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CommunityDoc {
    pub id: String,
    pub version: u32,
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

/// `[runtime]`: which program, and which program goes with it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RuntimeDoc {
    /// The label of a build in the manager's list (`v0.40.0`, `b11007-vulkan`): the name of the
    /// directory the build is installed in. Left out, the program the computer uses by default.
    pub build: Option<String>,
    /// The version the program said it was, for the notes.
    pub version: Option<String>,
    /// The programs that go with the engine (`[runtime.openwebui]`), each the same two keys, and any key
    /// this version does not know (`profile::read` tells them apart).
    #[serde(flatten)]
    pub others: BTreeMap<String, toml::Value>,
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
    /// is no entry of the recommended list. The window says it and never fetches from it by itself (the
    /// person asks, and looks it up); `quickai-run` fetches from it, once the person has read it.
    pub from: Option<String>,
    /// Where in the repository `from` names the file is (`split_files/vae/wan2.2_vae.safetensors`);
    /// left out, the file is at the top and is called `file`.
    pub path: Option<String>,
    /// The branch, tag or commit of the repository; left out, `main`. A commit is the same file for ever.
    pub revision: Option<String>,
    /// An address (`https://`) it can be fetched from, instead of `from`.
    pub url: Option<String>,
    /// The folder of the models folder the file is kept in (`diffusion_models`, `vae`), for an engine
    /// that finds its files by the folder they are in; left out, the one its document says for a file
    /// like this.
    pub into: Option<String>,
    /// The named reference, by the kind the engine accepts (`ollama = "qwen3:8b"`).
    #[serde(flatten)]
    pub refs: BTreeMap<String, toml::Value>,
}

/// `[[needs]]`: one thing that whoever runs the configuration has to give, or that the computer has to
/// have. Exactly one of `token`, `setting`, `env` and `system` says what.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NeedDoc {
    /// A login the fetching needs: `huggingface` or `github`.
    pub token: Option<String>,
    /// A setting that belongs to a computer, as `<engine>.<field>` (`comfyui.models_dir`).
    pub setting: Option<String>,
    /// A variable of a program's environment (`WEBUI_SECRET_KEY`).
    pub env: Option<String>,
    /// Something the computer has to have, as the id of a prerequisite of the engine's document
    /// (`c_compiler`): the document says how it is checked and what brings it, and the file only says
    /// that it is needed.
    pub system: Option<String>,
    /// The program an `env` goes to (`openwebui`); left out, the engine of the configuration.
    #[serde(rename = "for")]
    pub for_program: Option<String>,
    /// Whether the value is a secret, that is never shown (a token always is).
    #[serde(default)]
    pub secret: bool,
    /// Said to whoever has not given it.
    pub why: Option<String>,
}

/// `[app]`: settings of the application that are part of a run.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AppDoc {
    /// Whether the live-traffic proxy sits in front of the engine.
    pub traffic_proxy: Option<bool>,
}
