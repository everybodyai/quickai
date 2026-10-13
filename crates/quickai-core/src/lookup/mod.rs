//! Looking models up online, the part of it that is pure (docs/lookup-and-bundles.md,
//! docs/architecture.md section 8: `[[models.lookup]]`). What is asked of the network and when is the
//! application's; what an address is, what the answer says, what kind of model it describes, which
//! buttons fetch it and which files go together is here, so that it is tested on answers recorded
//! from the real sites (`crates/quickai-runtime/tests/fixtures/`) and never needs the internet.
//!
//! ```text
//! source ──► Candidate ──present(rules)──► Entry { type, offers } ──arrange(filter)──► sections
//! ```

pub mod bundle;
pub mod candidate;
pub mod catalog;
pub mod glob;
pub mod huggingface;
pub mod ollama_library;
pub mod present;
pub mod quant;
pub mod size;
pub mod types;
pub mod view;
