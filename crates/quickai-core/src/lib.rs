//! QuickAI's pure kernel (docs/architecture.md, section 14).
//!
//! Everything that decides *what* the application runs lives here: the document schema, the
//! expression language, layered configuration loading, the registry, the planner and the form
//! model. Nothing here starts a process, opens a socket, touches the filesystem (configuration is
//! read through a `LayerSource` the caller supplies) or draws anything, so all of it is tested
//! headless and deterministically. `tests/purity.rs` enforces that.

#![forbid(unsafe_code)]

pub mod acquire;
pub mod advice;
pub mod api_info;
pub mod config;
pub mod decode;
pub mod diag;
pub mod doc;
pub mod docpath;
pub mod expr;
pub mod form;
pub mod guard;
pub mod heuristics;
pub mod kvlines;
pub mod loaderenv;
pub mod lookup;
pub mod models;
pub mod plan;
pub mod prerequisite;
pub mod profile;
pub mod registry;
pub mod stream;
pub mod telemetry;
pub mod value;
