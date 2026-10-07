# QuickAI architecture: a declarative runtime

_Status: accepted (Q1-Q15 as recommended); Phases 0 to 3 implemented, and parts of 4 (look-ups, bundles, components) at the user's request, see
[implementation-plan.md](implementation-plan.md) · Last updated: 2026-10-07 · Supersedes
[ollama-backend-architecture.md](ollama-backend-architecture.md)_

Companion files: [engine-facts.md](engine-facts.md) (what was verified about llama.cpp and Ollama, and what is
still assumed) and [examples/](examples/) (the design written as documents: llama.cpp, Ollama, vocabulary,
recipes, a profile, policy, UI). The examples parse as TOML 1.0 and cross-reference each other cleanly under a
throwaway checker; that is all they have been checked for.

The first proposal added Ollama by putting a Rust trait between the core and each engine. Three requirements
change the shape of the design:

1. Everything that defines how the application runs should be configuration.
2. Models run with adapters (picture, audio, video). Their extra models and the command-line arguments each
   runner needs live in configuration, and the UI grows extra controls when the configuration calls for them.
3. The whole must be extremely modular and extensible.

This document re-thinks the architecture around them. llama.cpp is re-expressed on it; Ollama is the first
new engine built on it.

## 1. Summary

- **QuickAI becomes an interpreter.** Rust supplies a small, audited vocabulary of *primitives*: spawn a process,
  probe an endpoint, call HTTP, install an archive, list releases, scan models, decode traffic, draw a widget.
  *Documents* compose them into engines, model recipes, adapters, settings forms, layouts and policies. llama.cpp
  and Ollama are two engine documents with no engine-specific Rust.
- **One pipeline.** Documents are loaded, validated and merged into an immutable *registry*. A *selection*
  (engine, model, adapters, values) is turned by a pure *planner* into a *run plan*, which the *runtime* executes.
  The UI is a projection of registry, selection and state.
- **Adapters are first-class.** A recipe lists components (main weights, a projector, LoRA, a draft model). Each
  engine document says how to attach each kind; the vocabulary says what UI each kind needs. Forms appear and
  disappear from data. For example (the flags are verified on the pinned llama-server), the `--video-ffmpeg-dir`
  and `--video-fps` fields would exist only when the selected model takes video input.
- **Capabilities are discovered, not guessed.** Flags come from the installed build's own `--help` (b11007 lists
  259 option lines, 139 with environment names), modalities from `/props` or `/api/show`. A binding says
  `requires_flag`; a build without the flag hides the field.
- **A hard line between data and code.** Data decides *what*; primitives decide *how*. A new flag, field, model,
  adapter or engine is a document. A new kind of action, protocol, archive format or widget is a primitive.
- **Safe by construction.** Documents that can run programs or widen the network (engines, policy, UI, bundle)
  load only from the embedded, bundle and user layers; remote content is data-only. All HTTP passes one
  allow-list. A broken document becomes a diagnostic, and the embedded defaults keep the app working.
- **Dogfooded.** The built-in llama.cpp and Ollama are ordinary documents. Parity with today is a golden test:
  the plan's argv equals today's argv, the `settings.json` keys are unchanged, the 215-case GUI suite passes
  untouched.
- **The cost, stated plainly.** This is a larger rewrite than the first proposal, and Ollama arrives later
  (Phase 3 rather than Phase 2). UI parity is the main risk (§17). The design pays off when several axes vary
  together (engines, models, adapters, UI), which is where the new requirements point. If only llama.cpp and
  Ollama without adapters were ever needed, the first proposal would be cheaper.

What changed from the first proposal:

| First proposal | Now |
|---|---|
| One Rust module per engine behind traits | No engine code: engine documents over shared primitives |
| `LaunchPlan` built by Rust | `RunPlan` built by a pure planner from documents, with provenance |
| Per-engine egui panels | Forms and panels projected from documents |
| `TrafficDecoder` per engine | Data: `decode` blocks in the vocabulary (framing, JSON pointers) |
| `ModelCatalog` and `RuntimeProvider` per engine | Primitives with parameters (`catalog.*`, `locate.*`, `feed.*`, `install.*`) |
| Additive settings in Rust | `legacy_key` on fields (compatibility) plus a generic values map |
| A Cargo feature to drop Ollama | A document can simply be absent |
| One model at a time | A run plan with one or more units (one in v1) |

What survives unchanged: the lifecycle (attach, warm-up, unload, management mode), the install pipeline (verify,
atomic rename, entry filter, smoke test), process hygiene, the privacy and test-suite impact analysis, and the
idea of a behaviour-preserving first phase.

## 2. Questions

### 2.1 Decisions that shape the design

Each has a recommendation. The rest of the document assumes the recommendations.

| # | Question | Recommendation | Why it matters |
|---|---|---|---|
| Q1 | What does "adapter" mean? (a) companion files loaded by the same runner: projector, LoRA, draft model; (b) separate sidecar runners side by side: whisper.cpp for speech in, stable-diffusion.cpp for images out; (c) both | **(a) now.** (b) is designed in (a run plan has units, a gateway can route) and built later | (b) means several processes at once, which breaks "one model at a time" and needs a gateway |
| Q2 | Is "one model at a time" still a product rule? | **Keep it for v1, relax later.** Ollama and llama.cpp's router mode (L9) already hold several; sidecars need several processes | Single versus multi-unit sessions, and the UI |
| Q3 | Which modalities first? | **Image input through a projector on llama.cpp**, then **LoRA** as the second kind (it proves "many" and live scale). Audio and video input come with the same projector mechanism; video adds an ffmpeg prerequisite field. Speech and image *output* need sidecars | Scope of Phase 4 |
| Q4 | Trust: who may define things that run programs or reach hosts? | **Tiers.** Privileged documents (engine, policy, ui, bundle) only from embedded, bundle and user layers; data documents (vocab, catalog, profile) may later also come from remote content packs. A user-layer engine that names a program the app did not install asks for confirmation once | A document that says `program = …` is code |
| Q5 | Format and expression language | **TOML** for authored documents (JSON accepted, same schema), JSON Schema generated from the Rust types for editor completion and CI. A **closed, hand-written expression language** (§7), no scripting. Alternatives: YAML (footguns), JSON (no comments), CEL (heavier), Lua or Rhai (arbitrary logic) | It is the surface everything else rests on |
| Q6 | How much is data in the first releases? | **A ladder:** engines and launch, then forms, then catalog, recipes and adapters, then layout, policy and strings. The shell (window, tab strip, log pane) stays code until Phase 5 | Parity risk |
| Q7 | Layout rule | **Until Phase 5 the data-driven UI renders exactly today's window in llama.cpp mode.** Redesigns come after | The GUI suite's OCR crops are calibrated to it |
| Q8 | Sequencing | **Declarative core first** (Phases 1-2), Ollama as the first new engine (Phase 3). The alternative, Ollama on the first proposal's design and a migration later, reaches a first Ollama sooner but throws away the per-engine Rust (API client, catalog, launcher, panel). Run the Ollama spike (A1-A9) **now, in parallel**; it does not depend on the architecture | Time to first Ollama versus rework |
| Q9 | Packaging contract with `quickai_base` | **Keep today's convention** (`llama-server` and `models/` next to the executable) as the built-in `next_to_exe` locate strategy, so existing bundles keep working. Later, `stage_bundle` also writes a `bundle.toml` (what ships, with hashes) | It changes the contract between the two repositories and the `packaged_bundle` suite |
| Q10 | Profiles in the UI? | **Data model now** (a selection already is a profile), **save/load UI later**, in Settings first | Layout parity |
| Q11 | (was D1) When an Ollama already answers on the port, attach to it? | **Yes.** Strategy `attach_if_running`; field `mode` (auto, attach, spawn) | Most Ollama users run it as a service |
| Q12 | (was D2) Own model store for a QuickAI-started Ollama? | **Yes** (`models_dir`), with a `use_default_store` switch | Pull and Delete must not touch models pulled with the Ollama CLI |
| Q13 | (was D3) Version policy | **A `tested` tag with pinned digests in the engine document**; newest through an explicit look-up | Ollama ships often (v0.35.1 on 2026-09-29) |
| Q14 | (was D7) zstd decoder | **`ruzstd`** (pure Rust), behind a throughput check (A9) | Cross-builds in old-glibc Docker images |
| Q15 | Will the Microsoft Store or Snap Store be a channel? | If yes, a **locked build mode** (`--features locked-config`) that ignores user-layer privileged documents and runtime downloads. Check each store's policy on downloaded executables before submitting | A runtime that downloads and launches configured programs may conflict |

