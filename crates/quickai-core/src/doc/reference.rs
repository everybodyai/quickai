//! The reference pages for documents (docs/implementation-plan.md, WP2.7 and WP3.6).
//!
//! What a document may say is defined by the types in `doc/engine.rs` and `doc/vocab.rs`; this
//! module describes each key in a sentence and renders the whole as Markdown, which is checked in as
//! `docs/reference/engine-fields.md` and `docs/reference/vocab-fields.md`. `quickai config docs`
//! (and `quickai config docs vocab`) prints the same text. A test compares the two, another compares
//! the keys listed here with the fields of the types, so a key added to a type without a line here
//! (or the reverse) fails the build rather than leaving the page behind.

use crate::doc::engine::{EngineDoc, FieldType};
use crate::registry::context::{schema, Site};

/// One key of a table in a document.
pub struct Attribute {
    pub name: &'static str,
    /// The kind of value it takes.
    pub value: &'static str,
    pub meaning: &'static str,
}

/// A table of keys: the section it belongs to and the type that defines it.
pub struct Group {
    pub heading: &'static str,
    pub intro: &'static str,
    /// The struct in `doc/engine.rs` whose fields are exactly these keys; `None` for a group that is
    /// one shape of an enum, which a test of its own covers.
    pub source: Option<&'static str>,
    pub attributes: &'static [Attribute],
}

const fn a(name: &'static str, value: &'static str, meaning: &'static str) -> Attribute {
    Attribute {
        name,
        value,
        meaning,
    }
}

