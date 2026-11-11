# Developing QuickAI

This guide explains how to build QuickAI from source, run it and test it. For an overview of what QuickAI does, see
the [README](../README.md); for how it is designed, see the design records listed in [docs/README.md](README.md).

## Layout

- `crates/quickai-core/`: the pure logic (documents, expressions, the registry, the planner, the form model). It uses
  no processes, network, files or window, and a test enforces this.
- `crates/quickai-runtime/`: QuickAI without a window. It locates, installs, downloads, starts and monitors the
  programs a configuration specifies; stores the settings and configurations on disk; and holds the documents and
  assets the application ships (`manifests/`, `assets/`). It uses no GUI toolkit, and a test enforces this.
- `crates/quickai/`: the desktop application (`main.rs`, `app/`), binary `quickai`. It handles the user interface;
  everything it does to the computer is done through the runtime.
- `crates/quickai-headless/`: the headless program ([headless.md](headless.md)), binary `quickai-headless`. It uses the same
  runtime, without a window.
- `assets/`: the icon embedded in the binary (`icon.png` for the window, and `icon.ico` for the Windows executable,
  through `build.rs`).

Engines, the model catalog and the vocabularies are **data**: TOML documents under
`crates/quickai-runtime/manifests/`, documented key by key in [reference/](reference/). Many changes require no Rust
at all.

## First-time setup

You need Rust (stable, installed through [rustup](https://rustup.rs)) and, to run a model during development, a local
build of [llama.cpp](https://github.com/ggml-org/llama.cpp).

Speech-to-text for the microphone is handled by a separate program, `quickai-whisper` (whisper.cpp,
[everybodyai/quickai-whisper](https://github.com/everybodyai/quickai-whisper)), which the packages include next to the
application, as they include llama-server. When using `cargo run`, download the program from that repository's
releases (or build it there) and place it in `~/.quickai/speech/bin`, or set `QUICKAI_WHISPER_BIN` to its path.
Without it, the application works as before, and the microphone reports what is missing.

`llama.cpp/` and `models/` are ignored by git. Build llama.cpp once, with the Vulkan backend (which runs on AMD, NVIDIA
and Intel GPUs); released packages bundle a pinned official llama.cpp build instead, so packaging does not require
this:

```bash
git clone https://github.com/ggml-org/llama.cpp
cd llama.cpp
cmake -B build -G Ninja -DGGML_VULKAN=ON -DCMAKE_BUILD_TYPE=Release
cmake --build build -j$(nproc)
```

On Debian and Ubuntu, the Vulkan build requires `libvulkan-dev glslc glslang-tools spirv-headers vulkan-tools
ninja-build`. `vulkaninfo --summary` confirms that the GPU is visible to Vulkan.

Place GGUF models in `models/`; `cargo run` lists its contents. The model bundled with the packages is
[Bonsai 2 4B](https://huggingface.co/prism-ml/Ternary-Bonsai-2-4B-gguf) (`Ternary-Bonsai-2-4B-Q2_0.gguf`, about
1.2 GB), a ternary Q2_0 GGUF. Mainline llama.cpp runs it from release b11889 onward (the runtime from #29600; older
builds load the file but produce nonsense), and QuickAI starts it with thinking disabled.

If you have no `llama.cpp/` of your own, the application offers to install llama.cpp: **Install llama.cpp** downloads
the release that the packages ship into `~/.quickai/llama-bin`.

To update llama.cpp later, run `git pull` in `llama.cpp/` and run the two `cmake` commands again.

## Running QuickAI

```bash
cargo run --release -p quickai        # the window
cargo run --release -p quickai-headless -- --help
```

In development, the application falls back to this checkout's `llama.cpp/` and `models/`
(`crates/quickai-runtime/src/paths.rs`). A packaged build looks for `llama-server` and `models/` next to its own
executable instead. In either case, that folder is treated as read-only: downloads go to `~/.quickai/models`.

Settings are stored as JSON in the operating system's configuration folder (`~/.config/quickai/` on Linux,
`%APPDATA%\quickai` on Windows).

## Checks

CI runs these checks on Linux, Windows and macOS (`.github/workflows/ci.yml`). Run them before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p quickai -- config check --strict --embedded
```

Tests that require a real `llama-server` and model are marked `#[ignore]`, so `cargo test` passes on a fresh clone.
With `llama.cpp/build/bin/llama-server` built and `models/Ternary-Bonsai-2-4B-Q2_0.gguf` in place, run them with
`cargo test -p quickai -- --ignored`.

The application is also tested end to end through its GUI by a Robot Framework suite of several hundred test cases,
which runs the real executable against fake engines on a virtual display. The maintainers keep and run this suite
outside this repository.

## Design records

The [design records](README.md#design-records) explain why QuickAI is built the way it is: the declarative runtime,
the phases in which it was built, and every fact about the engines it relies on, with how each was verified. Read
[architecture.md](architecture.md) before changing the kernel.
