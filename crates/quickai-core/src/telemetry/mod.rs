//! Telemetry (docs/reports-and-community.md, WP4.42): what QuickAI may send EverybodyAI, at which level, in which shape,
//! and how text is cleaned before anything leaves the computer.
//!
//! Pure, as the rest of this crate: the runtime keeps the ids, the queue and the sending (`quickai_runtime::telemetry`),
//! and the window asks the person. Everything that can be sent is a row of `schema::EVENTS`, with its level, the kind
//! of data each field is and why it is collected; an event that is not in the schema, or a field of the wrong shape,
//! is never queued. `quickai --telemetry` prints the schema, and docs/reference/telemetry-events.md is made from it.

pub mod clean;
pub mod event;
pub mod level;
pub mod measure;
pub mod schema;

pub use event::{AppIdentity, Batch, Common, Event};
pub use level::Level;
