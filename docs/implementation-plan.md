# QuickAI implementation plan

_Status: Phases 0, 1 and 2 done and verified; Phase 3 not started · Last updated: 2026-10-06 · Implements
[architecture.md](architecture.md) with Q1-Q15 accepted as recommended_

This plan orders and verifies the architecture; it does not restate it. Section numbers (§) refer to
[architecture.md](architecture.md), fact and assumption numbers (L, O, A) to [engine-facts.md](engine-facts.md).
The plan is also the progress record: tick work packages as they land and append to the logs in §12.

## 1. Status

| Phase | Name | Size | Depends on | User-visible change | Status |
|---|---|---|---|---|---|
| 0 | Contracts and a parity harness | M | none | none | **done**; gate passed (§12.1) |
| 1 | Kernel: `quickai-core`, planner, `Session` | L | 0 | none | **done**; gate passed (§12.1) |
| 2 | Forms: form model, renderer, `legacy_key` persistence, runtime manager | L | 1 | none | **done**; gate passed (§12.1) |
| 3 | Ollama as data | L | 2, spike A1-A9 (done, A8 needs Windows) | engine selector, Ollama | not started |
| 4 | Catalog, recipes, adapters | L | 3 | Adapters panel, vision and LoRA | not started |
| 5 | Layout, policy, strings as data; tooling | M | 4 | none (layout identical) | not started |
| 6 | Optional: units, content packs, router mode | XL | 5 | per feature | not started |
| S | Spike track (Ollama and llama.cpp facts, A1-A17) | S each | none | none | Ollama half **done** (A1-A7, A9, A12, A14, A17); open: A8, A10, A11, A13, A15, A16 |

Phases 0-2 change nothing a user can see, by design (Q7). Ollama arrives in Phase 3, adapters in Phase 4.

## 2. Ground rules

1. **The architecture is the contract.** If implementation shows it is wrong, change `architecture.md` in the same
   change and add a line to the decision log (§12.2). Do not let code and document drift apart.
2. **Every phase ends shippable, every work package lands green.** Green means `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass on Linux, and every
   `cfg` branch still compiles for Windows and macOS (CI builds all three). A phase's *gate* must pass before the
   next phase starts.
3. **Parity before features.** Old code is deleted only after the new path reproduces its golden output. Until a
   gate says otherwise, llama.cpp mode is pixel-identical (Q7) and `settings.json` keeps its nine keys (§6.5).
4. **Test first for kernel code.** Write the table or golden test, watch it fail, then write the code. Primitives
   are tested against a real resource (a `tiny_http` fake, a generated archive, a fake binary), as the project
   already does. Tests that need the gitignored `llama.cpp/` and `models/` stay `#[ignore]`d.
5. **Purity is enforced, not hoped for.** `quickai-core` has no process, network, filesystem (except through a
   `LayerSource` trait) or egui. A guard test greps its sources for forbidden `use` lines (§14).
6. **Repositories.** The project repo (`quickai/`) holds the app, its documents, embedded manifests and in-crate
   tests. Packaging (`bundle.toml` writer, channel manifests) stays in `quickai_base`. GUI suites and fakes
   (`fake_ollama.py`) live in the test repo (`quickai_base/test`). Cross-repo steps are listed per phase (§10).
7. **Nobody commits for the user.** Work stays in the working tree. Each work package names a suggested commit
   boundary; the user commits (existing practice).
8. **No surprise network or execution.** Nothing downloads or starts a third-party program unless the user
   clicked for it. The spike (track S) runs by hand in an isolated `HOME`.
9. **One GUI suite run at a time**, and `df -h /` first (the suite needs room for screenshots and a staged
   package). A full run takes about 35 minutes.

## 3. Decisions in force

All fifteen questions of §2.1 are accepted as recommended. They are restated so implementers need not look back.

| # | Decision |
|---|---|
| Q1 | "Adapter" means companion files loaded by the same runner (projector, LoRA, draft model) now; sidecar runners are designed in (run plans have units) and built in Phase 6 |
| Q2 | One model at a time in v1; the plan type already allows several units |
| Q3 | Image input through a projector on llama.cpp first, LoRA second; video adds the ffmpeg fields; speech and image output need sidecars |
| Q4 | Trust tiers: `engine`, `policy`, `ui`, `bundle` only from embedded, bundle and user layers; `vocab`, `catalog`, `profile` may later come from remote packs; a user-layer engine naming a program the app did not install asks once |
| Q5 | TOML (JSON accepted) plus the closed expression language of §7; JSON Schema generated from the Rust types; no scripting |
| Q6 | Data ladder: engines and launch, forms, catalog and adapters, then layout, policy and strings; the shell stays code until Phase 5 |
| Q7 | llama.cpp mode renders exactly today's window until Phase 5 |
| Q8 | Declarative core first; Ollama is its first new engine (Phase 3); the Ollama spike runs now, in parallel (track S) |
| Q9 | Keep the executable-relative convention as the built-in `next_to_exe` strategy; `bundle.toml` comes in Phase 5 |
| Q10 | Profiles: data model now (Phase 4), save/load UI later, in Settings first |
| Q11 | Attach to an Ollama that already answers on the port (`attach_if_running`; field `mode`: auto, attach, spawn) |
| Q12 | A QuickAI-started Ollama uses its own model store (`models_dir`), with a `use_default_store` switch |
| Q13 | A `tested` tag with pinned digests in the engine document; newest only through an explicit look-up |
| Q14 | `ruzstd` for `.tar.zst`, behind a throughput check (A9) |
| Q15 | Store channels are undecided: build the `locked-config` feature in Phase 5 and check each store's policy before any submission |

Defaults of §2.2 apply as written (layers and directories, manual reload, headless `config check` and
`plan --explain` for development and CI, keyed strings, sha256 wherever a hash is known, `legacy_key`
compatibility, a pure `quickai-core`, no scripting).

### 3.1 Implementation choices the architecture leaves open

These are settled here so the work packages are unambiguous. Each can be revisited with a line in §12.2.