Dissolved into data, so no decision is needed: **D4** (the engine selector is a `ui` panel, placed in Settings by
default), **D5** ("Open Chat UI" exists iff the engine `provides.web_ui`; Ollama does not), **D6** (a recipe with
no `run.ollama` is not offered under Ollama), **D8** (a document can be removed; see Q15).

### 2.2 Defaults assumed unless changed

- Documents live in `~/.config/quickai/<kind>/` (user), `<exe dir>/config/` (bundle) and inside the binary
  (embedded defaults). `QUICKAI_SAFE=1` ignores the user layer.
- Reloading is manual ("Reload configuration" in Settings) and automatic when idle; never mid-session.
- A headless tool in the same binary for development and CI: `quickai config check`,
  `quickai plan <profile> --explain`. Not a product feature yet.
- Strings are keyed from day one; no translations yet.
- sha256 is verified wherever a hash is known; a download with no hash (a user URL) is shown as unverified.
- Adapter compatibility comes from the recipe's pairing plus a filename hint (`mmproj*`); inspecting GGUF
  metadata comes later.
- Capabilities come from both the recipe and the running engine; the recipe wins.
- Existing `settings.json` keys stay (through `legacy_key`); unknown keys are preserved on save.
- A pure crate `quickai-core` (schema, expressions, config, registry, planner, form model); all effects stay in
  `quickai`.
- No scripting, plugins or WASM now; revisit when a real need appears.

## 3. Requirements as understood

| # | Requirement |
|---|---|
| R1 | Everything that defines how the app runs is configuration: engines (find, install, launch, probe, stop), models and their sources, adapters and the runner arguments they need, settings fields, layouts, limits, policies. The app ships its defaults as configuration too. |
| R2 | A model can run with adapters (picture, audio, video and more). Their extra models and the per-runner arguments are configuration, and the UI shows extra controls when the configuration calls for them. |
| R3 | Extremely modular and extensible: a new engine, model, adapter or setting is a document, not a recompile. A new *kind* of behaviour is a primitive that leaves existing documents untouched. |
| R4 | Nothing existing breaks: bundles, `settings.json`, the packaging contract, the 215-case GUI suite. (Added constraint.) |
| R5 | A configuration system that can launch programs and fetch URLs needs a trust model and a network policy. (Added.) |
| R6 | A broken document never bricks the app. (Added.) |

Non-goals: scripting inside documents; a visual document editor (later); remote execution or non-loopback
serving; making egui rendering itself data (widgets stay Rust).

## 4. The model in one picture

```
 embedded defaults ─┐
 bundle (exe dir) ──┤                                          ┌──────────────┐
 user config dir ───┼─► Loader ──► Registry ──┐                │   RunPlan    │
 override (tests) ──┘   parse · validate      │                │ units, phases│
                        merge · diagnose      ├─► Planner ───► │  provenance  │──► Runtime
                                              │   (pure)       └──────────────┘    supervisor · jobs
 Selection: engine, model, components, values ┘                                     gateway · installer
        ▲                                                                                  │
        │ edits                                                                            │ events
        └────────── UI: forms and panels projected from Registry + Selection + State ◄─────┘
```

| Term | Meaning |
|---|---|
| Document | A TOML file with `schema`, `kind`, `id`. Seven kinds (§5). |
| Engine | A runner described by a document: llama.cpp, Ollama, later whisper.cpp. |
| Primitive | A versioned Rust capability a document can name: `spawn@1`, `http_call@1`, `install.archive@1`. |
| Registry | The immutable, validated, merged set of documents. |
| Recipe | A model described by a catalog document: components and per-engine bindings. |
| Component | An artifact attached to a model at run time; the UI calls it an *adapter*. Each has a *kind* (`mmproj`, `lora`, `draft`, …). |
| Field | One setting: its UI, default, validation and bindings in one block. |
| Binding (sink) | How a value reaches an engine: `arg`, `env`, `file`, `http`, `port`. |
| Selection | Engine, model, components and values. A profile is a saved selection. |
| Run plan | The planner's output: units, each with phases. Serializable and explainable. |
| Unit | One supervised process or one attached endpoint. A v1 plan has one. |
| Context | The facts expressions can read: OS, RAM, GPU, runtime flags, model, values. |
| Tier | *Privileged* or *data*: what a document may define (§12). |

## 5. Documents

### 5.1 Kinds

| Kind | Defines | Tier | Typical source |
|---|---|---|---|
| `engine` | A runner: where it comes from, how it launches, probes and stops; the APIs it serves; its settings fields; how it attaches component kinds; hints | privileged | embedded, bundle, user |
| `vocab` | Shared nouns: modalities, component (adapter) kinds with their fields, API definitions with decoding and examples, and (WP4.9) the types of model and the classes of size a look-up sorts by | data | embedded, user, later remote |
| `catalog` | Model recipes, their components and sources; curated lists | data | embedded, user, later remote |
| `profile` | A saved selection, or a stack of units | data | user |
| `ui` | Window, screens and panels, settings sections, app-level fields, strings, theme | privileged | embedded, bundle, user |
| `policy` | Paths, the network allow-list, limits, timeouts, heuristics | privileged | embedded, bundle, user |
| `bundle` | What the package ships, with hashes (written by `quickai_base`) | privileged | next to the executable |

### 5.2 Envelope, layers, merging

```toml
schema = 1                      # the document's schema version; the app supports a range
kind = "engine"                 # engine, vocab, catalog, profile, ui, policy, bundle
id = "llamacpp"                 # unique per kind
extends = "llamacpp"            # optional: derive from another document of the same kind
requires = { app = ">=0.2", primitives = ["spawn@1"] }
```

- **Layers**, lowest first: embedded, bundle, user, override (`QUICKAI_CONFIG_DIR`, used by tests).
- A later document with the same `(kind, id)` *patches* the earlier one (`replace = true` replaces it).
- Tables merge key by key. Arrays replace, except arrays of tables that carry an `id`, which merge by `id`. A user
  can therefore add or override one field of the llama.cpp engine without copying the file.
- A privileged document from a data-tier source is rejected.

### 5.3 Compatibility and failure

- Documents declare `schema`; the app supports the current and previous version and migrates in memory. Unknown
  keys are warnings, and errors under `config check --strict`.
- Primitives are versioned (`archive@1`). An app without a required primitive version skips that document with a
  diagnostic that names it. Primitives keep the previous version for one release.
- Problems are diagnostics `{ severity, file, line, path, message, hint }`, shown in Settings, in the log, and by
  the CLI. An invalid user document is skipped and the embedded one stays in force. `QUICKAI_SAFE=1` ignores the
  whole user layer. Invalid *embedded* documents are a build failure (a test).

## 6. The domain model

### 6.1 Engine

An engine document has these sections ([`llamacpp.toml`](examples/engines/llamacpp.toml),
[`ollama.toml`](examples/engines/ollama.toml)). The two files have the same shape and different values.

| Section | Purpose | Primitives |
|---|---|---|
| `[engine]`, `[provides]` | Identity, license, the APIs served, accepted model formats, modalities, whether a web UI exists, whether the model name matters in a request, `egress` (hosts the engine itself contacts) | none |
| `[runtime]` | What the installed build reports: version, and its flags from `--help` | `exec.probe` |
| `[locate]` | Find the binary without touching the network | `locate.*` |
| `[acquire]` | List, download, verify and install builds | `feed.github_release`, `install.archive` |
| `[launch]` | Start strategies; spawn (program, args, environment, stop policy) or attach; readiness | `spawn`, `attach`, `probe.http` |
| `[[launch.prepare]]`, `[[launch.warm]]`, `[[launch.unload]]` | Steps around serving | `http_call`, `write_file` |
| `[models]` | Local sources, acquire, remove, look-up, how a typed string is read | `catalog.*`, `input_rules` |
| `[[field]]` | The settings form; each field carries its UI and its bindings | widgets, sinks |
| `[components.<kind>]` | How each component kind is attached | sinks |
| `[traffic]`, `[api_info]` | Which APIs the proxy decodes; what the API panel shows | `decode`, templates |
| `[[diagnostic]]` | Hints from log text or exit state | `diagnostic_rules` |

### 6.2 Sources and artifacts

A *source* says where bytes come from; an *artifact* is what a recipe component resolves to.

