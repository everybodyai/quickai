//! Test fixtures shared by the registry and planner tests.

use crate::config::{load, Layer, LoadOptions, MemorySource};

use super::{KnownPrimitives, Registry};

/// A small but complete engine document: every section the current build interprets.
pub const GOOD: &str = r#"schema = 1
kind = "engine"
id = "llamacpp"
requires = { primitives = ["spawn@1", "locate.setting@1"] }

[engine]
name = "llama.cpp"

[locate]
strategies = [
  { use = "setting", field = "bin_override" },
  { use = "next_to_exe", file = "llama-server" },
]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["--model", "${model.main.path}", "--host", "127.0.0.1", "--port", "${listen_port}"]
env = [ { name = "LD_LIBRARY_PATH", prepend = "${runtime.dir}", sep = ":" } ]

[launch.ready]
probe = { use = "http", path = "/health", expect = 200 }
timeout = "120s"
interval = "500ms"

[[field]]
id = "port"
type = "port"
default = 8080
legacy_key = "port"
bind.port = "public"
ui = { order = 10 }

[[field]]
id = "ctx_size"
type = "int"
range = [256, 131072]
default = { fn = "recommend_ctx_size", args = ["model.size_bytes", "sys.ram_bytes"] }
recompute_on = ["model"]
legacy_key = "ctx_size"
bind.arg = ["--ctx-size", "${value}"]
ui = { order = 20 }

[[field]]
id = "n_gpu_layers"
type = "int"
range = [0, 999]
default = 999
legacy_key = "n_gpu_layers"
bind.arg = ["--n-gpu-layers", "${value}"]
derive = { gpu_offload = "value > 0" }
ui = { order = 30 }

[[field]]
id = "extra_args"
type = "text"
default = ""
legacy_key = "extra_args"
bind.arg_split = true
ui = { order = 90 }

[[field]]
id = "bin_override"
type = "runtime_path"
legacy_key = "llama_bin_override"
"#;

pub fn known() -> KnownPrimitives {
    KnownPrimitives::new(&["spawn@1", "locate.setting@1", "probe.http@1"])
}

/// The registry of a single embedded engine document.
pub fn registry_from(text: &str) -> Registry {
    let e = MemorySource::new(&[("embedded:engines/llamacpp.toml", text)]);
    Registry::build(
        load(&[(Layer::Embedded, &e)], &LoadOptions::default()),
        &known(),
    )
}

/// The registry of `GOOD` alone.
pub fn good_registry() -> Registry {
    registry_from(GOOD)
}