| # | Choice | Reason |
|---|---|---|
| C1 | **Merge on generic tables, then deserialize.** Each layer's file is parsed into a generic `toml::Table`; layers are merged (§5.2); the merged table is deserialized into typed documents. A provenance map (value path to file) is built during the merge | Merge rules are about structure, not types; provenance feeds diagnostics and `plan --explain` |
| C2 | **Diagnostics resolve lines lazily.** The typed parse needs no spans. When a diagnostic is produced, the loader finds the line of a value path by walking `toml_edit`'s span-retaining document of the originating file | Keeps the hot path simple; spans are only needed on the error path |
| C3 | **Unknown keys are found with `serde_ignored`** during the typed deserialize, giving warnings (errors under `--strict`) without `deny_unknown_fields` | The architecture wants warnings, not failures |
| C4 | **Hand-written lexer and Pratt parser** for expressions; a `ContextSchema` types every context path; evaluation is a total function over `Value` | Precise load-time diagnostics; no dependency; no way to loop |
| C5 | **Known primitives are data passed in.** `quickai-core` takes a `KnownPrimitives` set from the app when validating `requires.primitives` | The core stays pure, the app declares what exists |
| C6 | **Host facts are an input.** `HostFacts` (os, arch, RAM, GPU, runtime binary, dir, build and flags, path roots, dev root) is built by `quickai` and handed to the planner | The planner never calls the OS |
| C7 | **Embedded manifests with `include_dir`** under `crates/quickai/manifests/`; a unit test parses and cross-checks every file | A new embedded document needs no Rust edit |
| C8 | **Goldens are checked-in files** under `crates/<crate>/tests/golden/`, compared by a small helper that rewrites them when `QUICKAI_UPDATE_GOLDEN=1` | A reviewable diff when behaviour changes on purpose |
| C9 | **`quickai` stays a binary crate** that also hosts the headless `config` and `plan` subcommands; arguments are inspected before eframe starts, so the GUI launch path is unchanged. Windows release builds have no console, so the tools are for debug builds and CI | Avoids a second binary in the packaging contract |
| C10 | **Generic `Job<T>`** replaces `DownloadHandle` and `InstallHandle`; `poll()` drains the channel and yields `Option<Result<T, String>>` | Removes two copies of the same drain loop; `Job<()>` serves model pulls later |
| C11 | **One resolver for the planner and the form.** `plan::resolve::resolve_fields` gives every field's value, where it came from and what is wrong with it; the planner turns the problems into errors, the form into a message under the field. A `null` in a selection means "not set", so the default applies | The interface can never show a number the command line then does not use (test F2 compares them over a matrix) |
| C12 | **The form is rebuilt each time it is needed, and an edit is a value.** `FormModel::build` is cheap (a handful of fields), so the panels build it from the value map when they draw and apply what a widget hands back; `form::set` is the strict, validating writer for callers that are not a person typing | No form state to keep in step with the values; typing a half-finished text is never refused |
| C13 | **`Settings` keeps its struct for the nine keys and gains one flattened map.** A field reaches the file through its `legacy_key` by way of the struct's JSON form: "absent" means the struct's default for that key, and a value that does not fit its key is refused with the struct unchanged | The typed struct is still the oracle for what a file may hold (a wrongly typed value discards the whole file, as it always has) and the key order of a saved file is unchanged |
| C14 | **The controls row stays one group, disabled as a whole.** Every field of the `primary` section says `enabled_when = "not session.running"` and the panel is disabled when all of them are | That is how today's window draws it; the snapshots would differ if labels stayed enabled. Per-field styling waits for the layout redesign (Phase 5) |
| C15 | **A condition that cannot be evaluated does not hold**, and the form reports a warning naming the document, line and field | A broken `visible_when` hides a field instead of showing something wrong, and the author hears about it |
| C16 | **The runtime manager's sentences are fixed in the widget, with the engine's names put in** (its name, its server's name, the repository, where builds are kept, the backends); the document supplies only `filter_hint` | Parity: the sentences are the ones the window had, word for word (a test compares them), and a second engine gets the same sentences about itself without a table of strings to write |
| C17 | **The colon after a label belongs to the panel**, not to the document | A label is a name; the colon is how a row of controls is laid out |

## 4. Gate commands

Run from `quickai/` unless noted.

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                       # real-model tests stay ignored
cargo test -p quickai -- --ignored           # only with llama.cpp/ and models/ present (manual)
QUICKAI_UPDATE_GOLDEN=1 cargo test --workspace   # rewrite goldens on purpose, then read the diff

# from quickai_base/ (GUI acceptance suite; df -h / first; one run at a time)
build/test.sh                                # all 215 cases
build/test.sh suites/settings_window         # one suite
```

A quick pixel check for UI-touching work packages is described in WP0.7; it complements, and never replaces, the
suite.

Baseline before any change (2026-10-06): fmt and clippy clean, `cargo test --workspace` 53 passed and 2 ignored,
GUI suite: see §12.1.

## 5. Phase 0: contracts and a parity harness

**Goal.** Capture today's observable behaviour as data, and make the code ready to be replaced piece by piece,
without changing what the app does or looks like.

**Entry.** None. **Size.** M.

| WP | Work | Deliverable | Verified by |
|---|---|---|---|
| 0.1 | **Baseline.** Record fmt, clippy, unit and GUI results before touching anything | Numbers in §4 and §12.1 | the runs themselves |
| 0.2 | **Golden helper.** A test-only `golden` module: `assert_golden("name", &actual)` reads `tests/golden/name`, compares, and rewrites it when `QUICKAI_UPDATE_GOLDEN=1` (C8) | `src/golden.rs` (`#[cfg(test)]`) | its own tests: match, mismatch message, update |
| 0.3 | **Launch parity.** Lift the argument, environment and port logic of `ServerManager::start` into pure functions with no behaviour change: `launch_spec(...)` returning program, arguments and environment, and `plan_ports(...)` for the proxy rule. Record a matrix (ports at the limits, context size and GPU layers at the limits, empty, quoted and single-quoted extra arguments, paths with spaces, `LD_LIBRARY_PATH` unset and set, proxy off and on) as golden JSON together with the first log line (`$ <binary> <args>`) | `server_manager.rs` (pure fns), `tests/golden/launch_matrix.json` | golden test; plus **WP0.4** proves the lift is faithful |
| 0.4 | **Real-spawn proof.** A Unix test runs the real `ServerManager::start` against a generated shell script that writes its argv and selected environment to a file, for every row of the matrix, and compares with the golden. This is what makes the golden "today's argv" and not "what the lifted function says". It is also the oracle `Session` must match in Phase 1 | `server_manager.rs` test (`#[cfg(unix)]`) | `cargo test` |
| 0.5 | **Settings parity.** Split `Settings` into pure `to_json` and `from_json` plus `load_from(path)` and `save_to(path)`. Golden files: the default document, a fully populated document, key order, and an old-format file loading with the defaults of §6.5. Keep today's behaviour exactly, including that unknown keys are dropped on save (Phase 2 changes that, on purpose, with its own gate) | `settings.rs`, `tests/golden/settings_*.json` | golden and round-trip tests |
| 0.6 | **`events.rs`.** Move `ServerState`, `Event` and `TrafficEvent` out of `server_manager.rs` and `proxy.rs`; the proxy stops importing `server_manager` (§11.3) | `src/events.rs` | compiles, tests unchanged |
| 0.7 | **UI snapshot harness.** A script (kept in the scratchpad, not shipped) that launches a given executable on an isolated Xvfb display with a throw-away `HOME` and the fake llama-server, drives it to a fixed set of states (main, running, crashed, Settings open, About open, Live tab, API info) and writes a PNG per state. Compare before and after byte for byte; any difference is investigated before the suite is run | a runnable script and a note in §12.1 | run on the baseline binary twice (must be identical, or the harness is not deterministic and is tightened) |
| 0.8 | **`Job<T>`.** One generic job type replacing `DownloadHandle`, `InstallHandle`, `DownloadEvent` and `InstallEvent`, with `Progress`, `Done(T)` and `Failed` events (C10); the `Status` event of §11.4 arrives with its first producer, the Ollama pull, in Phase 3. `poll_download` and `poll_llama_install` collapse onto `Job::poll`, and both progress lines onto `Job::progress_line` | `src/jobs.rs`; `downloader.rs`, `llama_install.rs`, `app.rs` | existing downloader and installer tests, adapted; `jobs.rs` tests; pixel check |
| 0.9 | **Split `app.rs` by panel, pure moves.** (Verified as a pure move by comparing the token multiset of the old file with the new ones: nothing removed, only imports, `impl` wrappers and signatures added.) `app/mod.rs` keeps the `App` struct, `new`, small helpers and the `eframe::App` impl, which now only composes panels. One file each for `header`, `model_bar`, `acquire`, `controls`, `run_bar`, `api_info`, `diagnostics`, `log_pane`, `live_traffic`, `settings_window`, `about_window`, `confirm_delete`. Code inside is not edited beyond what the borrow checker forces (a closure becomes a method that takes the same locals as arguments) | `src/app/*.rs` | pixel check after every few moves; GUI suite at the end |
| 0.10 | **Doc touch.** `architecture.md` status line; this plan's logs | docs | read-through |

**Order.** 0.1 then 0.2, 0.3, 0.4, 0.5 (data first, before anything is restructured), then 0.6, 0.7, 0.8, 0.9.
Suggested commits: (a) golden helper, launch and settings goldens; (b) `events.rs`; (c) `Job<T>`; (d) the `app/`
split.

**Gate.**
- [x] fmt, clippy `-D warnings`, `cargo test --workspace` green
- [x] `launch_matrix` and settings goldens exist, and the real-spawn proof matches them
- [x] the UI snapshot comparison shows no difference between the baseline and the new binary (15 states, byte for byte)
- [x] the GUI suite passes with the same result as the baseline (200 cases by default: 192 passed, 0 failed, 8 skipped, the same 8 cases); run once on the final Phase 1 tree, which contains Phase 0, see §12.1
- [x] no behaviour change in `git diff` beyond moves, the `Job` type and the pure extractions (reviewed)

**As built.**
- `golden.rs` also has `read_golden`, which the real-spawn proof uses to compare against the same file.
- The launch matrix has 13 rows (`tests/golden/launch_matrix.json`); the real-spawn proof runs every one. It is
  not vacuous: breaking only the spawn path (not the pure function) made it fail while the golden test stayed
  green. Its first run also caught a bug in the proof itself (rows shared a directory, so a stale record from the
  previous row was read), which is why each row now clears the records first.
- The snapshot harness lives outside the repo (it reuses the GUI suite's `AppLibrary` with its own stage directory,
  display and fixed paths) and covers 15 states: idle, light, no models, running, API info, crashed, Settings
  (closed, Advanced, with lists), delete confirmation, About, Live (empty and with traffic), a stalled download
  and a stalled install. Two things were not deterministic and were pinned: the stall server's port (it shows in a
  field) and, in the crashed state, the order of the engine's stdout and stderr lines in the log (two reader
  threads race; that is the application's behaviour today, so that region is masked).
