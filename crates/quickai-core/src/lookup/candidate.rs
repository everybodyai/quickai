//! What a look-up learned about one model, in the words of the place it looked and with no
//! judgement yet: the type, the buttons and the grouping are made from it by `present`.

/// One model a look-up found.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// What names it among the results: `ollama:qwen3`, `hf:unsloth/Qwen3-4B-GGUF`.
    pub key: String,
    /// What the window calls it: the name of the library model, the id of the repository.
    pub name: String,
    pub description: String,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    /// What the source says the model can do (an Ollama card's `tools`, `vision`), lower case.
    pub capabilities: Vec<String>,
    /// The pipeline tag of a Hugging Face repository (`image-text-to-text`), lower case.
    pub pipeline: Option<String>,
    /// The other tags of the repository, lower case.
    pub tags: Vec<String>,
    /// Where it is and what there is of it to fetch.
    pub origin: Origin,
}

/// Where a model is, and what it is made of there.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// A model of Ollama's library: the engine pulls it by name, in one of the sizes the card shows
    /// (`0.6b`, `8b`); a card that shows none is pulled by its name alone.
    Library { name: String, sizes: Vec<String> },
    /// A repository of Hugging Face: files, which QuickAI fetches itself.
    Hub {
        /// The root of the site, with no slash at the end.
        site: String,
        /// `owner/name`.
        repo: String,
        /// The branch or commit the files are fetched at (`main`).
        revision: String,
        /// Every file of the repository the look-up listed, with its size when it knows it.
        files: Vec<RemoteFile>,
    },
}

/// A file of a repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteFile {
    /// Its path in the repository (`MTP/mtp-x.gguf`).
    pub path: String,
    pub size: Option<u64>,
    /// What the bytes of the file hash to (lower case hexadecimal), when the site says: the hub states
    /// it for every large file it keeps (the ones in LFS), and a download checks it as the bytes arrive.
    pub sha256: Option<String>,
}

impl RemoteFile {
    pub fn new(path: &str, size: Option<u64>) -> Self {
        RemoteFile {
            path: path.to_string(),
            size,
            sha256: None,
        }
    }

    /// The same file, with the hash the site stated for it.
    pub fn with_sha256(mut self, sha256: &str) -> Self {
        self.sha256 = Some(sha256.to_ascii_lowercase());
        self
    }

    /// The last component of its path.
    pub fn name(&self) -> &str {
        super::glob::name_of(&self.path)
    }
}
