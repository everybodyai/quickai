# Developing QuickAI

How to build QuickAI from source, run it, and test it. For what QuickAI does, see the [README](../README.md); for
how it is designed, the design records listed in [docs/README.md](README.md).

## Layout

- `crates/quickai-core/`: the pure part (documents, expressions, the registry, the planner, the form model).
  No processes, no network, no files, no window; a test holds it to that.
- `crates/quickai-runtime/`: QuickAI without a window. It finds, installs, fetches, starts and watches the
  programs a configuration names; keeps the settings and configurations on disk; and holds the documents and
  assets the application ships (`manifests/`, `assets/`). No GUI toolkit; a test holds it to that.
- `crates/quickai/`: the window (`main.rs`, `app/`), binary `quickai`. It draws; what it does to the
  computer is the runtime's.
- `crates/quickai-run/`: the headless program ([headless.md](headless.md)), binary `quickai-run`. The same
  runtime, with no window.
- `assets/`: the icon the binary embeds (`icon.png` for the window, `icon.ico` for the Windows executable,
  through `build.rs`).

Engines, the model catalog and the vocabularies are **data**: TOML documents under
`crates/quickai-runtime/manifests/`, described key by key in [reference/](reference/). Many changes need no Rust
at all.

## First-time setup

You need Rust (stable, through [rustup](https://rustup.rs)) and, to run a model in development, a local build of
[llama.cpp](https://github.com/ggml-org/llama.cpp). Released packages bundle a pinned official llama.cpp build
instead, so packaging does not need this.

`llama.cpp/` and `models/` are git-ignored. Build llama.cpp once, with the Vulkan backend (it runs on AMD, NVIDIA
and Intel GPUs):

```bash
git clone https://github.com/ggml-org/llama.cpp
cd llama.cpp
cmake -B build -G Ninja -DGGML_VULKAN=ON -DCMAKE_BUILD_TYPE=Release
cmake --build build -j$(nproc)
```

On Debian and Ubuntu the Vulkan build needs `libvulkan-dev glslc glslang-tools spirv-headers vulkan-tools
ninja-build`. `vulkaninfo --summary` confirms that the GPU is visible to Vulkan.

Put GGUF models in `models/`; `cargo run` lists what is in it. The two models the packages bundle are
[Bonsai 8B](https://huggingface.co/prism-ml/Bonsai-8B-gguf) (`Bonsai-8B-Q1_0.gguf`, about 1.1 GB) and
[Bonsai 1.7B](https://huggingface.co/prism-ml/Bonsai-1.7B-gguf) (`Bonsai-1.7B-Q1_0.gguf`, about 0.25 GB), both
1-bit Q1_0 GGUFs that mainline llama.cpp runs.

To update llama.cpp later: `git pull` in `llama.cpp/` and run the two `cmake` lines again.

## Running it

```bash
cargo run --release -p quickai        # the window
cargo run --release -p quickai-run -- --help
```

In development the app falls back to this checkout's `llama.cpp/` and `models/`
(`crates/quickai-runtime/src/paths.rs`). A packaged build looks for `llama-server` and `models/` next to its own
executable instead. Either way that folder is treated as read-only: downloads go to `~/.quickai/models`.

Settings persist as JSON in the OS config folder (`~/.config/quickai/` on Linux, `%APPDATA%\quickai` on Windows).

## Checks

CI runs these on Linux, Windows and macOS (`.github/workflows/ci.yml`). Run them before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p quickai -- config check --strict --embedded
```

The tests that need a real `llama-server` and model are `#[ignore]`d, so `cargo test` passes on a fresh clone.
With `llama.cpp/build/bin/llama-server` built and `models/Bonsai-8B-Q1_0.gguf` in place, run them with
`cargo test -p quickai -- --ignored`.

The window is also tested end to end, through its GUI, by a Robot Framework suite of several hundred cases. It
runs the real executable against fake engines on a virtual display. The maintainers keep and run it outside this
repository.

## Design records

The [design records](README.md#design-records) explain why QuickAI is built the way it is: the declarative
runtime, the phases it was built in, and every fact about the engines it relies on, with how each was checked.
Read [architecture.md](architecture.md) before changing the kernel.
