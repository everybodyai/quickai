//! Executing a run plan: the supervisor (`Session`), the process helpers behind `spawn@1` and the
//! readiness probe behind `probe.http@1` (docs/architecture.md, section 11).

pub mod probe;
pub mod process;
pub mod session;

pub use session::Session;