pub const GROUPS: &[Group] = &[
    Group {
        heading: "`[engine]`",
        intro: "Who the engine is.",
        source: Some("EngineInfo"),
        attributes: &[
            a("name", "text", "What the interface calls the engine (`llama.cpp`). Required."),
            a("server_name", "text", "The name of its server program (`llama-server`), for sentences about it."),
            a("summary", "text", "One line about what it does."),
            a("license", "text", "Its licence, for the About window."),
            a("bundled", "bool", "Whether packages ship it. Default false."),
            a("homepage", "text", "Where to read about it."),
            a("egress", "list of text", "The hosts the engine itself contacts, which the privacy table lists; a name that begins `*.` stands for every host under it."),
        ],
    },
    Group {
        heading: "`[provides]`",
        intro: "What a running engine offers.",
        source: Some("Provides"),
        attributes: &[
            a("apis", "list of text", "The ids of the APIs it serves."),
            a("web_ui", "`{ path, label }`", "Its built-in web page, if it has one; the button that opens it carries `label`."),
            a("accepts", "`{ formats, refs }`", "What a model is for it: the file formats it loads (`gguf`), or the kinds of named reference it takes (`ollama`)."),
            a("model_name_matters", "bool", "Whether a request naming another model loads that one."),
            a("default_port", "whole number", "The port it uses when nothing says otherwise."),
        ],
    },
    Group {
        heading: "`[locate]`",
        intro: "Finding the executable without touching the network. `strategies` is a list, tried in order, and the first that yields a path wins. Each entry has `use` and its own keys:",
        source: Some("Locate"),
        attributes: &[
            a("strategies", "list of tables", "`{ use = \"setting\", field }` takes the path a field holds; `{ use = \"next_to_exe\", file }` looks beside the application's executable; `{ use = \"dev_tree\", path }` looks below the source checkout the application was built in; `{ use = \"installed\", root, pick, binary }` takes the newest build QuickAI installed under `root` (a template such as `${data}/ollama`) and the executable at `binary` inside it, a table by system (`{ linux = \"bin/tool\", windows = \"tool.exe\" }`); `{ use = \"path_search\", name }` looks for a program of that name in the directories of the `PATH`; `{ use = \"known_paths\", linux, macos, windows }` looks in the places listed for this system, with `~` and `%NAME%` filled in from the environment."),
        ],
    },
    Group {
        heading: "`[runtime]`",
        intro: "Facts read from the program itself, once it is found.",
        source: Some("Runtime"),
        attributes: &[
            a("version", "table, below", "How to learn its version."),
            a("flags", "table, below", "How to learn which command-line flags it supports (`runtime.has_flag`): read from its help text. Read since Phase 3 and used by the adapters of Phase 4."),
        ],
    },
    Group {
        heading: "`[runtime]` `flags`",
        intro: "Runs the program and finds the flags it supports in what it prints.",
        source: Some("FlagsProbe"),
        attributes: &[
            a("exec", "list of text", "The command: the executable (`${binary}`) and its arguments (`--help`)."),
            a("parse", "text", "How the output is read (`help_flags`: the words that start with `--`)."),
            a("skip", "text", "Lines that contain this are left out: the tombstones of flags that were removed."),
        ],
    },
    Group {
        heading: "`[runtime]` `version`",
        intro: "Runs the program and reads the version from what it prints. The answer is remembered per executable and modification time, so asking again does not start the program again.",
        source: Some("VersionProbe"),
        attributes: &[
            a("exec", "list of text", "The command: the executable (`${binary}`) and its arguments (`--version`)."),
            a("regex", "regular expression", "Its first group is the version; standard output is searched, then standard error. The exit status does not matter."),
            a("as", "`version`", "The value it becomes (`runtime.version`)."),
        ],
    },
    Group {
        heading: "`[launch]`",
        intro: "How the engine is started.",
        source: Some("Launch"),
        attributes: &[
            a("strategies", "list of text", "The ways to bring it up, in order: `attach_if_running` (use one that already answers on its port, if it is the engine) and `spawn` (start one)."),
            a("strategy_field", "text", "The id of a `choice` field whose options are `auto` (try the strategies in order), `attach` (only the first) and `spawn` (only the second)."),
            a("attach", "table, below", "Using an engine that is already running."),
            a("spawn", "table", "The program to start, below."),
            a("ready", "table", "How to tell it is up, below."),
            a("warm", "list of tables, below", "Calls made once it answers, to load the model; they are left out when no model is selected, and the engine then runs with nothing loaded."),
            a("unload", "list of tables, below", "Calls made before it is stopped or left, to free the model."),
        ],
    },
    Group {
        heading: "`[launch.attach]`",
        intro: "An engine that may already be running (a person's own Ollama). A port that answers is used only if it answers like the engine.",
        source: Some("Attach"),
        attributes: &[
            a("endpoint", "template", "Where it would be: `127.0.0.1:${public_port}`, the port its settings give."),
            a("identity", "`{ path, pointer }`", "A request only this engine answers in a known way: the answer is JSON with a text at `pointer` (`/version`). Anything else on the port is something else using it."),
            a("liveness", "`{ path, every, failures }`", "How to notice that it has gone, since QuickAI has no process of its own to watch: `path` is asked every `every` (default `5s`), and `failures` misses in a row (default 3) mean it is gone."),
        ],
    },
    Group {
        heading: "`[[launch.warm]]` and `[[launch.unload]]`",
        intro: "One call of a phase.",
        source: Some("Step"),
        attributes: &[
            a("when", "condition", "Make the call only while this holds (`model`: when a model is selected)."),
            a("call", "table, below", "The request."),
        ],
    },
    Group {
        heading: "`call`",
        intro: "A request to the engine. Texts in `path` and `body` are templates; the body is JSON written as TOML.",
        source: Some("CallSpec"),
        attributes: &[
            a("method", "text", "`GET`, `POST`, `PUT`, `DELETE` or `PATCH`. Default `POST`."),
            a("path", "template", "A plain path on the engine, starting with `/`."),
            a("body", "table", "The JSON body: tables, lists, numbers, booleans and texts."),
            a("timeout", "duration", "How long it may take (`600s` for a warm-up, `5s` for an unload, by default)."),
            a("best_effort", "bool", "A failure is logged and nothing is done about it (an unload that does not answer must not hold up a stop). Default false."),
            a("stream", "`json`, `ndjson` or `sse`", "How the reply is read: `json` (the default) is one body, the others a stream of events. Only the calls of `[models]` may stream; a warm-up or an unload is read whole."),
        ],
    },
    Group {
        heading: "`[launch.spawn]`",
        intro: "Starting the program. Every text is a template: `${...}` holes are filled from the context below.",
        source: Some("Spawn"),
        attributes: &[
            a("program", "template", "The program to start (`${runtime.binary}`)."),
            a("args", "list of templates", "Its first arguments, one each. The fields' `bind.arg` follow, by `order`."),
            a("env", "list of tables", "Environment variables to set, below."),
            a("group", "bool", "Start it in a process group of its own (a job on Windows), so that stopping it takes the processes it started with it, and, on Linux, so that it follows QuickAI if QuickAI is killed. Default false."),
            a("stop", "table", "How to stop it: `{ policy = \"kill\" }` (the default: at once), or `{ policy = \"graceful\", grace = \"10s\", exit_grace = \"2s\" }`: ask it to leave (SIGTERM to its group; Windows has no such signal), wait `grace` (`exit_grace` while the application is closing), then kill what is left."),
        ],
    },
    Group {
        heading: "`[[launch.spawn.env]]` and `[[field]]` `bind.env`",
        intro: "One environment variable: `value` or `prepend`, not both. In `[[launch.spawn.env]]` the document sets it for every start; in a field's `bind.env` the field's value does (`${value}`), after the document's own variables, in the order of the field's other bindings, and only while the field has a value.",
        source: Some("EnvEntry"),
        attributes: &[
            a("name", "text", "The variable."),
            a("value", "template", "Set it to this."),
            a("prepend", "template", "Put this in front of what the variable already holds, joined with `sep`."),
            a("sep", "text", "The joiner for `prepend`. Default `:`."),
            a("when", "condition", "Only while this holds."),
            a("locked", "bool", "The person's own lines may not override it (`[[launch.spawn.env]]` only: what a field decides is theirs to change)."),
        ],
    },
    Group {
        heading: "`[launch.ready]`",
        intro: "Waiting for the engine to answer.",
        source: Some("Ready"),
        attributes: &[
            a("probe", "`{ use = \"http\", path, expect }`", "A request to make; `expect` is the status that means ready (default 200)."),
            a("timeout", "duration", "How long to wait before giving up (`120s`)."),
            a("interval", "duration", "How often to ask (`500ms`)."),
            a("on_timeout", "`fail` or `wait`", "What the timeout means. `fail` (the default): say so, stop what was started and end in Crashed. `wait`: say so and go on waiting, because the program may still be loading; only its exit or a Stop ends the wait."),
        ],
    },
    Group {
        heading: "`[[field]]`",
        intro: "One setting. Fields build the settings the window shows, and each says how its value reaches the engine. The same document can add, change or hide a field of the embedded one by `id`.",
        source: Some("Field"),
        attributes: &[
            a("id", "text", "Letters, digits and `_`. Names the value (`llamacpp.port`) and is how a patch finds the field. Required."),
            a("label", "text", "What the window calls it. Default: the `id`."),
            a("help", "text", "The hover text."),
            a("type", "a type, below", "What kind of value it holds. Required."),
            a("range", "`[low, high]`", "The numbers it may hold; the number box stops there."),
            a("step", "number", "How far one pixel of dragging moves a number."),
            a("default", "value or `{ fn, args }`", "The value when nothing was chosen: a literal, a text with `${...}` holes filled in from the engine's context (`${data}/ollama/models`), or a named function over context values (`recommend_ctx_size`)."),
            a("optional", "bool", "The field may stay empty."),
            a("recompute_on", "list of text", "What re-derives the default. `model`: picking another model."),
            a("legacy_key", "text", "The key it has always had in `settings.json`; its value is stored there, not under `values`."),
            a("visible_when", "condition", "Show it only while this holds."),
            a("enabled_when", "condition", "Let it be edited only while this holds (`not session.running`)."),
            a("required_when", "condition", "It needs a value while this holds."),
            a("options", "list of text", "The values a `choice` offers, and its default must be one of them."),
            a("pattern", "regular expression", "The whole text value must match it."),
            a("validate", "list of `{ check, message }`", "Rules that may look at other fields. The first whose `check` does not hold gives its `message` as the field's error."),
            a("persist", "`global`, `per_model`, `profile`, `session`", "Where the value is remembered. Default `global`."),
            a("applies", "`on_start`, `spawn_only`, `live`", "When a change takes effect: at the next start; at the next start, and only when QuickAI starts the engine itself; at once. Default `on_start`."),
            a("bind", "table, below", "How the value reaches the engine."),
            a("derive", "table of conditions", "Named facts for the rest of the program, from the field's value (`gpu_offload = \"value > 0\"`)."),
            a("ui", "table, below", "Where and how the window shows it."),
        ],
    },
    Group {
        heading: "`[[field]]` `bind`",
        intro: "Turning a value into engine configuration. In an argument template, `${value}` is the field's value.",
        source: Some("Bind"),
        attributes: &[
            a("port", "`public`", "This field is the port the engine listens on, and the one clients use."),
            a("arg", "list of templates", "Arguments to add to the command line."),
            a("arg_split", "bool", "The value is a text of arguments, split like a shell does (quotes group words); nothing is added when it is empty."),
            a("order", "whole number", "Where among the field arguments these go. Default: the field's `ui.order`."),
            a("env", "list of tables, below", "Environment variables of the engine that this field decides, each with an optional condition on `value`."),
            a("env_lines", "table, below", "For a `kv_lines` field: its lines are environment variables of the engine."),
        ],
    },
    Group {
        heading: "`[[field]]` `bind.env_lines`",
        intro: "What a field of `KEY=VALUE` lines may set.",
        source: Some("EnvLines"),
        attributes: &[
            a("protect", "list of text", "Names the person's lines may not set (`OLLAMA_MODELS`), besides every variable the document locks or adds to (`prepend`): the document's value stands. A name is protected in any spelling."),
        ],
    },
    Group {
        heading: "`[[field]]` `ui`",
        intro: "How the window shows a field.",
        source: Some("FieldUi"),
        attributes: &[
            a("section", "text", "Where it goes: `primary` (the row under the model), `advanced` (inside Settings), or a section of your own. Default `advanced`."),
            a("order", "whole number", "Its place in the section, lowest first; those without come last."),
            a("widget", "text", "A widget other than the type's usual one: `runtime_manager` (for a `runtime_path`) or `button` (a field that holds nothing, usually of type `hidden`: it is drawn as a button with the field's `label`, pressed only while `enabled_when` holds). Another name is an error when the document loads."),
            a("hint", "text", "Greyed text shown in an empty text box. A number with a hint is a box that can be empty (the hint says what empty means), not a drag box."),
            a("action", "text", "What a click on a `button` makes the application do, as `verb` or `verb:what` (`open:settings`). A button needs one and nothing else has one."),
        ],
    },
    Group {
        heading: "`[models]`",
        intro: "For an engine that keeps its own models, named references it lists, pulls and removes (Ollama): each model is a name, not a file. An engine without this section has files in a folder.",
        source: Some("Models"),
        attributes: &[
            a("needs_server", "bool", "The calls need the engine running. Stopped, only what it listed last time can be shown, and nothing is asked or started until the person presses a button. Default false."),
            a("list", "table, below", "The call that lists the models, and where each one is in the reply."),
            a("pull", "table, below", "The call that fetches a model by name, with its progress."),
            a("remove", "table, below", "The call that removes a model."),
            a("capabilities", "table, below", "The call that asks what one model can do."),
            a("input_rules", "list of tables, below", "What the model box makes of what was typed: the first rule whose `when` holds. Nothing typed is nothing; text no rule takes is refused."),
            a("sources", "list of tables, below", "For an engine whose models are files: where they are kept (read by Phase 4)."),
            a("acquire", "table, below", "For an engine whose models are files: how one gets into the folder (read by Phase 4)."),
            a("lookup", "list of tables, below", "Looking models up online (read by Phase 4)."),
        ],
    },
    Group {
        heading: "`[[models.sources]]`",
        intro: "A place models are found. Read from Phase 4; until then an engine with files keeps what the window has always done.",
        source: Some("ModelSource"),
        attributes: &[
            a("use", "text", "`bundle_dir` (a folder shipped with the application) or `scan_dir` (one the person can write to). Required."),
            a("path", "template", "The folder (`${exe_dir}/models`, `${field.models_dir}`). Required."),
            a("ext", "list of text", "The file endings that are models (`.gguf`)."),
            a("writable", "bool", "Whether downloads and imports may go here. Default false."),
        ],
    },
    Group {
        heading: "`[models]` `acquire`",
        intro: "How a model gets into the folder (read from Phase 4).",
        source: Some("ModelAcquire"),
        attributes: &[
            a("use", "text", "`file`: it is downloaded or linked as a file. Required."),
            a("dir", "template", "Where it goes."),
        ],
    },
    Group {
        heading: "`[[models.lookup]]`",
        intro: "One way of looking models up online (read from Phase 4): the primitive that does it and what it is told. Every key besides `id` and `use` is the primitive's own and is checked by it.",
        source: Some("ModelLookup"),
        attributes: &[
            a("id", "text", "What `input_rules` call it (`provider = \"hf_popular\"`). Required."),
            a("use", "text", "The primitive (`catalog.hf_gguf`). Required."),
        ],
    },
    Group {
        heading: "`[models]` `list`",
        intro: "The names of the models the engine has, in the order it gives them.",
        source: Some("ModelList"),
        attributes: &[
            a("call", "table", "The request (`GET /api/tags`); `${data}` and the other names of the engine's context are available in its path."),
            a("items", "pointer", "The array of models in the reply (`/models`). Required."),
            a("map", "table, below", "Where the parts of one model are. Required."),
            a("cache", "template", "Where the last reply is kept (`${data}/ollama/catalog-cache.json`), written whole or not at all, so that a list can be shown while the engine is not running."),
        ],
    },
    Group {
        heading: "`[models]` `list.map`",
        intro: "JSON pointers into one model of the list.",
        source: Some("ModelMap"),
        attributes: &[
            a("id", "pointer", "The name the engine knows it by, which is what a request names (`/name`). Required; a model with none is skipped, with a word in the log."),
            a("display", "pointer", "What the picker shows. Default: the name."),
            a("size_bytes", "pointer", "How big it is: what the context size and the speed estimate are worked out from."),
            a("digest", "pointer", "Its checksum."),
            a("capabilities", "pointer", "A list of what it can do (`completion`, `vision`)."),
        ],
    },
    Group {
        heading: "`[models]` `pull`",
        intro: "Fetching a model by name. A pull is the one time the engine reaches the internet, and only on a click.",
        source: Some("ModelPull"),
        attributes: &[
            a("call", "table", "The request, with `${ref}` the name to fetch (`body = { model = \"${ref}\" }`) and `stream = \"ndjson\"` when the engine reports progress as it goes."),
            a("progress", "table, below", "Where the progress is in the events of the stream; without it the usual Ollama shape is assumed."),
        ],
    },
    Group {
        heading: "`[models]` `pull.progress`",
        intro: "JSON pointers into one event of a pull. A pull adds layers up as the engine announces them, so a layer is told apart by `key`; the bar never goes back, and stays short of full until the engine says it is done.",
        source: Some("PullProgressSpec"),
        attributes: &[
            a("key", "pointer", "What tells one layer from another (`/digest`)."),
            a("completed", "pointer", "Bytes of the layer done (`/completed`)."),
            a("total", "pointer", "Bytes the layer needs (`/total`)."),
            a("status", "pointer", "The engine's own words (`/status`), for the log."),
            a("error", "pointer", "Where an event says the pull failed, whatever the HTTP status was. Default `/error`."),
            a("done", "text", "The `status` that ends the pull. Default `success`."),
        ],
    },
    Group {
        heading: "`[models]` `remove`",
        intro: "Removing a model, once the person has confirmed it. The model that is loaded is never removed: the person stops the server first.",
        source: Some("ModelRemove"),
        attributes: &[
            a("call", "table", "The request, with `${ref}` the name as the engine lists it (`DELETE /api/delete`)."),
        ],
    },
    Group {
        heading: "`[models]` `capabilities`",
        intro: "What one model can take as input, asked of the engine.",
        source: Some("ModelCapabilities"),
        attributes: &[
            a("call", "table", "The request, with `${ref}` the model."),
            a("pointer", "pointer", "Where the list of what it can do is in the reply (`/capabilities`). Required."),
        ],
    },
    Group {
        heading: "`[[models.input_rules]]`",
        intro: "One rule of the model box. The text is trimmed first; a rule with no `when` takes whatever is left, and a rule after it is never reached (a warning).",
        source: Some("InputRule"),
        attributes: &[
            a("when", "condition", "A condition over `input`, with `matches(input, \"regex\")`, `starts_with`, `is_url(input)`, `is_file(input)` (whether it names a file on this machine) and `path_after_host(input)`."),
            a("do", "`pull` or `reject`", "What to do with it. Required."),
            a("ref", "template", "For `pull`: the name to fetch, over `input` (`${input}`, or `hf.co/${path_after_host(input)}` for a link to a page). Required for `pull`."),
            a("message", "text", "For `reject`: what the person is told. Required for `reject`."),
            a("provider", "text", "For `search` (read by Phase 4): the lookup that answers it."),
        ],
    },
    Group {
        heading: "`[traffic]`",
        intro: "Which of the engine's APIs the Live tab reads.",
        source: Some("Traffic"),
        attributes: &[
            a("apis", "list of text", "Ids from a vocabulary; each must be defined there, and one with a `decode` is read. Without this table every API in `[provides]` `apis` is."),
        ],
    },
    Group {
        heading: "`[api_info]`",
        intro: "What the 🔌 API panel says to a person who connects an app. Every text is a template over the engine's context (`${public_port}`, `${session.model_display}`).",
        source: Some("ApiInfo"),
        attributes: &[
            a("base_url", "template", "The address of the OpenAI-compatible API (`http://127.0.0.1:${public_port}/v1`). Required."),
            a("extra_urls", "list of tables, below", "More addresses worth showing: the engine's own API."),
            a("api_key", "`{ value, note }`", "What to put in the key box and a note under it."),
            a("model", "`{ value, note }`", "The name a client should give for the model and a note under it. With no model selected the panel shows `local-model`."),
            a("example", "text", "The id of the API whose curl example is shown (`openai.chat`): the first example of the vocabulary whose `when` holds."),
        ],
    },
    Group {
        heading: "`[[api_info.extra_urls]]`",
        intro: "One more address.",
        source: Some("ExtraUrl"),
        attributes: &[
            a("label", "text", "What it is (`Ollama API`). Required."),
            a("url", "template", "The address. Required."),
        ],
    },
    Group {
        heading: "`[[diagnostic]]`",
        intro: "Advice for a run that went wrong. It is shown under the log when every condition of `when` holds.",
        source: Some("DiagnosticRule"),
        attributes: &[
            a("when", "table, below", "What has to be so. Required."),
            a("hint", "template", "What to tell the person, over the engine's context (`${field.port}`). Required."),
            a("action", "text", "A button under the hint, as `verb` or `verb:what` (`focus_field:port`)."),
        ],
    },
    Group {
        heading: "`[[diagnostic]]` `when`",
        intro: "Both keys may be given; then both must hold. One at least is needed.",
        source: Some("DiagnosticWhen"),
        attributes: &[
            a("log_matches", "regular expression", "Looked for in the lines of the log; a pattern that does not compile is an error when the document loads."),
            a("exited_during", "`starting` or `running`", "The run ended while it was in this state."),
        ],
    },
    Group {
        heading: "`[acquire]`",
        intro: "Where builds of the engine come from and where they are kept. It configures the runtime manager in Settings.",
        source: Some("Acquire"),
        attributes: &[
            a("root", "template", "The directory builds are kept in, one subdirectory each (`${data}/llama-bin`). Installing needs it."),
            a("feed", "table, below", "Where the list of builds comes from."),
            a("assets", "table, below", "Which file of a release is a build for this machine."),
            a("install", "table, below", "How a downloaded file becomes an installed build."),
            a("backends", "list of tables, below", "What a file name says about the GPU it was made for."),
            a("filter_hint", "text", "The placeholder of the box that narrows the list of builds."),
            a("tested", "`{ tag }`", "The release this document was tested with: it is offered first, marked, and `pins` are for it."),
            a("overlays", "list of tables, below", "Archives unpacked over a build on request, by platform (ROCm's libraries): never a build of their own, and off until chosen."),
            a("pins", "table of text", "The sha256 of each file of the tested release by file name, as `sha256:` and 64 hexadecimal digits. A download of that release must have it, and must agree with the digest the host lists."),
        ],
    },
    Group {
        heading: "`[acquire]` `feed`",
        intro: "`use = \"github_release\"`: the releases of a GitHub repository.",
        source: None,
        attributes: &[
            a("use", "`github_release`", "The kind of feed."),
            a("repo", "`owner/name`", "The repository."),
            a("per_page", "1 to 100", "How many releases to ask for. Default 10."),
            a("show", "whole number", "How many releases that have a build for this machine to show. Default 5."),
            a("prereleases", "bool", "Whether a release GitHub marks as a prerelease counts. Default false."),
            a("api", "address", "The root of the API (`https://api.github.com`, the default). Another root, `http` or `https`, is how a test points the feed at a fake; only documents QuickAI trusts may name one."),
            a("digest", "bool", "The feed lists a sha256 for each file (GitHub's `digest`) and a build carries it, for the installer to check. Default false."),
        ],
    },
    Group {
        heading: "`[acquire]` `assets`, `by = \"keywords\"`",
        intro: "A file qualifies when it ends in one of `ext`, contains none of the `exclude` words, and contains a word for this machine's OS and one for its architecture. Matching ignores case.",
        source: None,
        attributes: &[
            a("by", "`keywords`", "The kind of rule."),
            a("ext", "list of text", "The endings a build can have, each with its dot (`.zip`, `.tar.gz`). Required."),
            a("exclude", "list of text", "Words that rule a file out (`cudart`)."),
            a("os", "table of lists", "`linux`, `windows`, `macos` to the words that mean it in a file name."),
            a("arch", "table of lists", "`x86_64`, `aarch64` to the words that mean it."),
            a("label", "template", "The directory a build is installed under, over `tag`, `stem` and `asset`. Default `${tag}-${stem}`."),
        ],
    },
    Group {
        heading: "`[acquire]` `assets`, `by = \"names\"`",
        intro: "The file is chosen by exact name: the first rule whose condition holds names it.",
        source: None,
        attributes: &[
            a("by", "`names`", "The kind of rule."),
            a("rules", "list of `{ when, name }`", "`when` is a condition over the context (`os == 'linux' and arch == 'x86_64'`), `name` the file."),
            a("label", "template", "As above."),
        ],
    },
    Group {
        heading: "`[acquire]` `install`",
        intro: "`use = \"archive\"`: unpack a `.zip`, `.tar.gz` or `.tar.zst`, safely, into a directory of its own that is renamed into place when it is whole, and find the executable inside.",
        source: None,
        attributes: &[
            a("use", "`archive`", "The kind of install."),
            a("binary", "`{ name, search_depth }`", "The executable's file name without a platform suffix, and how many directory levels below the build's directory to look for it (default 4, at most 16)."),
            a("chmod", "bool", "Mark the executable as executable (Unix)."),
            a("verify", "`none` or `digest`", "`digest`: compare the download with the sha256 the host lists for the file and the one this document pins before anything is unpacked; a build with neither is installed and said to be unchecked. Default `none`."),
            a("smoke", "list of text", "A command that must succeed on the unpacked build before it counts as installed, starting with the executable: `[\"${binary}\", \"--version\"]`."),
            a("skip", "list of `{ when, glob }`", "Entries of the archive to leave out: `glob` is a path inside the build (`lib/ollama/cuda_*`, `*` for a run of characters inside one name), `when` a condition on this machine. What is below a directory that matches goes with it."),
        ],
    },
    Group {
        heading: "`[[acquire.overlays]]`",
        intro: "One archive that can be added to a build.",
        source: Some("Overlay"),
        attributes: &[
            a("id", "text", "Letters, digits and `_`; names the choice."),
            a("label", "text", "What the window says about it (`AMD ROCm (adds about 1 GB)`)."),
            a("rules", "list of `{ when, name }`", "The file for this machine, by exact name; the first rule whose condition holds names it. A platform no rule matches has no such overlay."),
        ],
    },
    Group {
        heading: "`[acquire]` `backends`",
        intro: "Each entry names a GPU backend by a word in a file name. The first entry whose word is in the name wins.",
        source: Some("Backend"),
        attributes: &[
            a("match", "text", "The word to look for."),
            a("label", "text", "What the interface calls the backend (`Vulkan`)."),
            a("needs_gpu", "`nvidia`, `amd`, `intel`, `apple`", "The vendor a build for this backend needs; choosing one on a machine without it shows a warning."),
        ],
    },
];