- The `app.rs` split was verified as a pure move twice: the token multiset of the old file against the new files
  (nothing removed, only imports, `impl` wrappers and signatures added), and the 15 snapshots. `rustfmt` re-flowed
  code that it had been unable to format at the old indentation; that is formatting only.
- `Job<T>` also removed two copies of the progress-line formatting.
- Found on the way, not changed: `llama_install` and `downloader` tests download real archives and the former
  writes into the real `~/.quickai/llama-bin` while it runs; worth moving to a temporary home when those modules
  become primitives.

**Risks.** The split shifts pixels through a changed layout order or an extra `ui.group` (mitigation: pure moves,
pixel check). The lifted pure functions subtly differ from the inline code (mitigation: WP0.4 runs the real spawn).

## 6. Phase 1: the kernel

**Goal.** A pure core that turns documents plus a selection into a run plan, and a `Session` that executes a
plan. llama.cpp runs through it, defined by an embedded document, with identical observable behaviour.

**Entry.** Phase 0 gate. **Size.** L.

Layout (§14):

```
crates/quickai-core/             pure: serde, serde_json, toml, toml_edit, serde_ignored, regex, semver
  src/ diag.rs  value.rs  expr/  doc/  config/  registry.rs  plan/  host.rs  lib.rs
  tests/ golden/  ...
crates/quickai/
  manifests/engines/llamacpp.toml            embedded (C7)
  src/ prims/{locate,spawn,probe}.rs  runtime/{session,process}.rs  cli.rs  ...
```

| WP | Work | Deliverable | Verified by |
|---|---|---|---|
| 1.1 | **Workspace and purity guard.** Add the crate, wire it into the workspace, add the guard test that rejects `std::process`, `std::net`, `std::fs`, `ureq`, `egui` and `eframe` in `quickai-core` (the loader reads through a `LayerSource` trait) | `crates/quickai-core`, `tests/purity.rs` | the guard fails on a planted violation, then passes |
| 1.2 | **Diagnostics and values.** `Diagnostic { severity, file, line, path, message, hint }`, a `Value` enum (null, bool, int, float, string, list), a path type | `diag.rs`, `value.rs` | unit tests; display format golden |
| 1.3 | **Expressions** (C4, §7.1): lexer, Pratt parser, AST, `ContextSchema` type check, evaluator, interpolation (`${path}`; a string that is exactly one `${...}` keeps its type), the function table of §7.1 (`has`, `len`, `lower`, `matches`, `starts_with`, `is_url`, `semver_ge`, `min`, `max`, `clamp`, `recommend_ctx_size`, `speed_tier`; `is_file` is parsed but rejected outside `input_rules`) | `expr/` | table-driven tests per operator and function; type-error cases with exact messages; a seeded fuzz test that feeds random text and asserts no panic and a bounded run time |
| 1.4 | **Document schema** (typed, serde): the envelope (`schema`, `kind`, `id`, `extends`, `requires`), the seven kinds with the engine document fully typed for what Phase 1 uses (`[engine]`, `[provides]`, `[runtime]`, `[locate]`, `[launch]` with `spawn`, `ready`, `stop`, `[[field]]` with `type`, `default`, `range`, `legacy_key`, `bind.*`, `ui.order`, `[[diagnostic]]`) and every section of the examples accepted (later phases fill in behaviour). The other kinds parse as envelope plus an opaque body and a note that this build does not interpret them yet | `doc/` | the examples in `docs/examples/` all parse; unknown keys warn (C3) |
| 1.5 | **Loader** (C1, C2): layers (embedded, bundle, user, override), file discovery, merge rules (tables deep-merge, arrays replace, arrays of tables with an `id` merge by `id`, `replace = true`), `extends`, tier check (a privileged kind from a data-tier source is rejected), `QUICKAI_SAFE=1`, provenance map, lazy line lookup | `config/` | merge table tests; a broken user document is skipped and the embedded one stays; safe mode ignores the user layer; diagnostics carry file and line |
| 1.6 | **Registry and validation.** Immutable merged set; cross-reference checks (the port of the throwaway checker used on the examples); `requires.primitives` against `KnownPrimitives` (C5); every expression parsed and type-checked at load | `registry.rs` | the examples validate; a typo in a `visible_when` is a diagnostic naming file and line |
| 1.7 | **The planner** (§6.6): `plan(registry, selection, host facts) -> RunPlan`. Types: `RunPlan { units, gateway }`, `Unit { id, engine, endpoint, apis, start, ready, stop }`, spawn args with an origin per element, environment operations (`set`, `prepend`), the argument order rule of §6.6 (base arguments, field bindings by `bind.order` defaulting to `ui.order`, `extra_args` last). Sinks needed now: `arg`, `arg_split`, `env`, `port`, `derive` (compiled; used from Phase 2). The phases other than start, ready and stop arrive with their first user. `render_explain(plan)` | `plan/` | golden plan files; **the parity test** against WP0.3's launch matrix; explain output golden |
| 1.8 | **Embedded `llamacpp.toml`.** Today's behaviour as data: locate, launch (`--model --host --port --ctx-size --n-gpu-layers`, then `extra_args`), `LD_LIBRARY_PATH` prepend, ready probe (`/health`, 120 s, 500 ms), the fields with their `legacy_key`s. It contains only the sections this version interprets, so it never promises behaviour that is not wired; labels and hover texts join it in Phase 2, when the code's copies are removed | `manifests/engines/llamacpp.toml` | loads clean under `--strict`; the parity test runs on it |
| 1.9 | **Primitives, first set.** `locate.setting`, `.next_to_exe`, `.dev_tree` (replacing `paths.rs`), `spawn@1` and `probe.http@1` (lifted from `server_manager.rs` into `runtime/process.rs` and `prims/`), with a `KnownPrimitives` registry that lists them with versions. Behaviour stays identical: `kill`, no process group yet | `prims/`, `runtime/process.rs` | primitive tests against a fake binary and a `tiny_http` fake |
| 1.10 | **`Session`.** `ServerManager` becomes `Session`: it executes a `RunPlan` (start, ready, stop), keeps the five states, the event channel and `poll()`. The proxy is started from the plan's `gateway` as today. The old constructor arguments disappear; `App` builds a `Selection` from its fields and calls `plan` | `runtime/session.rs`; `server_manager.rs` removed once green | the WP0.4 real-spawn proof, now against `Session`; the two ignored real-model tests, run by hand |
| 1.11 | **Headless tools.** `quickai config check [--strict]` and `quickai plan --explain` (C9), and a CI step that runs `config check --strict` on the embedded set | `cli.rs`, CI file | CLI tests on the embedded documents and on a broken fixture directory |
| 1.12 | **Doc touch.** Mark A-numbers settled so far, record the grammar as frozen in `architecture.md` §5 to §7, regenerate the example check against the real loader | docs | read-through |

**Order.** 1.1, 1.2, 1.3 (independent of each other after 1.1), 1.4, 1.5, 1.6, 1.7, 1.8, then 1.9, 1.10, 1.11.
Everything up to 1.8 adds code without changing the app, so the app keeps working throughout. Suggested commits:
core crate with expressions; documents and loader; registry and planner with the embedded document and parity
test; primitives and `Session` switch (this one is the risky commit, so it goes alone); headless tools.