| Source | Meaning |
|---|---|
| `bundle` | A path under the package's directory |
| `huggingface` | `{ repo, rev, file }` |
| `url` | Any URL (checked against the network policy) |
| `github_release` | A release asset of a repository |
| `ollama_ref` | A name the engine pulls itself (`qwen3:4b`, `hf.co/user/repo:quant`) |
| `local` | A file the user points at |

Verification modes: `pinned` (a sha256 in a document), `digest` (the host reports one, such as GitHub's asset
digest), `registry` (the engine's own registry is content-addressed, as with an Ollama pull), `none` (a user URL;
shown as unverified). The acquire phase makes every artifact of the selection exist: present means skip, missing
means a `Job` with progress, and the result is a path.

### 6.3 Vocabulary

[`vocab/core.toml`](examples/vocab/core.toml) defines modalities (`text`, `image`, `audio`, `video`), adapter
kinds and APIs. An adapter kind states what the user sees (label, help, fields) and what the artifact is
(formats, how to spot one on disk); *how an engine attaches it is the engine's business*:

```toml
[[adapter_kind]]                # vocab: what it is
id = "mmproj"
label = "Vision / audio / video projector"
cardinality = "0..1"
discover = { glob = ["mmproj*.gguf", "*mmproj*.gguf"] }
fields = [ { id = "offload", label = "Offload projector to GPU", type = "bool", default = true } ]

[components.mmproj]             # engine: how llama.cpp attaches it
requires_flag = "--mmproj"
bind.arg = ["--mmproj", "${component.path}"]
[components.mmproj.fields.offload]
bind.arg_when_false = ["--no-mmproj-offload"]
```

An API definition carries the paths, the capabilities, how the proxy decodes it, and curl examples whose
availability is an expression (`when = "has(model.modalities.input, 'image')"` shows the image example only for a
model that takes images).

### 6.4 Recipes and components

A recipe ([`catalog/starter.toml`](examples/catalog/starter.toml)) names its components and, per engine, how to
run it. Today's `KNOWN_MODELS` and the bundled-model pins of `build/config.env` become recipes:

```toml
[[model]]
id = "qwen3-4b"
name = "Qwen3 4B"
modalities = { input = ["text"], output = ["text"] }
[model.components.main]
file = "qwen3-4b-q4_k_m.gguf"
[model.run.llamacpp]                      # a file for llama.cpp
[model.run.ollama]
ref = "qwen3:4b"                          # a named reference for Ollama
```

A recipe is *offered* under an engine when it has a `run.<engine>` table, the engine can attach every component
kind the table lists, and the model format is one the engine accepts (`formats` for files, `refs` for names). A
file with no recipe still appears (as today) with an automatic recipe built from its name; a sibling
`mmproj*.gguf` is suggested as its projector. A recipe with no `sources` only names a file the user brings.

