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
| `egress` | list of text | The hosts the engine itself contacts, which the privacy table lists; a name that begins `*.` stands for every host under it. |
| `order` | whole number | Where the engine stands in the engine selector, lowest first. Default 100; engines with the same number stand in the order of their ids. |
| `companion_of` | text | The id of the engine this program goes with, for a program that is not an engine to choose but something that is started beside one (Open WebUI beside Ollama). It is not in the engine selector and is never the engine in use; the settings of the engine it goes with offer it, it is started when that engine is ready and stopped with it, and it is a run of its own: when it stops unasked, the engine's run goes on. In every other way it is an engine's document: it is installed, located, started and given settings as an engine is, and keeps everything in a root of its own. Its templates may ask about the engine it goes with: `${parent.port}`, `${parent.url}`, `${parent.id}`. |
| `first_address_field` | text | For a companion: the id of a field of its own (of type `hidden`) that holds the address of the engine it goes with, as it was when the program was first started. A program that keeps the address it is first given in a database of its own and does not read it from its environment again goes on calling that one when the engine is somewhere else later. QuickAI writes the address down in this field at the first start, says in the log when a later start would give another, and says so in the notes of a configuration that would put the engine elsewhere. The field is the person's own and is never part of a configuration. |

## `[provides]`

What a running engine offers.

| Key | Value | Meaning |
|---|---|---|
| `apis` | list of text | The ids of the APIs it serves. |
| `web_ui` | `{ path, label }` | Its built-in web page, if it has one; the button that opens it carries `label`. |
| `accepts` | `{ formats, refs }` | What a model is for it: the file formats it loads (`gguf`), or the kinds of named reference it takes (`ollama`). |
| `model_name_matters` | bool | Whether a request naming another model loads that one. |
| `default_port` | whole number | The port it uses when nothing says otherwise. |
| `needs_model` | bool | Whether it has to be given a model to start (llama.cpp). Default true. An engine that says false runs on its own, and Start needs no model: the work a client sends it names the models it uses (ComfyUI). |

## `[locate]`

Finding the executable without touching the network. `strategies` is a list, tried in order, and the first that yields a path wins. Each entry has `use` and its own keys:

| Key | Value | Meaning |
|---|---|---|
| `strategies` | list of tables | `{ use = "setting", field }` takes the path a field holds; `{ use = "next_to_exe", file }` looks beside the application's executable; `{ use = "dev_tree", path }` looks below the source checkout the application was built in; `{ use = "installed", root, pick, binary }` takes the newest build QuickAI installed under `root` (a template such as `${data}/ollama`) and the executable at `binary` inside it, a table by system (`{ linux = "bin/tool", windows = "tool.exe" }`); `{ use = "path_search", name }` looks for a program of that name in the directories of the `PATH`; `{ use = "known_paths", linux, macos, windows }` looks in the places listed for this system, with `~` and `%NAME%` filled in from the environment. |

## `[runtime]`

Facts read from the program itself, once it is found.

| Key | Value | Meaning |
|---|---|---|
| `version` | table, below | How to learn its version. |
| `flags` | table, below | How to learn which command-line flags it supports (`runtime.has_flag`): read from its help text. Read since Phase 3 and used by the adapters of Phase 4. |

## `[runtime]` `flags`

Runs the program and finds the flags it supports in what it prints.

| Key | Value | Meaning |
|---|---|---|
| `exec` | list of text | The command: the executable (`${binary}`) and its arguments (`--help`). |
| `parse` | text | How the output is read (`help_flags`: the words that start with `--`). |
| `skip` | text | Lines that contain this are left out: the tombstones of flags that were removed. |

## `[runtime]` `version`

Runs the program and reads the version from what it prints. The answer is remembered per executable and modification time, so asking again does not start the program again.

| Key | Value | Meaning |
|---|---|---|
| `exec` | list of text | The command: the executable (`${binary}`) and its arguments (`--version`). |
| `regex` | regular expression | Its first group is the version; standard output is searched, then standard error. The exit status does not matter. |
| `as` | `version` | The value it becomes (`runtime.version`). |

## `[launch]`

How the engine is started.

| Key | Value | Meaning |
|---|---|---|
| `strategies` | list of text | The ways to bring it up, in order: `attach_if_running` (use one that already answers on its port, if it is the engine) and `spawn` (start one). |
| `strategy_field` | text | The id of a `choice` field whose options are `auto` (try the strategies in order), `attach` (only the first) and `spawn` (only the second). |
| `attach` | table, below | Using an engine that is already running. |
| `spawn` | table | The program to start, below. |
| `ready` | table | How to tell it is up, below. |
| `warm` | list of tables, below | Calls made once it answers, to load the model; they are left out when no model is selected, and the engine then runs with nothing loaded. |
| `unload` | list of tables, below | Calls made before it is stopped or left, to free the model. |

## `[launch.attach]`

