# Configuration documents: a runtime as a file

_Generated from the types in `crates/quickai-core/src/doc/profile.rs` by `quickai config docs profile`. Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` rewrites it, and a test fails while it is out of date._

A configuration document (`kind = "profile"`) says what QuickAI runs and how: the engine, the model, which build of the program, the settings and command-line arguments of the engine and of what goes with it. The window has one active configuration, a dropdown chooses it, and a file can be given to someone else, who is told what their computer lacks or does differently before anything is taken (docs/configurations.md). Configurations are data: they can pick things the application already knows (an engine, a model, a build, the value of a setting), say where a model file can be had from, and cannot name a program. They never carry a model, a folder of a computer, a login or a secret. The application ships `default`; a package may ship more in `<exe>/config/profiles/`; the person's own are files in `profiles/` of their configuration directory, and one with the same `id` and `replace = true` stands in for a shipped one. A key this version does not know is kept when the file is written again and told as a warning; `quickai config check --strict` makes warnings fail.

## the document

The top level of a configuration, besides the envelope (`schema`, `kind`, `id`, `requires`, and `replace` for a file of the person's that stands in for one of the same `id` in a lower layer). The keys that are not tables come first in the file.

| Key | Value | Meaning |
|---|---|---|
| `name` | text | What the dropdown and the list call it, up to 60 characters; control and direction characters are taken out. Default: the `id`. The `id` is made from it (lower case, letters, digits and `-`) and is never a path. |
| `meta` | table, below | For the person who is given the file. Nothing in it changes what runs. |
| `engine` | text | The id of the engine it runs: one a person can choose, not a companion. Required. An engine this version does not have is told to the person who opens the file, and nothing is stored. |
| `runtime` | table, below | Which program, as who it is and not where it is. |
| `model` | list of tables, below | The models it uses, as who they are and where they can be had from, never as a path and never the model itself. The first is the one the engine loads. |
| `needs` | list of tables, below | What whoever runs the configuration has to give besides the file: a login, a setting that belongs to a computer, a variable of a program's environment. The file never carries them. |
| `values` | table of tables | The value of settings: `[values.<engine>]` holds the fields of that engine by their ids (`ctx_size = 16384`), and a companion's settings sit in a table of its own (`[values.openwebui]`). Only what differs from the default is written. A field whose `share` is `never`, and one the engine does not have, is told and ignored; a value that does not fit its field is told and the default is used. |
| `app` | table, below | Settings of the application that are part of a run. |

## `[meta]`

Who made the file and for what. Coarse on purpose: no name of a person or of a computer is ever written.

| Key | Value | Meaning |
|---|---|---|
| `note` | text | A sentence or two from whoever made the file. |
| `made_with` | text | The application that wrote it (`QuickAI 0.1.0`). |
| `made_on` | `{ os, arch, gpu, memory_gb }` | The kind of computer it was made on: `linux`, `windows` or `macos`; `x86_64` or `aarch64`; the makers of the graphics cards (`amd`, `nvidia`, `intel`); the memory in whole gigabytes. The person who opens the file is told where the computer differs from theirs. |
| `community` | `{ id, version }` | The configuration of quickai.everybodyai.dev the file was fetched from (`{ id = "vision-chat-16gb", version = 3 }`). QuickAI writes it when it imports one, and an export keeps it, so that a configuration made from another can say what it is based on. Nothing in it changes what runs. |

## `[meta]` `community`

Where on quickai.everybodyai.dev a configuration came from.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | The configuration's id on the site. |
| `version` | whole number | Its version. |

## `[meta]` `made_on`

The kind of computer a file was made on.

| Key | Value | Meaning |
|---|---|---|
| `os` | text | `linux`, `windows` or `macos`. |
| `arch` | text | `x86_64` or `aarch64`. |
| `gpu` | list of text | The makers of the graphics cards. |
| `memory_gb` | whole number | The memory, in whole gigabytes. |

## `[runtime]`

The program a configuration runs, and the programs that go with it. Left out, it is the program the computer uses by default.

| Key | Value | Meaning |
|---|---|---|
| `build` | text | The label of a build in the manager's list (`v0.40.0`): the name of the directory it is installed in, with no folder in it. Choosing the configuration sets the engine's program to the installed build of that label, and to nothing else. One that is not installed is told, with the labels that are. |
| `version` | text | The version the program said it was, for the notes. |

## `[runtime.<program>]`

A program that goes with the engine (`[runtime.openwebui]` for Open WebUI beside Ollama), with the same two keys. `quickai-run` installs that build of it, and the window leaves the file's pin as it is when it saves. A key that is none of these is one this version does not know.

| Key | Value | Meaning |
|---|---|---|
| `build` | text | The label of the build of that program to install and run. |
| `version` | text | The version the program said it was, for the notes. |

## `[[model]]`

One model, by exactly one source: a name the engine takes (the key is one of the engine's `accepts.refs`: `ollama = "qwen3:8b"`), an entry of the recommended list (`catalog`, and `file` for one of its files), or a file (`file`) with, if it is to be fetched, where it can be had from (`from`, and `path` and `revision` in it, or `url`). A path of a computer is never a source, and the model is never in the file. Choosing a configuration in the window selects a model that is here and fetches nothing: one that is not here is told, with its size and where it can be had. `quickai-run` fetches it from where the file says, once whoever runs it has read where that is.

| Key | Value | Meaning |
|---|---|---|
| `role` | text | What it is for (`chat`, `embedding`); letters, digits, `_` and `-`. Default `main`. |
| `name` | text | What people call it; nothing finds it by this. |
| `catalog` | text | The id of an entry of the recommended list. |
| `file` | text | A file name, with no folder in it: the file of a `catalog` entry, or a file of the models folder. |
| `size` | whole number | Its size in bytes, to recognise it by and to say what getting it costs. |
| `sha256` | text | Its SHA-256, 64 hexadecimal digits. |
| `from` | text | Where it can be got from, as a kind of place and a name in it (`huggingface:owner/repo`), for a file that is no entry of the recommended list. The window says it and never fetches from it by itself (the person asks, and looks it up); `quickai-run` fetches from it. |
| `path` | text | Where in the repository `from` names the file is (`split_files/vae/wan2.2_vae.safetensors`): names joined by `/`, none of them `..`, at most eight. Left out, the file is at the top and is called `file`. Needs `from`. |
| `revision` | text | The branch, tag or commit of the repository: letters, digits, `.`, `_` and `-`. Left out, `main`. A commit is the same file for ever. Needs `from`. |
| `url` | text | An address the file can be fetched from instead of `from`: `https://host/path`, with no sign-in in it (`http://` only for this computer's own). Not with `from`. |
| `into` | text | The folder of the models folder the file is kept in (`diffusion_models`, `vae`), for an engine that finds its files by the folder they are in: one name of lower case letters, digits, `_` and `-`. Left out, the folder the engine's document gives a file like this. |