**What is built of this (WP4.9, [lookup-and-bundles.md](lookup-and-bundles.md)).** The recipe as a catalog entry is not (that is Phase 4's), but the
part of it that a person meets first is: a model found online is fetched *with the files that go with it*. `[[models.bundle]]` of an engine document
names the kinds of file that go with weights (`mmproj`) and how to find one among a repository's files (patterns, a preferred quantization); the
engine's `[components.<kind>]` says how the program is told about one; a download of the bundle is one job whose end writes a record beside the
weights (`<weights>.bundle.json`), the picker lists weights only, and Start puts the files the record names on the command line
(`Selection.components`, after the fields of a lower `order` and before `extra_args`). A person adds a bundle (a draft model, say) with a document
of their own, as they add a field.

**What is built of the catalog kind (WP4.10, [catalog.md](catalog.md)).** The kind is read now: `[[model]]` with `[model.run.<engine>]` (files for an engine whose models are files,
a name for one that pulls), checked against the vocabulary and the engines, offered per engine as the entries the look-up draws, and shipped as eight pages
(81 models) that `scripts/catalog.py` makes from the real sites. It is the first piece of 4.1 and of `catalog.curated` (done as documents, not as a primitive: nothing is
asked of a network to list the models); components other than a file beside the weights, `sources` other than a repository, and verification modes other than a SHA-256 are Phase 4's.
`[[models.mode]]` of an engine document (how the engine is started for a kind of model) is new with it, so that a document that is data never carries a command-line word.

### 6.5 Selection, values, persistence

```
Selection { engine, model, components: { kind: [ { source, values } ] }, values: { "<engine>.<field>": value } }
```

- Engine and app fields are keyed by id (`llamacpp.ctx_size`, `app.dark_mode`). Component values belong to the
  attached component, because the fields come from the adapter kind, not from the engine.
- `persist` says where a value is remembered: `global`, `per_model`, `profile`, `session`.
- **Compatibility.** A field with `legacy_key` is stored at the top level of `settings.json` under that key, so
  files written today load unchanged and files written later still carry the old keys (the GUI suite probes
  them). Everything else goes under `values`. Unknown keys are preserved, so removing an engine document does not
  lose its stored values.

| Today's `settings.json` key | Field |
|---|---|
| `port`, `ctx_size`, `n_gpu_layers`, `extra_args` | `llamacpp.port`, `.ctx_size`, `.n_gpu_layers`, `.extra_args` |
| `llama_bin_override`, `models_dir_override`, `last_model_filename` | `llamacpp.bin_override`, `.models_dir`, `.last_model` |
| `dark_mode`, `enable_traffic_proxy` | `app.dark_mode`, `app.traffic_proxy` (fields of the `ui` document) |

### 6.6 The run plan

The planner is a pure function: `plan(registry, selection, host facts) → RunPlan`. A plan is JSON, so it can be
snapshot-tested, shown to the user, and run headless.

```
RunPlan { units: [ Unit { id, engine, role, phases, endpoint, apis, provenance } ], gateway }
```

| Phase | What | From |
|---|---|---|
| acquire | Make the selection's artifacts exist (download, verify) | recipe components |
| prepare | Derive what the engine needs: write a file, build a derived model | `[[launch.prepare]]` |
| start | Spawn a process, or attach to an endpoint; an ordered list of *strategies* | `[launch]` |
| ready | Wait until the engine answers | `[launch.ready]` |
| warm | Load the model when the engine loads lazily | `[[launch.warm]]` |
| serve | Steady state; a liveness probe when attached; the traffic tap | none |
| unload | Release the model | `[[launch.unload]]` |
| stop | Stop what QuickAI started | `launch.spawn.stop` |

llama.cpp uses acquire, start, ready and stop. Ollama uses all but prepare (unless an adapter needs a derived
model). *Management mode* is the same plan with no model: no warm-up, so a daemon can list, pull and delete.

`plan --explain` shows where every value came from. For the fictional vision recipe on llama.cpp:

```
plan: example-vl-7b on llamacpp
 acquire  main    Example-VL-7B-Q4_K_M.gguf          huggingface   missing: download
          mmproj  mmproj-Example-VL-7B-f16.gguf      huggingface   present
 start    spawn   /…/llama-server
            --model /…/Example-VL-7B-Q4_K_M.gguf     recipe components.main
            --host 127.0.0.1                         engine launch.spawn.args
            --port 8080                              field llamacpp.port
            --ctx-size 8192                          field llamacpp.ctx_size (default: recommend_ctx_size)
            --n-gpu-layers 999                       field llamacpp.n_gpu_layers
            --mmproj /…/mmproj-Example-VL-7B-f16.gguf  component mmproj
          env LD_LIBRARY_PATH=/…:$LD_LIBRARY_PATH    engine launch.spawn.env
 ready    GET /health == 200   (120 s)
 stop     kill
```

The argument order is: the engine's base arguments, then field bindings by `bind.order` (default: the field's
`ui.order`, which reproduces today's order), then component bindings in attachment order, then `extra_args`.

## 7. Expressions and bindings

### 7.1 The language

One small language serves every condition and default, so documents need no scripting. It is closed: no loops,
no recursion, no assignment, no user functions. Every expression is parsed and type-checked at load time against
the context table, so a typo is a diagnostic with a file and line, not a runtime surprise.

| Part | Content |
|---|---|
| Values | boolean, integer, float, string, list, null |
| Operators | `==  !=  <  <=  >  >=  and  or  not  in` and parentheses |
| Paths | dotted names resolved against the context |
| Functions (pure) | `has(list, x)`, `len`, `lower`, `matches(s, re)`, `starts_with`, `is_url`, `semver_ge`, `min`, `max`, `clamp`, `recommend_ctx_size(size, ram)`, `speed_tier(…)` |
| Functions (the machine, asked through the caller) | `is_file(path)`: whether the typed string names a file on this machine, answered by the caller for the one string the rules are about (the context carries the answer as `input_is_file`); any other text is not a file. `path_after_host(url)`: what follows the host, without the slashes around it |
| Interpolation | `${path}` inside strings. A string that is exactly one `${…}` keeps the value's type (a number stays a number in a JSON body); otherwise the result is a string |

| Context | Meaning |
|---|---|
| `os`, `arch` | `linux`, `windows`, `macos`; `x86_64`, `aarch64` |
| `sys.ram_bytes`, `sys.cpus` | detected hardware |
| `gpu.known`, `gpu.vendors` | from GPU detection; `known` is false when nothing could be detected; a vendor is asked for with `has(gpu.vendors, 'nvidia')` |
| `runtime.binary`, `runtime.dir`, `runtime.build` or `.version`, `runtime.has_flag('--x')` | the located build and what it reported about itself |
| `model.id`, `.ref`, `.size_bytes`, `.modalities.input`, `.main.path`, `.components.*` | the selected recipe |
| `component.path`, `component.<field>`; `components.<kind>` | inside a component binding; the attached components of a kind with their values (for `each` and `live` bindings) |
| `field.<id>`, `value` | engine fields; `value` is the current field inside its own bindings |
| `app.<id>` | app-level fields |
| `listen_port`, `public_port` | the port the engine binds; the port clients use (different when the proxy is on) |
| `session.running`, `.crashed`, `.model_display`, `.model_ref`; `ui.show_api` | runtime and UI state |
| `data`, `config`, `exe_dir`, `home` | path variables from policy |
| `input` | the typed string, inside `input_rules` only |

### 7.2 Bindings

A field carries the rule that turns its value into engine configuration, next to its label and range. Adding a
setting is one block.

| Sink | Writes | Notes |
|---|---|---|
| `arg` | argv fragments | `arg_when_false`; `arg_split` (shell-like split of a text field, nothing when empty); `each` plus `sep` joins per-component items (`--lora-scaled a:1.0,b:0.5`); `emit = "always"` or `"when_set"` |
| `env` | environment variables | a field's `bind.env` sets named variables from its value (a `when` over `value`; nothing while the field has no value), after the document's own; `prepend` with `sep`. `locked = true` (on the document's own entries) and the names listed in `env_lines.protect` keep the person's own lines from overriding them, and so does every variable the document adds to with `prepend`; a protected name is refused in any spelling. `env_lines` turns a `kv_lines` field into the person's own `KEY=VALUE` lines: they come last, a later line for a name replaces an earlier one in place (and a document's variable that is not protected where it stood), blank lines and `#` comments are skipped, a bad or protected line is a message under the field and the text stays as typed. The log's second line shows them, secrets hidden |
| `file` | a rendered config file | for engines configured by file: llama.cpp router presets (L10), a Modelfile |
| `http` | a field of a named HTTP call | a field of the warm-up body (not `num_ctx`: Ollama undoes a per-request value on the next request without one, so the context travels as `OLLAMA_CONTEXT_LENGTH`, A7); `live` calls run while serving |
| `port` | the engine's listen address | `public` marks the port clients use |
| `derive` | a named fact for expressions | `gpu_offload = "value > 0"` feeds the speed-tier and recommended-model heuristics |

Argv is a list: no shell is involved and nothing needs quoting.

### 7.3 Flag and version predicates

Engines change their flags between releases, and a user may install an older or newer build than the one the
manifest was written against. So bindings do not guess from version numbers; they ask the build.

- `runtime.has_flag('--mmproj')` is answered from the build's own `--help`, parsed once per binary and cached by
  path and modification time. In b11007 the text is regular: 259 option lines start in column 0 and 139 flags
  name an environment variable (L2). Aliases are collected too (`-md` and `--model-draft`).
- `runtime.build` comes from `--version` (`build 11007`, L1); `runtime.version` from `ollama --version` (the pattern `version is (\S+)` reads both its forms, A17) or, once a server answers, `GET /api/version`.
  These are the fallback when a build's help cannot be parsed, and for engines with no flags.
- **The parser must skip tombstones.** llama.cpp leaves removed flags in `--help` with a description such as "the
  argument has been removed. use --spec-draft-n-max" (L13: five in b11007). Counting every column-0 option as
  available would report `--draft-max` as present, so the `help_flags` parse takes a `skip` pattern.
- `requires_flag` on a field or a component hides it, and blocks attaching it, with a one-line reason. The
  speculative-decoding flags show why this is worth having: the draft-token limit moved from `--draft-max` to
  `--spec-draft-n-max` and the build says so itself. A binding written for the old name simply does not appear on
  a build that has removed it, and the manifest is fixed in one place when a flag moves.

## 8. Primitives: what stays code

| Group | Primitives (versioned) | Parameters that live in documents | Replaces today's |
|---|---|---|---|
| Locate | `locate.setting`, `.installed`, `.next_to_exe`, `.path_search`, `.known_paths`, `.dev_tree` | roots, names, per-OS binary paths | `paths.rs` |
| Introspect | `exec.probe` (run a command, parse its output: version, `help_flags`) | command, regex | new |
| Feeds | `feed.github_release` (asset rules by keywords or by exact name, prerelease policy; digests and `feed.static` are not built yet) | repository, counts, rules | `llama_releases.rs` (replaced in Phase 2) |
| Install | `install.archive` (zip and tar.gz built; tar.zst, verify, entry filters, smoke test and atomic rename are Phase 3) | root, binary name and search depth | `llama_install.rs` (replaced in Phase 2) |
| Fetch | `fetch.file` (stream to a file with progress and sha256), `net` (the allow-list) | directory, hash | `downloader.rs`, `download_to` |
| Process | `spawn` (argv, environment, own process group or job object, stop policy), `attach` | program, args, env, stop | `server_manager.rs` |
| Probe | `probe.http`, `probe.tcp`, `probe.exit` | path, expected status, timeout | `spawn_health_poller` |
| Calls | `http_call` (method, path, body, timeout; streaming `sse` or `ndjson`; progress and extract mappings by JSON pointer) | everything | new (Ollama) |
| Files | `write_file` (render a template) | template, path | new |
| Catalog | `catalog.scan_dir`, `.bundle_dir`, `.engine_list` (an `http_call` plus a mapping), `.curated` (the multi-step Hugging Face look-up that was to be `.hf_gguf` is `lookup.huggingface_gguf`, below) | directories, filters, counts | `models.rs`, `remote.rs` |
| Look-ups | `lookup.ollama_library` and `lookup.huggingface_gguf` (both built: WP3.15 and WP4.9). Ollama publishes no list a program can read, so the page of its search is read; Hugging Face's search and the files of each repository are asked of its API. Both end in the same kind of result, which `quickai-core::lookup::present` makes into what the window draws: a type from the vocabulary (`[[model_type]]`), a button for each way to fetch it with its size class (`[[size_class]]`), and the bundles that go with it; declared by `[[models.lookup]]` (one button of the model box for each) and `[[models.bundle]]`, see [lookup-and-bundles.md](lookup-and-bundles.md) | the site, how many repositories, which quantizations, which files are not weights | `remote.rs`, the look-up panels of `app.rs` |
| Components | `[components.<kind>]` of an engine document: the arguments a companion file (a projector) adds to the command line, in the order of the fields' (WP4.9; the rest of §6.3's design for a component is Phase 4) | `bind.arg`, `order` | none (a file path given with `extra_args`) |
| Rules | `input_rules`, `diagnostic_rules`, `discover` (globs) | patterns, hints | `app.rs` branches |
| Decode | `decode` (framing sse, ndjson or json; JSON pointers; chat-message shape) | pointers | `proxy.rs` |
| Gateway | `gateway` (relay, tap, routes) | ports, routes | `proxy.rs` |
| Functions | `recommend_ctx_size`, `recommend_model`, `speed_tier`, `format_bytes` | thresholds from policy | `hardware.rs` |
| Hardware | `gpu.detect`, `sys.ram` | vendor patterns | `gpu.rs`, `sysinfo` |
| UI | widgets and panels (§9) | none | `app.rs` |

**The rule.** A need is *data* when it is a new value, flag, field, endpoint, rule, model or engine built from
existing primitives. It is a *primitive* when it needs a new kind of action: a protocol, an archive format, a
widget, a platform capability. Ollama needs new primitives (tar.zst, `http_call` with NDJSON progress, attach,
process-group stop) but no Ollama-specific code: every one of them is useful to the next engine.

A new primitive ships with a name and version, a schema for its parameters (documentation is generated from the
type), golden tests, and a test against a real resource, following the project's convention.

