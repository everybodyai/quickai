//! The primitives: the small audited vocabulary of actions that documents compose
//! (docs/architecture.md, section 8). Each has a name and a version; a document says which it
//! needs, and a document needing one this build lacks is skipped with a diagnostic.
//!
//! `locate.*` lives here; `spawn` and `probe.http` are in `runtime`, next to the supervisor that
//! drives them. `PROVIDED` is the one list of what exists.

pub mod engine_models;
pub mod exec_probe;
pub mod feed;
pub mod http_call;
pub mod hub;
pub mod install;
pub mod locate;
pub mod lookup;

use quickai_core::registry::KnownPrimitives;

/// Every primitive this build provides, by `name@version`.
pub const PROVIDED: &[&str] = &[
    "locate.setting@1",
    "locate.next_to_exe@1",
    "locate.dev_tree@1",
    "locate.installed@1",
    "locate.path_search@1",
    "locate.known_paths@1",
    "exec.probe@1",
    "feed.github_release@1",
    "feed.pypi@1",
    "lookup.ollama_library@1",
    "lookup.huggingface_gguf@1",
    "install.archive@1",
    "install.python_app@1",
    "spawn@1",
    "attach@1",
    "probe.http@1",
    "http_call@1",
    "decode@1",
];

/// The primitives this build provides, the widgets its interface draws and the files it carries, which is
/// what documents are held to when they load.
pub fn known() -> KnownPrimitives {
    KnownPrimitives::new(PROVIDED)
        .with_widgets(crate::app::NAMED_WIDGETS)
        .with_assets(&crate::assets::names())
}
