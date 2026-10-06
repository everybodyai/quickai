# Engine documents: sections, fields and bindings

_Generated from the types in `crates/quickai-core/src/doc/engine.rs` by `quickai config docs`. Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` rewrites it, and a test fails while it is out of date._

An engine document (`kind = "engine"`) says how QuickAI finds, starts and talks to one engine, and which settings it has. A document in the user's configuration directory with the same `id` patches the embedded one: tables merge by key and `[[field]]` entries by `id` (docs/architecture.md, section 5.2). A key this version does not know is a warning; `quickai config check --strict` makes warnings fail.

## `[engine]`

Who the engine is.

| Key | Value | Meaning |
|---|---|---|
| `name` | text | What the interface calls the engine (`llama.cpp`). Required. |
| `server_name` | text | The name of its server program (`llama-server`), for sentences about it. |
| `summary` | text | One line about what it does. |
| `license` | text | Its licence, for the About window. |
| `bundled` | bool | Whether packages ship it. Default false. |
| `homepage` | text | Where to read about it. |
| `egress` | list of text | The hosts the engine itself contacts, which the privacy table lists. |

## `[provides]`

What a running engine offers.

| Key | Value | Meaning |
|---|---|---|
| `apis` | list of text | The ids of the APIs it serves. |
| `web_ui` | `{ path, label }` | Its built-in web page, if it has one; the button that opens it carries `label`. |
| `accepts` | `{ formats }` | The model file formats it loads. |
| `model_name_matters` | bool | Whether a request naming another model loads that one. |
| `default_port` | whole number | The port it uses when nothing says otherwise. |

## `[locate]`

Finding the executable without touching the network. `strategies` is a list, tried in order, and the first that yields a path wins. Each entry has `use` and its own keys:

| Key | Value | Meaning |
|---|---|---|
| `strategies` | list of tables | `{ use = "setting", field }` takes the path a field holds; `{ use = "next_to_exe", file }` looks beside the application's executable; `{ use = "dev_tree", path }` looks below the source checkout the application was built in. |

## `[launch]`

How the engine is started.

| Key | Value | Meaning |
|---|---|---|
| `strategies` | list of text | The ways to bring it up, in order. This version knows `spawn`. |
| `spawn` | table | The program to start, below. |
| `ready` | table | How to tell it is up, below. |

## `[launch.spawn]`

Starting the program. Every text is a template: `${...}` holes are filled from the context below.

| Key | Value | Meaning |
|---|---|---|
| `program` | template | The program to start (`${runtime.binary}`). |
| `args` | list of templates | Its first arguments, one each. The fields' `bind.arg` follow, by `order`. |
| `env` | list of tables | Environment variables to set, below. |
| `group` | bool | Start it in its own process group (not used yet). |
| `stop` | table | How to stop it: `{ policy = "kill" }`. |

## `[[launch.spawn.env]]`

One environment variable: `value` or `prepend`, not both.

| Key | Value | Meaning |
|---|---|---|
| `name` | text | The variable. |
| `value` | template | Set it to this. |
| `prepend` | template | Put this in front of what the variable already holds, joined with `sep`. |
| `sep` | text | The joiner for `prepend`. Default `:`. |
| `when` | condition | Only while this holds. |
| `locked` | bool | A user's own environment may not override it. |

## `[launch.ready]`

Waiting for the engine to answer.

| Key | Value | Meaning |
|---|---|---|
| `probe` | `{ use = "http", path, expect }` | A request to make; `expect` is the status that means ready (default 200). |
| `timeout` | duration | How long to wait before giving up (`120s`). |
| `interval` | duration | How often to ask (`500ms`). |

## `[[field]]`

One setting. Fields build the settings the window shows, and each says how its value reaches the engine. The same document can add, change or hide a field of the embedded one by `id`.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Letters, digits and `_`. Names the value (`llamacpp.port`) and is how a patch finds the field. Required. |
| `label` | text | What the window calls it. Default: the `id`. |
| `help` | text | The hover text. |
| `type` | a type, below | What kind of value it holds. Required. |
| `range` | `[low, high]` | The numbers it may hold; the number box stops there. |
| `step` | number | How far one pixel of dragging moves a number. |
| `default` | value or `{ fn, args }` | The value when nothing was chosen: a literal, or a named function over context values (`recommend_ctx_size`). |
| `optional` | bool | The field may stay empty. |
| `recompute_on` | list of text | What re-derives the default. `model`: picking another model. |
| `legacy_key` | text | The key it has always had in `settings.json`; its value is stored there, not under `values`. |
| `visible_when` | condition | Show it only while this holds. |
| `enabled_when` | condition | Let it be edited only while this holds (`not session.running`). |
| `required_when` | condition | It needs a value while this holds. |
| `options` | list of text | The values a `choice` offers, and its default must be one of them. |
| `pattern` | regular expression | The whole text value must match it. |
| `validate` | list of `{ check, message }` | Rules that may look at other fields. The first whose `check` does not hold gives its `message` as the field's error. |
| `persist` | `global`, `per_model`, `profile`, `session` | Where the value is remembered. Default `global`. |
| `applies` | `on_start`, `spawn_only`, `live` | When a change takes effect: at the next start; at the next start, and only when QuickAI starts the engine itself; at once. Default `on_start`. |
| `bind` | table, below | How the value reaches the engine. |
| `derive` | table of conditions | Named facts for the rest of the program, from the field's value (`gpu_offload = "value > 0"`). |
| `ui` | table, below | Where and how the window shows it. |

## `[[field]]` `bind`

Turning a value into engine configuration. In an argument template, `${value}` is the field's value.

| Key | Value | Meaning |
|---|---|---|
| `port` | `public` | This field is the port the engine listens on, and the one clients use. |
| `arg` | list of templates | Arguments to add to the command line. |
| `arg_split` | bool | The value is a text of arguments, split like a shell does (quotes group words); nothing is added when it is empty. |
| `order` | whole number | Where among the field arguments these go. Default: the field's `ui.order`. |

## `[[field]]` `ui`

How the window shows a field.

| Key | Value | Meaning |
|---|---|---|
| `section` | text | Where it goes: `primary` (the row under the model), `advanced` (inside Settings), or a section of your own. Default `advanced`. |
| `order` | whole number | Its place in the section, lowest first; those without come last. |
| `widget` | text | A widget other than the type's usual one (`runtime_manager`). |
| `hint` | text | Greyed text shown in an empty text box. |

## `[acquire]`

Where builds of the engine come from and where they are kept. It configures the runtime manager in Settings.

| Key | Value | Meaning |
|---|---|---|
| `root` | template | The directory builds are kept in, one subdirectory each (`${data}/llama-bin`). Installing needs it. |
| `feed` | table, below | Where the list of builds comes from. |
| `assets` | table, below | Which file of a release is a build for this machine. |
| `install` | table, below | How a downloaded file becomes an installed build. |
| `backends` | list of tables, below | What a file name says about the GPU it was made for. |
| `filter_hint` | text | The placeholder of the box that narrows the list of builds. |

## `[acquire]` `feed`

`use = "github_release"`: the releases of a GitHub repository.

| Key | Value | Meaning |
|---|---|---|
| `use` | `github_release` | The kind of feed. |
| `repo` | `owner/name` | The repository. |
| `per_page` | 1 to 100 | How many releases to ask for. Default 10. |
| `show` | whole number | How many releases that have a build for this machine to show. Default 5. |
| `prereleases` | bool | Whether a release GitHub marks as a prerelease counts. Default false. |

## `[acquire]` `assets`, `by = "keywords"`

A file qualifies when it ends in one of `ext`, contains none of the `exclude` words, and contains a word for this machine's OS and one for its architecture. Matching ignores case.

| Key | Value | Meaning |
|---|---|---|
| `by` | `keywords` | The kind of rule. |
| `ext` | list of text | The endings a build can have, each with its dot (`.zip`, `.tar.gz`). Required. |
| `exclude` | list of text | Words that rule a file out (`cudart`). |
| `os` | table of lists | `linux`, `windows`, `macos` to the words that mean it in a file name. |
| `arch` | table of lists | `x86_64`, `aarch64` to the words that mean it. |
| `label` | template | The directory a build is installed under, over `tag`, `stem` and `asset`. Default `${tag}-${stem}`. |

## `[acquire]` `assets`, `by = "names"`

The file is chosen by exact name: the first rule whose condition holds names it.

| Key | Value | Meaning |
|---|---|---|
| `by` | `names` | The kind of rule. |
| `rules` | list of `{ when, name }` | `when` is a condition over the context (`os == 'linux' and arch == 'x86_64'`), `name` the file. |
| `label` | template | As above. |

## `[acquire]` `install`

`use = "archive"`: unpack a `.zip` or `.tar.gz` and find the executable inside.

| Key | Value | Meaning |
|---|---|---|
| `use` | `archive` | The kind of install. |
| `binary` | `{ name, search_depth }` | The executable's file name without a platform suffix, and how many directory levels below the build's directory to look for it (default 4, at most 16). |
| `chmod` | bool | Mark the executable as executable (Unix). |

## `[acquire]` `backends`

Each entry names a GPU backend by a word in a file name. The first entry whose word is in the name wins.

| Key | Value | Meaning |
|---|---|---|
| `match` | text | The word to look for. |
| `label` | text | What the interface calls the backend (`Vulkan`). |
| `needs_gpu` | `nvidia`, `amd`, `intel`, `apple` | The vendor a build for this backend needs; choosing one on a machine without it shows a warning. |

## Field types

| `type` | Value | Drawn as |
|---|---|---|
| `int` | whole number | a drag box |
| `float` | number | a drag box |
| `bool` | true or false | a checkbox |
| `text` | text | a one-line box |
| `choice` | text, one of `options` | a drop-down |
| `port` | whole number, 1 to 65535 | a drag box |
| `dir` | text, a directory | a one-line box |
| `file` | text, a file | a one-line box |
| `url` | text, an address | a one-line box |
| `runtime_path` | text, a file or a download address | the runtime manager when `ui.widget` says so, else a one-line box |
| `kv_lines` | text, one `KEY=VALUE` per line | a one-line box (a multi-line one is not built yet) |
| `model_ref` | text, a model | a one-line box |
| `hidden` | any | not drawn: the value is only stored |

## The context

Conditions, defaults and templates are written in one small language (docs/architecture.md, section 7) and see these names. A name that is not here is an error when the document loads, with a suggestion when one is close. Inside a field's own bindings, rules and conditions `value` is that field's current value; `field.<id>` is the value of any field.

| Name | Type |
|---|---|
| `app` | any value |
| `arch` | a string |
| `config` | a string |
| `data` | a string |
| `exe_dir` | a string |
| `gpu.known` | a bool |
| `gpu.vendors` | a list of string |
| `home` | a string |
| `listen_port` | an int |
| `model.components` | any value |
| `model.display` | a string |
| `model.id` | a string |
| `model.main.path` | a string |
| `model.modalities.input` | a list of string |
| `model.modalities.output` | a list of string |
| `model.ref` | a string |
| `model.size_bytes` | an int |
| `os` | a string |
| `public_port` | an int |
| `runtime.binary` | a string |
| `runtime.build` | an int |
| `runtime.dir` | a string |
| `runtime.flags` | a list of string |
| `runtime.version` | a string |
| `session.crashed` | a bool |
| `session.model_display` | a string |
| `session.model_ref` | a string |
| `session.running` | a bool |
| `sys.cpus` | an int |
| `sys.ram_bytes` | an int |
| `field.<id>` | the field's own type |
| `value` | the field's own type |