## `[[needs]]`

One thing that whoever runs the configuration has to give, because the file cannot carry it, or that the computer has to have: exactly one of `token`, `setting`, `env` and `system`. `quickai-run` stops before it fetches anything, says which are missing and how to give them, and `quickai-run init` writes the file to fill in; it is given in an environment file, on the command line, or, for a login, in the usual variable. What the computer has to have is looked at on the computer, and the engine's document says how.

| Key | Value | Meaning |
|---|---|---|
| `token` | text | A login the fetching needs: `huggingface` or `github`. |
| `setting` | text | A setting that belongs to a computer, as `<engine>.<field>` (`comfyui.models_dir`). |
| `env` | text | A variable of a program's environment (`WEBUI_SECRET_KEY`). |
| `system` | text | Something the computer has to have, as the id of a `[[prerequisite]]` of the engine's document (`c_compiler`): the document says how it is checked and what brings it, and the file says only that it is needed and why. An id the document does not know is refused when the file is read. |
| `for` | text | The program an `env` goes to (`openwebui`). Left out, the engine of the configuration. |
| `secret` | bool | Whether the value is a secret, that is never shown. A token always is one, and so is a variable whose name has KEY, TOKEN, SECRET or PASSWORD in it. |
| `why` | text | Said to whoever has not given it, up to 300 characters. |

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
- a model's `path` is names joined by `/` with no `..`, its `revision` is a branch, tag or commit, its `url` is `https://host/path` with no sign-in, its `into` is one folder name, and `from` and `url` are not both given;
- a need is exactly one of `token`, `setting` and `env`;
- at most 512 settings, eight models and sixteen needs.

A warning, that leaves the document in use: a setting the engine does not have or does not carry, a value that does not fit its field (the default is used), keys this version does not know.
