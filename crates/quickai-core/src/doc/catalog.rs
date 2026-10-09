//! The typed form of a catalog document (`kind = "catalog"`): the models the application recommends,
//! with what is known of each and, per engine, how to get it and how to run it
//! (docs/catalog.md). A catalog can name files and say where they are; it cannot start a program or
//! add a command-line word: what an engine does with a model is the engine document's.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CatalogDoc {
    /// What the page of the list this document is called, for messages.
    pub title: Option<String>,
    /// The day the facts of the document were last checked against the sites, `YYYY-MM-DD`.
    pub verified: Option<String>,
    #[serde(default, rename = "model")]
    pub models: Vec<ModelDoc>,
}

/// One model of the list.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelDoc {
    /// What names it across every catalog document: lower case letters, digits and `.`, `_`, `-`.
    pub id: String,
    /// What the window calls it (default: the id).
    pub name: Option<String>,
    /// Who made it.
    pub by: Option<String>,
    /// Its type, an id of the vocabulary's `[[model_type]]` (default: the vocabulary's default type).
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// What it is good for, in a sentence or two.
    pub note: Option<String>,
    pub license: Option<String>,
    /// How big it is as its makers say (`4B`, `30B-A3B`).
    pub params: Option<String>,
    /// The tokens of context it was made for.
    pub context: Option<u64>,
    /// What it reads, as ids of the vocabulary's `[[modality]]` (default: text).
    #[serde(default)]
    pub input: Vec<String>,
    /// What else is worth knowing, as short words (`thinking`, `tools`, `multilingual`).
    #[serde(default)]
    pub features: Vec<String>,
    /// Whether it is one of the models "Start here" shows.
    #[serde(default)]
    pub start: bool,
    /// Whether the packages ship it, so that it counts as there when its file is in the folder of the
    /// models that come with the application.
    #[serde(default)]
    pub bundled: bool,
    /// A person's document sets it to take a shipped model out of the list.
    #[serde(default)]
    pub hidden: bool,
    /// How each engine gets and runs it, by the id of the engine. An engine that has no table here
    /// does not offer the model.
    #[serde(default)]
    pub run: BTreeMap<String, RunDoc>,
}

/// What one engine does with a model: the files it takes (an engine whose models are files) or the
/// name it pulls (an engine that keeps its own).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RunDoc {
    /// The repository the files are in, `owner/name`.
    pub repo: Option<String>,
    /// The commit the files are fetched at (default: `main`; the application's own documents give a
    /// commit, so that what was checked is what arrives).
    pub rev: Option<String>,
    /// One of the engine's `[[models.mode]]`: how the engine is started for this model.
    pub mode: Option<String>,
    /// The folder of the models folder the weights are kept in, when it is not the one the engine
    /// keeps weights in (`checkpoints`): for an engine that finds its files by the folder they are in.
    pub place: Option<String>,
    /// The weights, one entry for each quantization on offer.
    #[serde(default)]
    pub files: Vec<FileDoc>,
    /// The files that go with the weights, by the kind of component they are (`mmproj`).
    #[serde(default)]
    pub companions: BTreeMap<String, FileDoc>,
    /// The name the engine pulls (`qwen3.5:4b`).
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// What the pull downloads, in bytes.
    pub size: Option<u64>,
}

/// A file of a repository.
#[derive(Clone, Debug, Deserialize)]
pub struct FileDoc {
    /// Its path in the repository (`Qwen3.5-4B-Q4_K_M.gguf`).
    pub file: String,
    /// Its size in bytes.
    pub size: Option<u64>,
    /// Its SHA-256, lower case hexadecimal: what a download is checked against.
    pub sha256: Option<String>,
    /// What its button says (default: the quantization its name says).
    pub label: Option<String>,
    /// Whether it is the one the list recommends of the weights (default: the first).
    #[serde(default)]
    pub best: bool,
}