**Gate.**
- [x] fmt, clippy, tests green on Linux; `cfg` branches compile for Windows and macOS (CI): Linux is verified here; the Unix-only tests are `#[cfg(unix)]` and the Windows and macOS builds are CI's to confirm
- [x] the planner reproduces the golden argument, environment and port matrix exactly
- [x] the real-spawn proof passes against `Session`
- [x] diagnostics tested: a broken user document, a typo in an expression, a missing primitive, safe mode
- [x] `quickai config check --strict --embedded` is clean on the embedded set (and runs in CI)
- [x] the UI snapshot comparison is identical (15 states, including running, crashed and live traffic through the proxy)
- [x] the GUI suite has the baseline result, see §12.1
- [x] the old `ServerManager` start path is deleted (`server_manager.rs` became `runtime/session.rs`)

**As built.**
- `quickai-core` has 145 unit tests, 3 purity tests and 2 example tests; `quickai` has 104 (plus the 2 real-model
  tests, which pass when run by hand against the real `llama-server` and Bonsai-8B).
- **Expression language.** A tree-height limit (200) exists because the pathological-input test found a real stack
  overflow: `a and a and ...` repeated 5000 times is parsed in a loop but builds a left-deep tree that checking,
  evaluating and dropping all recurse over. Conditions accept lists and maps by presence (`when = "model"`), `null`
  reads as `false`, `0`, `""` or `[]` where a function needs one (no model selected is common), and a string literal
  keeps backslashes that are not escapes so `"^hf\.co/"` reads as written.
- **Loader.** Each contributing file keeps its parsed body, so the registry can drop the highest layer of a document
  that fails validation and try again with the rest: a mistake in a user's patch leaves the embedded document in
  force, with the error naming the skipped file (R6). `Layer::Remote` exists already so the tier rule (no privileged
  kinds from remote content) is tested before anything uses it. File sources report unreadable files as diagnostics.
- **Registry.** Every expression and template is type-checked against the context where it is written
  (`registry/context.rs` is the one definition of that context). The check also catches a regular expression that does
  not compile, a literal default that does not fit its field, two public ports and duplicate field ids or
  `legacy_key`s. A document needing a primitive this build lacks is skipped with the primitive named.
- **Planner.** Pure; origins are attached to every argument and environment entry. The proxy's internal port is a
  fact (`HostFacts::internal_port`) that the application allocates before planning, which keeps `plan` free of the OS;
  an allocation failure is reported through `Session::report_start_failure` with the old message.
- **Embedded document.** `crates/quickai/manifests/engines/llamacpp.toml` holds only what this version interprets.
  `the_embedded_documents_are_valid_and_complain_about_nothing` makes a broken embedded document a failing test.
- **Primitives.** `locate.*` keeps today's semantics exactly (an explicit setting wins even when its file is gone;
  next-to-the-executable only when the file exists; the dev tree always yields its path so there is something to name
  in "not found at ..."). File names get the platform's executable suffix. `spawn@1` and `probe.http@1` live in
  `runtime/`; `prims::PROVIDED` is the one list of what exists and a test checks the embedded documents against it.
- **Moved to the core:** `recommend_ctx_size` (`hardware::recommend_ctx_size` now calls it) and the shell-like
  argument split (`plan::args::split_args`, replacing `parse_extra_args`).
- **Session.** `Session::start(&RunPlan, name, wake)`; no ready step means running at once. New hermetic tests use a
  `tiny_http` stand-in for the engine and `sleep`, covering ready, stop, crash, a program that cannot start, a plan with
  nothing to start, the gateway binding and freeing the public port, and a taken public port.
- **Headless tools** (`cli.rs`): `config check [--strict] [--embedded]` and `plan [--explain | --json] ...`; a CI step
  runs the former on the embedded set.
- **Known gap.** The confirmation guard of Q4 (a user-layer engine naming a program the application did not install) is not built: a user document can already patch `launch.spawn.program`. The user layer is not advertised yet and sits in the user's own directory; the guard is WP3.10, before it is documented.
- **Planner strictness.** A selection value of the wrong type is an error naming the field (it used to be easy to fall back silently to a default), and an argument that is exactly one `${...}` with no value (no model chosen) is an error rather than an empty argument. Range checks wait for the form model (Phase 2), which is also where the interface clamps values, so a hand-edited out-of-range `settings.json` still behaves as before.
- **Examples.** `crates/quickai-core/tests/examples.rs` keeps `docs/examples/` valid TOML with correct envelopes and
  holds the llama.cpp example to the typed schema, reporting the sections this version ignores.

**Risks.** The `Session` switch changes timing (the health poll becomes a plan step): keep the constants and the
event order, and let the real-spawn proof and suite catch regressions. The expression language grows features
nobody asked for: add a function only when a document in `docs/examples/` needs it.

## 7. Phase 2: forms

**Goal.** The settings the user sees are fields of documents. The primary controls row and "Advanced: llama.cpp"
render from fields, with `settings.json` unchanged for existing users.

**Entry.** Phase 1 gate. **Size.** L. Parity is the whole difficulty (§9.4).

| WP | Work | Verified by |
|---|---|---|
| 2.1 | **Form model** in `quickai-core` (§9.1): `FormModel::build(registry, selection, facts)` returning sections of fields with resolved value, visibility, enabled state, error, help and origin; `set(field, value)`; defaults with `recompute_on` (picking another model re-derives the context size, as today); validation from `range`, `pattern`, `validate` | headless unit tests per attribute |
| 2.2 | **Values store and persistence.** `Selection.values`, `persist` scopes, `legacy_key` mapping to the top level of `settings.json`, everything else under `values`, **unknown keys preserved on save** (a deliberate change from Phase 0, with its own tests) | golden round-trips: today's files load unchanged, saved files keep the nine keys, an unknown key survives |
| 2.3 | **Widget registry** (egui, in `quickai`): integer drag, checkbox, text, path picker, port, key-value lines, runtime manager, progress, button. A widget takes a `FieldView` and returns an edit; hover text comes from `help` | snapshot-style tests of widget output where egui allows, pixel check |
| 2.4 | **`controls` panel** renders port, context size and GPU layers from fields, reproducing today's exact egui layout and hover texts verbatim | pixel check; OCR suite |
| 2.5 | **Settings window from data.** "App" fields from the `ui` document are not data yet (Phase 5), so the window shell stays code and renders the engine's `advanced` section from fields | pixel check; settings_window suite |
| 2.6 | **Runtime-manager widget** (§9.2) generalising "Advanced: llama.cpp": binary override, installed builds, look-up, install progress, configured by `[acquire]`. Underneath, `llama_releases.rs` becomes `feed.github_release@1` and `llama_install.rs` becomes `install.archive@1` (zip and tar.gz; the same code, now parameterised by the engine document) | the existing release-feed and installer tests, adapted; settings_window and online suites |
| 2.7 | **Docs.** A reference page generated from the field and binding types | read-through |

**Entry notes** (what Phase 1 leaves ready, and the approach worth taking):
- The engine document already carries the five engine fields with their `legacy_key`s; `models_dir` and
  `last_model` (the other two engine-owned keys) join it here, and `dark_mode` and `enable_traffic_proxy` stay
  application settings in code until the `ui` document exists (Phase 5).
- **Persistence is the small part.** Giving `Settings` a `#[serde(flatten)]` map for keys it does not know already
  preserves them across a save; fields without a `legacy_key` go under a `values` object. The old struct and its
  goldens (`settings_default.json`, `settings_populated.json`, and the `unknown_keys_are_dropped_on_save_today` test,
  which flips) are the compatibility oracle.
- **Rendering parity is the large part.** Build the data-driven row beside the existing one, switch with a constant,
  and compare the 15 snapshots after each panel (the harness of WP0.7 is the fast check; the suite is the gate).
  A panel moves only when its snapshots are identical.