An engine that may already be running (a person's own Ollama). A port that answers is used only if it answers like the engine.

| Key | Value | Meaning |
|---|---|---|
| `endpoint` | template | Where it would be: `127.0.0.1:${public_port}`, the port its settings give. |
| `identity` | `{ path, pointer }` | A request only this engine answers in a known way: the answer is JSON with a text at `pointer` (`/version`). Anything else on the port is something else using it. |
| `liveness` | `{ path, every, failures }` | How to notice that it has gone, since QuickAI has no process of its own to watch: `path` is asked every `every` (default `5s`), and `failures` misses in a row (default 3) mean it is gone. |

## `[[launch.warm]]` and `[[launch.unload]]`

One call of a phase.

| Key | Value | Meaning |
|---|---|---|
| `when` | condition | Make the call only while this holds (`model`: when a model is selected). |
| `call` | table, below | The request. |

## `call`

A request to the engine. Texts in `path` and `body` are templates; the body is JSON written as TOML.

| Key | Value | Meaning |
|---|---|---|
| `method` | text | `GET`, `POST`, `PUT`, `DELETE` or `PATCH`. Default `POST`. |
| `path` | template | A plain path on the engine, starting with `/`. |
| `body` | table | The JSON body: tables, lists, numbers, booleans and texts. |
| `timeout` | duration | How long it may take (`600s` for a warm-up, `5s` for an unload, by default). |
| `best_effort` | bool | A failure is logged and nothing is done about it (an unload that does not answer must not hold up a stop). Default false. |
| `stream` | `json`, `ndjson` or `sse` | How the reply is read: `json` (the default) is one body, the others a stream of events. Only the calls of `[models]` may stream; a warm-up or an unload is read whole. |

## `[launch.spawn]`

Starting the program. Every text is a template: `${...}` holes are filled from the context below.

| Key | Value | Meaning |
|---|---|---|
| `program` | template | The program to start (`${runtime.binary}`). |
| `args` | list of templates | Its first arguments, one each. The fields' `bind.arg` follow, by `order`. |
| `env` | list of tables | Environment variables to set, below. |
| `group` | bool | Start it in a process group of its own (a job on Windows), so that stopping it takes the processes it started with it, and, on Linux, so that it follows QuickAI if QuickAI is killed. Default false. |
| `cwd` | template | The folder the program starts in; it is made, with the ones above it, if it is not there. Without it the program starts wherever QuickAI was started from, which is the person's, and a program that writes a file to its working directory (Open WebUI keeps its secret key there) writes it there. |
| `mkdir` | list of templates | Folders made, with the ones above them, before the program starts: for a program that refuses a folder it is told about that does not exist yet (ComfyUI's `--models-directory`). |
| `write` | list of tables | Files of the application's own that are put where the program is told to read them, before it starts, below. |
| `stop` | table | How to stop it: `{ policy = "kill" }` (the default: at once), or `{ policy = "graceful", grace = "10s", exit_grace = "2s" }`: ask it to leave (SIGTERM to its group; Windows has no such signal), wait `grace` (`exit_grace` while the application is closing), then kill what is left. |

## `[[launch.spawn.write]]`

A file or a folder that is part of the application, put on the disk before the program starts, at every start: what is there already and the same is left alone, what differs is written again (a file that is not in the application is not removed). For a program that reads extensions from a folder it is told about (ComfyUI's custom nodes, with `--extra-model-paths-config`), so that every build has them and a person's own files are not touched. A document of the person's own can only name what the application has.

| Key | Value | Meaning |
|---|---|---|
| `asset` | text | The name of the file or folder among the application's own files, with `/` between the parts (`comfyui/quickai_bridge`). |
| `to` | template | Where it is put (`${data}/comfyui/extensions/quickai_bridge`). The folders above it are made. |

## `[[launch.spawn.env]]` and `[[field]]` `bind.env`

One environment variable: `value` or `prepend`, not both. In `[[launch.spawn.env]]` the document sets it for every start; in a field's `bind.env` the field's value does (`${value}`), after the document's own variables, in the order of the field's other bindings, and only while the field has a value.

| Key | Value | Meaning |
|---|---|---|
| `name` | text | The variable. |
| `value` | template | Set it to this. |
| `prepend` | template | Put this in front of what the variable already holds, joined with `sep`. |
| `sep` | text | The joiner for `prepend`. Default `:`. |
| `when` | condition | Only while this holds. |
| `locked` | bool | The person's own lines may not override it (`[[launch.spawn.env]]` only: what a field decides is theirs to change). |

## `[launch.ready]`

Waiting for the engine to answer.

| Key | Value | Meaning |
|---|---|---|
| `probe` | `{ use = "http", path, expect }` | A request to make; `expect` is the status that means ready (default 200). |
| `timeout` | duration | How long to wait before giving up (`120s`). |
| `interval` | duration | How often to ask (`500ms`). |
| `on_timeout` | `fail` or `wait` | What the timeout means. `fail` (the default): say so, stop what was started and end in Crashed. `wait`: say so and go on waiting, because the program may still be loading; only its exit or a Stop ends the wait. |

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
| `default` | value or `{ fn, args }` | The value when nothing was chosen: a literal, a text with `${...}` holes filled in from the engine's context (`${data}/ollama/models`), or a named function over context values (`recommend_ctx_size`). |
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
| `share` | `yes`, `review`, `never` | Whether the value travels in a configuration, a saved and shareable runtime. `yes`: it does. `review`: it does, and a person who is given a file that has it is shown it and asked (it becomes a command-line word, an environment line or an address, or it widens what the program may do). `never`: it belongs to a computer or a person and is never in one. Without it the type decides: `int`, `float`, `bool`, `choice` and `port` are `yes`; `text`, `url` and `kv_lines` are `review`; `dir`, `file` and `runtime_path` are `never`; `hidden` and `model_ref` are not settings (the model has a section of its own). A field added later is private until its document says otherwise. |
| `share_note` | text | The sentence a person is told when this field is asked about or left out of a configuration (`Open WebUI may use the internet`). Only for a field that is not `yes`. |
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
| `arg_by_value` | table of lists of text | For a `choice` field: the arguments each option adds, as plain words (`low = ["--lowvram"]`); an option that is not listed adds none. |
| `order` | whole number | Where among the field arguments these go. Default: the field's `ui.order`. |
| `env` | list of tables, below | Environment variables of the engine that this field decides, each with an optional condition on `value`. |
| `env_lines` | table, below | For a `kv_lines` field: its lines are environment variables of the engine. |

## `[[field]]` `bind.env_lines`

What a field of `KEY=VALUE` lines may set.

| Key | Value | Meaning |
|---|---|---|
| `protect` | list of text | Names the person's lines may not set (`OLLAMA_MODELS`), besides every variable the document locks or adds to (`prepend`): the document's value stands. A name is protected in any spelling. |

## `[[field]]` `ui`

How the window shows a field.

| Key | Value | Meaning |
|---|---|---|
| `section` | text | Where it goes: `primary` (the row under the model), `advanced` (inside Settings), or a section of your own. Default `advanced`. |
| `order` | whole number | Its place in the section, lowest first; those without come last. |
| `widget` | text | A widget other than the type's usual one: `runtime_manager` (for a `runtime_path`) or `button` (a field that holds nothing, usually of type `hidden`: it is drawn as a button with the field's `label`, pressed only while `enabled_when` holds). Another name is an error when the document loads. |
| `hint` | text | Greyed text shown in an empty text box. A number with a hint is a box that can be empty (the hint says what empty means), not a drag box. |
| `action` | text | What a click on a `button` makes the application do, as `verb` or `verb:what` (`open:settings`). A button needs one and nothing else has one. |

## `[models]`

For an engine that keeps its own models, named references it lists, pulls and removes (Ollama): each model is a name, not a file. An engine without this section has files in a folder.

| Key | Value | Meaning |
|---|---|---|
| `needs_server` | bool | The calls need the engine running. Stopped, only what it listed last time can be shown, and nothing is asked or started until the person presses a button. Default false. |
| `list` | table, below | The call that lists the models, and where each one is in the reply. |
| `pull` | table, below | The call that fetches a model by name, with its progress. |
| `remove` | table, below | The call that removes a model. |
| `capabilities` | table, below | The call that asks what one model can do. |
| `input_rules` | list of tables, below | What the model box makes of what was typed: the first rule whose `when` holds. Nothing typed is nothing; text no rule takes is refused. |
| `sources` | list of tables, below | For an engine whose models are files: where they are kept (read by Phase 4). |
| `acquire` | table, below | For an engine whose models are files: how one gets into the folder (read by Phase 4). |
| `lookup` | list of tables, below | The ways the window can look models up online: a button of the model box for each. Without one the window has none. |
| `bundle` | list of tables, below | What is fetched together with a model's weights: a vision model's projector. A result that has a file for every part of a bundle is offered in it, as one download. |
| `mode` | list of tables, below | The ways the engine is started for a kind of model (`embedding`). A catalog names one by its `id`, and the record beside a fetched model remembers it. |

## `[[models.sources]]`

A place models are found. Read from Phase 4; until then an engine with files keeps what the window has always done.

| Key | Value | Meaning |
|---|---|---|
| `use` | text | `bundle_dir` (a folder shipped with the application) or `scan_dir` (one the person can write to). Required. |
| `path` | template | The folder (`${exe_dir}/models`, `${field.models_dir}`). Required. |
| `ext` | list of text | The file endings that are models (`.gguf`). |
| `writable` | bool | Whether downloads and imports may go here. Default false. |

## `[models]` `acquire`

How a model gets into the folder.

| Key | Value | Meaning |
|---|---|---|
| `use` | text | `file`: it is downloaded or linked as a file. Required. |
| `dir` | template | Where it goes. |
| `place` | text | For an engine that finds its files by the folder they are in (ComfyUI): the folder of `dir` the weights of a model go in (`diffusion_models`), a plain name. The recommended list may say another for a model, and `[components.<kind>] place` says where each kind of companion goes. |
| `places` | list of text | The other folders of `dir` a file can be put in by hand, plain names (`checkpoints`, `controlnet`): what the window offers a person who gives a link or a file of their own and says where it goes, besides `place` and the folder of each `[components.<kind>]`. |

## `[[models.lookup]]`

One way of looking models up online: the primitive that does it and what it is told. The window draws a "Look up online" button for it; nothing is asked of the network until the button is pressed. Every key besides `id` and `use` is the primitive's own and is checked by it: one the look-up does not take is a warning, and the list of tables merges by `id`, so a document of the person's can change the address of a look-up and leave the rest.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Its name, with no spaces (`library`), unique in the document. Required. |
| `use` | text | The look-up: `ollama_library` (needs `lookup.ollama_library@1`) or `huggingface_gguf` (needs `lookup.huggingface_gguf@1`). Another name is an error that lists the ones this version has. Required. |

## `[[models.lookup]]` `use = "ollama_library"`

A search of Ollama's library of models, the one at ollama.com, for models that run on this machine. The site publishes no list a program can read, so the look-up reads the page of the site's own search, and says so when the page is not one it understands. It finds the library's own models (a person's uploads are left out) with the sizes each is offered in; a size is pulled as `name:size`, a model with no size shown by its name. The library gives no size in bytes, so a button says how big a size is by its count of parameters times `bytes_per_parameter`, with a `~`.

| Key | Value | Meaning |
|---|---|---|
| `site` | address | The root of the site (`https://ollama.com`, the default). Another root, `http` or `https`, is how a test points the look-up at a fake, or how a mirror is used. |
| `bytes_per_parameter` | number | Bytes a parameter takes in what a name pulls, to tell how big a size is (0.05 to 4; default 0.6, about 4.8 bits, which is what most of the library is quantized to). |
| `show_variants` | whole number | The most size buttons a model gets (1 to 64; default 16), the smallest first. |
| `types` | list of text | The ids of the model types this engine runs (`chat`, `vision`); a model of another type is shown under `other`. Without it every type is run. |

## `[[models.lookup]]` `use = "huggingface_gguf"`

A search of the GGUF repositories of Hugging Face, the most downloaded first, for the words typed or for none. For each repository it looks into, it reads the files and their sizes, and offers the quantizations of the weights as buttons that download them; a file that a `[[models.bundle]]` part matches is not weights but a companion of them. A repository that needs a login is left out and counted in the status line.

| Key | Value | Meaning |
|---|---|---|
| `site` | address | The root of the hub (`https://huggingface.co`, the default); files are fetched from `<site>/<repository>/resolve/main/<path>`. Another root is how a test points the look-up at a fake. |
| `popular` | whole number | How many repositories of the plain list it looks into (1 to 50; default 30). |
| `keyword` | whole number | How many it looks into for the words typed (1 to 50; default 20). |
| `similar` | whole number | How many it looks into for each single word of a phrase typed, to find what the whole phrase misses (1 to 50; default 20). |
| `pipelines` | list of text | The pipelines of the hub (`text-to-video`, `text-to-image`: lower case letters, digits and `-`; at most eight) the plain list is made of: one search for each, `popular` of the repositories of each looked into. For an engine that runs one kind of model, whose plain list would otherwise be the most downloaded GGUF repositories of any kind, which are language models. Words typed are one search that no pipeline narrows. Default none. |
| `words` | list of text | Words the plain list is also made of: one search for each (at most four; `encoder`), `popular` of the repositories of each looked into, for what no pipeline tag finds (a text encoder has none). A search is a few words with single spaces, up to 100 characters, and no `:`. Default none. |
| `alone` | list of text | Kinds of file (the `kind` of a `[[models.bundle]]` part: `vae`, `text_encoder`) that are offered by themselves as well as with a model: each file of the repository that a part of that kind finds gets a button in a row of its own (`Get VAE only:`), the ones `prefer` likes and then the smallest, and is kept under its own name in the folder of its kind (`[components.<kind>] place`). A repository that holds only such files is offered by them. For an engine whose workflows name their files one by one. A kind no part finds is an error. Default none: a companion is fetched with the weights it goes with. |
| `prefer` | list of text | The quantizations to offer first, best first (default `Q4_K_M`, `Q4_K_S`, `Q5_K_M`, `Q4_0`, `Q8_0`, `Q6_K`, `Q3_K_M`). A variant of one (`UD-Q4_K_M`) counts as it; the best is shown in bold. |
| `skip` | list of text | A file whose name or path one of these patterns matches (`*` any run, `?` one character, any case) is not weights: shards (`*-?????-of-?????*`: five digits each side of `-of-`), draft models (`mtp/*`, `mtp-*`), importance matrices (`imatrix*`). Default those. A pattern is about the file, not about the repository: a model whose own name says `MTP` is still offered. |
| `show_variants` | whole number | The most buttons a model gets (1 to 64; default 4): the ones `prefer` likes, then the smallest, shown smallest first. |
| `types` | list of text | The ids of the model types this engine runs (`chat`, `code`, `vision`, `embedding`); a model of another type is shown under `other`. Without it every type is run. |

## `[[models.bundle]]`

A way to fetch a model with the files that go with it: the weights of a result and one file for each part. It is offered as another row of buttons on every result that has a file for every part, and fetches them as one download; the files are kept as the pairing says (docs/lookup-and-bundles.md, section 4), and what the engine does with them is `[components.<kind>]`. The list merges by `id`, so a document of the person's can change a bundle or add another.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Its name, with no spaces (`vision`), unique in the document. Required. |
| `label` | text | The words of its row (`with projector` makes `Get with projector:`); default the id. |
| `help` | text | A sentence for the person who wonders what it is. |
| `parts` | list of tables, below | The files beside the weights, one kind each. A bundle with none is an error. Required. |

## `[[models.bundle]]` `parts`

One part of a bundle: a kind of file and how to find it among the files of a repository.

| Key | Value | Meaning |
|---|---|---|
| `kind` | text | The kind of component (`mmproj`). The engine must have a `[components.<kind>]` for it, else the document is refused and says which kinds it has. Required. |
| `label` | text | What the window calls it in a sentence (`the projector mmproj-F16.gguf`); default the kind. |
| `match` | list of text | Patterns a file's name or path matches (`mmproj*.gguf`, `*mmproj*`): `*` stands for any run of characters, `?` for one, case is ignored. A file that matches is a companion and is not offered as a model. Required. |
| `prefer` | list of text | Of several files that match, the quantizations to take first, best first (`F16`, `BF16`); else the smallest. |

## `[[models.mode]]`

A way the engine is started for a model: the words it adds to the command line, after the arguments of the fields and of the model's files and before the person's own extra arguments. A catalog (which is data) and the record beside a fetched model name a mode by its `id` and never carry words of their own, so what reaches the command line is always the engine document's. A mode a model names that the engine does not have adds nothing, and the log says so. The list merges by `id`.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Its name: lower case letters, digits and `.`, `_`, `-` (`embedding`), unique in the document. Required. |
| `label` | text | What the model picker says of a model that runs so (`embedding server`); default the id. |
| `help` | text | A sentence for the person who wonders why. |
| `arg` | list of text | The words added, as they are (no templates); none is an error. Required. |

## `[components.<kind>]`

How one kind of companion file reaches the program once a model that has one is started: the arguments it adds to the command line, after the fields of a lower `order` and before the person's own extra arguments. A model has a companion of a kind when the pairing file beside it names one that is still there. The rest of Phase 4's design for a component (`requires_flag`, `fields`, `live`, `bind.each`) is not read yet.

| Key | Value | Meaning |
|---|---|---|
| `order` | whole number | Where its arguments stand among the fields': after those of a lower `order`, before those of a higher one. Default 80, which is before `extra_args` (90). |
| `bind` | table, below | What it adds. |
| `place` | text | For an engine that finds its files by the folder they are in (ComfyUI): the folder of the models folder this kind of file is kept in (`text_encoders`), a plain name. A component needs `place` or `bind.arg`, and may have both. |

## `[components.<kind>.bind]`

The arguments a companion adds.

| Key | Value | Meaning |
|---|---|---|
| `arg` | list of templates | The arguments, one entry each, over `${component.path}` (the file): `["--mmproj", "${component.path}"]`. Required unless the component has a `place`. |

## `[models]` `list`

The names of the models the engine has, in the order it gives them.

| Key | Value | Meaning |
|---|---|---|
| `call` | table | The request (`GET /api/tags`); `${data}` and the other names of the engine's context are available in its path. |
| `items` | pointer | The array of models in the reply (`/models`). Required. |
| `map` | table, below | Where the parts of one model are. Required. |
| `cache` | template | Where the last reply is kept (`${data}/ollama/catalog-cache.json`), written whole or not at all, so that a list can be shown while the engine is not running. |
| `hide` | list of patterns | Names the engine lists that are its own bookkeeping, not models to pick (`["llamacpp:.*"]`): each pattern must match the whole name. The picker, the recommendation and the cache never see them. A name the engine lists twice is shown once, whatever this says. |

## `[models]` `list.map`

JSON pointers into one model of the list.

| Key | Value | Meaning |
|---|---|---|
| `id` | pointer | The name the engine knows it by, which is what a request names (`/name`). Required; a model with none is skipped, with a word in the log. |
| `display` | pointer | What the picker shows. Default: the name. |
| `size_bytes` | pointer | How big it is: what the context size and the speed estimate are worked out from. |
| `digest` | pointer | Its checksum. |
| `capabilities` | pointer | A list of what it can do (`completion`, `vision`). |

## `[models]` `pull`

Fetching a model by name. A pull is the one time the engine reaches the internet, and only on a click.

| Key | Value | Meaning |
|---|---|---|
| `call` | table | The request, with `${ref}` the name to fetch (`body = { model = "${ref}" }`) and `stream = "ndjson"` when the engine reports progress as it goes. |
| `progress` | table, below | Where the progress is in the events of the stream; without it the usual Ollama shape is assumed. |

## `[models]` `pull.progress`

JSON pointers into one event of a pull. A pull adds layers up as the engine announces them, so a layer is told apart by `key`; the bar never goes back, and stays short of full until the engine says it is done.

| Key | Value | Meaning |
|---|---|---|
| `key` | pointer | What tells one layer from another (`/digest`). |
| `completed` | pointer | Bytes of the layer done (`/completed`). |
| `total` | pointer | Bytes the layer needs (`/total`). |
| `status` | pointer | The engine's own words (`/status`), for the log. |
| `error` | pointer | Where an event says the pull failed, whatever the HTTP status was. Default `/error`. |
| `done` | text | The `status` that ends the pull. Default `success`. |

## `[models]` `remove`

Removing a model, once the person has confirmed it. The model that is loaded is never removed: the person stops the server first.

| Key | Value | Meaning |
|---|---|---|
| `call` | table | The request, with `${ref}` the name as the engine lists it (`DELETE /api/delete`). |

## `[models]` `capabilities`

What one model can take as input, asked of the engine.

| Key | Value | Meaning |
|---|---|---|
| `call` | table | The request, with `${ref}` the model. |
| `pointer` | pointer | Where the list of what it can do is in the reply (`/capabilities`). Required. |

## `[[models.input_rules]]`

One rule of the model box. The text is trimmed first; a rule with no `when` takes whatever is left, and a rule after it is never reached (a warning).

| Key | Value | Meaning |
|---|---|---|
| `when` | condition | A condition over `input`, with `matches(input, "regex")`, `starts_with`, `is_url(input)`, `is_file(input)` (whether it names a file on this machine) and `path_after_host(input)`. |
| `do` | `pull` or `reject` | What to do with it. Required. |
| `ref` | template | For `pull`: the name to fetch, over `input` (`${input}`, or `hf.co/${path_after_host(input)}` for a link to a page). Required for `pull`. |
| `message` | text | For `reject`: what the person is told. Required for `reject`. |
| `provider` | text | For `search` (read by Phase 4): the lookup that answers it. |

## `[traffic]`

Which of the engine's APIs the Live tab reads.

| Key | Value | Meaning |
|---|---|---|
| `apis` | list of text | Ids from a vocabulary; each must be defined there, and one with a `decode` is read. Without this table every API in `[provides]` `apis` is. |

## `[api_info]`

What the 🔌 API panel says to a person who connects an app. Every text is a template over the engine's context (`${public_port}`, `${session.model_display}`).

| Key | Value | Meaning |
|---|---|---|
| `base_url` | template | The address of the OpenAI-compatible API (`http://127.0.0.1:${public_port}/v1`). Required. |
| `extra_urls` | list of tables, below | More addresses worth showing: the engine's own API. |
| `api_key` | `{ value, note }` | What to put in the key box and a note under it. |
| `model` | `{ value, note }` | The name a client should give for the model and a note under it. With no model selected the panel shows `local-model`. |
| `example` | text | The id of the API whose curl example is shown (`openai.chat`): the first example of the vocabulary whose `when` holds. |

## `[[api_info.extra_urls]]`

One more address.

| Key | Value | Meaning |
|---|---|---|
| `label` | text | What it is (`Ollama API`). Required. |
| `url` | template | The address. Required. |

## `[[diagnostic]]`

Advice for a run that went wrong. It is shown under the log when every condition of `when` holds.

| Key | Value | Meaning |
|---|---|---|
| `when` | table, below | What has to be so. Required. |
| `hint` | template | What to tell the person, over the engine's context (`${field.port}`). Required. |
| `action` | text | A button under the hint, as `verb` or `verb:what` (`focus_field:port`). |

## `[[diagnostic]]` `when`

Both keys may be given; then both must hold. One at least is needed.

| Key | Value | Meaning |
|---|---|---|
| `log_matches` | regular expression | Looked for in the lines of the log; a pattern that does not compile is an error when the document loads. |
| `exited_during` | `starting` or `running` | The run ended while it was in this state. |

## `[[prerequisite]]`

Something the computer has to have for a configuration of the engine to work, how to tell whether it has it, and how to put it right. A configuration says which of them it needs (`[[needs]] system = "<id>"`); the packages are named here and nowhere else, because a package that is installed as the administrator must not come from a file that was given away. `quickai-run` looks at the computer before it fetches anything, says what is missing and the exact command that brings it, and installs the packages only when it is told to (`--install-system-packages`).

| Key | Value | Meaning |
|---|---|---|
| `id` | text | What a configuration calls it. Letters, digits and `_`. Required. |
| `label` | text | What a sentence calls it (`a C compiler`), up to 120 characters. Required. |
| `when` | condition | Only on a computer where this holds (`os == 'linux'`); always when left out. |
| `check` | table, below | How to tell. Required. |
| `fix` | table, below | The packages that bring it, by package manager. Left out, the program only says what is missing, and `hint` what to do. |
| `hint` | text | What to tell the person when nothing here can put it right, or when it did not help, up to 600 characters. |

## `[[prerequisite]]` `check`, `use = "program"`

One of the programs is on the computer, in a folder of the `PATH`, or the one a variable names is.

| Key | Value | Meaning |
|---|---|---|
| `use` | `program` | The kind of check. |
| `any_of` | list of text | Names of programs, any of which will do (`["gcc", "clang"]`). Plain names, with no path. |
| `env` | text | A variable that names the program instead (`CC`), as the tool that needs it reads it. When it is set and not empty it decides: the program it names must be there, and the packages of `fix` are not offered. |

## `[[prerequisite]]` `check`, `use = "python_header"`

A header file of Python is in the folder of Python's headers, for the Python the engine's environment is made with (the one the install will use, or the one the installed environment was made with). Only for an engine installed by `python_app`. The packages of `fix` are offered only when that Python is the system's.

| Key | Value | Meaning |
|---|---|---|
| `use` | `python_header` | The kind of check. |
| `file` | text | The file, by its name alone (`Python.h`). |

## `[[prerequisite]]` `check`, `use = "python_module"`

A module of Python's own library can be imported by the Python the engine's environment is made with: `ensurepip`, which Debian and Ubuntu leave out of the Python and put in the package `python3.12-venv`, without which no environment can be made. Only for an engine installed by `python_app`; made before the install with the Python the install will use, and an environment that exists was made, so it passes. The packages of `fix` are offered only when that Python is the system's.

| Key | Value | Meaning |
|---|---|---|
| `use` | `python_module` | The kind of check. |
| `module` | text | The module, as Python names it (`ensurepip`). |

## `[[prerequisite]]` `check`, `use = "nvidia_driver"`

The driver of an NVIDIA card is of this version or a newer, read from the driver's own file or from `nvidia-smi`. A driver is not installed by this program: say what to do in `hint`.

| Key | Value | Meaning |
|---|---|---|
| `use` | `nvidia_driver` | The kind of check. |
| `at_least` | text | The version, as numbers with dots between them (`580`, `580.65`), compared number by number. |

## `[[prerequisite]]` `check`, `use = "python_script"`

A script of the application's own files is run by the Python of the engine's environment, with the environment the engine is started with, and passes when it ends with status 0. What it prints is shown when it fails. It tests that the installed environment works, so it is run after the install and before the engine is started. Only for an engine installed by `python_app`.

| Key | Value | Meaning |
|---|---|---|
| `use` | `python_script` | The kind of check. |
| `asset` | text | The script, as a name of the application's own files, ending in `.py` (`comfyui/selftest/triton_kernel.py`). |
| `timeout` | duration | How long it may take (default `120s`). |

## `[[prerequisite]]` `fix`

The packages that bring a prerequisite, for each package manager that has them. Plain package names only. `${python.version}` (`3.12`) and `${python.digits}` (`312`) stand for the Python of the environment. Only for `program`, `python_module` and `python_header`.

| Key | Value | Meaning |
|---|---|---|
| `apt` | list of text | Debian and Ubuntu, installed with `apt-get` (its list of packages is refreshed first). |
| `dnf` | list of text | Fedora and Red Hat, installed with `dnf`. |
| `zypper` | list of text | openSUSE, installed with `zypper`. |
| `pacman` | list of text | Arch, installed with `pacman`. |
| `apk` | list of text | Alpine, installed with `apk`. |

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
| `tested` | `{ tag }` | The release this document was tested with: it is offered first, marked, and `pins` are for it. |
| `overlays` | list of tables, below | Archives unpacked over a build on request, by platform (ROCm's libraries): never a build of their own, and off until chosen. |
| `pins` | table of text | The sha256 of each file of the tested release by file name, as `sha256:` and 64 hexadecimal digits. A download of that release must have it, and must agree with the digest the host lists. |

## `[acquire]` `feed`

`use = "github_release"`: the releases of a GitHub repository.

| Key | Value | Meaning |
|---|---|---|
| `use` | `github_release` | The kind of feed. |
| `repo` | `owner/name` | The repository. |
| `per_page` | 1 to 100 | How many releases to ask for. Default 10. |
| `show` | whole number | How many releases that have a build for this machine to show. Default 5. |
| `prereleases` | bool | Whether a release GitHub marks as a prerelease counts. Default false. |
| `api` | address | The root of the API (`https://api.github.com`, the default). Another root, `http` or `https`, is how a test points the feed at a fake; only documents QuickAI trusts may name one. |
| `digest` | bool | The feed lists a sha256 for each file (GitHub's `digest`) and a build carries it, for the installer to check. Default false. |
| `archive` | address | Where the source archive of a release is fetched from, for `assets` of `by = "source"`: `<archive>/<owner>/<name>/archive/refs/tags/<tag>.tar.gz`. Default `https://github.com`; as with `api`, only documents QuickAI trusts may name another. |

## `[acquire]` `feed`, `use = "pypi"`

The releases of a package of the Python Package Index, for an application that is installed from its wheel (`assets` of `by = "wheel"`). The index lists a sha256 for every file, so every build carries one.

| Key | Value | Meaning |
|---|---|---|
| `use` | `pypi` | The kind of feed. |
| `package` | text | The package, as the index names it (`open-webui`). |
| `show` | whole number | How many releases that have a wheel to show. Default 5. |
| `prereleases` | bool | Whether a release the index calls a pre-release (`1.0rc1`) counts. Default false. |
| `api` | address | The root of the index's JSON API (`https://pypi.org`, the default). Another root, `http` or `https`, is how a test points the feed at a fake; only documents QuickAI trusts may name one. |

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

## `[acquire]` `assets`, `by = "source"`

The release's own source archive is the build, for an application that is installed from its source (`install` of `use = "python_app"`). Every release has one, on every platform; its digest is not listed by the host, so only the pin of the tested release can check it.

| Key | Value | Meaning |
|---|---|---|
| `by` | `source` | The kind of rule. |
| `label` | template | The directory a build is installed under, over `tag`, `stem` and `asset`. Default `${tag}`. |

## `[acquire]` `assets`, `by = "wheel"`

The release's wheel is the build, for an application that is installed from the package index (`feed` of `use = "pypi"`, `install` of `use = "python_app"` with a `console_script`): the one file of the release that is a wheel for every platform (`py3-none-any`). It is downloaded, checked against its sha256, and installed into an environment with what it needs.

| Key | Value | Meaning |
|---|---|---|
| `by` | `wheel` | The kind of rule. |
| `label` | template | The directory a build is installed under, over `tag`, `stem` and `asset`. Default `${tag}`. |

## `[acquire]` `install`

`use = "archive"`: unpack a `.zip`, `.tar.gz` or `.tar.zst`, safely, into a directory of its own that is renamed into place when it is whole, and find the executable inside.

| Key | Value | Meaning |
|---|---|---|
| `use` | `archive` | The kind of install. |
| `binary` | `{ name, search_depth }` | The executable's file name without a platform suffix, and how many directory levels below the build's directory to look for it (default 4, at most 16). |
| `chmod` | bool | Mark the executable as executable (Unix). |
| `verify` | `none` or `digest` | `digest`: compare the download with the sha256 the host lists for the file and the one this document pins before anything is unpacked; a build with neither is installed and said to be unchecked. Default `none`. |
| `smoke` | list of text | A command that must succeed on the unpacked build before it counts as installed, starting with the executable: `["${binary}", "--version"]`. |
| `skip` | list of `{ when, glob }` | Entries of the archive to leave out: `glob` is a path inside the build (`lib/ollama/cuda_*`, `*` for a run of characters inside one name), `when` a condition on this machine. What is below a directory that matches goes with it. |

## `[acquire]` `install`, `use = "python_app"`

Install an application written in Python from its source: the source archive is checked and unpacked as `app/`, a Python makes a virtual environment `venv/` beside it, the `pip` steps that apply to this machine install into it (only as wheels: no package runs a build script on the person's machine), the `nodes` are unpacked into the source, and a launcher is written that runs the application with that environment. The launcher is the build's executable. Everything is made in a directory of its own that is renamed into place when it is whole, and can be stopped at any moment.

| Key | Value | Meaning |
|---|---|---|
| `use` | `python_app` | The kind of install. |
| `binary` | `{ name }` | The launcher the installer writes (`comfyui`; `comfyui.cmd` on Windows). |
| `entry` | text | The script of the unpacked source that the launcher runs, relative to it (`main.py`). One of `entry` and `console_script` is needed. |
| `console_script` | text | For a build that is a wheel, which has no source to run a script of: the program the wheel puts in the environment (`open-webui`). The installer asks the environment which function it calls, and the launcher has the environment's Python call it; the script itself is never run, because it holds the path of the folder it was made in, and a build is moved to its name when it is whole. |
| `python` | `{ versions }` | The versions of Python to look for, best first (`["3.13", "3.12"]`): on the `PATH`, in pyenv's and uv's folders, and through the Windows launcher. The person may name a Python of their own. |
| `verify` | `none` or `digest` | `digest`: compare the source archive with the sha256 this document pins for the tested release before anything is unpacked; another release is installed and said to be unchecked. Default `none`. |
| `smoke` | list of text | A command that must succeed on the finished build before it counts as installed, starting with the launcher: `["${binary}", "--quick-test-for-ci"]`. |
| `smoke_timeout` | duration | How long the smoke test may take (default `120s`). |
| `space` | size | How much free room an install needs (`30GiB`): the download and the environment together. Checked before anything is asked of the network. |
| `pip` | list of tables, below | The packages of the environment, in the order they are installed. |
| `nodes` | list of tables, below | Packs of extra nodes, unpacked into the source. |
| `options` | list of tables, below | Parts of the install that are the person's to choose: a box in the window, ticked or not, and the `pip` steps that belong to it. |

## `[[acquire.install.options]]`

One part of the install that the person chooses (ComfyUI-Manager), and what the build made with it does differently. Its steps are the `pip` steps that name it; the window offers it as a box, above the list of builds, and what was chosen is what the install does, what the line under the bar counts and what an install that stopped is gone on with. A caller that chooses nothing (the headless runner) gets the install without it.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Letters, digits and `_`; the `pip` steps that belong to it name it with `option`. |
| `label` | text | What the box says (`Install ComfyUI-Manager`). |
| `badge` | text | A few words beside the label, in the colour of a recommendation (`highly recommended`). |
| `help` | text | What the pointer on the box is told. |
| `default` | bool | Ticked until the person unticks it. Default false: a part nobody asked for is not installed. |
| `launch_args` | list of text | What the launcher of a build made with it passes to the program before the arguments it is given, one plain word each (`--enable-manager-legacy-ui`): the build is the one that knows what it has, so a build made without it is started without it. |

## `[[acquire.install.pip]]`

One `pip install`.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Letters, digits and `_`; a document of the person's replaces a step by it. |
| `label` | text | What the log and the window call it (`PyTorch for AMD GPUs (ROCm 7.2)`). |
| `when` | condition | Only on a machine where this holds (`has(gpu.vendors, 'amd')`); always when left out. |
| `option` | text | Only when the person has chosen this option of the install (the `id` of an `[[acquire.install.options]]`); always when left out. |
| `packages` | list of text | Packages, as requirement specifiers (`torch`, `torchvision==0.29.1`). |
| `requirements` | text | A requirements file of the unpacked source, relative to it (`requirements.txt`). |
| `wheel` | bool | Installs the wheel the build is (`assets` of `by = "wheel"`): the file that was downloaded and checked, with what it needs from the index. A build that is a wheel has exactly one such step, and it names no packages or requirements file. |
| `index` | address | The package index the packages come from instead of PyPI. It is the only one used: a second could offer a newer package built for something else. |
| `weight` | whole number | How much of the progress bar this step is, next to the others (default 1). |

## `[[acquire.install.nodes]]`

One pack of extra nodes (a custom node of ComfyUI), pinned to one commit by the sha256 of its archive.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Letters, digits and `_`. |
| `label` | text | What the log and the window call it. |
| `when` | condition | Only on a machine where this holds; always when left out. |
| `url` | address | A `.tar.gz` of one commit. |
| `sha256` | text | What the archive must hash to, as `sha256:` and 64 hexadecimal digits. Always checked. |
| `into` | text | Where it goes, relative to the unpacked source (`custom_nodes/ComfyUI-GGUF`). |
| `requirements` | text | Its requirements file, relative to the pack; installed into the environment. |
| `license` | text | Its licence, for the About window. |

## `[[acquire.overlays]]`

One archive that can be added to a build.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Letters, digits and `_`; names the choice. |
| `label` | text | What the window says about it (`AMD ROCm (adds about 1 GB)`). |
| `rules` | list of `{ when, name }` | The file for this machine, by exact name; the first rule whose condition holds names it. A platform no rule matches has no such overlay. |

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
| `dir` | text, a directory | a one-line box with a Browse button |
| `file` | text, a file | a one-line box with a Browse button |
| `url` | text, an address | a one-line box |
| `runtime_path` | text, a file or a download address | the runtime manager when `ui.widget` says so, else a one-line box |
| `kv_lines` | text, one `KEY=VALUE` per line | a box of several rows, with what is wrong with a line under it |
| `model_ref` | text, a model | a one-line box |
| `hidden` | any | not drawn: the value is only stored (unless `ui.widget` names a widget) |

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
| `gpu.targets` | a list of string |
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
| `parent.id` | a string |
| `parent.port` | an int |
| `parent.url` | a string |
| `public_port` | an int |
| `runtime.binary` | a string |
| `runtime.build` | an int |
| `runtime.dir` | a string |
| `runtime.flags` | a list of string |
| `runtime.version` | a string |
| `session.attached` | a bool |
| `session.crashed` | a bool |
| `session.model_display` | a string |
| `session.model_ref` | a string |
| `session.running` | a bool |
| `sys.cpus` | an int |
| `sys.ram_bytes` | an int |
| `field.<id>` | the field's own type |
| `value` | the field's own type |