## 9. UI as a projection

### 9.1 The form model

`FormModel::build(registry, selection, host facts)` returns sections of fields, each with its resolved value,
visibility, enabled state, error, help and origin. It lives in the pure crate, has no egui, and is tested
headless. The egui layer only renders it and sends back `set(field, value)` and `invoke(action)`.

| Field attribute | Purpose |
|---|---|
| `id`, `label`, `help`, `type` | Identity and text. Types: `int`, `float`, `bool`, `text`, `choice`, `port`, `dir`, `file`, `url`, `runtime_path`, `kv_lines`, `model_ref`, `hidden` |
| `range`, `step`, `options`, `pattern`, `optional` | Validation and widget hints |
| `default` | A value, or `{ fn, args }` computed from the context |
| `recompute_on` | When a default is re-derived (`model`: picking another model re-derives the context size, as today). A second trigger, `reset_on`, was designed and is not built: nothing needs it yet |
| `visible_when`, `enabled_when`, `required_when` | Conditions over the context |
| `validate` | Rules `{ check, message }` that may look at other fields; the first whose `check` does not hold gives its `message` as the field's error |
| `persist` | `global`, `per_model`, `profile`, `session` |
| `applies` | `on_start` (default), `spawn_only`, `live` |
| `legacy_key` | The `settings.json` key it keeps |
| `bind` | Sinks (§7.2) |
| `ui` | `section` (`primary`, `advanced`, or one of your own; the section is what makes a field "advanced"), `order`, `widget`, `hint` (the placeholder of an empty text box) |

Sections are contributed by the app form, the engine form, one form per attached component (from the adapter
kind) and, optionally, the recipe. Conditions filter them.

### 9.2 Widgets and panels

Widgets and panels are Rust, in registries. A document picks which, in what order and when. A new widget is a
primitive: code, documentation and a snapshot test.

- **Widgets:** integer drag, slider, checkbox, combo, text, key-value lines, path picker, port, model picker,
  component slot, runtime manager, progress, markdown, button.
- **Panels:** `header`, `model_bar`, `acquire`, `adapters`, `controls`, `run_bar`, `api_info`, `diagnostics`,
  `log`, `engine_selector`, `form`, `installed_models`, `diagnostics_config`.
- The **runtime manager** generalises today's llama-specific "Advanced: llama.cpp" block (the binary override,
  installed builds, look-up results, install progress) into one widget configured by an engine's `[acquire]`.
  Built in Phase 2: its sentences are fixed in the widget with the engine's names put in (its name, its
  server's name, the repository, where builds are kept, the backends), and the engine document supplies only the
  placeholder of the filter box. The widgets built so far are integer and float drag boxes, port, checkbox,
  combo, one-line text and the runtime manager, and (Phase 3) key-value lines, a path picker with a browse
  button, a progress bar and a button. A widget hands back a value (`set`) or an action (`invoke`); the names a
  document may give `ui.widget` are the ones the interface declares, and the loader refuses another. A button is
  a field that holds nothing, with `ui.action`.
- The keys of an engine document are described in [reference/engine-fields.md](reference/engine-fields.md),
  generated from the types (`quickai config docs`); those of a vocabulary document in
  [reference/vocab-fields.md](reference/vocab-fields.md) (`quickai config docs vocab`).

### 9.3 What appears when

| When | UI effect | Data behind it |
|---|---|---|
| The selected model takes video input | "ffmpeg folder" and "Video frames per second" appear under Advanced | `visible_when` on two fields (flags verified, L5) |
| The recipe has an `mmproj` component and the engine can attach it | An Adapters panel appears: a projector slot, "Offload projector to GPU", and (advanced) image-token limits | `model.components`, `[components.mmproj]`, the adapter kind's `fields` |
| The installed `llama-server` lacks a flag | The field or component is hidden, with a reason on hover | `requires_flag`, `runtime.has_flag` |
| A LoRA is attached | A Scale slider per adapter; changes apply live | the adapter kind's `fields`; `[components.lora].live` (L6) |
| The engine has no web UI (Ollama) | No "Open Chat UI" button | `provides.web_ui` is absent |
| Attached to a daemon QuickAI did not start | Environment-bound fields are disabled: "owned by the running Ollama" | `applies = "spawn_only"` |
| The GPU is positively known not to be NVIDIA | The installer card says the CUDA files (about 1.8 GB) will be skipped | `install.skip` with `gpu.known` |
| A `mmproj*.gguf` sits next to a model with no recipe | The Adapters panel suggests it | the adapter kind's `discover` glob |

### 9.4 Parity strategy

The GUI suite reads pixels: header icon positions, tab positions, crops of the main buttons, a crop of the Live
tab. So the data-driven UI is adopted on a ladder, each rung gated by the suite:

1. The shell (window, tab strip, log pane) stays code until Phase 5.
2. Phase 2 renders the "Advanced" block and the primary controls row from fields. The `controls` panel
   reproduces today's exact egui layout for the three fields it has now.
3. Labels and hover texts move into documents **verbatim**; the suite reads some of them with OCR.
4. The default layout document *is* today's layout. New rows (Engine, Adapters) appear only when a document
   asks, so llama.cpp mode with only the embedded documents is pixel-identical.
5. A redesign is a later, separate step, and a deliberate recalibration of the suite.

## 10. Adapters and modalities

### 10.1 Four ways a capability is added

| Way | Example | Mechanism |
|---|---|---|
| Engine-native | An Ollama multimodal model | Nothing to attach; modalities are discovered (`/api/show` `capabilities`, O6) |
| Companion component | A projector, a LoRA, a draft model, a control vector | Same process, extra arguments; `[components.<kind>]` bindings |
| Tool prerequisite | ffmpeg for video input | A `locate` for the tool plus fields that pass its directory |
| Sidecar unit | whisper.cpp for speech in, stable-diffusion.cpp for images out | A second unit in the run plan, behind the gateway (§10.5) |

### 10.2 Kinds against engines

llama.cpp column verified on the pinned build (b11007, `--help`); Ollama column read from its source at v0.35.1 (`server/create.go`, O7 to O8), with run-time behaviour confirmed on a real v0.35.1 binary (A14).

| Kind | llama.cpp | Ollama | UI fields |
|---|---|---|---|
| `mmproj`: image, audio, video input | `--mmproj FILE`, `--mmproj-url`, `--no-mmproj`, `--mmproj-offload` / `--no-mmproj-offload`, `--mmproj-device`, `--image-min-tokens`, `--image-max-tokens`; with `-hf` the projector is fetched automatically | Library models are self-contained and `/api/show` reports `capabilities`; for an imported GGUF, a projector GGUF in `files` of `/api/create` is stored as a projector layer (whether it is used at run time is open, A14) | offload, device, image-token limits |
| video input (with the projector) | `--video-fps`, `--video-timestamp-interval`, `--video-ffmpeg-dir`; request part `input_video` | not documented | ffmpeg folder, fps |
| `lora` | `--lora FNAME[,…]`, `--lora-scaled FNAME:SCALE,…`, `--lora-init-without-apply`; `GET` and `POST /lora-adapters` change scales live | **Not supported in v0.35.1**: `/api/create` refuses `adapters` and adapter GGUFs ("LoRA adapters are no longer supported", O7). The Ollama engine document declares no `lora` component, so the panel never offers it there | scale (live for llama.cpp) |
| `control_vector` | `--control-vector[-scaled]`, `--control-vector-layer-range` | none | scale, layer range |
| `draft` (speculative decoding) | `--model-draft` (`-md`), `--spec-draft-n-max`, `--spec-draft-n-min`, `--n-gpu-layers-draft`, `--spec-type …` | `draft_files` in `/api/create` stores a draft layer; PARAMETER `draft_num_predict` sets the tokens per step (O7, O8) | tokens to draft, draft GPU layers |
| vocoder (speech out) | **no flag in `llama-server` b11007 (L8)** | none | none: sidecar territory |
| embedding or rerank | `--embedding`, `--rerank`, `--pooling` (a dedicated instance) | separate models | mode |

### 10.3 Worked example: adding a vision model

1. Someone adds a recipe with `main` and `mmproj` components and `run.llamacpp.components = ["main", "mmproj"]`
   to a catalog document in the user layer, and reloads.
2. Validation passes: the vocabulary has the `mmproj` kind and the llama.cpp engine has `[components.mmproj]`.
   Under Ollama the recipe is offered only if it has a `run.ollama.ref`.