/// Every field type, with the kind of value it holds and how the window draws it.
pub const TYPES: &[(FieldType, &str, &str)] = &[
    (FieldType::Int, "whole number", "a drag box"),
    (FieldType::Float, "number", "a drag box"),
    (FieldType::Bool, "true or false", "a checkbox"),
    (FieldType::Text, "text", "a one-line box"),
    (FieldType::Choice, "text, one of `options`", "a drop-down"),
    (FieldType::Port, "whole number, 1 to 65535", "a drag box"),
    (
        FieldType::Dir,
        "text, a directory",
        "a one-line box with a Browse button",
    ),
    (
        FieldType::File,
        "text, a file",
        "a one-line box with a Browse button",
    ),
    (FieldType::Url, "text, an address", "a one-line box"),
    (
        FieldType::RuntimePath,
        "text, a file or a download address",
        "the runtime manager when `ui.widget` says so, else a one-line box",
    ),
    (
        FieldType::KvLines,
        "text, one `KEY=VALUE` per line",
        "a box of several rows, with what is wrong with a line under it",
    ),
    (FieldType::ModelRef, "text, a model", "a one-line box"),
    (
        FieldType::Hidden,
        "any",
        "not drawn: the value is only stored (unless `ui.widget` names a widget)",
    ),
];

