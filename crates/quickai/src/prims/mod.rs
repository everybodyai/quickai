//! The primitives: the small audited vocabulary of actions that documents compose
//! (docs/architecture.md, section 8). Each has a name and a version; a document says which it
//! needs, and a document needing one this build lacks is skipped with a diagnostic.
//!
//! `locate.*` lives here; `spawn` and `probe.http` are in `runtime`, next to the supervisor that
//! drives them. `PROVIDED` is the one list of what exists.

pub mod feed;
pub mod install;
pub mod locate;

use quickai_core::registry::KnownPrimitives;

/// Every primitive this build provides, by `name@version`.
pub const PROVIDED: &[&str] = &[
    "locate.setting@1",
    "locate.next_to_exe@1",
    "locate.dev_tree@1",
    "feed.github_release@1",
    "install.archive@1",
    "spawn@1",
    "probe.http@1",
];

pub fn known() -> KnownPrimitives {
    KnownPrimitives::new(PROVIDED)
}
