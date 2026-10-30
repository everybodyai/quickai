//! QuickAI without a window (docs/headless.md).
//!
//! Everything the application does to the computer lives here: it reads the documents that say what
//! can run, finds and installs the programs, fetches the models, builds the plan of a run from what a
//! person chose, starts the program and watches it, keeps the person's settings and configurations,
//! and puts the live-traffic proxy in front of an engine when asked. `quickai-core` is the pure part
//! below it (documents, expressions, the planner); the window (`quickai`) and the headless program
//! (`quickai-run`) are the clients above it, and neither knows how the other works.
//!
//! Nothing in this crate draws anything or names a toolkit: a test holds it to that
//! (`tests/purity.rs`), as the core is held to having no effects. What a client is told arrives as
//! events on channels, and the client decides how to show them.

pub mod assets;
pub mod bundles;
pub mod chats;
pub mod cli;
pub mod community;
pub mod config;
pub mod configuration_cli;
pub mod configurations;
pub mod downloader;
pub mod events;
#[cfg(any(test, feature = "testkit"))]
pub mod golden;
pub mod gpu;
pub mod hardware;
pub mod host;
pub mod jobs;
pub mod launch;
pub mod models;
#[cfg(test)]
mod p3;
pub mod paths;
pub mod ports;
pub mod prims;
pub mod provision;
pub mod proxy;
pub mod remote;
pub mod resolve;
pub mod run_file;
pub mod runtime;
pub mod settings;
pub mod sources;
pub mod telemetry;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