/// The name a type has in a document (`runtime_path`).
pub fn type_name(ty: FieldType) -> String {
    serde_json::to_value(ty)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The page, as Markdown.
pub fn render() -> String {
    let mut out = String::new();
    out.push_str("# Engine documents: sections, fields and bindings\n\n");
    out.push_str(
        "_Generated from the types in `crates/quickai-core/src/doc/engine.rs` by `quickai config docs`. \
         Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` \
         rewrites it, and a test fails while it is out of date._\n\n",
    );
    out.push_str(
        "An engine document (`kind = \"engine\"`) says how QuickAI finds, starts and talks to one engine, \
         and which settings it has. A document in the user's configuration directory with the same `id` \
         patches the embedded one: tables merge by key and `[[field]]` entries by `id` \
         (docs/architecture.md, section 5.2). A key this version does not know is a warning; `quickai config check \
         --strict` makes warnings fail.\n\n",
    );
    render_groups(&mut out, GROUPS);

    out.push_str("## Field types\n\n| `type` | Value | Drawn as |\n|---|---|---|\n");
    for (ty, value, drawn) in TYPES {
        out.push_str(&format!("| `{}` | {value} | {drawn} |\n", type_name(*ty)));
    }
    out.push('\n');

    out.push_str(
        "## The context\n\nConditions, defaults and templates are written in one small language \
         (docs/architecture.md, section 7) and see these names. A name that is not here is an error \
         when the document loads, with a suggestion when one is close. Inside a field's own bindings, rules and \
         conditions `value` is that field's current value; `field.<id>` is the value of any field.\n\n",
    );
    out.push_str("| Name | Type |\n|---|---|\n");
    let mut names = Vec::new();
    flatten(
        &schema(&EngineDoc::default(), Site::Engine),
        String::new(),
        &mut names,
    );
    for (name, ty) in names {
        out.push_str(&format!("| `{name}` | {ty} |\n"));
    }
    out.push_str("| `field.<id>` | the field's own type |\n| `value` | the field's own type |\n");
    out
}

/// The tables of a page: a heading, what the table is, and one row for each key.
fn render_groups(out: &mut String, groups: &[Group]) {
    for group in groups {
        out.push_str(&format!("## {}\n\n{}\n\n", group.heading, group.intro));
        out.push_str("| Key | Value | Meaning |\n|---|---|---|\n");
        for attribute in group.attributes {
            out.push_str(&format!(
                "| `{}` | {} | {} |\n",
                attribute.name, attribute.value, attribute.meaning
            ));
        }
        out.push('\n');
    }
}

/// The tables of a vocabulary document (`kind = "vocab"`), by the types in `doc/vocab.rs`.
pub const VOCAB_GROUPS: &[Group] = &[
    Group {
        heading: "`[[modality]]`",
        intro: "A kind of input or output a model can have, named once here so that everything else can refer to it by `id`.",
        source: Some("Modality"),
        attributes: &[
            a("id", "text", "Names it (`image`). Required."),
            a("label", "text", "What the window calls it (`Images`)."),
        ],
    },
    Group {
        heading: "`[[api]]`",
        intro: "An API a client can call on a running engine. An engine lists the ones it serves in `[provides]` `apis`, and an engine that names an `id` no vocabulary defines does not load.",
        source: Some("ApiDef"),
        attributes: &[
            a("id", "text", "Lower case letters, digits and `.`, `_`, `-` (`openai.chat`). Required. Every API of every vocabulary has a different one."),
            a("paths", "list of text", "Where the engine serves it, exactly as a request names it (`/v1/chat/completions`): a `/` first and no query. An API an engine serves under two names lists both. A path belongs to one API across all vocabularies. Required when there is a `decode`."),
            a("capabilities", "list of text", "What a client can do with it: `chat`, `completion`, `embedding`."),
            a("decode", "table, below", "How the Live tab reads its traffic. A request to an API without one is passed on and not shown."),
            a("examples", "list of tables, below", "Curl examples for the API panel (the window does not show them yet)."),
        ],
    },
    Group {
        heading: "`[api.decode]`",
        intro: "What the proxy makes of the requests and replies of an API. Each value that says where something is, is a JSON pointer (RFC 6901): a `/` and then the keys and list positions down to it, as in `/choices/0/delta/content`. The proxy passes every byte on as it is whatever this says; something it does not describe is an entry left blank in the Live tab, never a failure.",
        source: Some("DecodeSpec"),
        attributes: &[
            a("framing", "list of `sse`, `ndjson`, `json`", "How a reply comes. The type the engine says it is sending chooses among the ones listed: `text/event-stream` is read event by event (`sse`: lines of `data: {json}`, ending with `data: [DONE]`), a type that says `ndjson` or `jsonl` line by line (`ndjson`: one JSON object a line), anything else when the whole body is there (`json`). A reply that none of the listed framings fits is not read. Required."),
            a("prompt", "pointer", "Where the prompt is in a request that is not a chat. A request that has none, or an empty one, is not a conversation: it is a load or an unload, and it is not shown. This or `chat`, not both."),
            a("delta", "list of pointers", "Where the new text is in one event of a stream; the first that gives a text is used."),
            a("final", "list of pointers", "Where the text is in a reply that comes whole."),
            a("error", "list of pointers", "Where the engine says what went wrong, in a reply or in an event: the first that is there and not null is shown as the error (a text as it is, an object by its `message`)."),
            a("chat", "table, below", "The shape of a chat request: what the person said is the text of the last message that is not the system's. An empty list of messages is not a conversation either. This or `prompt`, not both."),
        ],
    },
    Group {
        heading: "`[api.decode.chat]`",
        intro: "Where the messages of a chat request are and what a message looks like. Pointers inside a message start at that message.",
        source: Some("ChatSpec"),
        attributes: &[
            a("messages", "pointer", "The list of messages in the request. Required."),
            a("role", "pointer", "Within a message: who says it. Required."),
            a("content", "pointer", "Within a message: what is said, a text or a list of parts. Required."),
            a("skip_roles", "list of text", "Messages with these roles are not what the person asked (`system`)."),
            a("parts", "table, below", "For a content that is a list of typed parts. Without it such a content shows no text."),
            a("images", "pointer", "Within a message: a list of base64 images, shown as attachments with their type when the first bytes say it is a PNG, JPEG, GIF or WebP."),
        ],
    },
    Group {
        heading: "`[api.decode.chat.parts]`",
        intro: "A part of a content that is a list. The text parts are joined for the prompt; the others are shown as attachments by their type (`image_url`, `input_audio`, `file`, or a label of their own for a type the proxy has no word for).",
        source: Some("PartsSpec"),
        attributes: &[
            a("type", "pointer", "Within a part: its type, `text` for one that is text. Required."),
            a("text", "pointer", "Within a part: its text. Required."),
        ],
    },
    Group {
        heading: "`[[api.examples]]`",
        intro: "One curl example for the API panel.",
        source: Some("ApiExample"),
        attributes: &[
            a("label", "text", "What the example shows (`Chat with an image`). Required."),
            a("when", "condition", "Show it only while this holds (`has(model.modalities.input, 'image')`)."),
            a("curl", "text", "The command, as text. Required."),
        ],
    },
];

/// The page for vocabulary documents, as Markdown.
pub fn render_vocab() -> String {
    let mut out = String::new();
    out.push_str("# Vocabulary documents: modalities and APIs\n\n");
    out.push_str(
        "_Generated from the types in `crates/quickai-core/src/doc/vocab.rs` by `quickai config docs vocab`. \
         Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` \
         rewrites it, and a test fails while it is out of date._\n\n",
    );
    out.push_str(
        "A vocabulary document (`kind = \"vocab\"`) names what engines and the window have in common: \
         the kinds of input and output a model can have, and the APIs an engine can serve, with where the \
         text is in the requests and replies of each, so that the Live tab can read the traffic of an \
         engine the code has never heard of. A vocabulary can name things and describe them; it cannot \
         start a program or reach a host. A document in the user's configuration directory with the same \
         `id` patches the embedded one: tables merge by key and `[[modality]]` and `[[api]]` entries by `id` \
         (docs/architecture.md, section 5.2). A key this version does not know is a warning; \
         `quickai config check --strict` makes warnings fail.\n\n\
         The primitives a document relies on go in `requires` (`requires = { primitives = [\"decode@1\"] }` \
         for one with `decode` tables); a build that lacks one skips the document and says which.\n\n",
    );
    render_groups(&mut out, VOCAB_GROUPS);
    out.push_str(
        "## What is checked\n\n\
         Each of these is an error that names the file and the place, and a document with an error in it \
         defines nothing:\n\n\
         - an API `id` is lower case letters, digits and `.`, `_`, `-`, and is used once; two documents \
         may not define the same API or claim the same path;\n\
         - a path starts with `/` and has no query, and is listed once;\n\
         - a pointer starts with `/`;\n\
         - `framing` is not empty and names only `sse`, `ndjson` and `json`;\n\
         - a `decode` comes with `paths`, and says where the text of a request is, by `prompt` or by `chat`, \
         not both.\n\n\
         An engine that names an API in `[provides]` `apis` that no vocabulary defines does not load (when \
         there is a vocabulary at all), with the nearest name suggested.\n",
    );
    out
}

/// The dotted names of a context schema with their types, in order.
fn flatten(ty: &crate::expr::Type, prefix: String, out: &mut Vec<(String, String)>) {
    match ty {
        crate::expr::Type::Map(map) => {
            for (key, inner) in map {
                let name = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match inner {
                    crate::expr::Type::Map(m) if !m.is_empty() => flatten(inner, name, out),
                    // `field` is open: one entry per field of the document
                    crate::expr::Type::Map(_) => {}
                    other => out.push((name, other.name())),
                }
            }
        }
        other => out.push((prefix, other.name())),
    }
}