3. The model picker lists it with an image badge (from `modalities`). Selecting it adds an Adapters section: the
   projector slot (default: the recipe's file), "Offload projector to GPU", and the advanced token limits.
4. Start plans an acquire phase that makes both artifacts exist (download, sha256, progress), then the
   `--mmproj` argument and, if the box is unchecked, `--no-mmproj-offload`.
5. The API panel's examples include the image request (its `when` is true), and the Live tab already shows image
   attachments.
6. Nothing in Rust changed. If the installed build lacks `--mmproj-offload`, the checkbox is not shown.

### 10.4 llama.cpp's router mode: a second launch variant

`llama-server` started without a model becomes a router that loads and unloads models on demand, routes by the
request's `model` field, and reads an INI preset file whose keys are CLI arguments (L9, L10). That is the Ollama
lifecycle (daemon, then models on demand) on llama.cpp. The architecture absorbs it without a new concept: a
`[launch.variants.router]` in the same engine document, the existing attach, management and warm phases, and a
`file` sink that renders recipes (with their components) into the preset. A recipe's `mmproj` then becomes a
`model-draft`-style key of its preset section instead of a command-line argument. Whether to offer it is a
product question (Q2).

### 10.5 Sidecars and stacks (designed in, built later)

A profile may list several units; a gateway routes by path and taps all traffic for the Live tab
([`profiles/voice-chat.toml`](examples/profiles/voice-chat.toml)). The run plan already has `units`, and the
supervisor starts them in dependency order (`optional = true` units may fail without failing the session). What
is missing today is only the supervisor starting more than one and the gateway routing, plus engine documents for
the sidecars (whisper.cpp and stable-diffusion.cpp flags are from third-party guides, S1 and S2).

## 11. Runtime

### 11.1 Supervisor and session

`Session` is `ServerManager` made generic: it executes a run plan and keeps the five states, the event channel,
and `poll()` once per frame. The health poller becomes a bring-up step (probe, then warm-up, then Running).

| Moment | llama.cpp | Ollama, spawned | Ollama, attached |
|---|---|---|---|
| Start | spawn, probe `/health`, Running | spawn `serve`, probe `/api/version`, warm up, Running | probe (the port must answer *as Ollama*: JSON with a `version`), warm up, Running |
| Stop | kill, Idle | unload, stop the process group (SIGTERM, grace, kill), Idle | unload, Idle; the daemon is untouched |
| Process exits unasked | Crashed | Crashed | no child; a liveness probe every 5 s reports Crashed after three misses in a row (one slow answer while a model loads is not a crash) |
| Warm-up fails (out of memory, unsupported model) | not applicable | log the engine's error, stop what QuickAI started, Crashed | log the error, Crashed |
| App exit | `stop_blocking` | the same, with a 2 s grace so closing the window is not held up | unload, detach |

A start that is not answered within the ready timeout is, by default, a failure: the log says so, what QuickAI started is stopped (with the plan's grace) and the state is Crashed. An engine whose program may simply be slow says `on_timeout = "wait"` (llama.cpp does): the log says so and the run goes on being watched, in Starting, until the process exits or Stop is pressed.

*Management mode* starts the plan with no model so a daemon can list, pull and delete: state Running, status
"Ollama ready, no model loaded". Choosing a model and pressing Start then only runs the warm phase (`Session::load`; Start is available again exactly while the engine runs with nothing loaded and the picker has a model, and not while a load is under way, `Session::loading()`); a load that fails there is logged and leaves the daemon running with nothing loaded, where a warm-up that fails *during* Start stops what QuickAI started and ends in Crashed. A first run is therefore two steps, because nothing is listed before a daemon runs: Start brings it up, and Start again loads the model the picker then shows.

A port that answers is attached to only if it answers like Ollama (`GET /api/version` returns JSON with a string `version`): a llama-server, a web server or anything else on the port is "something else is using the port", neither attached to nor a reason to start a second daemon on it. With `mode = spawn` a port where an Ollama already answers is refused. The plan carries both strategies in order (`mode = auto`) and the session chooses at the moment of the start, because the planner cannot know what answers (docs/phase3-test-plan.md, section 3). The place that is asked is the port the settings give (`${public_port}` in the document), not the internal one a gateway would use: a daemon that was already there is never behind a gateway of ours.

### 11.2 Process hygiene

- **Unix:** a program whose document says `group = true` is spawned in its own process group. Stop sends SIGTERM
  to the group, waits the grace period, then SIGKILLs the group (and once more after the leader has gone, for
  what it left behind). On Linux, `PR_SET_PDEATHSIG` makes every child, with a group or without, follow QuickAI if
  QuickAI dies (it fires when the spawning *thread* exits, so each run has one supervisor thread that starts the
  program and stays until it is gone; `runtime/supervisor.rs`). It matters more for Ollama, whose child
  owns GPU memory and runner subprocesses. The signal is **SIGTERM**, not SIGKILL: the flag is not inherited by the
  runner, and only a graceful stop of `ollama serve` takes its `llama-server` runner down (A3: after a SIGKILL of
  the server alone the runner was still alive 36 s later; SIGTERM or a group kill removed both in 0.1 s).
- **Windows:** assign every child to a Job Object with `KILL_ON_JOB_CLOSE`; Stop terminates the job. A GUI process
  has no practical way to deliver a console Ctrl event, so there is no graceful signal. Spawn with
  `CREATE_NO_WINDOW`: a console child of a GUI-subsystem release build otherwise opens a console window (A8; it
  may already happen with `llama-server.exe`).
- CI builds Linux, Windows and macOS, so every `cfg` branch must compile.
- Do not leak the AppImage or snap loader environment into a downloaded runtime: drop the `LD_LIBRARY_PATH` and
  `LD_PRELOAD` entries under `$APPDIR` or `$SNAP` before spawning, and only for an engine that is not `bundled`
  (a llama-server in a snap needs the snap's own libraries). The plan's operations then apply to what is left;
  llama.cpp keeps today's behaviour of prepending its own directory to what it inherited.
- The first log lines are the plan: the command (`$ program args`, unchanged) and, when the plan sets any, a second
  line `env NAME=value ...` of the variables it *sets* (a loader path it only prepends to is not shown), with the
  value of anything named like a secret (KEY, TOKEN, SECRET, PASSWORD) written as `***`.

### 11.3 Gateway and traffic

- `Event`, `ServerState` and `TrafficEvent` move to `events.rs`; the proxy stops importing `server_manager`.
- The proxy takes its decoders from the vocabulary: framing (SSE or NDJSON, `application/x-ndjson`, one JSON
  object per line) and JSON pointers for the text. Relaying stays byte-for-byte; sniffing stays best-effort.
  Message-content summarising (text parts, attachments) is a primitive with pointer parameters
  (`images = "/images"` for Ollama's native chat). The decoders travel in the run plan (`RunPlan.decode`, with the
  gateway), each API claims exact paths, the content type the engine sends chooses among an API's framings, a
  request with no prompt or no messages (a load or an unload) is not shown, an `error` in a reply or a stream is
  shown as an error, and an error status is relayed to the client as it is. A client of an engine that dies
  mid-reply waits for its own timeout (the server library cannot reset a connection); the Live entry is closed
  at once.
- **Attach plus proxy.** The proxy must bind a port other than the attached daemon's (both default to 11434), and a
  second port would show a URL nobody configured, so in v1 **no gateway is started in front of a daemon that was
  already there**: the public port is the daemon's own, the log says that live traffic needs an Ollama QuickAI started,
  and the toggle's hover text says so.
- With several units, the same component routes by path (§10.5).

### 11.4 Jobs

One `Job { rx, label, verb, downloaded, total }` replaces `DownloadHandle` and `InstallHandle`, which have
identical fields. Kinds: artifact download, runtime install, model pull. `Status(String)` events carry the
engine's own words (`pulling manifest`, `verifying sha256 digest`) to the log pane.

### 11.5 The install pipeline

Generalised from `llama_install::spawn_install`; the Ollama document ([`ollama.toml`](examples/engines/ollama.toml))
is its first user.

```
click "Download"                              the only trigger
  ├─ preflight   free space ≥ archive + 1.6 × archive + 256 MB, from the size the host lists; target not already complete
  ├─ download    <root>/<tag>.<ext>.part, with progress, its sha256 worked out on the way in
  ├─ verify      sha256 equals the pin, or GitHub's `digest` for the asset (before anything is unpacked)
  ├─ extract     into <root>/<tag>.partial/ through the entry filter; `.zip`, `.tar.gz`, `.tgz`, `.tar.zst`
  ├─ locate      the binary; run `<binary> --version` as a smoke test
  └─ rename      <tag>.partial → <tag>          atomic
```

- **Atomic.** `spawn_install` today extracts into the final directory and returns Done as soon as `find_binary`
  succeeds. If the app is killed mid-extraction, the next run can find a directory where the binary exists and
  call it installed. With Ollama the binary is extracted first and 1.4 GB of libraries after it, so
  extract-then-rename is a requirement.
- **Verification.** A digest from the host catches truncation and corruption but not a tampered release, so the
  tested tag's digests are also pinned in the document. When both exist they must agree, or the file is refused (it
  changed after it was tested); a build with neither (an address the person pasted) is installed because they asked for
  exactly that file, and is shown as unverified. A digest that does not match is found **before** extraction starts.
- **Entry filter.** A path must be relative and stay inside the target, a symbolic link must be relative and stay
  inside it, and an entry that is not a file, a directory or such a link (a hard link, a device) is refused; **any
  unsafe entry refuses the whole install**, where the `tar` reader on its own skips an entry that climbs out and goes
  on (found while preparing the tests, Z10). The checks are QuickAI's own, and `tar::Entry::unpack_in` and
  `zip::ZipFile::enclosed_name` stay underneath them. Default policy: skip `lib/ollama/cuda_*` only when `gpu.known` and the GPU is
  not NVIDIA; unknown means nothing is skipped. Of the Windows zip's 1.94 GB extracted, 1.83 GB is CUDA (O3).
- **Overlays.** A build may have several assets: Ollama's ROCm archive extracts over the base. Off by default,
  because Vulkan in the base covers AMD and Intel.
- **Smaller downloads (Phase 5).** The Windows zip supports HTTP Range requests on GitHub's CDN (O3), so only the
  wanted entries need be fetched (about 0.11 GB instead of 1.47 GB). A `.tar.zst` is a stream and cannot be
  ranged: a Linux user without Ollama pays the full 1.44 GB, which is why finding an installed Ollama and
  attaching matter more there.

## 12. Security and privacy

### 12.1 Tiers

| Tier | Kinds | May define | Loaded from |
|---|---|---|---|
| Privileged | engine, policy, ui, bundle | programs to run, hosts to contact, what the user is told | embedded, bundle, user |
| Data | vocab, catalog, profile | names, sources, selections, text | embedded, user, later remote |

A data document cannot introduce a program or a host: sources are checked against the policy's allow-list. A
user-layer privileged document is the user's own business, with one guard: an engine whose document has a layer above
the app's own and that starts a program the app did not install or locate (by the strategies of its own document) asks
for confirmation before it starts. A yes is remembered by the hash of what the merged document says, not by how it is
written, so the question is asked once for each version of the document (`quickai-core::guard`; implementation-plan C49).

### 12.2 Enforcement

- **One network layer.** Every primitive that fetches goes through `net`, which enforces `policy.network.allow`
  and the "only on a click" mode. A blocked host is a diagnostic, and the user may add it explicitly.
- **Engine-side traffic.** QuickAI cannot police a child's own connections (an Ollama pull, a `llama-server -hf`).
  It (a) never triggers them without a click, (b) passes the flags that keep them off (`OLLAMA_NO_CLOUD=1`;
  llama.cpp's `--offline`), and (c) requires the engine to declare `egress`, which feeds the privacy table.
- **No shell.** Argv is a list. No scripting. Path variables are confined to known roots in non-user layers.
- **Hashes.** `verify = digest | pinned | registry | none`; `none` is shown as unverified.
- **Binds.** `policy.network.bind = "127.0.0.1"`; an engine's `locked` environment (`OLLAMA_HOST`) cannot be
  overridden by the user's extra environment.

### 12.3 Privacy by construction

The set of hosts the app can contact is the policy's allow-list plus each engine's declared `egress`. A generated
table in `PRIVACY.md` and a check in the network_privacy suite keep prose, documents and behaviour in step. What
changes in the policy text when Ollama arrives:

- New requests QuickAI itself makes: GitHub's releases API and the release asset download, both on a click.
- Requests *Ollama* makes when the user clicks Pull (its registry, or Hugging Face for `hf.co/…` names): QuickAI
  does not make them but triggers them, and the text says so. Measured through a forwarding proxy: a pull of a 46 MB
  model contacted `registry.ollama.ai:443` and nothing else (O21); a large model and a Hugging Face reference are not
  yet observed, and `egress` says only what was seen.
- The daemon is started with `OLLAMA_NO_CLOUD=1`, locked against the user's extra environment: measured, without it an idle `serve` contacts `ollama.com` within 40 ms of starting and again about every 4 hours, and with it a whole session made no non-loopback connection and no DNS lookup (A6). Ollama writes its own key pair under `~/.ollama` at the first start (A4), and the GPU driver writes shader caches under `~/.cache`; the privacy text says so.
- New on-disk data: `~/.quickai/ollama/` (runtime, models, catalog cache).
- "Bundled software" gains a line: Ollama is not bundled; it is downloaded on request, MIT-licensed, and its
  archive carries its own licence files.

## 13. Testing

| Layer | What | How |
|---|---|---|
| Schema | Every embedded document parses and validates; JSON Schema is published | a unit test over `manifests/`; `quickai config check --strict` in CI |
| Expressions | Parse, type-check, evaluate; always terminates | table-driven and property tests |
| Planner (golden) | `(registry, selection, host facts)` to a `RunPlan` with provenance | golden files, and a **parity test**: the llama.cpp plan's argv equals today's argv across a matrix of selections |
| Form model | Schema plus values to visible fields, defaults and errors | headless unit tests |
| Primitives | Each against a real resource, as the project does: `tiny_http` fakes, generated archives of all three kinds, a fake release feed | `cargo test`; real-binary tests `#[ignore]`d |
| Settings compatibility | Old `settings.json` files load; saved files keep the legacy keys | extend `loads_settings_json_missing_newer_fields` |
| GUI suite | Rendering parity and behaviour (215 cases) | `build/test.sh`, unchanged |
| Fake engines | A fake engine is a document plus the existing `fake_llama_server.py` or a new `fake_ollama.py`, dropped into the throw-away HOME's user layer (or the override layer). Suites for adapters and dynamic UI need no GPU and no model | new suites |
| Privacy | No request outside the allow-list; hosts used are a subset of the documented ones | network_privacy suite plus the generated table |

A bonus of the design: tests that today stage a fake `llama-server` next to the executable can instead drop a
document into the throw-away HOME, so a test can describe any engine, recipe or UI it needs.

## 14. Layout and impact on the other repositories

**Crates and modules**

```
crates/
  quickai-core/            PURE: no processes, no network, no egui
    src/ schema/ expr/ config/ registry/ plan/ form/
  quickai/
    manifests/             embedded defaults: engines/ vocab/ catalog/ ui/ policy/   (include_dir)
    src/ prims/ runtime/ ui/ app/ main.rs    effects, supervisor, egui, shell
```

Dependency rule: `quickai-core` imports nothing effectful, `prims` never imports egui, `ui` never spawns or does
HTTP (it goes through `runtime` and jobs), and documents name only primitive ids, never Rust types. A guard test
greps for forbidden `use` lines. New dependencies: `toml`, `schemars` (build and docs), `regex`, `semver`, `sha2`,
`ruzstd` (Q14), `libc` (Unix only), `windows-sys` (Windows only, job objects).

**`quickai_base`.** Bundle contents and size do not change; nothing Ollama-related is bundled. `next_to_exe`
keeps today's contract (Q9). In Phase 5 `stage_bundle` also writes `bundle.toml` from `config.env` (what ships,
with hashes), the `packaged_bundle` suite checks it, and the channel notes of the first proposal still apply
(Store policy for downloaded executables: Q15; snap plugs `network` and `network-bind`; optional-dependency hints
in package manifests, since detecting an installed Ollama avoids a 1.4 GB download).

**`quickai_base/test`.** New suites for adapters and dynamic UI, config diagnostics, engine selection and the
Ollama lifecycle; `fake_ollama.py` modelled on `fake_llama_server.py` (it speaks `GET /`, `/api/version`,
`/api/tags`, `/api/ps`, `POST /api/pull` as NDJSON, `DELETE /api/delete`, `/api/generate` for load and unload,
`/api/chat` as NDJSON and `/v1/chat/completions` as SSE; its command line is `serve` and `--version`; it records
the `OLLAMA_*` environment it received; it handles SIGTERM, with an ignore mode to test escalation; it spawns a
child so a test can assert that neither survives Stop). Existing suites that change: settings_window (Engine
row), persistence, network_privacy. One suite at a time, as before.

## 15. Migration plan

Sizes are relative. A *golden parity* gate means the new path must reproduce the old path's observable output
before the old path is deleted.

| Phase | Content | Size | Gate |
|---|---|---|---|
| 0 | Contracts and a parity harness: capture today's argv, environment and `settings.json` round-trips as golden data; `events.rs`; `Job`; split `app.rs` by panel (pure moves) | M | fmt, `clippy -D warnings`, tests, 213 cases |
| 1 | Kernel: `quickai-core` (schema, expressions, config, registry, planner); the embedded `llamacpp.toml`; `Session` executes a run plan; `process.rs` | L | golden parity (argv, environment, ports); GUI suite unchanged; diagnostics tested |
| 2 | Forms: the form model and renderer; "Advanced: llama.cpp" and the primary controls row from data; `legacy_key` compatibility; the runtime-manager widget | L | OCR suite unchanged; `settings.json` round-trips |
| 3 | **Ollama as data:** new primitives (tar.zst, sha256, atomic install; releases with digests; `http_call` with NDJSON progress; attach; group or job stop; `catalog.engine_list`; NDJSON decode); the engine selector; `fake_ollama` | L | lifecycle suite; spike A1-A9 settled; no orphan process after Stop or exit |
| 4 | Catalog, recipes, adapters: the recipe schema; components; an acquire phase for several artifacts; the Adapters panel; discovery; profiles; `/props` and `/api/show` discovery | L | new suites with fake engines and recipes; a manual smoke test with a real vision model |
| 5 | Layout, policy and strings as data; hot reload; `bundle.toml`; `config check` and `plan --explain`; generated reference and privacy table; ranged Windows download | M | `PRIVACY.md` checked against the generated table |
| 6 | Optional: several units and gateway routing (sidecars); remote content packs with signing; router-mode variant; external drivers | XL | per feature |

The spike (A1-A17 in [engine-facts.md](engine-facts.md)) can run any time; its Ollama half ran on 2026-10-06 and
settled everything Phase 3 needs except the Windows console question (A8). Phases 0-2 change nothing a user can
see, by design; Ollama arrives in Phase 3, adapters in Phase 4.

## 16. Alternatives considered

| Alternative | Why not |
|---|---|
| The first proposal: a Rust module per engine behind traits | Extending needs a recompile, and the UI stays hand-wired per engine; it cannot express "adapters and their arguments in configuration". Its good parts survive as primitives |
| An embedded scripting language (Lua, Rhai, JavaScript) | Flexible but unbounded: untestable plans, a bigger trust problem, and a standing temptation to put logic in documents. The closed expression language covers every case found so far |
| WASM plugins | A strong sandbox, but a heavy toolchain for something data and primitives already cover. Revisit if a real case appears |
| External driver processes (JSON over stdio) | A good escape hatch for an exotic engine; later, privileged tier, user-installed only |
| An arbitrary workflow graph (DAG, loops, conditionals) | A fixed phase list with a closed step vocabulary does the job and stays explainable |
| YAML or JSON for documents | YAML has well-known footguns; JSON has no comments. TOML (JSON accepted) is the Rust default |
| CEL for expressions | Capable, but heavier and harder to give precise load-time diagnostics. Switch if the language outgrows its table |
| One big settings file | Mixes shipped defaults, user choices and state; no layering, no trust tiers |

## 17. Risks

| Risk | Mitigation |
|---|---|
| **DSL creep:** documents grow logic | Closed vocabulary and language; no loops or scripts; each new primitive needs docs, golden tests and a version; review rule "no logic in documents" |
| **UI parity:** data-driven rendering shifts pixels | The ladder (§9.4); labels verbatim; the GUI suite gates every phase |
| **Debuggability:** why did it run with that flag? | `plan --explain` with provenance; a "View plan" panel; diagnostics with file and line; safe mode |
| **Trust and supply chain** | Tiers, one allow-list, hashes, no shell, confirmation for foreign programs (§12) |
| **Drift between documents and primitives** | `requires` with versions; `config check --strict` in CI; golden plans |
| **Engines change flags** (verified: b11007 lists five removed ones, L13) | `requires_flag`, flag discovery that skips tombstones, one place to fix (§7.3) |
| **Over-engineering for two engines** | Phased with a parity gate; Ollama is built on the kernel as the proof; value shows from Phase 4. If no adapters or further engines are wanted, stop after the first proposal's scope |
| **Documentation burden** | Reference documentation and JSON Schema generated from the Rust types |
| **Startup cost** | A handful of small TOML files; parsing is negligible |
| **Unverified Ollama facts** | The spike (engine-facts.md, A1-A17) before Phase 3 relies on them; done for Phase 3 on 2026-10-06 (it corrected two documented claims: LoRA is refused, and the default context is tiered by VRAM), A8 needs a Windows run |

## Appendix: today's constants and where they go

| Today | Value | Becomes |
|---|---|---|
| `server_manager.rs` `HEALTH_POLL_INTERVAL`, `HEALTH_TIMEOUT` | 500 ms, 120 s | `[launch.ready]` `interval`, `timeout`; defaults in `policy.timeouts` |
| `server_manager.rs` `WAITER_POLL_INTERVAL` | 200 ms | stays code (internal) |
| `app.rs` `MAX_LOG_LINES`, `MAX_TRAFFIC_TURNS` | 2000, 200 | `policy.limits` |
| `proxy.rs` `MAX_REQUEST_BODY`, `MAX_SNIFF_BUFFER` | 8 MiB, 4 MiB | `policy.limits` |
| `remote.rs`, `llama_releases.rs` `CALL_TIMEOUT` | 15 s | `policy.timeouts.http` |
| `remote.rs` `REPOS_TO_INSPECT`, `KEYWORD_…`, `SIMILAR_…` | 30, 20, 20 | `[[models.lookup]]` `popular`, `keyword`, `similar` (done, WP4.9) |
| `remote.rs` `PREFERRED_QUANTS`, `STOPWORDS`, skipped `-of-` and `mtp` files, the search URL | lists | `[[models.lookup]]` `prefer`, `skip` (patterns), `site` (done, WP4.9; the stop words are code, in `lookup::huggingface`) |
| `llama_releases.rs` releases URL, `per_page`, `MAX_RELEASES_TO_SHOW`, OS and arch keywords, `cudart` exclusion | 10, 5, lists | `[acquire]` `feed` and `assets` |
| `llama_install.rs` `MAX_SEARCH_DEPTH`; `CHUNK` | 4; 256 KiB | `binary.search_depth`; chunk stays code |
| `hardware.rs` `OVERHEAD_FACTOR`, `RAM_FRACTION`, speed caps | 1.2, 0.5, 6 GB, 12 GB | `policy.heuristics.recommend` |
| `hardware.rs` `speed_tier` ratios | 8.0, 3.0 | `policy.heuristics.speed_tier` |
| `hardware.rs` `recommend_ctx_size` ceilings, ratios, min, max, default | tables | `policy.heuristics.recommend_ctx` |
| `gpu.rs` vendor keywords | lists | `policy.gpu.vendors`; the OS commands (`lspci`, PowerShell, `system_profiler`) stay in `gpu.detect` |
| `settings.rs` defaults | port 8080, context 4096, GPU layers 999, dark mode | field `default`s; `ui.theme` |
| `main.rs` window size, minimum, title, `APP_ID` | 760×560, 520×380 | `ui.window` |
| `app.rs` `DELETE_LIST_MAX_ROWS`, scroll heights, look-up size buckets | 4; 280, 200; 15 and 4 GB | panel parameters; the buckets are the vocabulary's `[[size_class]]` (done, WP4.9) |
| `app.rs` state colours | five colours | `ui.theme.state_colors` |
| `models.rs` `KNOWN_MODELS` | 5 entries | catalog recipes |
| `paths.rs` `~/.quickai/models`, `~/.quickai/llama-bin`, the executable-relative `llama-server` and `models/` | | `policy.paths`, `[locate]`, `[models].sources` |
| `app.rs` hover texts, About text, crash-panel advice, curl example | text | field `help`, `[api_info]`, `[[diagnostic]]`, `ui.strings`, vocabulary examples |
| `proxy.rs` `classify()` and `extract_delta()` shapes | code | vocabulary `[[api]].decode` |
| the `127.0.0.1` bind | | `policy.network.bind` |
