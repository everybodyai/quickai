# QuickAI

A Rust/[egui](https://github.com/emilk/egui) GUI for starting and stopping
local GGUF models via `llama-server` (from
[llama.cpp](https://github.com/ggml-org/llama.cpp)), built with the
**Vulkan** backend so inference runs on the AMD GPU (RX 6700 XT / RADV)
instead of ROCm, which doesn't officially support this card. Packages into
a Linux tar.gz, a self-integrating AppImage, and a Windows zip — each
bundled with the recommended model, ready to run standalone.

## Layout

- `crates/quickai/` — the GUI (`main.rs`, `app.rs`, `server_manager.rs`,
  `models.rs`, `settings.rs`, `paths.rs`). Binary name: `quickai`.
- `llama.cpp/` — vendored llama.cpp source (its own git checkout —
  gitignored here, see "First-time setup" below), built locally with
  `-DGGML_VULKAN=ON`. Binaries live in `llama.cpp/build/bin/`.
- `models/` — GGUF files the GUI lists. Gitignored (large binaries, not
  source); running any `build/*` script fetches the bundled models
  automatically if missing (`fetch_model()` in `build/common.sh`,
  downloaded from `prism-ml/Bonsai-8B-gguf` and `prism-ml/Bonsai-1.7B-gguf`
  on Hugging Face). The others below were symlinked from
  `~/repo/fillpdf/models/` (that's where they were first downloaded) and
  aren't fetched automatically:
  - `Bonsai-8B-Q1_0.gguf` — Bonsai 8B, 1-bit (Q1_0), ~1.1 GB, Apache-2.0.
    Bundled into every packaged build.
  - `Bonsai-1.7B-Q1_0.gguf` / `bonsai-1.7b-q1_0.gguf` — Bonsai 1.7B, 1-bit
    (Q1_0), ~0.25 GB. Also bundled into every packaged build, as the
    lighter fallback if the 8B runs slowly. Both filenames are recognized;
    packaged builds fetch the properly-cased one.
  - `qwen3-1.7b-q8_0.gguf` / `qwen3-4b-q4_k_m.gguf` — instruction-tuned
    Qwen3 models, small and larger respectively.

  Which model is actually **recommended** in the GUI isn't fixed to either
  Bonsai model — it's computed at startup from this machine's total RAM
  (see `hardware.rs`): the largest available model that comfortably fits
  is preferred, falling back to the smallest one on constrained hardware.
  The two bundled Bonsai models get a subtle green highlight in the picker
  regardless, as the two safe, always-available choices.

  All of these use the plain 1-bit **Q1_0** GGUF format, which mainline
  llama.cpp already supports. PrismML also publishes **ternary (Q2_0)**
  Bonsai variants (better quality, ~50% bigger) and a 27B multimodal
  variant, but per PrismML's own docs the ternary ones currently need a
  separate PrismML fork of llama.cpp — skipped here in favor of mainline.
- `assets/` — app icon (PNG/ICO/hicolor set for the AppImage).
- `build/` — packaging scripts (see below).
- `dist/` — packaged output. Gitignored, along with `target/`, `models/`,
  `llama.cpp/`, and the packaging scripts' own caches (`build/.cache/`,
  `build/AppDir/`, `build/appimagetool-*.AppImage`) — see `.gitignore`.

The AppImage self-registers a desktop launcher
(`~/.local/share/applications/quickai.desktop`) the first time it's run.
Delete that file if you don't want it there.

## First-time setup

`llama.cpp/` is gitignored (it's a vendored checkout, not this project's
source), so a fresh clone needs it built once before `cargo run` or
`build/linux.sh`/`build/appimage.sh` will work:

```bash
git clone https://github.com/ggml-org/llama.cpp
cd llama.cpp
cmake -B build -G Ninja -DGGML_VULKAN=ON -DCMAKE_BUILD_TYPE=Release
cmake --build build -j$(nproc)
```

Requires the Vulkan build deps: `libvulkan-dev`, `glslc`, `glslang-tools`,
`spirv-headers`, `vulkan-tools`, `ninja-build`.

`models/` is also gitignored; the bundled model doesn't need a manual step
though — any `build/*.sh` script (or `windows.bat`) fetches it into
`models/` automatically the first time, if it's not already there.

## Running it (dev)

```bash
cargo run --release -p quickai
```

In the window: pick a model, optionally adjust port / context size / GPU
layers, click **Start**, watch the log pane until the status turns green
("Running"), then **Open Chat UI** to open llama-server's built-in web
chat in your browser, or **🔌 API** to show the base URL/API key/model
name for connecting an external OpenAI-compatible client instead. **Stop**
kills the server. Only one model runs at a time — Start is disabled while
something is already running or starting.

Picking a different model resets context size to a suggested default for
it (`hardware::recommend_ctx_size` — bigger file, or less RAM headroom,
means a smaller suggestion); it's still a plain editable field afterward,
this only sets the starting point.

Top-right of the window: a subtle theme toggle (☀/🌙, persisted across
runs), a speed icon (🐢/🐂/🐎) that estimates how fast the *currently
selected* model will run on this machine — from its file size relative to
detected RAM, downgraded a tier if GPU layers is `0`. It's a rough,
hardware-only estimate (no VRAM detection, no live token/sec measurement),
not a benchmark — hover it for the exact reasoning — and next to it, **⚙**
(settings, see below) and **ℹ** (about/usage/license). Most other controls
(model picker, port, context size, GPU layers, the download fields, the
main buttons) have their own hover tooltips explaining what they do and,
for the numeric ones, what values make sense.

Settings (last-used model, port, context size, GPU layers, plus everything
in ⚙ below) persist between runs as JSON in the OS config dir
(`~/.config/quickai/` on Linux, via the `directories` crate).

## Settings (⚙)

Click the gear icon next to the speed indicator for:

- **Downloaded models folder** — where "Model URL / Path", "Import", and
  "Look up online" save files (defaults to `~/.quickai/models`; see
  "Adding more models" below). Change it and click **Apply** to switch and
  re-scan, or **Reset to default**.
- **Advanced: llama.cpp** (collapsed by default):
  - **llama-server binary** — point at a custom build instead of the
    bundled/auto-detected one (e.g. a different backend or newer version).
    Blank uses the default.
  - **Extra command-line arguments** — appended verbatim to the
    llama-server invocation after port/context-size/GPU-layers, e.g.
    `--threads 8 --flash-attn on`. Space-separated; wrap a value containing
    spaces in `"double quotes"`. An unsupported flag will make the server
    fail to start — check the log pane. Hover the field for more example
    flags.

Both folder/binary overrides and the extra-args string persist across
restarts alongside the other settings.

In dev (`cargo run`), the app falls back to this repo's own `llama.cpp/`
and `models/` paths (see `crates/quickai/src/paths.rs`); in a packaged build
it looks for `llama-server`/bundled `models/` right next to its own
executable instead. Either way, that directory is treated as read-only —
see "Adding more models" below for where downloads actually go.

## Adding more models

Three ways, from most to least manual:

- **Drop a file in**: put any `.gguf` file into the bundled `models/`
  folder (next to the executable in a packaged build, or this repo's
  `models/` in dev) and click **Refresh** in the GUI. To give it a
  friendly display name/description instead of just its filename, add an
  entry to `KNOWN_MODELS` in `crates/quickai/src/models.rs`.
- **Model URL / Path**: paste a direct link to a `.gguf` file, or a local
  filesystem path to one (Linux or Windows style — `/home/you/model.gguf`,
  `C:\Users\you\model.gguf`, a UNC share, whatever this OS understands),
  into the field and click **Download**/**Import** (the button relabels
  itself based on what you typed, and is disabled until the URL/path looks
  usable — for a path, that means the file has to actually exist). Either
  way it lands in `~/.quickai/models` and gets auto-selected once done. A
  URL streams over HTTP as `<name>.gguf.part`, renamed on completion, and
  works for any file host, not just Hugging Face. A local path is hard-linked
  in when possible (instant, no extra disk space, same-filesystem only) and
  falls back to a real chunked copy — with the same progress bar — when it
  isn't (e.g. a different drive).
- **Look up online**: click **Look up online** to search Hugging Face for
  the current top GGUF-tagged repos (`remote.rs`), sorted by downloads as
  a rough proxy for "best", plus High/Medium/Small sections bucketed by
  file size so there's something browsable regardless of hardware. For
  each repo it inspects the file listing and picks one reasonably-sized
  quantization (preferring `Q4_K_M`-style K-quants, skipping sharded
  multi-part files and MTP draft-model files), building the direct
  `resolve/main/...` download URL. Click **Get** on a result to download
  it the same way as a pasted URL (also into `~/.quickai/models`). This is
  a heuristic best-effort pick, not a guarantee of quality or llama.cpp
  compatibility for every quant format that exists on the Hub —
  mainstream K-quants work; unusual/experimental formats may not. The
  result list sits in a fixed-height, scrollable, collapsible section (the
  collapsing-header arrow tucks the list away without discarding it; the
  **X** at the top right of the Model URL row closes the whole section,
  and only lights up once there are results on screen to close) so a long
  list of results doesn't take over the window. It scrolls both ways: a
  bottom scrollbar appears for any entry too wide for the panel, rather
  than clipping it or stretching the window — the panel keeps the same
  width as the rest of the window and only grows if you resize the window
  itself.

Downloads always land in `~/.quickai/models` rather than next to the
executable: a packaged build's own directory can be read-only (an
AppImage's squashfs mount, most obviously) or owned by another user, so it
isn't a reliable write target on any platform. The GUI lists models from
both `~/.quickai/models` and the bundled `models/` together — a downloaded
file persists there and is picked up by **Refresh** and remembered by
Settings across restarts like any other model, it's just not in the same
folder as the ones the package shipped with.

## GPU layers setting

`999` (the default) tells llama.cpp to offload every layer it can to the
GPU. Set it to `0` to force CPU-only if you ever want to compare, or a
smaller number to partially offload a model too large for VRAM.

## Packaging

```bash
build/all.sh          # linux tar.gz + AppImage + windows zip
build/linux.sh         # Linux tar.gz only
build/appimage.sh      # AppImage only
build/windows.sh       # Windows zip, cross-compiled via `cross` + Docker
build\windows.bat      # Windows zip, run natively ON Windows instead
```

Every package bundles: the GUI binary, `llama-server` + its runtime libs,
both `models/Bonsai-8B-Q1_0.gguf` and `models/Bonsai-1.7B-Q1_0.gguf`, and a
short end-user `README.md` (`build/PACKAGE_README.md`). Output lands in
`dist/`.

- **Linux/AppImage** ship this repo's own Vulkan-accelerated `llama-server`
  build, plus only the llama-specific `.so`s it needs (`libggml*`,
  `libllama*`, `libmtmd*`) — not libc/libstdc++/Vulkan-loader, which are
  assumed present on any modern desktop with a working GPU driver, same as
  llama.cpp itself assumes.
- **Windows** does *not* cross-compile llama.cpp (a C++/CMake/Vulkan
  project — `cross` only handles the Rust half). Instead it downloads the
  **official prebuilt Vulkan build** from llama.cpp's own GitHub releases
  for the same version tag the local build came from (`LLAMA_RELEASE_TAG`
  in `build/common.sh`, currently `b11007`), caching it under
  `build/.cache/`. Bump that tag if `llama.cpp/` is ever updated/rebuilt.
- The GUI binary itself cross-compiles cleanly via `cross` (already set up
  on this machine — same tool/Docker image the sibling `jsonquery` project
  uses for its Windows builds).

All three were built and smoke-tested end-to-end while setting this up:
extracted tar.gz resolves its bundled `.so`s standalone (verified with
`ldd`), the AppImage self-integrates a desktop entry and runs its bundled
`llama-server` from the mounted `usr/bin/`, and the Windows `.exe`/DLLs
were verified as valid PE32+ binaries (can't execute Windows binaries on
this machine, so that side wasn't runtime-tested).

## Updating llama.cpp

The build dependencies installed for this (`libvulkan-dev`, `glslc`,
`glslang-tools`, `spirv-headers`, `vulkan-tools`, `ninja-build`) are now on
the system, so `git pull` + rebuild works going forward:

```bash
cd llama.cpp
git pull
cmake -B build -G Ninja -DGGML_VULKAN=ON -DCMAKE_BUILD_TYPE=Release
cmake --build build -j$(nproc)
```

`vulkaninfo --summary` confirms the GPU is visible to Vulkan
(`AMD Radeon RX 6700 XT (RADV NAVI22)`) if anything ever needs re-checking.

## License

QuickAI's own source code is licensed under the PolyForm Noncommercial License 1.0.0
(`LICENSE`): free for personal and other noncommercial use. It also ships two things under
their own separate licenses, reproduced in full in `THIRD_PARTY_LICENSES.md`:
the `llama-server` binary built from llama.cpp (MIT), and the bundled
Bonsai-8B/Bonsai-1.7B model weights from PrismML (Apache-2.0, unmodified).
Neither requires QuickAI's own code to share their license — each covers
only the files it applies to. The in-app **ℹ** button summarizes all of
this plus a usage/disclaimer blurb.
