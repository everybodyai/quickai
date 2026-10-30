# Telemetry events

_Generated from the schema in `crates/quickai-core/src/telemetry/schema.rs` by `quickai config docs telemetry`; `quickai --telemetry` prints the same as JSON. Do not edit by hand._

This is everything QuickAI can send EverybodyAI. Nothing is sent until you choose a telemetry level, and a level sends only the events of its own level and those below it: **Crashes only** sends `crash` events, **Errors and crashes** adds `error` events, **All** adds the rest. Text is cleaned before it is sent: a home folder, user and computer names, addresses, email addresses and anything that looks like a password or a token are replaced. See [the privacy policy](https://quickai.everybodyai.dev/privacy).

## Sent with every batch

| Property | Shape | Kind | What it is |
|---|---|---|---|
| `install` | a name (letters, digits and ._:/+@-) | pseudonymous id | random, made the first time anything is sent; Settings shows it and can replace it |
| `session` | a name (letters, digits and ._:/+@-) | pseudonymous id | random for each start of the app, never kept |
| `app.version` | a name (letters, digits and ._:/+@-) | system metadata | QuickAI's version |
| `app.channel` | one of: official, source | system metadata | a build EverybodyAI released, or one built from source |
| `app.commit` | a name (letters, digits and ._:/+@-) | system metadata | the commit it was built from |
| `app.target` | a name (letters, digits and ._:/+@-) | system metadata | the kind of computer it was built for (`x86_64-unknown-linux-gnu`) |
| `app.package` | one of: appimage, tar, snap, msix, zip, dmg, brew, scoop, choco, aur, other | system metadata | how it was installed |
| `os` | one of: linux, windows, macos, other | system metadata | the system |
| `os_version` | short text, cleaned | system metadata | the system's version |
| `arch` | a name (letters, digits and ._:/+@-) | system metadata | the processor's architecture |
| `memory_gb` | one of: 8, 16, 32, 64, 128 | system metadata | the memory, rounded to 8, 16, 32, 64 or 128 GB and more |
| `gpus` | any of: nvidia, amd, intel, apple | system metadata | the makers of the graphics adapters |
| `language` | short text, cleaned | system metadata | the system's language, without its region |
| `level` | one of: off, crash, error, all | system metadata | the telemetry level in force |

The server adds the country the request came from, as two letters, and never stores the address.

## Crashes only: events of level `crash`

### `crash.panic`

QuickAI stopped because of an error. Written when it happens, sent at the next start. Purpose: health: to find and fix what breaks.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `message` | text, cleaned, at most 2000 characters | error, cleaned | the error's message, cleaned |
| `location` | a file of QuickAI's source and a line | call stack | where in QuickAI's code it happened |
| `thread` | short text, cleaned | call stack | the name of the thread it happened in |
| `backtrace` | function names, at most 64 | call stack | the names of the functions that led to it |
| `engine` | a name (letters, digits and ._:/+@-) | public name | the engine (llamacpp, ollama, comfyui) |
| `engine_state` | one of: idle, starting, running, stopping, crashed | feature use | whether the engine was running |
| `open_windows` | any of: add_model, recommended, details, settings, configurations, help, about, telemetry_log, report_problem | feature use | which windows were open |
| `minutes_open` | one of: <5, 5-30, 30-60, 60-180, 180+ | measurement | how long the app had been open |

## Errors and crashes: events of level `error`

### `error.engine_exit`

An engine stopped by itself, while loading a model or while serving. Purpose: health: to find and fix what breaks.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `engine` | a name (letters, digits and ._:/+@-) | public name | the engine (llamacpp, ollama, comfyui) |
| `build` | a name (letters, digits and ._:/+@-) | public name | the label of the engine's build (`b11889-vulkan`), `bundled`, or `custom` for a program of the person's own |
| `model` | a name (letters, digits and ._:/+@-) | public name | `catalog:<id>` for a model of the recommended list, `hf:<owner>/<repo>` for one found through a look-up, otherwise `other:<size in GB>` |
| `phase` | one of: loading, serving | feature use | what the engine was doing |
| `status` | short text, cleaned | error, cleaned | how it ended (`exit 1`, `signal 9`, `unknown`) |
| `after_seconds` | whole seconds | measurement | how long after Start |

### `error.download`

A download failed: its kind, the site and the reason, never the file or the address. Purpose: health: to find and fix what breaks.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `what` | one of: model, build, program | feature use | what was being downloaded |
| `source` | one of: huggingface, github, ollama, pypi, pytorch, other | public name | the site it came from |
| `reason` | one of: checksum, network, disk_full, http_4xx, http_5xx, other | error, cleaned | why it failed |

### `error.stall`

The window stopped answering for five seconds or more. Purpose: health: to find and fix what breaks.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `seconds` | one of: 5-10, 10-30, 30-60, 60+ | measurement | for how long |

### `session.unclean_exit`

The last session ended without closing (a crash of the system's graphics, a forced kill, a power cut). Sent at the next start; never asked about. Purpose: health: to find and fix what breaks.

No fields.

## All: events of level `all`

### `session.start`

QuickAI started. Purpose: feature insight: what is used, and what to build or recommend.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `first_run` | yes or no | feature use | whether it is the first start, with no model yet |

### `session.end`

QuickAI was closed. Purpose: feature insight: what is used, and what to build or recommend.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `minutes_open` | one of: <5, 5-30, 30-60, 60-180, 180+ | measurement | how long it was open |

### `window.open`

A window was opened. Purpose: feature insight: what is used, and what to build or recommend.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `window` | one of: add_model, recommended, details, settings, configurations, help, about, telemetry_log, report_problem | feature use | which |

### `feature.use`

A feature was used or switched on. Purpose: feature insight: what is used, and what to build or recommend.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `feature` | one of: live_traffic, keep_chats, remote_run, open_webui, configuration_import, configuration_export, chat | feature use | which |

### `model.add`

A model was added. Purpose: feature insight: what is used, and what to build or recommend.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `how` | one of: recommended, lookup, link, import, pull | feature use | how it was added |
| `model` | a name (letters, digits and ._:/+@-) | public name | `catalog:<id>` for a model of the recommended list, `hf:<owner>/<repo>` for one found through a look-up, otherwise `other:<size in GB>` |

### `run.start`

Start was pressed. Purpose: feature insight: what is used, and what to build or recommend.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `engine` | a name (letters, digits and ._:/+@-) | public name | the engine (llamacpp, ollama, comfyui) |
| `build` | a name (letters, digits and ._:/+@-) | public name | the label of the engine's build (`b11889-vulkan`), `bundled`, or `custom` for a program of the person's own |
| `model` | a name (letters, digits and ._:/+@-) | public name | `catalog:<id>` for a model of the recommended list, `hf:<owner>/<repo>` for one found through a look-up, otherwise `other:<size in GB>` |
| `where` | one of: local, remote | feature use | this computer or another |

### `run.ready`

The engine answered after Start, with its model loaded. Purpose: performance: to know how fast things are.

| Field | Shape | Kind | What it is |
|---|---|---|---|
| `engine` | a name (letters, digits and ._:/+@-) | public name | the engine (llamacpp, ollama, comfyui) |
| `model` | a name (letters, digits and ._:/+@-) | public name | `catalog:<id>` for a model of the recommended list, `hf:<owner>/<repo>` for one found through a look-up, otherwise `other:<size in GB>` |
| `seconds` | whole seconds | measurement | how long that took |
