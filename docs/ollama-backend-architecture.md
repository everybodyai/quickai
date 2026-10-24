# Ollama support: architecture

_Status: **superseded** on 2026-10-06 by [architecture.md](architecture.md), after new requirements (everything
data-driven, adapters for picture, audio and video, extreme modularity). Kept for history. Its Ollama findings
(§5, §11) now live in [engine-facts.md](engine-facts.md), and its lifecycle, install pipeline and process-hygiene
sections carry over into architecture.md._

QuickAI today starts and stops one kind of server, `llama-server` from llama.cpp, which the packages bundle.
This document proposes how it gains **Ollama** as a second engine. Ollama is **downloaded on demand and never
bundled**, so the bundles built by `quickai_base` do not change. The emphasis is modularity: after this work,
adding a third engine should mean adding one folder, not editing the core.

## 1. Summary

- **A backend seam.** The core (process handling, readiness checks, proxy, installer, settings, the UI frame)
  stops knowing the words "llama-server" or "GGUF file". Each engine is a module under `backend/` that implements
  a few narrow traits: where its runtime comes from, how to launch it, which models it has and how to get more,
  how to read its traffic. llama.cpp is the first implementation, Ollama the second.
- **Launching is data.** A backend's only launch job is to *describe* the run. `Launcher::plan()` is a pure
  function that returns a `LaunchPlan` (process or attach, endpoint, readiness probe, warm-up, unload, stop
  policy). One generic `Session` (today's `ServerManager`) executes any plan. Most of what needs a real binary to
  test today becomes a table-driven unit test.
- **Two layers, one state machine.** llama-server is one process serving one model. Ollama is a daemon that
  loads models on demand. `Session` models both: *Start* brings the daemon up (spawns it, or attaches to one that
  is already running), warms the selected model, and reaches Running. *Stop* unloads the model and stops what
  QuickAI started. The five existing states and the once-per-frame `poll()` stay as they are.
- **Download, don't bundle.** `llama_install` and `llama_releases` become a backend-agnostic runtime installer
  (download, verify sha256, extract zip / tar.gz / **tar.zst**, rename atomically, smoke-test). The Ollama
  runtime is resolved in this order: custom path, installed by QuickAI, Ollama already on this machine, offer
  the download. Attaching to an Ollama that is already running needs no binary at all.
- **Isolation.** Ollama gets its own install root (`~/.quickai/ollama/`) and its own model store. Neither
  collides with `~/.quickai/llama-bin` or with the user's own `~/.ollama`.
- **Additive settings.** Every existing `settings.json` key keeps its name and meaning (the GUI suite probes
  them). New: `backend`, plus one `backends.<id>` object per new engine.
- **The window does not move in llama.cpp mode.** The Engine selector lives in Settings. The header and main
  panel keep their layout in llama.cpp mode, because the GUI suite's OCR crops are calibrated to it.
- **Phased.** Phase 0 is a behaviour-preserving refactor with llama.cpp only, gated by the existing 213-case
  suite. Ollama arrives in Phases 1-3 on top of it (§9).

Three findings from checking Ollama's real packaging shaped the design (details in §5):

1. The download is big: `ollama-linux-amd64.tar.zst` is 1.44 GB and `ollama-windows-amd64.zip` 1.47 GB. Linux
   ships as **tar.zst**, a format the app cannot read today. About 1.83 GB of the Windows zip's 1.94 GB
   (extracted) is CUDA, which an AMD or Intel machine never loads.
2. `ollama serve` has no web chat page, and its model list needs a running daemon. "Open Chat UI" has no Ollama
   equivalent, and the model picker cannot be filled from disk alone.
3. Ollama's package contains a `llama-server` of its own (under `lib/ollama/`). A shared install root would let
   llama.cpp's recursive binary search find Ollama's internal copy.

## 2. Decisions needed

Each has a recommended default. The rest of the document assumes the recommendation.

| # | Question | Recommendation | Why it matters |
|---|---|---|---|
| D1 | When Ollama already answers on the configured port, attach to it or refuse? | **Attach.** Mode `auto`: attach if something answers, otherwise spawn. The status line says "attached, not started by QuickAI". | Most Ollama users already run it as a service. Attaching makes Start/Stop mean load/unload a model, and Stop leaves their daemon alone. |
| D2 | Which model store does a QuickAI-started Ollama use? | **Its own** (`~/.quickai/ollama/models`), with a Settings switch for Ollama's default store. | Pull and Delete in QuickAI must not touch models the user pulled with the Ollama CLI. |
| D3 | Version policy | **Offer a tested version first** (pinned tag and sha256 in the source), newest second, via an explicit look-up. | Ollama ships often (v0.35.1 on 2026-09-29). The look-up is the user's click, never a background check. |
| D4 | Where does the engine switch live? | **Settings → Engine.** | A header or main-panel control would move the "Live traffic" tab and shift the existing suite's OCR crops. |
| D5 | "Open Chat UI" for Ollama | **Hide it in v1.** A QuickAI-hosted chat page, served by the proxy for both engines, is a separate feature. | Ollama has nothing to open. |
| D6 | The bundled Bonsai Q1_0 models under Ollama | **Not offered** until a smoke chat proves Ollama can run them. | They are llama.cpp-specific today (A5). |
| D7 | zstd decoder | **`ruzstd`** (pure Rust), behind a throughput check; `zstd` (C) if too slow. | Builds go through `cross` in old-glibc Docker images and cross-compile for Windows. A C dependency adds risk there. |
| D8 | Cargo feature | An `ollama` feature, on by default. | Lets a build leave the runtime downloader out if a channel's policy requires it (§8). |

## 3. Goals and non-goals

**Goals**

- Ollama is selectable as an engine. It can be installed in the app or found on the machine; started, attached
  to and stopped; models can be pulled, listed and deleted; the Connect an app panel and Live traffic tab work.
- llama.cpp behaves exactly as it does today, including the layout the GUI suite reads.
- Packaging is untouched: nothing Ollama-related is bundled.
- A new backend needs no change to the core or to another backend.
- The logic is testable headless: `plan()` is pure, the API client runs against a fake.

**Non-goals**

- Bundling or redistributing Ollama.
- Linking Ollama as a library. QuickAI talks to the process through environment, stdout/stderr and HTTP.
- Ollama Cloud, sign-in and web search (the daemon is started with `OLLAMA_NO_CLOUD=1`).
- Modelfile authoring, `ollama create` and GGUF import in v1.
- Non-loopback binding and remote Ollama hosts.
- macOS packaging. QuickAI does not ship there, but CI compiles on macOS, so the code must too.
- A hard crate split now (§6.10).

## 4. Starting point: where the code assumes llama.cpp

| Code | Assumption | Becomes |
|---|---|---|
| `server_manager.rs`, `start()` | Nine positional arguments; builds llama-server's argv; polls `/health`; `child.kill()`. | `Session::start(StartRequest)` runs a `LaunchPlan`. Process handling moves to `process.rs`. |
| `proxy.rs` | Imports `server_manager::Event`; recognises OpenAI paths and llama's `/completion`; SSE only. | `events.rs` owns the events. Framing and decoders come from the backend. NDJSON is added. |
| `app.rs` (1,966 lines) | About 40 fields mix UI and llama state; a 500-line settings window; llama text in hovers, About and the crash panel; four near-identical `poll_*` functions. | Split per panel. Text comes from `BackendInfo`. One `poll_jobs`. |
| `settings.rs` | Flat: `n_gpu_layers`, `llama_bin_override`, `extra_args`. | Legacy keys stay; `backends.<id>` for new engines. |
| `paths.rs` | `llama_server_bin()`, the `llama-bin` roots. | Install root and binary name come from the backend's runtime provider. |
| `models.rs`, `downloader.rs`, `remote.rs` | A model is a `.gguf` path. | `ModelEntry` is keyed by a backend-scoped id. These three become llama.cpp's `ModelCatalog`. |
| `llama_install.rs`, `llama_releases.rs` | Binary name, repository, `.zip`/`.tar.gz`, substring platform matching. | A generic installer; per-backend release source and asset rules. |
| `hardware.rs`, `gpu.rs` | `n_gpu_layers`; llama backend labels. | `gpu_offload: bool`; labels from the backend. |
| `downloader::stream_to_part`, `llama_install::download_to` | Two copies of "stream a URL into a file with progress". | One `net::download`. |

## 5. What Ollama changes

Checked on 2026-10-06 against Ollama v0.35.1 and its docs (provenance and open items in §11).

| | llama-server (today) | Ollama |
|---|---|---|
| Process | One process, one model, loaded at start. | `ollama serve` is a daemon. It loads a model into a runner subprocess on first request and unloads it after `keep_alive` (default 5 minutes). |
| Model identity | A `.gguf` path. | `name:tag` in Ollama's store, pulled by the daemon. `hf.co/<user>/<repo>[:quant]` pulls a GGUF from Hugging Face. |
| Listing models | Scan folders. | `GET /api/tags`, which needs a running daemon. |
| Readiness | `GET /health` | `GET /api/version` |
| Configuration | Command-line flags. | Environment: `OLLAMA_HOST` (default `127.0.0.1:11434`), `OLLAMA_MODELS`, `OLLAMA_CONTEXT_LENGTH` (default 4096), `OLLAMA_KEEP_ALIVE`, `OLLAMA_MAX_LOADED_MODELS`, `OLLAMA_NO_CLOUD`, and more. |
| GPU | `--n-gpu-layers`. | No server-wide layer count. Vulkan is on by default when its backend is present (`OLLAMA_VULKAN=0` turns it off). Force CPU with `CUDA_VISIBLE_DEVICES=-1` and `ROCR_VISIBLE_DEVICES=-1`. |
| Web chat | `GET /` serves one. | None. |
| APIs | OpenAI `/v1/*` and `/completion`, streamed as SSE. | OpenAI `/v1/*` (SSE) and native `/api/chat`, `/api/generate` (NDJSON). |
| Model name in a request | Ignored; there is only one model. | **Matters.** A different name loads another model, and with `OLLAMA_MAX_LOADED_MODELS=1` evicts ours. |
| Stopping | Kill the process. | `SIGTERM` lets it unload its runners. `SIGKILL` may orphan them (A3). |
| Distribution | GitHub release; `.zip` or `.tar.gz`; tags like `b11007`. | GitHub release; see below. |
| Licence | MIT | MIT. The archive carries its own licence files (`lib/ollama/*_LICENSE`). |

Release assets of v0.35.1 (published 2026-09-29), from the GitHub API:

| Platform | Asset | Size | Notes |
|---|---|---|---|
| Linux x86-64 | `ollama-linux-amd64.tar.zst` | 1.44 GB | `bin/ollama` and `lib/ollama/…`; CUDA included |
| Linux arm64 | `ollama-linux-arm64.tar.zst` | 1.56 GB | |
| Linux AMD ROCm | `ollama-linux-amd64-rocm.tar.zst` | 1.05 GB | An **overlay** extracted over the base |
| Windows x86-64 | `ollama-windows-amd64.zip` | 1.47 GB | 1.94 GB extracted; `ollama.exe` and `lib/ollama/…`; includes `vulkan/` |
| Windows arm64 | `ollama-windows-arm64.zip` | 0.21 GB | |
| Windows ROCm | `ollama-windows-amd64-rocm.zip` | 0.26 GB | An overlay |
| macOS | `ollama-darwin.tgz` | 0.16 GB | CLI and libs (the `.zip`/`.dmg` are the GUI app) |
| Ignored | `-mlx` variants, `jetpack5`/`jetpack6`, `OllamaSetup.exe`, `Ollama.dmg`, `install.sh`, `install.ps1`, `sha256sum.txt` | | |

What follows from this:

- The Windows base zip already contains the **Vulkan** backend, so an AMD or Intel machine (RADV on Linux) needs no
  ROCm overlay. That is the premise of this app (README: Vulkan on an RX 6700 XT instead of ROCm).
- CUDA is 1.83 GB of the 1.94 GB extracted, so the part a non-NVIDIA machine needs is about 0.11 GB.
- The layouts differ by OS (`bin/ollama` on Linux, `ollama.exe` at the root on Windows), so the binary locator is
  a per-OS path list, not a search. A recursive search would find `lib/ollama/llama-server(.exe)`.
- Ollama's docs describe the standalone Windows zip as suitable for embedding, extracted into one directory.
- A user may already have Ollama running on 11434 as a service or tray app, which is why attach (D1) matters.

## 6. Architecture

### 6.1 Layers and dependency rules

```
┌──────────────────────────────── app/ (egui) ───────────────────────────────┐
│ header · model bar · acquire box · controls · API panel · live · settings   │
│ panels/llamacpp.rs          panels/ollama.rs     (backend-specific widgets) │
└───────────────┬─────────────────────────────────────────────────────────────┘
                │ asks the Backend for data, labels and capabilities;
                │ never matches on a backend id
┌───────────────▼─────────────────────────────────────────────────────────────┐
│ core/                                                                       │
│   session ─ process ─ probe │ proxy + traffic │ jobs │ runtime │ net        │
│   settings · paths · hardware · gpu · events · backend (the traits)         │
└───────────────▲─────────────────────────────────────────────────────────────┘
                │ implement the traits
┌───────────────┴──────────────────────┬──────────────────────────────────────┐
│ backend/llamacpp/                    │ backend/ollama/                      │
└──────────────────────────────────────┴──────────────────────────────────────┘
```

1. `core/**` never imports `backend/**`. The traits live in `core/backend.rs`; only the registry in
   `backend/mod.rs` names the concrete backends.
2. `backend/**` never imports `egui`, `app/**` or another backend. Backends are UI-free, so they can run headless
   and be tested without a window.
3. `app/**` names a concrete backend only in `app/panels/` and at the registry call site.

A small test (`tests/architecture.rs`) greps the sources for forbidden `use` lines and fails the build.

### 6.2 The backend contract

Sketches, not compiled code.

```rust
// core/backend.rs
pub type Wake = Arc<dyn Fn() + Send + Sync>;   // trait methods cannot take `impl Fn`

pub trait Backend: Send + Sync {
    fn info(&self) -> &'static BackendInfo;     // id, names, capabilities, defaults, help texts
    fn runtime(&self) -> &dyn RuntimeProvider;
    fn launcher(&self) -> &dyn Launcher;
    fn catalog(&self) -> &dyn ModelCatalog;
    fn traffic(&self) -> &dyn TrafficDecoder;
    fn api_info(&self, ctx: &ApiCtx) -> ApiInfo;       // base URLs, model value, key note, curl example
    fn diagnose(&self, failure: &Failure) -> Vec<Hint>; // crash-panel advice
}

pub struct Caps {
    pub gpu: GpuControl,                  // Layers (llama.cpp) or Toggle (Ollama)
    pub extra: ExtraKind,                 // CliArgs or EnvLines
    pub web_ui: bool,                     // llama.cpp: true. Ollama: false → no "Open Chat UI"
    pub attach: bool,                     // can use a server QuickAI did not start
    pub bundled: bool,                    // ships in the package (llama.cpp) or not (Ollama)
    pub catalog_needs_server: bool,       // Ollama: true
    pub model_name_matters: bool,         // Ollama: true → API panel text
}
```

| Port | Responsibility | llama.cpp (from today's code) | Ollama (new) |
|---|---|---|---|
| `RuntimeProvider` | Find the executable (no network). List installable builds (network, on click). Describe an install: assets, entry filter, how to find the binary. List and delete installed builds. | `paths::llama_server_bin`, `llama_releases`, `llama_install::list_installed` | `PATH`, `~/.quickai/ollama/<tag>/`, GitHub releases of `ollama/ollama` |
| `Launcher` | `plan(StartRequest) → LaunchPlan`, pure. | argv builder, `parse_extra_args`, `LD_LIBRARY_PATH` | env builder, warm-up and unload calls, attach |
| `ModelCatalog` | List local models. Classify what was typed in the Model box. Acquire (download, import, pull) as a `Job`. Remove. Look up online. Recommend. | `models.rs`, `downloader.rs`, `remote.rs` | `ollama/api.rs`: tags, pull, delete; curated list; Hugging Face refs |
| `TrafficDecoder` | Classify a request path, summarise a request body, pick the response framing, extract text. | Today's functions in `proxy.rs` | OpenAI decoder plus a native NDJSON decoder |
| `api_info` | What the Connect an app panel shows. | `http://127.0.0.1:<port>/v1`, model value ignored | OpenAI and native URLs, exact `name:tag` required |
| `diagnose` | Turn an exit or log tail into hints. | "unknown model architecture" → try another build | port taken, out of memory, no GPU found, unknown model name |

### 6.3 `LaunchPlan`: the central seam

```rust
pub struct StartRequest {
    pub model: Option<ModelId>,   // None = management mode (Ollama only, §6.6)
    pub display: String,
    pub public_port: u16,         // what clients and the GUI use
    pub listen_port: u16,         // where the engine binds: = public, or ephemeral when the proxy is on
    pub ctx_size: u32,
    pub gpu: GpuMode,             // Layers(n) or Auto/Off, per Caps.gpu
    pub extra: String,            // CLI args or env lines, per Caps.extra
}

pub struct LaunchPlan {
    pub process: Spawn,           // Child(ProcessSpec) or Attach
    pub endpoint: Endpoint,       // where the engine answers: proxy upstream and probe target
    pub ready: Probe,             // Http { path, expect: 200 } plus a timeout
    pub warm_up: Option<HttpCall>,// after ready, before Running
    pub unload: Option<HttpCall>, // on Stop, best effort, short timeout
    pub stop: StopPolicy,         // Kill or Graceful { grace }
    pub banner: String,           // first log line: the command and the env QuickAI set
}
pub struct ProcessSpec { program, args, env: Vec<(String, String)>, env_remove: Vec<String>, lib_dir: Option<PathBuf> }
```

| | llama.cpp | Ollama (spawned) |
|---|---|---|
| Command | `llama-server --model <path> --host 127.0.0.1 --port <p> --ctx-size <n> --n-gpu-layers <n> <extra>` | `ollama serve` |
| Environment | `LD_LIBRARY_PATH=<binary dir>:<inherited>` | `OLLAMA_HOST=127.0.0.1:<p>`, `OLLAMA_MODELS=<store>`, `OLLAMA_CONTEXT_LENGTH=<n>`, `OLLAMA_KEEP_ALIVE=-1`, `OLLAMA_MAX_LOADED_MODELS=1`, `OLLAMA_NO_CLOUD=1`, GPU switches, then the user's extra lines |
| Ready | `GET /health` is 200 (120 s) | `GET /api/version` is 200 (120 s) |
| Warm-up | none | `POST /api/generate {"model": m, "keep_alive": -1, "options": {"num_ctx": n}}` (10 min) |
| Unload | none | `POST /api/generate {"model": m, "keep_alive": 0}` |
| Stop | Kill | Graceful (10 s) |
| Attached | not supported | `process: Attach`. Environment-based settings do not apply: the daemon's owner decides. |

`OLLAMA_KEEP_ALIVE=-1` keeps the model resident until Stop, as llama-server does. `OLLAMA_MAX_LOADED_MODELS=1`
matches "only one model runs at a time". The warm-up passes `num_ctx` and the environment sets the default, so a
client that sends no options does not trigger a reload.

### 6.4 Session lifecycle

`Session` is `ServerManager` made generic. It keeps `ServerState` (Idle, Starting, Running, Stopping, Crashed),
the event channel, `poll()` and the port bookkeeping. What changes: it starts from a `LaunchPlan`, and the health
poller thread becomes a *bring-up thread* (probe, then warm-up, then report Running).

| Moment | llama.cpp | Ollama, spawned | Ollama, attached |
|---|---|---|---|
| Start | spawn, probe `/health`, Running | spawn `serve`, probe `/api/version`, warm up, Running | probe, warm up, Running |
| Stop | kill, Idle | unload, stop the process group (SIGTERM, grace, kill), Idle | unload, Idle; the daemon is untouched |
| Process exits unasked | Crashed | Crashed | no child; a liveness probe of `/api/version` every 5 s while Running reports Crashed |
| Warm-up fails (out of memory, unsupported model) | not applicable | log Ollama's error, stop what QuickAI started, Crashed | log the error, Crashed |
| App exit | `stop_blocking` | same, with a 2 s grace so closing the window is not held up | unload, detach |

**Management mode.** Pull, Delete and listing need a daemon (§5). If none is running, the app starts the same plan
with `model: None`: no warm-up, state Running, status "Ollama ready, no model loaded". Choosing a model and
pressing Start then only warms it (`Session::load`); no restart. Stop ends everything. One session at a time,
as today.

### 6.5 Getting the runtime: download, don't bundle

**What triggers a download.** Only the user's click on "Download Ollama" or "Install" on a listed version. Nothing
is fetched at startup, when the engine is selected, or when Start finds no runtime (it shows the setup card).
That keeps PRIVACY.md's "never connects by itself" true.

**Resolution order** (`RuntimeProvider::locate`, no network):

1. `backends.ollama.bin_override`
2. `~/.quickai/ollama/<tag>/`: `install_tag` from the settings, else the newest complete install
3. `ollama` on `PATH`, then the usual system locations (`/usr/local/bin`, `/usr/bin`, `%LOCALAPPDATA%\Programs\Ollama`)
4. Missing: the setup card offers "Use Ollama already on this computer", "Download Ollama v0.35.1 (1.4 GB)" and "Browse…"

Attach (D1) comes before all of this: if the endpoint answers `GET /api/version`, no binary is needed.

**Install pipeline**, generalised from `llama_install::spawn_install`:

```
click "Download Ollama"                    the only trigger
  ├─ preflight   free space ≥ archive + extracted size + margin; target not already complete
  ├─ download    <root>/<tag>.part, with progress
  ├─ verify      sha256 equals GitHub's `digest` for the asset
  ├─ extract     into <root>/<tag>.partial/ through an EntryFilter
  ├─ locate      bin/ollama or ollama.exe; run `<bin> --version` as a smoke test
  └─ rename      <tag>.partial → <tag>          atomic
```

Where this differs from `llama_install` today:

- **Atomic install.** `spawn_install` extracts straight into the final directory and returns Done as soon as
  `find_binary` succeeds. If the app is killed mid-extraction, the next run can find a directory where the binary
  already exists and report it installed (and `list_installed` lists it). With Ollama the binary is extracted first
  and 1.4 GB of libraries after it, so the extract-then-rename step is a requirement.
- **Verification.** The API reports a sha256 `digest` per asset, and the installer checks the download against it.
  That catches truncation and corruption. It does not defend against a tampered release, so the tested tag's
  digests are also pinned in the source (`backend/ollama/runtime.rs`, as `build/config.env` does for llama.cpp;
  the base repository plays no part because it does not fetch Ollama).
- **Archive kinds.** `Zip`, `TarGz` and `TarZst`.
- **Entry filter.** Extraction goes through an `EntryFilter` instead of unpacking the whole archive. It must keep
  refusing paths that escape the target (`tar::Entry::unpack_in`, `zip::ZipFile::enclosed_name`). Default policy:
  skip `lib/ollama/cuda_*` **only** when `gpu::detect()` is non-empty and contains no NVIDIA adapter. Empty keeps
  meaning "couldn't tell", so nothing is skipped.
- **Multi-asset builds.** A `RuntimeBuild` carries several assets. Ollama's ROCm overlay is a second asset
  extracted over the first. It is off by default, because Vulkan in the base covers AMD.
- **Binary locator per OS** (an explicit path list; see §5).
- **Disk preflight** with a clear message. Streaming the tar.zst body straight into the extractor would avoid the
  1.4 GB staging file on Linux, but gives up resume and after-the-fact verification, so it is not v1.

**Making the download smaller (Phase 4).**

1. Skipping CUDA at extraction cuts the Windows install from 1.94 GB to about 0.11 GB on disk.
2. The Windows zip can be fetched with HTTP Range requests, keeping only the wanted entries: about 0.11 GB of
   traffic instead of 1.47 GB. GitHub's CDN served Range requests for the zip's central directory in the check
   behind §11. The `zip` crate needs `Read + Seek`, so this takes a small `RangeReader` with read-ahead.
3. A `.tar.zst` is a stream and cannot be ranged. A Linux user without Ollama pays the full 1.44 GB, which is why
   detecting an installed Ollama (step 3 above) and attach (D1) matter more on Linux.

**Update and removal.** There are no automatic update checks. "Look up Ollama versions" is a click that lists
releases. It should skip prereleases, since Ollama flags its release candidates that way (confirm when writing the
matcher; llama.cpp is the opposite, it flags every CI build as a prerelease, so its look-up cannot filter on it).
Installed versions are listed with Use and 🗑, the existing "Installed builds" list generalised. A version cannot be deleted
while a session runs from it.

### 6.6 Models

```rust
pub struct ModelEntry {
    pub id: ModelId,                 // llama.cpp: file name. Ollama: "qwen3:4b"
    pub display: String, pub note: String,
    pub size_bytes: u64,
    pub origin: Origin,              // Bundled, Downloaded or External
    pub location: Option<PathBuf>,   // llama.cpp: the file. Ollama: None
}
```

`hardware::recommend` and `speed_tier` keep working, since they read `size_bytes`. They take
`gpu_offload: bool` instead of `n_gpu_layers`.

**Listing.** Ollama's list comes from `GET /api/tags` when a daemon is reachable. Otherwise it is the last answer,
cached in `~/.quickai/ollama/catalog-cache.json`, so the picker is filled at launch without starting a process.
Embedding-only models are listed but flagged: they cannot be warmed for chat.

**What the Model box means.** `ModelCatalog::classify` is the one place that looks at the shape of the input.

| Typed | llama.cpp (today) | Ollama |
|---|---|---|
| `https://…` | download the file | a `huggingface.co/<user>/<repo>` URL becomes a pull of `hf.co/<user>/<repo>`; any other URL gets "Ollama pulls by name" |
| an existing local path | import (symlink or copy) | disabled in v1 (GGUF import through `/api/create` later) |
| `hf.co/<user>/<repo>[:quant]` | treated as a search | pull |
| `name[:tag]` | search Hugging Face | pull from the Ollama library |
| words with spaces | search Hugging Face | search the curated list and Hugging Face |

**Look up online.** For Ollama: a curated shortlist (data embedded in the binary, refreshed with releases) plus the
existing Hugging Face lookup, whose rows become pulls of `hf.co/<repo>:<quant>`. `pick_file` already prefers
Q4_K_M, which is also Ollama's default quantisation. `RemoteModel` generalises to
`LookupRow { title, detail, size, popularity, action: AcquireAction }`, so the results list is generic. No public
JSON listing of Ollama's library was found (A7), so scraping ollama.com is deliberately not part of the design. (Changed on 2026-10-07, when the user asked for a look-up online for Ollama: the library's own search page is read after all, as work package 3.15, decision C56 of `implementation-plan.md`; the Hugging Face and curated-list rows above are still not built.)

**Pull and Delete.** A pull is a `Job` over `POST /api/pull`. Its NDJSON status lines go to the log pane, and
progress is summed over layer digests. Cancelling drops the connection; the daemon keeps partial blobs, so the next
pull resumes. Delete is `DELETE /api/delete` through the existing confirm modal (`PendingDelete::Model` gains the
backend id). Both use management mode when no session is running.

**`Job`** replaces `DownloadHandle` and `InstallHandle`, which have identical fields:

```rust
pub struct Job { pub rx: Receiver<JobEvent>, pub label: String, pub verb: &'static str,
                 pub downloaded: u64, pub total: Option<u64> }
pub enum JobEvent { Progress { downloaded: u64, total: Option<u64> }, Status(String),
                    Done(JobOutput), Failed(String) }
pub enum JobOutput { ModelFile(String), ModelPulled(String), Runtime(PathBuf) }
```

### 6.7 Traffic proxy and Live traffic

- `Event`, `ServerState` and `TrafficEvent` move to `events.rs`; the proxy stops importing `server_manager`.
- `proxy::spawn` receives the backend's `TrafficDecoder` along with the two ports.
- **Framing.** `SniffingReader` handles SSE today. Add NDJSON (`application/x-ndjson`, one JSON object per line;
  text in `message.content` for `/api/chat` and `response` for `/api/generate`; `done: true` at the end). Relaying
  stays byte-for-byte and sniffing stays best-effort.
- **Decoders.** `OpenAiDecoder` (today's functions, moved with their tests) is shared. Ollama uses it plus an
  `OllamaNativeDecoder` for `/api/chat` and `/api/generate` (requests carry `messages[]` or `prompt`; images arrive
  base64 in `images` and feed `attachments`).
- **Attach plus proxy.** The proxy must bind a port other than the attached daemon's (both default to 11434). In
  attach mode it would take the next free port, and the API panel would show that URL. v1 may simply disable the
  toggle in attach mode.

### 6.8 Settings and persistence

Additive. Existing keys keep their meaning and stay at the top level; they are llama.cpp's settings in effect.
Every new engine stores its own JSON object under `backends.<id>`, so adding a backend does not change `Settings`.
Each backend (de)serialises its own config, with defaults for anything missing or invalid. An unknown `backend`
falls back to llama.cpp.

```json
{
  "last_model_filename": "Bonsai-8B-Q1_0.gguf", "port": 8080, "ctx_size": 4096, "n_gpu_layers": 999,
  "dark_mode": true, "models_dir_override": null, "llama_bin_override": null,
  "extra_args": "", "enable_traffic_proxy": false,

  "backend": "ollama",
  "backends": {
    "ollama": {
      "mode": "auto", "port": 11434, "ctx_size": 4096, "gpu": "auto",
      "last_model": "qwen3:4b", "bin_override": null, "install_tag": "v0.35.1",
      "models_dir_override": null, "use_default_store": false, "extra_env": ""
    }
  }
}
```

How the shared controls map:

| Control | llama.cpp | Ollama |
|---|---|---|
| Port | `--port` (default 8080) | `OLLAMA_HOST=127.0.0.1:<port>` (default 11434) |
| Context size | `--ctx-size` | `OLLAMA_CONTEXT_LENGTH`, plus `options.num_ctx` on warm-up |
| GPU layers | `--n-gpu-layers` | Hidden. A "GPU: Auto / Off" choice instead; Off sets `OLLAMA_VULKAN=0`, `CUDA_VISIBLE_DEVICES=-1`, `ROCR_VISIBLE_DEVICES=-1` |
| Extra arguments | appended to argv | `KEY=VALUE` lines become environment; they cannot override `OLLAMA_HOST` |
| Models folder | scanned directories | `OLLAMA_MODELS` |
| Binary override | `llama_bin_override` | `backends.ollama.bin_override` |

When attached, only per-request options can be applied (`num_ctx` on warm-up). The daemon's owner decides the
environment-based settings, and the UI says so.

### 6.9 UI

- **Engine selector** at the top of Settings ("llama.cpp (bundled)" or "Ollama (downloaded)"), disabled while a
  session is active, like the proxy checkbox. Choosing Ollama without a runtime shows the setup card there and in
  the main panel's model area.
- **Main panel in llama.cpp mode: unchanged.** In Ollama mode: model combo (from the catalog), Model box (Pull or
  Search), Port, Context size, GPU (Auto/Off), Start, Stop, Connect an app (no "Open Chat UI"), and a status line such as
  `Running: qwen3:4b (port 11434)` or `Attached: qwen3:4b (port 11434)`.
- **Text** comes from `BackendInfo.help`. After Phase 0 no hover text names llama.cpp outside `backend/llamacpp`.
- **API panel** comes from `Backend::api_info()`. For Ollama it shows the OpenAI base URL and the native URL, and
  the model value is the exact `name:tag`, with a note that other names load other models.
- **Per-backend panels** (`app/panels/<id>.rs`) hold the UI that is genuinely backend-specific: the
  "Advanced: llama.cpp" block (moved out of `app.rs` unchanged) and "Advanced: Ollama" (mode, runtime card with
  installed versions, look up and install, binary override, models folder, extra environment).
- **Crash panel** renders `diagnose()` hints.
- **Layout invariants for llama.cpp mode:** the header (`HEADER_Y`, icon x positions, tab x positions in
  `test/resources/keywords.resource`) and the main panel's row order. The Settings window gains an Engine row at
  the top, so re-run the settings suites and recalibrate if needed. The dialog crops are centred on the window,
  and the ✕ x positions are unaffected by added rows.

### 6.10 Module layout, features, dependencies

```
crates/quickai-runtime/src/
├── main.rs
├── app/                       UI only; no backend names outside panels/
│   ├── mod.rs                 state and frame loop
│   ├── header.rs  model_bar.rs  acquire.rs  controls.rs  api_panel.rs  crash_panel.rs
│   ├── live.rs  settings.rs  about.rs  confirm_delete.rs
│   └── panels/                llamacpp.rs  ollama.rs
├── core/
│   ├── backend.rs             the traits and shared types (§6.2, §6.3)
│   ├── events.rs              Event, ServerState, TrafficEvent
│   ├── session.rs             was server_manager.rs
│   ├── process.rs             spawn, log readers, exit watcher, graceful stop
│   ├── probe.rs               HTTP readiness
│   ├── proxy.rs  traffic.rs   relay and sniffing; framing and decoders
│   ├── jobs.rs                Job and JobEvent
│   ├── net.rs                 one streaming download: progress, sha256, later resume and ranges
│   ├── runtime/               release listing, install pipeline, archive kinds and entry filters
│   └── settings.rs  paths.rs  hardware.rs  gpu.rs
└── backend/
    ├── mod.rs                 registry
    ├── llamacpp/              launcher.rs  catalog.rs  runtime.rs  traffic.rs
    └── ollama/                launcher.rs  api.rs  catalog.rs  runtime.rs  traffic.rs  settings.rs
```

Phase 0 moves files; it does not rewrite their logic.

- **Features.** `default = ["llamacpp", "ollama"]`. The `ollama` feature gates `backend/ollama/` and
  `app/panels/ollama.rs`.
- **Dependencies.** New: `sha2`, `ruzstd` (D7), `libc` (Unix only), `windows-sys` (Windows only, job objects). The
  Ollama API client is about seven endpoints hand-written over `ureq` and `serde_json`; no async runtime.
- **Crate split.** If a third backend appears, `core/` and `backend/<id>/` can become crates mechanically, because
  the dependency rules already hold.

## 7. Cross-cutting

### 7.1 Process hygiene

- **Unix:** spawn in its own process group. Stop sends SIGTERM to the group, waits for the grace period, then
  SIGKILLs the group. On Linux, `PR_SET_PDEATHSIG` makes the child follow QuickAI if QuickAI dies (it fires when
  the spawning thread exits, so the spawn must stay on the UI thread, as it is today). This matters more for
  Ollama, whose child owns GPU memory and runner subprocesses.
- **Windows:** assign the child to a Job Object with `KILL_ON_JOB_CLOSE`; Stop terminates the job. A GUI process
  has no practical way to deliver a console Ctrl event, so there is no graceful signal. Spawn with
  `CREATE_NO_WINDOW`: a console
  child of a GUI-subsystem release build otherwise opens a console window (A8; it may already happen with
  `llama-server.exe`).
- CI builds Linux, Windows and macOS, so every `cfg` branch must compile.
- **Environment.** Do not leak the AppImage or snap loader environment into a downloaded runtime: drop
  `LD_LIBRARY_PATH` entries under `$APPDIR` before spawning Ollama. llama.cpp keeps today's behaviour of prepending
  its own directory.
- **Log pane.** stdout and stderr stream into it as today. The first line is the whole plan: the command and the
  environment variables QuickAI set.

### 7.2 Security

- Downloads are verified (§6.5). Extraction rejects paths that escape the target. No shell is involved.
- Loopback only: `OLLAMA_HOST` is always `127.0.0.1:<port>`. The user's extra environment cannot override it, or
  `OLLAMA_MODELS` while the isolated store is on.
- The local API has no authentication, as with llama-server today (PRIVACY.md already says so).
- A downloaded runtime runs with the user's rights. PRIVACY.md's sentence about llama.cpp builds extends to Ollama.

### 7.3 Privacy: PRIVACY.md, About and README change with the feature

- New requests QuickAI itself makes: `api.github.com/repos/ollama/ollama/releases` (look-up) and the release asset
  download, both on click.
- Requests *Ollama* makes when the user clicks Pull: its registry, or Hugging Face for `hf.co/…` names. QuickAI
  does not make these, but it triggers them, and the policy should say so.
- The daemon is started with `OLLAMA_NO_CLOUD=1` so cloud models and web search stay off (A6).
- New on-disk data: `~/.quickai/ollama/` (runtime, models, catalog cache). Ollama may also write its own key pair
  under `~/.ollama` (A4).
- "Bundled software" gains a line: Ollama is not bundled, is downloaded on request, is MIT-licensed, and its archive
  includes its licence files.

### 7.4 Failure modes

| Situation | Behaviour |
|---|---|
| Port taken by something that is not Ollama | Spawn fails, Crashed, hint "pick another port". |
| Port taken by Ollama | D1: attach. |
| No runtime | The setup card appears; Start is disabled with the reason. |
| Download interrupted, disk full, digest mismatch | The job fails, `.part` is removed, nothing half-installed remains. |
| Daemon dies while running | Crashed (child exit, or the liveness probe when attached). |
| Warm-up fails (out of memory, unsupported model) | Log Ollama's error, stop what QuickAI started, Crashed with hints (smaller model or context). |
| Pull fails (offline, unknown name) | The job fails with Ollama's message; unknown name adds a hint "check the spelling or use hf.co/…". |
| Another client asks for a different model | Ollama loads it, evicting ours (`MAX_LOADED_MODELS=1`). The Live traffic row shows the model name. |
| Ollama finds no usable GPU | Its log says so; the hint explains the Vulkan driver requirement on Linux. |

## 8. Impact outside `quickai/`

**Packaging (`quickai_base`).** No change to `build/`, `stage_bundle`, bundle size (about 1.4 GB) or the channel
manifests' payload; `build/config.env` gains nothing. Notes:

- `THIRD_PARTY_LICENSES.md` and the About text: Ollama is "downloaded on demand, not distributed".
- MSIX / Microsoft Store: running a downloaded 1.4 GB executable is today's "Look up llama.cpp builds" behaviour
  made primary. Re-read Store policy before submitting. The `ollama` feature (D8) is the escape hatch.
- Snap: strict confinement needs `network` and `network-bind`. GPU and Vulkan access for downloaded binaries is the
  same open question as for the bundled `llama-server` (the snap is drafted, not build-tested).
- AppImage: nothing is written inside the image; installs go to `~/.quickai`.
- Optional: manifests whose package manager carries an `ollama` package can list it as an optional dependency,
  since detection of an installed Ollama avoids the 1.4 GB download.
- The base README's "What goes into a bundle" gets a line saying Ollama is not part of it.

**Test suite (`quickai_base/test`).** The same operating rules apply: one suite run at a time, because runs share
the staged package.

- `resources/fake_ollama.py`, modelled on `fake_llama_server.py`. It speaks `GET /`, `/api/version`, `/api/tags`,
  `/api/ps`, `POST /api/pull` (NDJSON progress), `DELETE /api/delete`, `POST /api/generate` (empty prompt loads;
  `keep_alive: 0` unloads; NDJSON stream), `POST /api/chat` (NDJSON) and `POST /v1/chat/completions` (SSE, reused
  from the llama fake). Its command line is `serve` and `--version` only; anything else exits 1, like the real
  thing. Because Ollama is configured through environment, it records the `OLLAMA_*` variables it received to a
  file, the counterpart of the fake llama's argv file. Environment switches provide the `--fake-*` behaviours.
  It handles SIGTERM, and an optional ignore-SIGTERM mode tests escalation to kill. It spawns a child process, so
  a test can assert that neither the daemon nor its child survives Stop.
- A new suite `ollama_backend` with `docs/15_ollama_backend.md`: engine selection, runtime found in the throw-away
  HOME (`~/.quickai/ollama/<tag>/bin/ollama`), attach, Start/Stop, the exact environment mapping, pull progress,
  delete confirm, picker, API panel text, no "Open Chat UI", and Live traffic with NDJSON.
- Existing suites to update: settings_window (Engine row), persistence (`backend`, `backends`), network_privacy (an
  idle session with the Ollama engine makes no outside request; install and pull are the only triggers).
- The download and extract pipeline is tested in Rust, not through the GUI: an in-process `tiny_http` server serves
  generated archives of all three kinds and a GitHub-shaped JSON fixture (the v0.35.1 asset list is a ready-made
  one). One `--online` check lists real releases without downloading anything.
- After Phase 0, llama.cpp mode must pass all 213 existing cases unchanged.

## 9. Delivery plan

Sizes are relative. Every phase is shippable on its own, and Phase 0 pays for itself (smaller files, testable
launching) even if Ollama were never added.

| Phase | Content | Size | Exit gate |
|---|---|---|---|
| 0 | Behaviour-preserving refactor, llama.cpp only: `events.rs`; `Job`; the traits and `LlamaCppBackend`; `LaunchPlan` and `Session`; `process.rs`; split `app/`; `BackendInfo.help`; update the README layout section | L | `cargo fmt`, `clippy -D warnings` and `cargo test` clean; `build/test.sh` passes 213 cases with no crop changes; the two `#[ignore]`d real-model tests pass; the architecture guard test is in |
| 1 | Runtime acquisition: the generalised installer (tar.zst, sha256, atomic rename, entry filter), `ollama/runtime.rs`, the setup card, and the **spike** for A1-A4 and A9 on a throw-away HOME, including the AMD/Vulkan machine | M | Install and `--version` smoke on Linux (Windows through the VM rig); spike results written into §11 |
| 2 | Ollama launch: `ollama/launcher.rs`, attach, warm-up and unload, `backends.ollama` settings, API panel, crash hints; `fake_ollama.py` and the lifecycle part of `ollama_backend` | M | Lifecycle cases green; no orphan process after Stop or after app exit |
| 3 | Models: `ollama/api.rs`, the catalog (list, cache, pull, delete, curated, Hugging Face refs), management mode, the NDJSON decoder | M | Pull, delete and list cases green; decoder unit tests |
| 4 | Polish: skip-CUDA filter, ranged Windows download, resume, version look-up UI, PRIVACY, README and About text, packaging notes | M | PRIVACY.md reviewed against the network_privacy suite |

The shortcut, adding Ollama as an `enum` and `match` inside `app.rs` without Phase 0, gets to a first demo
sooner but copies the 500-line settings window and the lifecycle code (§10).

## 10. Alternatives considered

| Alternative | Why not |
|---|---|
| Bundle Ollama like llama-server | Ruled out by the requirement. It is also 1.4 to 1.5 GB per OS, which would double today's 1.4 GB bundle. |
| `enum Backend` with `match` in the app | Fine for two engines, but each new difference adds arms through about 2,000 lines of UI. That is the coupling this document removes. |
| A crate per backend now | The right end state if a third backend appears. Modules plus the architecture guard give the same boundaries without Cargo churn, and the split is mechanical later. |
| The `ollama-rs` client crate | It is async (tokio) in a thread-and-channel codebase. Seven endpoints are about 250 lines over `ureq`. |
| Use Ollama only as a model downloader and run its GGUF blobs with the bundled llama-server | Models that need Ollama's own engine would not run, and it is not what was asked. A manifest-to-blob resolver is a cheap later add-on. |
| Require the user to install Ollama themselves | Simplest, but the requirement is an in-app download. Using the one on this machine stays the first choice when present. |
| Stream the archive straight into the extractor | No staging file, but no resume and no after-the-fact verification. |

## 11. What was verified, and what is still assumed

**Verified on 2026-10-06**

- V1 Release v0.35.1, published 2026-09-29: asset names, sizes and sha256 digests, from the GitHub API
  (`repos/ollama/ollama/releases/latest`).
- V2 Linux archive: `.tar.zst`. First entries are `bin/ollama` (28.6 MB), `lib/ollama/*_LICENSE` and
  `lib/ollama/cuda_v12/libggml-cuda.so` (424 MB). Seen by decoding the first 64 MB of the stream.
- V3 Windows zip: `ollama.exe` at the root and `lib/ollama/` (87 entries, 1.94 GB extracted: `cuda_v12` 1.16 GB,
  `cuda_v13` 0.67 GB, `vulkan/ggml-vulkan.dll`, `llama-server.exe`). Read from the central directory through three
  HTTP Range requests (10 KB in total).
- V4 Ollama docs on `main` (`faq.mdx`, `gpu.mdx`, `linux.mdx`, `windows.mdx`): the environment variables above;
  Vulkan "enabled by default when the backend is installed", `OLLAMA_VULKAN=0`, `GGML_VK_VISIBLE_DEVICES`; Linux
  base and ROCm archives extracted over one prefix; the Windows standalone zips extracted into one directory and
  suitable for embedding; the RX 6700 XT is not in the ROCm list.
- V5 API reference (`docs/api.md`): endpoints, `/api/pull` stream shapes, preload and unload through
  `/api/generate` with `keep_alive`.
- V6 Pulling from Hugging Face as `hf.co/<user>/<repo>:<quant>`, defaulting to Q4_K_M: from several guides, not
  from Ollama's own page. Confirm there.
- V7 No public JSON endpoint for the library listing turned up (the searches found scrapers only). Absence is not
  proven.

**Assumed: settle in the Phase 1 spike** (throw-away HOME; the maintainer's RX 6700 XT / RADV machine is the case
Vulkan-in-base is for)

- A1 A `bin/ollama` extracted to an arbitrary prefix finds `../lib/ollama`. Documented for Windows embedding,
  assumed for Linux.
- A2 The Linux base archive contains `lib/ollama/vulkan/`, as the Windows one does.
- A3 `SIGTERM` makes `serve` unload its runners and exit; `SIGKILL` may orphan runner subprocesses. A precaution,
  not observed.
- A4 The first `serve` writes a key pair under `~/.ollama` even when `OLLAMA_MODELS` is set.
- A5 Whether v0.35 can run the Bonsai Q1_0 GGUFs through `/api/create`. Its package now ships a llama.cpp server
  runner, so it may.
- A6 `OLLAMA_NO_CLOUD=1` keeps a standalone `serve` free of any outbound call other than a pull. Check with the
  network_privacy stub.
- A7 The minimum Ollama version for `OLLAMA_CONTEXT_LENGTH` (the version gate for D3).
- A8 Windows: whether a console window appears for `llama-server.exe` or `ollama.exe` spawned from the release GUI
  build.
- A9 `ruzstd` throughput on a 1.4 GB archive (D7).

## Appendix: the Ollama API as QuickAI uses it

| Endpoint | Used for | Phase |
|---|---|---|
| `GET /api/version` | Readiness, attach detection, version in the log | 2 |
| `POST /api/generate` (no prompt) | Warm-up (`keep_alive: -1`, `options.num_ctx`); unload (`keep_alive: 0`) | 2 |
| `GET /api/tags` | The model list | 3 |
| `POST /api/pull` | Pull, with NDJSON progress | 3 |
| `DELETE /api/delete` | Delete | 3 |
| `GET /api/ps` | Optional: confirm the model is resident; real `size_vram` in place of the RAM heuristic | 3+ |
| `POST /api/show` | Optional: model capabilities (embedding-only) | 3+ |
| `/v1/*`, `/api/chat`, `/api/generate` from clients | Not called by QuickAI; decoded by the proxy for Live traffic only | 3 |