- **Texts move verbatim, once.** A labels-and-help test (document text equal to the code's text) lands before the
  code's copy is deleted, so the move cannot change a word the OCR suite reads.
- The form model sees the same context the planner does (`registry::context::schema`), so `visible_when` and
  defaults are checked at load already; Phase 2 adds evaluation, not checking.

**Gate.**
- [x] fmt, clippy, tests green
- [x] the pixel comparison is identical and the OCR suite (215 cases) has the baseline result
- [x] `settings.json` round-trips: old files load, new files keep the nine legacy keys, unknown keys survive
- [x] the form model has tests for visibility, enabled state, defaults and recomputation

**Test plan** (written before the code; a row is ticked in the as-built notes when its test exists and passes).
"Unit" is `cargo test`, "golden" is a checked-in file compared byte for byte, "pixel" is the snapshot comparison
against the untouched baseline, "GUI" is a case of the Robot suite.

| ID | WP | Kind | Case |
|---|---|---|---|
| F1 | 2.1 | unit | a value in the selection wins over the default, and the origin says which one was used (selection, literal default, computed default, none) |
| F2 | 2.1 | unit | the computed default over a matrix of model sizes and RAM; the form and the planner give the same value because they share one resolver |
| F3 | 2.1 | unit | `recompute("model")` yields edits only for fields that name that trigger, none for an unknown trigger, and replaces a value the user typed (as today) |
| F4 | 2.1 | unit | `visible_when`: true, false, by the model's modalities, by another field, by `os`; a `hidden` field is never visible |
| F5 | 2.1 | unit | `enabled_when` with `session.running`; no condition means enabled |
| F6 | 2.1 | unit | validation: range, port, `pattern`, `choice` options, `validate` rules with their messages, `required_when`; an error never hides the value |
| F7 | 2.1 | unit | sections and order: primary, advanced, then the others by name; fields by `ui.order`, ties in document order; the label falls back to the id |
| F8 | 2.1 | unit | `derive` facts (`gpu_offload`) are evaluated from the field's value |
| F9 | 2.1 | unit | `set` validates before it accepts, and leaves the selection as it was on an error |
| F10 | 2.1 | unit | load-time checks of the new attributes: a bad `enabled_when`, an invalid `pattern`, a `validate` rule without a message and a `choice` without options each give a diagnostic with file and line |
| F11 | 2.1 | golden | the form of the embedded engine as JSON in four states: no model, a small model, a large model on little RAM, a server running |
| P1 | 2.2 | unit | the Phase 0 goldens (`settings_default.json`, `settings_populated.json`) are unchanged byte for byte |
| P2 | 2.2 | unit | unknown keys survive a load and a save: nested objects, arrays, null, numbers (the Phase 0 characterisation test flips) |
| P3 | 2.2 | unit | a field without `legacy_key` is stored under `values`, one with it never is; a file with no extras has exactly the nine keys |
| P4 | 2.2 | unit | engine values are read from the legacy keys (null becomes absent) and written back through them; absent means the default; a value of the wrong type is refused and nothing changes |
| P5 | 2.2 | unit | a wrongly typed legacy value still discards the whole file, unknown keys included (parity with today) |
| P6 | 2.2 | unit | `values` of an engine that is no longer installed are kept |
| P7 | 2.2 | GUI | TC-PER-009: keys the app does not know are still in `settings.json` after the app saved |
| U1 | 2.3-2.5 | golden | every label, hover text and hint in the engine document equals the text the code had, extracted from the previous source |
| U2 | 2.3 | unit | widget helpers: a range clamps, `step` becomes the drag speed, a port stays within 1 to 65535 |
| U3 | 2.4, 2.5 | pixel | the 15 snapshots identical, plus the tooltips of the five fields |
| U4 | 2.4, 2.5 | GUI | the whole suite gives the baseline's result |
| A1 | 2.6 | unit | `[acquire]` parses and is checked at load: a bad keyword table, an unknown backend rule, an empty install section |
| A2 | 2.6 | unit | asset selection by keywords (the old cases, ported) and by exact names (the Ollama rules of the example) |
| A3 | 2.6 | unit | install labels: `${tag}-${stem}` for `.zip` and `.tar.gz`, and a label from a bare URL |
| A4 | 2.6 | unit | backend labels and vendor-mismatch warnings read from the document equal the old functions' answers over a list of names |
| A5 | 2.6 | unit | the feed against a `tiny_http` fake of the GitHub API: prereleases, `show`, the excluded companion package, failure messages; the real-network test stays |
| A6 | 2.6 | unit | the installer on generated `.zip` and `.tar.gz` archives: flat, wrapped, no binary inside, corrupt, already installed, executable bit, unknown extension; list and delete, in a temporary root |
| D1 | 2.7 | golden | the generated reference equals `docs/reference/engine-fields.md`, and names every attribute of the field type |

**As built.**
- **Every row of the test plan has a test that passes**, U4 and P7 (the GUI suite) included (§12.1).
  F1-F10 are in `form/tests.rs`, `registry/mod.rs` and `plan/mod.rs`; F11 is `tests/golden/form_llamacpp.json`
  (four states of the embedded engine's form); P1-P6 are in `settings.rs`; U1 is `tests/golden/ui_texts.json`,
  extracted from the code before it was deleted (the field texts, then the runtime manager's sentences); U2 drives
  real egui widgets on a headless context (a drag, a click, the range, the step); A1-A4 are in
  `registry/mod.rs`, `acquire.rs` and `gpu.rs`; A5 and A6 run the real primitives against a `tiny_http` fake of
  the GitHub API and a server of generated archives, with a temporary root (the old installer tests wrote to the
  real `~/.quickai/llama-bin` and downloaded llama.cpp's source archives); D1 is `tests/reference.rs`. The new
  tests were mutation-checked: each break listed in the log below failed the test meant to catch it.
- **Form model** (`quickai-core::form`): `FormModel::build`, `recompute`, `set`; sections in the order primary,
  advanced, then the others by name. New field attributes: `enabled_when`, `required_when`, `options`, `pattern`,
  `validate` (`{ check, message }`), `persist`, `applies`, `ui.hint`; each is checked when a document loads
  (F10). `reset_on` and `ui.advanced` of the design are not built: `recompute_on` and the section cover what they
  were for. A field's problems are reported in a fixed order (wrong type, required, range, pattern, options,
  rules) and the value is always kept and shown. `HostFacts` gained `session_running` and `session_crashed`.
- **Persistence**: `Settings.other` is the flattened map (unknown keys and `values`); `engine_values` and
  `set_engine_values` map the engine's fields onto the file through `legacy_key`. The engine document gained
  `models_dir` and `last_model`, so the seven engine-owned keys are all data; `dark_mode` and
  `enable_traffic_proxy` remain the application's own. The Phase 0 test that pinned "unknown keys are dropped"
  became its opposite. The two GUI cases that check it (TC-PER-009 and -010) are in the test repository.
- **Widgets** (`app/widgets.rs`): integer and port drag boxes, float drag box, checkbox, combo, one-line text.
  Key-value lines, a path picker with a browse button, a progress bar and a button were in the work package and
  are not built: nothing in a llama.cpp form uses one, and the first Ollama form (Phase 3) does.
- **Panels**: `controls` and the Settings window's Advanced section are drawn from the engine's fields; the
  window's "App" part (models folder, downloaded models, live traffic) stays code, but the models folder's label
  and hover text come from the `models_dir` field. `App` lost its loose `port`, `ctx_size`, `n_gpu_layers`,
  `extra_args` and `llama_bin_override`: they are `values`, read through small accessors. The speed estimate and
  the model recommendation take the engine's `gpu_offload` fact (`derive`) instead of the GPU-layers number.
- **Runtime manager** (`app/runtime_manager.rs`) with `[acquire]` in the document (`feed`, `assets`, `install`,
  `backends`, `root`, `filter_hint`): `quickai-core::acquire` holds the pure half (which file of a release is for
  this machine, by keywords or by exact name; install labels; backend labels; the root) and `prims/feed.rs` and
  `prims/install.rs` the effects. `llama_releases.rs`, `llama_install.rs` and the llama paths in `paths.rs` are
  gone. Two behaviours differ from the code they replace, deliberately: a platform the document has no words for
  matches nothing (the old code treated any unknown OS as Linux and any unknown architecture as x86-64), and a
  prerelease counts only when the document says so (llama.cpp's says so, as the old code behaved).
- **Reference page** (`docs/reference/engine-fields.md`): generated from the types, printed by `quickai config
  docs`, and checked by tests in both directions (every key of a type is described, every description names a key
  that exists).
- **Pixel check.** The snapshot harness gained 10 tooltip states (the hover text of the port, the context size,
  the GPU layers, the models folder, the binary box, the extra arguments, "Installed builds", the look-up button
  and the Apply and Download buttons), captured on the untouched baseline binary and on the Phase 2 build: all 25
  snapshots are byte-identical. The reference set for the first 15 is the one taken after the two
  non-deterministic things were pinned (Phase 0 notes); the very first baseline run predates that and differs
  from every later one in the crashed and the install-progress states, which is the harness, not the application.
- **Found on the way.** `/tmp` is a tmpfs shared with the other projects' sessions and can sit at its quota
  (EDQUOT even with free space shown): the snapshot run died on it once and the tool output of one command was
  lost. Running with `TMPDIR` on the real disk avoids it. egui's `Context::run_ui` is enough to test widgets
  headlessly, but a click or a drag needs the layout of the frame before it, so the harness keeps one context
  alive across frames. `tiny_http` answers a body above 32 KB with chunked encoding unless told otherwise, so
  the fake server says how long a file is, as a real one does.
- **Two flakes that were already there.** (1) Two unit tests of Phase 1, `the_gateway_binds_the_public_port_and_stop_frees_it`
  and `proxies_a_chat_completion_and_emits_traffic_events`, failed in roughly one full run in twenty on this
  machine with "Address already in use": both find a port by binding 0, release it and bind it again, and a child
  forked by another test in that moment holds a copy of the listener until it execs (an `strace` of a failing run
  shows the fork inside the window). Both now ask for another port, up to five times. 100 plain runs of the
  session and proxy tests, and 25 under `strace` (which widens the window, and made the old code fail at once),
  found no failure. (2) TC-SET-016 of the GUI suite passed or failed with the random name of its scratch
  directory, on the Phase 1 build as much as on this one (3 of 6 runs failed before, none of 16 after): the
  models-folder box shows that path, and in the OCR bands that contain it the dim label of the locked checkbox is
  split into lines differently from one path to the next, although the pixels of the label are identical. The case
  now reads the label below the box. The suite's long scratch path also truncates that box (TC-SET-001, -007), so
  the suite runs with a short `TMPDIR`; both points are in the test repository's strategy document.

**Risks.** egui layout depends on call order and spacing: keep the old code path behind a cargo feature or a git
revert point until the comparison is clean, then delete it.

## 8. Phase 3: Ollama as data

**Goal.** Ollama works end to end, downloaded on demand and never bundled, through new generic primitives and one
engine document. llama.cpp is untouched.

**Entry.** Phase 2 gate **and spike items A1-A9 and A14, A17 settled** (track S). Done on 2026-10-06 for all but
A8 (the Windows console window: it needs a Windows run and does not block, because WP3.5 sets `CREATE_NO_WINDOW` on
every spawn regardless). The spike found that Ollama can be run as the architecture assumes, with four corrections
now written into the documents (LoRA is refused, O7; the default context is tiered by VRAM, O12; a warm-up call's
`num_ctx` does not stick, A7; `PR_SET_PDEATHSIG` must be SIGTERM, A3) and one hardening: `OLLAMA_NO_CLOUD=1` was
already in the design and is now `locked`, because without it an idle `serve` calls `ollama.com` (A6). **Size.** L.

| WP | Work | Verified by |
|---|---|---|
| 3.1 | **`install.archive@1` grows up**: `.tar.zst` with `ruzstd` (Q14), sha256 verification (pinned or host digest), extract into `<tag>.partial`, entry filter (path escapes refused; `skip` globs with `gpu.known`: the install is 2.2 GB and 102 MB without the CUDA directories, A2), smoke test, atomic rename (§11.5). Measured: 6.3 s to unpack the 1.44 GB archive with `ruzstd` (A9). llama.cpp's installs gain the atomic rename | generated archives of all three kinds, a truncated download, an escaping entry, a kill mid-extract leaving no half-installed build |
| 3.2 | **`feed.github_release@1`** with asset digests and exact-name rules; overlays (off by default) | a fake release feed |
| 3.3 | **`http_call@1`** (JSON bodies with typed interpolation; streaming `ndjson` and `sse`; progress and extract mappings by JSON pointer) and `catalog.engine_list` | `tiny_http` fake with NDJSON progress |
| 3.4 | **`attach@1`** and liveness probe; strategies `attach_if_running`, `spawn`; management mode (a plan with no model); warm and unload phases in `Session` | fake daemon; Session tests for every row of the §11.1 table |
| 3.5 | **Process hygiene** (§11.2): own process group and `SIGTERM`, grace, `SIGKILL` on Unix; `PR_SET_PDEATHSIG` on Linux, with SIGTERM (A3: after a SIGKILL of `ollama serve` its runner lived on); Job Object with `KILL_ON_JOB_CLOSE` and `CREATE_NO_WINDOW` on Windows; loader-environment scrubbing under AppImage and snap; the plan as the first log line | a test that spawns a child that spawns a child and asserts neither survives Stop or an app exit |
| 3.6 | **Decode as data**: `decode@1` driven by the vocabulary's `decode` blocks (SSE and NDJSON framing, JSON pointers, message-content summary); the proxy takes its decoders from documents. The vocabulary document gets its first embedded copy | the existing proxy tests, plus Ollama native chat and generate fixtures |
| 3.7 | **Engine selector** panel and `engines/ollama.toml` (embedded, `bundled = false`), the `Engine` row in Settings, per-engine API panel and "Open Chat UI" visibility | settings_window and a new engine-selection suite |
| 3.8 | **Test repo**: `fake_ollama.py` (§14), a lifecycle suite, network_privacy additions (GitHub only on a click; no cloud call) | the suites |
| 3.10 | **Foreign-program guard (Q4).** A user-layer engine document whose resolved program is neither located by the engine's own `locate` strategies nor inside an installed runtime root asks for confirmation once, remembered by the document's content hash; until this exists the user layer is deliberately not advertised | a Session test that a patched `program` is refused until confirmed |
| 3.9 | **Privacy and licences.** `PRIVACY.md` and the About text (§12.3; the managed daemon runs with `OLLAMA_NO_CLOUD=1`, and its first start writes `~/.ollama/id_ed25519`, A4 and A6), `THIRD_PARTY_LICENSES.md` ("not bundled, downloaded on request") | the network_privacy suite |

**Gate.**
- [ ] fmt, clippy, tests green; zstd throughput measured (A9) and acceptable
- [ ] the lifecycle suite passes: install (fake archive), attach, spawn, warm, stop, crash, app exit
- [ ] no orphan process after Stop or exit, on Linux and (by CI test) on Windows
- [x] spike A1-A7, A9, A14, A17 settled and recorded in `engine-facts.md` (2026-10-06)
- [ ] A8 observed on Windows (the CI run or a manual start): no console window for `ollama.exe` or `llama-server.exe`
- [ ] the llama.cpp suites still have the baseline result
- [ ] a manual run with a real Ollama on this machine: install, pull a small model, start, chat, stop

**Risks.** Ollama facts that were only read from documentation (A14) turn out wrong (mitigation: the spike gates
the phase). A 1.4 GB download in tests (mitigation: tests use generated archives; the real download is manual).

## 9. Phase 4: catalog, recipes, adapters

**Goal.** A model is a recipe with components; adapters appear in the UI from data; a vision model works on
llama.cpp, a LoRA second.

**Entry.** Phase 3 gate; spike A11-A13 settled. **Size.** L.

| WP | Work | Verified by |
|---|---|---|
| 4.1 | **Catalog schema** (§6.4): `[[model]]`, `components`, `run.<engine>`, sources (`bundle`, `huggingface`, `url`, `github_release`, `ollama_ref`, `local`), verification modes; today's `KNOWN_MODELS` and the bundled-model pins of `build/config.env` become the embedded starter catalog | the examples validate; offered-under-engine rules tested |
| 4.2 | **Catalog primitives**: `catalog.scan_dir`, `.bundle_dir`, `.hf_gguf` (the existing Hugging Face look-up, parameterised), `.curated`; `fetch.file` replaces `downloader.rs` | the existing `models.rs`, `remote.rs` and `downloader.rs` tests, adapted |
| 4.3 | **Acquire phase for several artifacts**: present means skip, missing means a `Job` with progress and sha256 | planner and runtime tests with fakes |
| 4.4 | **Component bindings** in the planner (`each`, `sep`, `arg_when_false`, `requires_flag`) and **flag discovery**: `exec.probe` with the `help_flags` parser that skips tombstones (L13), cached by path and mtime; `runtime.build` from `--version` | golden parses of the saved b11007 `--help` (a fixture copied into the repo) and of a synthetic older one; the tombstone cases |
| 4.5 | **Adapters panel**, component slots, `visible_when` fields (`ffmpeg_dir`, `video_fps`), adapter-kind fields, live LoRA scale (`applies = "live"`), discovery of `mmproj*.gguf` beside a model | new suites with the fake llama-server and generated recipes; form-model tests |
| 4.6 | **Modality discovery** from `/props` and `/api/show` (A13, A14); API examples with `when` expressions | fake endpoints |
| 4.7 | **Profiles**: the data model and loading (no save/load UI, Q10) | loader tests |
| 4.8 | **Test repo**: adapter and dynamic-UI suites, recipes dropped into the throw-away HOME | the suites |

**Gate.**
- [ ] fmt, clippy, tests green
- [ ] new suites pass with fake engines and recipes (no GPU, no model needed)
- [ ] a vision recipe plans `--mmproj` and `--no-mmproj-offload` correctly; a build lacking the flag hides the field
- [ ] a manual smoke test with a real vision model on llama.cpp
- [ ] llama.cpp-only users see no change unless a recipe or document asks

## 10. Phase 5: layout, policy, strings, tooling

**Goal.** What remains hard-coded becomes data, and the tooling that keeps documents honest is complete.

**Entry.** Phase 4 gate. **Size.** M.

| WP | Work |
|---|---|
| 5.1 | `policy` document (paths, network allow-list, limits, timeouts, heuristics, GPU patterns) and one `net` layer that enforces it for every fetching primitive; the Appendix table of `architecture.md` is the checklist |
| 5.2 | `ui` document: window, screens, panels, theme, strings; the default layout *is* today's layout (§9.4); the shell (window, tab strip, log pane) becomes data last |
| 5.3 | Hot reload when idle and a "Reload configuration" action; configuration diagnostics panel |
| 5.4 | `bundle.toml` written by `stage_bundle` in `quickai_base` from `config.env` (what ships, with hashes); the `packaged_bundle` suite checks it |
| 5.5 | Generated reference documentation and JSON Schema from the Rust types; the generated privacy table checked against `PRIVACY.md` |
| 5.6 | Ranged download of the wanted entries of the Windows Ollama zip (about 0.11 GB instead of 1.47 GB) |
| 5.7 | `--features locked-config` (Q15) |

**Gate.** `PRIVACY.md` checked against the generated table; the pixel comparison identical for the default `ui`
document; the GUI suite at the baseline; `config check --strict` and the schema in CI.

## 11. Phase 6: optional

Several units and gateway routing for sidecars (whisper.cpp, stable-diffusion.cpp), remote content packs with
signing, the router-mode launch variant (§10.4), external drivers. Each is its own small plan, written when a
real need appears; none starts without a decision from the user.

## 12. Records

### 12.1 Run log

| Date | What | Result |
|---|---|---|
| 2026-10-06 | Baseline before any change: `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` | clean; 53 passed, 2 ignored |
| 2026-10-06 | Baseline GUI suite (`build/test.sh`, debug build of the untouched checkout) | 200 tests (the package and online suites are opt-in): 192 passed, 0 failed, 8 skipped (the known-issue cases) |
| 2026-10-06 | UI snapshots, baseline binary, three runs | the first two differed only in a stall-server port and the log-line order of the crashed state; after pinning both, deterministic |
| 2026-10-06 | UI snapshots, Phase 0 build against the baseline | 15 of 15 identical |
| 2026-10-06 | UI snapshots, Phase 1 build (`Session`, planner, registry) against the baseline | 15 of 15 identical |
| 2026-10-06 | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` on the Phase 1 build | clean; quickai 104 passed and 2 ignored, quickai-core 142 + 3 + 2 passed |
| 2026-10-06 | The 2 ignored real-model tests (`cargo test -p quickai -- --ignored`), real llama-server and Bonsai-8B | passed (4 s) |
| 2026-10-06 | `quickai config check --strict --embedded`, `quickai plan --explain` on the real binary | clean; the plan shows every argument with its origin |
| 2026-10-06 | GUI suite (`build/test.sh`, debug) on the Phase 1 build as first built, before the planner-strictness edits and the test fixes that followed | 200 tests: 192 passed, 0 failed, 8 skipped (the same as the baseline); another project's suite ran alongside |
| 2026-10-06 | Ollama spike (track S) on the real v0.35.1 binary: isolated `HOME`, `strace` for the privacy checks, both test GGUFs imported and chatted with | A1-A7, A9, A12, A14, A17 settled; two documented claims corrected (LoRA is refused, the default context is tiered by VRAM) and three design details changed (A3, A6, A7); see `engine-facts.md` |
| 2026-10-06 | Final tree: `cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `cargo test --workspace`, `quickai config check --strict --embedded` | clean; quickai 104 passed and 2 ignored, quickai-core 145 + 3 + 2 passed |
| 2026-10-06 | Final tree: UI snapshots against the untouched baseline | 15 of 15 identical |
| 2026-10-06 | Final tree: GUI suite (`build/test.sh`, debug), run alone from QuickAI's side (two other projects' suites ran alongside) | 200 tests: 192 passed, 0 failed, 8 skipped, exit 0; the skipped cases are the baseline's own eight (TC-LIVE-008, -014, -017, TC-MIN-023, TC-SRV-016, -022, TC-SET-035, -036) |
| 2026-10-06 | Phase 2 tree: `cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `cargo test --workspace`, `quickai config check --strict --embedded`, `quickai config docs` against `docs/reference/engine-fields.md` | clean; quickai 139 passed and 2 ignored, quickai-core 184 + 2 + 3 + 4 passed; the reference page is in sync |
| 2026-10-06 | The 2 ignored real-model tests, real llama-server and Bonsai-8B, on the Phase 2 tree | passed (6 s) |
| 2026-10-06 | UI snapshots, Phase 2 build against the untouched baseline binary: the 15 states of Phase 0 plus 10 tooltip states (the hover text of the five fields, the models folder, the binary box, the installed builds, the look-up button, Apply and Download) | 25 of 25 identical, byte for byte |
| 2026-10-06 | GUI suite on the Phase 2 build as first run (scratch directories under a long `TMPDIR`) | 202 tests: 191 passed, 3 failed (TC-SET-001 and -007: the long path truncated the models-folder box; TC-SET-016: see the next rows), 8 skipped |
| 2026-10-06 | TC-SET-016, alone, 3 runs each on the Phase 1 baseline binary and the Phase 2 build, alternating | baseline 2 passed, 1 failed; Phase 2 1 passed, 2 failed: the same failure on both builds, depending on the scratch directory's random name; the screenshots are identical in every pixel of the label |
| 2026-10-06 | TC-SET-016 reading the label below the models-folder box (`below=140`), 8 runs each on the baseline binary and the Phase 2 build | 16 of 16 passed |
| 2026-10-06 | Session and proxy unit tests looped, before and after asking for another port when the first is held | before: about 3 failures in 40 runs ("Address already in use"); after: 0 in 100 plain runs and 0 in 25 under `strace` (where the old code failed at once) |
| 2026-10-06 | Final tree: GUI suite (`build/test.sh`, debug, `TMPDIR=/var/tmp/qt`), run alone from QuickAI's side (other projects' suites ran alongside) | 202 tests: 194 passed, 0 failed, 8 skipped, exit 0; the skipped cases are the baseline's own eight; TC-PER-009 and -010 pass; the 13 opt-in cases (package, online) are not in a default run |

### 12.2 Decision log

| Date | Decision | Why |
|---|---|---|
| 2026-10-06 | Q1-Q15 accepted as recommended | the user |
| 2026-10-06 | C1-C10 chosen (§3.1) | open points of the architecture settled for the work packages |
| 2026-10-06 | The embedded `llamacpp.toml` holds only interpreted sections; hover texts and labels move in Phase 2 | a document should not promise behaviour that is not wired, and the code's copies of the texts are removed in Phase 2 anyway |
| 2026-10-06 | The `Status` job event waits for the Ollama pull (Phase 3) | no producer until then; an unused variant would be dead code |
| 2026-10-06 | The loader keeps contributor bodies and the registry falls back layer by layer | R6 and §5.3: an invalid user patch must leave the embedded document in force |
| 2026-10-06 | The proxy's internal port is a host fact allocated by the application before planning | keeps the planner pure; the failure message and behaviour are unchanged |
| 2026-10-06 | Expression trees are limited to 200 levels | found by the pathological-input test (stack overflow on a 5000-term chain) |
| 2026-10-06 | One full GUI suite run covers the Phase 0 and Phase 1 gates, taken on the Phase 1 build | Phase 0 is a strict subset of what the Phase 1 build contains; the per-phase pixel comparisons were run separately |
| 2026-10-06 | LoRA is a llama.cpp-only adapter kind: the Ollama engine document declares no `lora` component, so the Adapters panel never offers it there. A projector does work on Ollama (the spike: `files` carries it and `capabilities` gains `vision`, A14); a draft model is possible through `draft_files` (source only, O7) | Ollama v0.35.1 refuses adapters in `/api/create` (O7, read from `server/create.go`); architecture §10.2 had said `files` carries them and is corrected |
| 2026-10-06 | The Ollama context size travels only as `OLLAMA_CONTEXT_LENGTH` in the spawn environment; the warm-up call no longer passes `num_ctx`, and the field is `spawn_only` | Spike A7: a per-request `num_ctx` reloads the runner at that size and the next request without it reloads it at the default, so on an attached daemon a warm-up value only moves the reload |
| 2026-10-06 | `OLLAMA_NO_CLOUD=1` is `locked` and protected from the free-text environment field | Spike A6: without it an idle `serve` contacts `ollama.com` within 40 ms and every ~4 h; with it a whole session made no non-loopback connection. Opting in to Ollama's cloud features would be a separate, explicit setting if ever wanted |
| 2026-10-06 | `PR_SET_PDEATHSIG` uses SIGTERM, and Stop signals the whole process group | Spike A3: SIGKILL of `ollama serve` alone orphaned its `llama-server` runner (alive after 36 s); SIGTERM or a group kill removed both in 0.1 s |
| 2026-10-06 | The Ollama install skips the CUDA directories unless an NVIDIA GPU is known (`gpu.known`) | Spike A2: 2.2 GB installed against 102 MB without `cuda_v12` and `cuda_v13`; the download is 1.44 GB either way |
| 2026-10-06 | The widget registry of Phase 2 holds only the widgets a form uses today (integer and float drag boxes, port, checkbox, combo, one-line text, the runtime manager); key-value lines, the path picker, a progress bar and a button wait for Phase 3 | Nothing in the llama.cpp form needs them, and a widget with no user is code nobody has seen run (the plan listed them in WP2.3) |
| 2026-10-06 | `reset_on` and `ui.advanced` are not built; `validate` rules are written `{ check, message }` and the first whose check does not hold is the error | `recompute_on` and the `advanced` section cover what the first two were for; "check" says what must hold, where "when" would not say whether it is the error or the guard |
| 2026-10-06 | The speed estimate and the model recommendation take the engine's `gpu_offload` fact, not the GPU-layers number | That is what `derive` is for: Ollama's `gpu` choice derives the same fact, and neither heuristic should know which engine's setting it came from |
| 2026-10-06 | A platform the `[acquire]` words do not cover matches no build (the old code treated an unknown OS as Linux and an unknown architecture as x86-64) | Offering a Linux build to a BSD or a 32-bit ARM machine was a guess presented as a list; supported platforms are unchanged |
| 2026-10-06 | The old installer tests, which downloaded llama.cpp's source archives and wrote into the real `~/.quickai/llama-bin`, are replaced by tests against a local server of generated archives with a temporary root | The plan's Phase 0 note flagged them; a primitive that takes its root as a parameter can be tested without touching the user's home or the internet. The one real-network test that stays is the feed against GitHub (it checks the project's file naming) |
| 2026-10-06 | The first snapshot baseline (`base1`) is retired; the reference set for the 15 states is the one taken after the stall port and the crash-log order were pinned | `base1` predates the pinning and differs from every later run in two states for reasons of the harness, not the application |

## 13. Cross-repository changes

| Repository | Phase | Change |
|---|---|---|
| `quickai` (project) | 0-5 | everything above |
| `quickai_base` (packaging) | 2 | the README names the suite's new size (215 cases) |
| `quickai_base/test` | 2 | TC-PER-009 and -010 (keys the app does not know survive a save); TC-SET-016 reads its label below the models-folder box; the suite documents point at the new module layout; strategy notes on what OCR is sensitive to |
| `quickai_base` (packaging) | 5 | `stage_bundle` writes `bundle.toml`; nothing Ollama-related is ever bundled; later, optional-dependency hints in package manifests (an installed Ollama avoids a 1.4 GB download) |
| `quickai_base/test` | 3 | `fake_ollama.py`, an engine-selection suite, a lifecycle suite, network_privacy additions |
| `quickai_base/test` | 4 | adapter and dynamic-UI suites, config-diagnostics suite |
| `quickai_base/test` | 5 | `packaged_bundle` checks `bundle.toml`; privacy table |

## 14. New dependencies, by phase

| Phase | Crate | Where | Why |
|---|---|---|---|
| 1 | `toml`, `toml_edit`, `serde_ignored`, `regex`, `semver` | `quickai-core` | parsing and merging, line lookup, unknown keys, `matches`, `semver_ge` |
| 3 | `sha2`, `ruzstd`, `libc` (Unix), `windows-sys` (Windows) | `quickai` | digests, `.tar.zst`, process groups and `PDEATHSIG`, job objects |
| 5 | `schemars`, `include_dir` (if not earlier) | build and docs, `quickai` | JSON Schema, embedded manifests |

Each addition is checked for licence compatibility (dependencies must be permissively licensed) and for Windows, macOS and old-glibc
Linux builds before it is merged.

## 15. Spike track (S)

Runs by hand in an isolated `HOME`, results recorded in `engine-facts.md`. Not on the critical path of Phases
0-2. **Status (2026-10-06):** the Phase 3 items are settled except A8; A12 of the Phase 4 items is settled as well
(the table in `engine-facts.md` has the findings and how each was measured). Open: A8 and A11 need Windows, A10
needs two distant llama.cpp builds, A13 needs a model and projector that belong together, A15 and A16 are Phase 6.

| Needed before | Items |
|---|---|
| Phase 3 | A1 (archive layout under an arbitrary prefix), A2 (Vulkan in the Linux archive), A3 (SIGTERM and SIGKILL behaviour), A4 (`~/.ollama` key writes), A5 (Bonsai Q1_0 through `/api/create`), A6 (`OLLAMA_NO_CLOUD`), A7 (`OLLAMA_CONTEXT_LENGTH`), A8 (Windows console window), A9 (`ruzstd` throughput), A14 (Ollama API details), A17 (`ollama --version` output) |
| Phase 4 | A10 (flag drift between distant llama.cpp builds), A11 (`--lora-scaled` with a Windows path), A12 (projector mismatch error text), A13 (`/props` modality keys) |
| Phase 6 | A15 (router mode load and unload), A16 (sidecar flags) |

## 16. Resuming

A fresh session should read, in order: this file's status table and logs, `architecture.md` §15 and the section of
the current phase, `engine-facts.md` for the assumption numbers involved, and `git status` to see what is in the
working tree. Then run the gate commands of §4 to confirm the starting point is green.
