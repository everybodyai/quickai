# Configuration documents: a runtime as a file

_Generated from the types in `crates/quickai-core/src/doc/profile.rs` by `quickai config docs profile`. Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` rewrites it, and a test fails while it is out of date._

A configuration document (`kind = "profile"`) says what QuickAI runs and how: the engine, the model, which build of the program, the settings and command-line arguments of the engine and of what goes with it. The window has one active configuration, a dropdown chooses it, and a file can be given to someone else, who is told what their computer lacks or does differently before anything is taken (docs/configurations.md). Configurations are data: they can pick things the application already knows (an engine, a model, a build, the value of a setting) and cannot name a program, a host or an address to fetch from. The application ships `default`; a package may ship more in `<exe>/config/profiles/`; the person's own are files in `profiles/` of their configuration directory, and one with the same `id` and `replace = true` stands in for a shipped one. A key this version does not know is kept when the file is written again and told as a warning; `quickai config check --strict` makes warnings fail.

## the document

The top level of a configuration, besides the envelope (`schema`, `kind`, `id`, `requires`, and `replace` for a file of the person's that stands in for one of the same `id` in a lower layer). The keys that are not tables come first in the file.

| Key | Value | Meaning |
|---|---|---|
| `name` | text | What the dropdown and the list call it, up to 60 characters; control and direction characters are taken out. Default: the `id`. The `id` is made from it (lower case, letters, digits and `-`) and is never a path. |
| `meta` | table, below | For the person who is given the file. Nothing in it changes what runs. |
| `engine` | text | The id of the engine it runs: one a person can choose, not a companion. Required. An engine this version does not have is told to the person who opens the file, and nothing is stored. |
| `runtime` | table, below | Which program, as who it is and not where it is. |
| `model` | list of tables, below | The models it uses, as who they are and never as a path. The first is the one the engine loads. |
| `values` | table of tables | The value of settings: `[values.<engine>]` holds the fields of that engine by their ids (`ctx_size = 16384`), and a companion's settings sit in a table of its own (`[values.openwebui]`). Only what differs from the default is written. A field whose `share` is `never`, and one the engine does not have, is told and ignored; a value that does not fit its field is told and the default is used. |
| `app` | table, below | Settings of the application that are part of a run. |

## `[meta]`

Who made the file and for what. Coarse on purpose: no name of a person or of a computer is ever written.

| Key | Value | Meaning |
|---|---|---|
| `note` | text | A sentence or two from whoever made the file. |
| `made_with` | text | The application that wrote it (`QuickAI 0.1.0`). |
| `made_on` | `{ os, arch, gpu, memory_gb }` | The kind of computer it was made on: `linux`, `windows` or `macos`; `x86_64` or `aarch64`; the makers of the graphics cards (`amd`, `nvidia`, `intel`); the memory in whole gigabytes. The person who opens the file is told where the computer differs from theirs. |

## `[meta]` `made_on`

The kind of computer a file was made on.

| Key | Value | Meaning |
|---|---|---|
| `os` | text | `linux`, `windows` or `macos`. |
| `arch` | text | `x86_64` or `aarch64`. |
| `gpu` | list of text | The makers of the graphics cards. |
| `memory_gb` | whole number | The memory, in whole gigabytes. |

## `[runtime]`

The program a configuration runs. Left out, it is the program the computer uses by default.

| Key | Value | Meaning |
|---|---|---|
| `build` | text | The label of a build in the manager's list (`v0.40.0`): the name of the directory it is installed in, with no folder in it. Choosing the configuration sets the engine's program to the installed build of that label, and to nothing else. One that is not installed is told, with the labels that are. |
| `version` | text | The version the program said it was, for the notes. |

## `[[model]]`

One model, by exactly one source: a name the engine takes (the key is one of the engine's `accepts.refs`: `ollama = "qwen3:8b"`), an entry of the recommended list (`catalog`, and `file` for one of its files), or a file of the models folder (`file`). A path is never a source. Choosing a configuration selects a model that is here and fetches nothing: one that is not here is told, with its size and where it can be had.

| Key | Value | Meaning |
|---|---|---|
| `role` | text | What it is for (`chat`, `embedding`); letters, digits, `_` and `-`. Default `main`. |
| `name` | text | What people call it; nothing finds it by this. |
| `catalog` | text | The id of an entry of the recommended list. |
| `file` | text | A file name, with no folder in it: the file of a `catalog` entry, or a file of the models folder. |
| `size` | whole number | Its size in bytes, to recognise it by and to say what getting it costs. |
| `sha256` | text | Its SHA-256, 64 hexadecimal digits. |
| `from` | text | Where it can be got from, as a kind of place and a name in it (`huggingface:owner/repo`), for a file that is no entry of the recommended list and that the application remembers fetching. It is said to whoever has not got the model, and is never fetched from by itself: the person asks, and looks it up. |

## `[app]`

Settings of the application, not of an engine, that change a run.

| Key | Value | Meaning |
|---|---|---|
| `traffic_proxy` | bool | Whether the live-traffic proxy sits in front of the engine. |

## What travels: `share`

Which settings a configuration holds is not decided here but by the engine documents: each `[[field]]` has a `share` (`yes`, `review` or `never`, see `quickai config docs`). When it says nothing the type decides:

| `type` | `share` |
|---|---|
| `int` | `yes` |
| `float` | `yes` |
| `bool` | `yes` |
| `text` | `review` |
| `choice` | `yes` |
| `port` | `yes` |
| `dir` | `never` |
| `file` | `never` |
| `url` | `review` |
| `runtime_path` | `never` |
| `kv_lines` | `review` |
| `model_ref` | not a setting: the model has a section of its own |
| `hidden` | not a setting: the model has a section of its own |

A setting that is `review` travels, and the person who is given a file that has it is shown it in full and asked before it is taken: it becomes a command-line word, a line of environment or an address, or it widens what a program may do. A setting that is `never` is a folder, a file or a program, or belongs to a computer; it is not written and is ignored when it is read. The names of `deny_env` of the vocabulary (and `LD_*`, `DYLD_*`, `PATH`, `PYTHON*`, which are always refused) cannot be set from a configuration, even when it is ticked.

## What is checked

Each of these is an error that names the file and the place, and a document with an error in it is not used (the document below it, if any, stays in force):

- the file is TOML, no more than 256 KiB, with a `schema` this version reads and a `requires.app` it meets;
- `engine` is an id; `name` has no more than 60 characters;
- a `[[model]]` has exactly one source, of the kind the engine takes, and no name in it is a path (no `/` or `\\` in a file, no `..`, no drive, no NUL, no control character);
- `runtime.build` is a label, not a path;
- at most 512 settings and eight models.

A warning, that leaves the document in use: a setting the engine does not have or does not carry, a value that does not fit its field (the default is used), keys this version does not know.
