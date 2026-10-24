# QuickAI

A Rust/[egui](https://github.com/emilk/egui) GUI for starting and stopping
local GGUF models via `llama-server` (from
[llama.cpp](https://github.com/ggml-org/llama.cpp)) or, if you choose it,
[Ollama](https://ollama.com) (see "Choosing an engine" below), built with the
**Vulkan** backend so inference runs on the AMD GPU (RX 6700 XT / RADV)
instead of ROCm, which doesn't officially support this card. It is packaged
as a Linux tar.gz, a self-integrating AppImage, and a Windows zip — each
bundled with the recommended model, ready to run standalone — by the separate
`quickai_base` repository, which holds all the packaging and release tooling.

## Installing a release

Not published anywhere yet — the sections below describe how each channel
will work starting with the first tagged release (the packaging files are in
place in `quickai_base`; nothing has been pushed/tagged/submitted so far).

### AUR (Arch-based distros)

An AUR package (`quickai-bin`); its PKGBUILD is in `quickai_base`'s
`packaging/aur/`.

### Homebrew (Linux)

```bash
brew tap nujufas/quickai
brew install quickai
```

If Homebrew refuses the formula as an untrusted tap, run
`brew trust nujufas/quickai` first.

### Snap (Linux)

```bash
sudo snap install quickai
```

### Download a build

Prebuilt Linux/AppImage and Windows binaries, attached to each
[GitHub Release](https://github.com/nujufas/QuickAI/releases). You can also
build everything yourself with the scripts in `quickai_base` — see
"Packaging" below.

## On a server, with no window

`quickai-run` is QuickAI without the window: it takes a configuration (the file the window exports), gets what it needs on that computer (programs
and models, after printing what that is and how big), starts it and stays until it is stopped. It is for a rented server. It listens on the loopback
only; reach it with an SSH tunnel.

```sh
quickai-run init  vision-chat.toml      # the file to fill in: what the configuration needs from you, and what is your computer's
quickai-run check vision-chat.toml      # what would be done, from where, to where; changes nothing
quickai-run up    vision-chat.toml      # do it, start it, stay (Ctrl-C or SIGTERM stops it)
```

A configuration says where the files of its models can be had from (a repository and the file in it, a revision, or an address) and never carries them; it says which program goes with the engine
(Open WebUI beside Ollama, and which build of it) and what it needs from whoever runs it (a login, a folder, a variable). What is yours (a folder, a login, a mirror, a copy of a file you already have,
the variables of a program's environment) you say in the file `init` writes, or on the command line (`--environment`, `--set`, `--env`, `--model-source`, `--models-from`, `--hf-token`, `--offline`).
[docs/examples/profiles/wan22-ti2v-turbo.toml](docs/examples/profiles/wan22-ti2v-turbo.toml) is a whole example (the three files of a video model). [docs/examples/profiles/minimax-h3.toml](docs/examples/profiles/minimax-h3.toml) is another: both builds of MiniMax H3 (seven files, 65 GB) for an NVIDIA card of 48 GB, with a licence that does not cover the EU, UK, South Korea or USA; [minimax-h3-fl2va.toml](docs/examples/profiles/minimax-h3-fl2va.toml) is its lean half (the build for words and first and last frame: five files, 42 GB).

A configuration can also say what the computer has to have (`[[needs]] system = "c_compiler"`: the engine's document says how each is looked at and which packages bring it). `quickai-run` looks
before it fetches anything, says what is missing with the exact command (`sudo apt-get install -y build-essential python3.12-dev`, and code 6), installs the packages only with `--install-system-packages`,
and, after the install and before the models are fetched, builds and runs a tiny Triton kernel once, so that the first clip is not the one that finds out the server lacks a compiler or `Python.h`
([docs/headless.md](docs/headless.md) section 12).

[docs/headless.md](docs/headless.md) says what it does, what has been run for real and what has not. The base repository's `build/headless.sh` builds
and packages it.

## Layout

- `crates/quickai-core/` — the pure part: documents, expressions, the registry, the planner, the form model.
  No processes, no network, no files, no window (a test holds it to that).
- `crates/quickai-runtime/` — QuickAI without a window: finding, installing, fetching, starting and watching the
  programs a configuration names, the settings and the configurations on disk, the documents and assets the
  application ships (`manifests/`, `assets/`). No toolkit (a test holds it to that).
- `crates/quickai/` — the window (`main.rs`, `app/`). Binary name: `quickai`. It draws; what it does to the
  computer is the runtime's.
- `crates/quickai-run/` — the headless program (docs/headless.md): the same runtime with no window, for a
  server. Binary name: `quickai-run`.
- `llama.cpp/` — vendored llama.cpp source (its own git checkout —
  gitignored here, see "First-time setup" below), built locally with
  `-DGGML_VULKAN=ON`. Binaries live in `llama.cpp/build/bin/`.
- `models/` — GGUF files the GUI lists under `cargo run`. Gitignored (large
  binaries, not source). The two Bonsai models come from
  `prism-ml/Bonsai-8B-gguf` and `prism-ml/Bonsai-1.7B-gguf` on Hugging Face;
  `build/fetch-models.sh --link-into <this checkout>/models` in `quickai_base`
  downloads them (pinned and checksummed) and puts them here. The others
  below were symlinked from `~/repo/fillpdf/models/` (that's where they were
  first downloaded) and aren't fetched automatically:
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
- `assets/` — the app icon the binary embeds: `icon.png` (the window icon,
  `include_bytes!` in `main.rs`) and `icon.ico` (the Windows exe's icon
  resource, `build.rs`). The generator that draws it and every packaging asset
  derived from it (hicolor sizes, MSIX and Store images) live in
  `quickai_base/assets/`.
- `target/`, `models/` and `llama.cpp/` are gitignored (see `.gitignore`).
  Packaged output goes to `quickai_base/dist/`, not here.

The AppImage self-registers a desktop launcher
(`~/.local/share/applications/quickai.desktop`) the first time it's run.
Delete that file if you don't want it there.

## First-time setup

`llama.cpp/` is gitignored (it's a vendored checkout, not this project's
source), so a fresh clone needs it built once before `cargo run` will work
(packaging doesn't need it: `quickai_base` bundles a pinned official llama.cpp
release instead):

```bash
git clone https://github.com/ggml-org/llama.cpp
cd llama.cpp
cmake -B build -G Ninja -DGGML_VULKAN=ON -DCMAKE_BUILD_TYPE=Release
cmake --build build -j$(nproc)
```

Requires the Vulkan build deps: `libvulkan-dev`, `glslc`, `glslang-tools`,
`spirv-headers`, `vulkan-tools`, `ninja-build`.

`models/` is also gitignored: `cargo run` lists what is in it. The two
real-model integration tests in `server_manager.rs` are `#[ignore]`d by default
so that `cargo test` passes on a fresh clone and on CI, where neither
`llama.cpp/` nor `models/` exists; run them with
`cargo test -p quickai -- --ignored` once `llama.cpp/build/bin/llama-server` is
built and `models/Bonsai-8B-Q1_0.gguf` is in place (see "Layout" above for how
to fetch the model).

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

If "Show live conversation traffic" is turned on in ⚙ Settings, a **📡
Live** tab also appears here, next to **Main** — see "Live conversation
traffic" below.

Picking a different model resets context size to a suggested default for
it (`hardware::recommend_ctx_size` — bigger file, or less RAM headroom,
means a smaller suggestion); it's still a plain editable field afterward,
this only sets the starting point.

The window reads from top to bottom as the steps of a run: the **configuration** in use, the **model** (the picker and,
under it, the box that gets another one: a link, a file of this computer or a name to search for, with **Look up online**
and **Recommended** beside it), the **run options** (port, context size, GPU layers: they are dimmed, with the reason, while
the server runs), the buttons (**Start** is the green one while the server is stopped, **Open Chat UI** while it
runs) with the state in words, and under them the log. At the end of the picker's row a speed chip (🐢/🐂/🐎) estimates how
fast the *currently selected* model will run on this machine — from its file size relative to detected RAM, downgraded a tier
if GPU layers is `0`. It's a rough, hardware-only estimate (no VRAM detection, no live token/sec measurement), not a
benchmark — hover it for the exact reasoning. Under the picker one row of chips says what else is known of the model: what it
can do (an engine that lists its models says *vision*, *tools*, *thinking*), a green **★ Recommended** when it is the one this
computer is recommended (hover it for the RAM that was detected and why), and *from the last run* when the engine is not
running and the list is what it said then; a model the application knows has the first sentence of its note beside them (the
rest is there when the pointer is on it). In a window wider than about 960 px the cards keep that width, in the middle.

Top-right of the window: a pill that says what the server is doing (Stopped, Starting, Running or Crashed; it is there on
the Live tab too), then **ℹ** (about/usage/license), **⚙** (settings, see below) and a subtle theme toggle (☀/🌙, persisted
across runs). Most other controls (model picker, port, context size, GPU layers, the download fields, the
main buttons) have their own hover tooltips explaining what they do and, for the numeric ones, what values make sense.

Settings (last-used model, port, context size, GPU layers, plus everything
in ⚙ below) persist between runs as JSON in the OS config dir
(`~/.config/quickai/` on Linux, via the `directories` crate).

## Settings (⚙)

Click the gear icon in the top-right corner. The settings open in a **window of their own**, not on top of the main
one: move it beside the main window or to another screen, make it any size (what is in it scrolls, and **Close** stays
at its foot), and go on using the main window while it is open. Click the gear again, press **Close** or close the
window and what you changed is saved; closing the main window closes it too. It has:

- **Downloaded models folder** — where the **Get** box, "Import", and
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

- **Show live conversation traffic** — off by default. Turning it on adds
  a small local proxy in front of llama-server (see "Live conversation
  traffic" below) and reveals a **📡 Live** tab; off, clients connect
  straight to llama-server with no extra hop. Takes effect on the next
  Start, so it's disabled while a server is running.

Both folder/binary overrides, the extra-args string, and the live-traffic
toggle persist across restarts alongside the other settings.

In dev (`cargo run`), the app falls back to this repo's own `llama.cpp/`
and `models/` paths (see `crates/quickai-runtime/src/paths.rs`); in a packaged build
it looks for `llama-server`/bundled `models/` right next to its own
executable instead. Either way, that directory is treated as read-only —
see "Adding more models" below for where downloads actually go.

## Choosing an engine (⚙ → Engine)

Models run on **llama.cpp**, which comes with QuickAI and is what a first run uses, on **Ollama**, or, for pictures and
video, on **ComfyUI**. Ollama and ComfyUI are never part of a package and nothing is downloaded, listed or started
because you chose one: pick it in the Engine row of ⚙ Settings and the window shows what it needs instead of what
llama.cpp does; the settings of each engine are kept separately, and the row is locked while a server runs or something
is being fetched.

- **Getting Ollama.** If an `ollama` is on your `PATH` (or where Ollama usually installs itself) it is used.
  Otherwise open ⚙ Settings → **Advanced: Ollama** → **Look up Ollama builds** and click **Use** on a row (the first
  is the release QuickAI was tested with): QuickAI downloads it from GitHub, checks its checksum, unpacks it under
  `~/.quickai/ollama` and runs it once to see that it works. A checksum that does not match, or a download that is
  cut off, installs nothing.
- **A first run is two steps.** Ollama lists its models only while it runs. **Start** brings it up with nothing
  loaded (or uses an Ollama that is already running on that port, and leaves it running when you stop); it then
  lists what it has. Pull a model by typing a name (`qwen3:4b`) or a Hugging Face reference
  (`hf.co/user/repo:Q4_K_M`) into the **Model** box and pressing Enter or **Pull**, choose a model, and press
  **Start** again to load it. With a model already chosen, one Start does both.
- **Finding a model.** **Look up online**, beside **Pull**, searches the library at ollama.com for the words in the
  box (the models it shows first when the box is empty) and lists what it finds the way llama.cpp's look-up
  does: in sections by type (**Chat**, **Vision**, **Embedding**, ...), with the **Size** filter, and for each
  model a name, what it can do, how often it was pulled and a button for every size it is offered in
  (`4b · ~2.2 GB`: the `~` says the size is an estimate, the library gives none) that pulls it exactly as typing
  `qwen3:4b` would. It needs no Ollama running, only the internet, and asks nothing until you press it. ollama.com
  publishes no list a program can read, so QuickAI reads the page of the site's own search, and says so if the
  page ever stops looking like one (a name typed into the box still pulls); only the library's own models are
  listed, not people's uploads (type `user/name` to pull one).
- **What Ollama is told.** It listens on this machine only, with its own calls home switched off
  (`OLLAMA_NO_CLOUD=1`), keeps one model loaded, and keeps its models in `~/.quickai/ollama/models` unless you
  ask for Ollama's own store. Its advanced settings are what to do when one is already running, the model
  store, extra environment (`KEY=VALUE` lines), the executable, the GPU and the context size.
- **A chat page for Ollama: Open WebUI** (`docs/openwebui.md`). `ollama serve` has no page, so the button next to **Stop**
  says **Set up chat UI...** until you have one, and opens ⚙ Settings → **Chat UI: Open WebUI**. Tick *Start Open WebUI
  with Ollama*, then **Look up Open WebUI builds** → **Use** on a row (the first is the release QuickAI was tested with):
  QuickAI downloads its wheel from the Python Package Index, checks it, and installs it with what it needs (about 3 GB,
  in four minutes or so) into a Python environment **of its own** under `~/.quickai/openwebui`, which is neither
  ComfyUI's nor any Python you have; it needs Python 3.12 or 3.11 on the machine and installs none. From then on it
  starts after Ollama answers (`Ollama ready ... · Open WebUI on port 3000`), **Open Chat UI** opens it, and it stops
  with Ollama; if it stops by itself Ollama goes on and the window says so. It is started on this machine only and
  **offline** (no update check, no model fetched, nothing installed for a tool) unless you set its internet access to online;
  its documents are read with Ollama (pull `nomic-embed-text`, or choose another model in its own settings); its
  accounts, chats and files are in `~/.quickai/openwebui/data`, and it writes nothing anywhere else. The first account you
  make on its page is the administrator's. It is under the Open WebUI License (BSD-3-Clause with a condition that its
  name and logo are left as they are), is never part of a package, and is downloaded only when you ask.
- **ComfyUI** (`docs/comfyui.md`) makes pictures and video from workflows you build in its own page. **Getting it:** ⚙ Settings →
  **Advanced: ComfyUI** → **Look up ComfyUI builds** → **Use** on a row (the first is the release QuickAI was tested
  with). It needs a Python 3.10 or newer that can make an environment (on Debian and Ubuntu `sudo apt install
  python3-venv`; QuickAI installs no Python), downloads the source of the release and a pack of GGUF nodes (checked
  against checksums), makes an environment of its own and installs PyTorch for your graphics card into it, and runs it
  once to see that it works: **about twenty minutes and 17 GB with PyTorch for AMD, about five minutes and 7 GB with
  PyTorch for CUDA 13** (30 GiB free are asked for). The window says where it is
  (`Step 4 of 7 · PyTorch ... · downloading ...: 117.5 MB of 5.8 GB, 7.0 MB/s, about 13 min left`) and stays usable.
  **If it is interrupted** (the window is closed or killed, the machine is switched off, a step fails) nothing is
  lost: ⚙ Settings lists it under **Unfinished installs**, and **Continue** does the steps that are left; a directory
  you renamed by hand is not a build and is continued the same way. **Cancel** throws the work away. **Models:**
  three ways to get one, like llama.cpp's. **Recommended** offers LTX-Video 2B distilled with its text encoder and VAE,
  each file into the folder ComfyUI looks for it in under `~/.quickai/comfyui/models`. **Look up online** searches
  Hugging Face for GGUF models of video, pictures and text encoders (type a name in the box to look for that instead,
  e.g. `wan2.2 ti2v 5b turbo` or `umt5`): each result has its quantizations as buttons, and a VAE or a text encoder that
  a repository holds is a button of its own (*Get VAE only*), each put in the folder it belongs in (`diffusion_models`,
  `text_encoders`, `vae`) under its own name and checked against the checksum Hugging Face states. The **Get** box also
  takes the address of one `.gguf` or `.safetensors` file (or a file of this computer) and the folder to keep it in
  (**Into**: *auto* chooses it from the file's name). Files of your own go in those folders too (`docs/examples/workflows/` has a workflow for Wan 2.2 5B Turbo and one for LTX-Video, each run on a 12 GB card). **Start** runs ComfyUI
  with no model (a workflow names the files it uses), **Open ComfyUI** opens its page, and the **API** panel shows how to
  send it a workflow. **The page's Download button:** when a workflow needs a model you lack, ComfyUI's page lists it with a
  **Download**, which in a browser saves the file in the browser's own folder. With ComfyUI started by QuickAI it puts the
  file in the models folder, in the folder of its kind, checked against the hub's checksum and gone on from if it is
  interrupted (Hugging Face, `.safetensors` and `.gguf` only; anything else is left to the browser; ⚙ Settings → Advanced →
  *The page's Download button* turns it off). If the window ever stops answering while something runs, `~/.quickai/ui-stalls.log` says when and
  what it was waiting in; nothing leaves the computer.
- **Engines are documents.** What an engine is, how it starts and which settings it has is a TOML document
  (`crates/quickai-runtime/manifests/engines/`, `docs/reference/engine-fields.md`); one in your configuration directory
  patches it (`quickai config check` and `quickai plan --engine ollama --model NAME --explain` show what comes of
  it). A document of yours that starts a program QuickAI did not find or install asks you once before it does.

## Configurations

A **configuration** is what QuickAI runs and how: the engine, the model, the program, and the settings and command-line words of the engine, in a file that can be
chosen, kept under a name and given to someone else. A section under the title row has a **Configuration:** dropdown (the Default, and yours); while what the window holds is no longer what was saved the section
says **• Modified** in amber, in an amber outline, with *Save* (over the configuration in use), *Save as...* (under a name) and *Revert* (back to what was saved, after asking); *Manage configurations...*, under the list of the dropdown, opens a window of its own:
the list, what each one sets and what choosing it would change, *Use*, *Rename*, *Duplicate*, *Export*, *Import*, *Delete*, *Reset to factory* and *Revert*. Yours are files in `profiles/` of your
configuration directory; the first start after the upgrade keeps the settings you had as your own Default, so nothing is said to be modified. Choosing one is refused while a server runs, and starts, fetches and installs nothing.

**What is given away** is only what means the same on another computer: no folder, no program of yours, no word that looks like a secret. A model is named and never fetched; a build is named and never installed.
What the person who gets the file is told (another system or graphics card, a model or a program they lack, a port in use, a line that widens what a program may do) is said before anything is taken;
what asks is taken only when ticked, and a line that would make a program load other code (`LD_PRELOAD`, `PATH`) is refused whatever the file says. Open WebUI's environment is left out (it keeps most of its settings
in a database of its own after its first start): a configuration says whether it starts with Ollama, and its port. From the command line: `quickai configuration list | show | check | export | import`
and `quickai plan --configuration ID|FILE` (`docs/configurations.md`).

## Live conversation traffic (📡)

Off by default. When turned on in ⚙ Settings, llama-server itself binds an
internal-only port and QuickAI puts a small reverse proxy (`src/proxy.rs`,
built on `tiny_http` + `ureq`) in front of it on the port you actually
configured — every client (the built-in chat UI, or an external app via
🔌 API) keeps talking to that same port and sees byte-for-byte the same
responses, streaming included.

The proxy also sniffs `/v1/chat/completions`- and `/completion`-shaped
traffic (nothing else — page assets, `/health` checks, etc. are relayed
but not logged) to feed the **📡 Live** tab, next to **Main** — laid out
like a chat app: a list of the clients down the left (one row per client
connection: its number in a colour of its own, a live dot while an answer is
on its way, when it was last heard, and its message count and last prompt —
click a row, or **All**, to switch) with the picked client's conversation on
the right as it happens: what the client asked in a bubble at the right, the
answer filling in a bubble at the left (with three dots while it is awaited
and a **live** marker), each exchange with the request line, how long ago it
came and how long the answer took. With **All** every exchange has the tag
of its client. **Copy** puts what is shown on the clipboard as plain text and
**Clear all**, at the foot of the list, empties it. Before the first request
the tab says so and, while the server runs, shows the address to give an app
(with a copy button); while it does not run it has a button to the Main tab.

This sniffing is best-effort and never affects what a client actually
receives — a parsing miss just leaves an entry blank in the 📡 window.

## Adding more models

Three ways, from most to least manual:

- **Drop a file in**: put any `.gguf` file into the bundled `models/`
  folder (next to the executable in a packaged build, or this repo's
  `models/` in dev) and click **Refresh** in the GUI. To give it a
  friendly display name/description instead of just its filename, add an
  entry to `KNOWN_MODELS` in `crates/quickai-runtime/src/models.rs`.
- **Get** box: paste a direct link to a `.gguf` file, or a local
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
- **Look up online**: click **Look up online** to search Hugging Face for the most downloaded GGUF
  repositories, or type words into the box and press **Search** to find the ones that have them. Each is
  listed in a section by what kind of model it is (**Chat**, **Code**, **Vision**, **Embedding**, **Audio**, and
  **Other** for what llama.cpp cannot run, folded away) with a button for each quantization it has
  (`Q4_K_M · 2.8 GB`; the one it would pick for you is bold), and **Size** at the top keeps only the models with a
  button of a size (**Small**, **Medium**, **Large**). A vision model needs a second file, its **projector**:
  such a model has two rows, **Get model only** and **Get with projector**, the second a single download of
  both. Click a button to download it into `~/.quickai/models` like a pasted URL; the projector is kept
  beside the model (with a small record, `<model>.bundle.json`), the picker lists the model alone and says
  which projector it has, **Start** passes it to `llama-server` (`--mmproj`), and deleting the model in
  Settings deletes the projector too unless another model uses it. Repositories that need a Hugging Face
  login are left out and counted in the status line; a model that has been cut into shards is not
  offered. How a look-up reads and sorts, what a bundle is made of and which sizes there are is **data**, not
  code: see [docs/lookup-and-bundles.md](docs/lookup-and-bundles.md) for how to change it with a document of your own
  (a draft model for speculative decoding is a bundle, for one), and the generated pages
  [engine-fields.md](docs/reference/engine-fields.md) and [vocab-fields.md](docs/reference/vocab-fields.md)
  for every key. The look-up is a heuristic best-effort, not a guarantee of quality or of llama.cpp
  compatibility for every quantization that exists on the Hub: mainstream K-quants work; unusual or
  experimental formats may not. The results sit in a fixed-height, scrollable section (the arrow tucks it
  away without discarding it; the **X** at the top right of the Model row closes the whole section).

- **Recommended**: the button beside **Look up online** opens the list of models QuickAI recommends, which is part of
  the application (nothing is asked of the internet to open or browse it): 81 models from a few hundred megabytes to
  14 GB, mostly small and medium ones, in categories (**Chat**, **Reasoning**, **Code**, **Vision**, **Documents** for
  OCR, **Audio** for speech recognition, **Translation** and **Embedding** for search), with **Start here** for a
  handful to begin with, a search box (words such as `image`, `code`, `thinking` or a maker's name), the same **Size**
  chips, and a mark on what you already have. A size that would not fit in this computer's memory is greyed. A model that
  reads pictures or listens needs a second file, its projector, and the list has it: **Get with projector** fetches both
  as one download and **Get model only** just the weights — which to fetch is your choice. Every file is checked against
  the SHA-256 the list holds as it arrives (a file that is not the one is not kept), the model is then named as the list
  names it in the picker, and one that is served differently starts the way it needs (an embedding model with
  `--embeddings`). With Ollama chosen the same button lists the models Ollama pulls by name. The list, how it is made
  from the sites and how to add a model or hide one with a document of your own are in
  [docs/catalog.md](docs/catalog.md).

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

Packaging and distribution live in the separate `quickai_base` repository, not
here. It builds the Linux tar.gz, the AppImage, the Windows zip and the snap
from a checkout of this repository (check out a release tag first), and holds
the AUR / Homebrew / Scoop / Chocolatey / Microsoft Store packaging
for them:

```bash
quickai_base/build/all.sh      # linux tar.gz + AppImage + windows zip -> quickai_base/dist/
```

Every package bundles the GUI binary, `llama-server` + its runtime libs (the
official llama.cpp Vulkan release, pinned there), both
`models/Bonsai-8B-Q1_0.gguf` and `models/Bonsai-1.7B-Q1_0.gguf`, and a short
end-user README. In a packaged build the app finds `llama-server` and the
bundled `models/` right next to its own executable (`paths.rs`), so that layout
is part of the contract between this repository and `quickai_base`.

## Updating llama.cpp

This is the dev copy `cargo run` uses; the packages bundle a pinned official
release instead (`quickai_base/build/config.env`). The build dependencies
installed for this (`libvulkan-dev`, `glslc`,
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

## Privacy

QuickAI has no telemetry, accounts or update checks, and keeps your prompts on
your computer. It talks to the internet only when you ask it to (searching for
or downloading a model or a llama.cpp or Ollama build, or pulling a model into Ollama); the details are in
[PRIVACY.md](PRIVACY.md).

## License

QuickAI's own source code is licensed under the PolyForm Noncommercial License 1.0.0
(`LICENSE`): free for personal and other noncommercial use. It also ships two things under
their own separate licenses, reproduced in full in `THIRD_PARTY_LICENSES.md`:
the `llama-server` binary built from llama.cpp (MIT), and the bundled
Bonsai-8B/Bonsai-1.7B model weights from PrismML (Apache-2.0, unmodified).
Neither requires QuickAI's own code to share their license — each covers
only the files it applies to. The in-app **ℹ** button summarizes all of
this plus a usage/disclaimer blurb.
