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
            a("order", "whole number", "Where the engine stands in the engine selector, lowest first. Default 100; engines with the same number stand in the order of their ids."),
            a("companion_of", "text", "The id of the engine this program goes with, for a program that is not an engine to choose but something that is started beside one (Open WebUI beside Ollama). It is not in the engine selector and is never the engine in use; the settings of the engine it goes with offer it, it is started when that engine is ready and stopped with it, and it is a run of its own: when it stops unasked, the engine's run goes on. In every other way it is an engine's document: it is installed, located, started and given settings as an engine is, and keeps everything in a root of its own. Its templates may ask about the engine it goes with: `${parent.port}`, `${parent.url}`, `${parent.id}`."),
            a("first_address_field", "text", "For a companion: the id of a field of its own (of type `hidden`) that holds the address of the engine it goes with, as it was when the program was first started. A program that keeps the address it is first given in a database of its own and does not read it from its environment again goes on calling that one when the engine is somewhere else later. QuickAI writes the address down in this field at the first start, says in the log when a later start would give another, and says so in the notes of a configuration that would put the engine elsewhere. The field is the person's own and is never part of a configuration."),
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
            a("needs_model", "bool", "Whether it has to be given a model to start (llama.cpp). Default true. An engine that says false runs on its own, and Start needs no model: the work a client sends it names the models it uses (ComfyUI)."),
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
            a("cwd", "template", "The folder the program starts in; it is made, with the ones above it, if it is not there. Without it the program starts wherever QuickAI was started from, which is the person's, and a program that writes a file to its working directory (Open WebUI keeps its secret key there) writes it there."),
            a("mkdir", "list of templates", "Folders made, with the ones above them, before the program starts: for a program that refuses a folder it is told about that does not exist yet (ComfyUI's `--models-directory`)."),
            a("write", "list of tables", "Files of the application's own that are put where the program is told to read them, before it starts, below."),
            a("stop", "table", "How to stop it: `{ policy = \"kill\" }` (the default: at once), or `{ policy = \"graceful\", grace = \"10s\", exit_grace = \"2s\" }`: ask it to leave (SIGTERM to its group; Windows has no such signal), wait `grace` (`exit_grace` while the application is closing), then kill what is left."),
        ],
    },
    Group {
        heading: "`[[launch.spawn.write]]`",
        intro: "A file or a folder that is part of the application, put on the disk before the program starts, at every start: what is there already and the same is left alone, what differs is written again (a file that is not in the application is not removed). For a program that reads extensions from a folder it is told about (ComfyUI's custom nodes, with `--extra-model-paths-config`), so that every build has them and a person's own files are not touched. A document of the person's own can only name what the application has.",
        source: Some("WriteSpec"),
        attributes: &[
            a("asset", "text", "The name of the file or folder among the application's own files, with `/` between the parts (`comfyui/quickai_bridge`)."),
            a("to", "template", "Where it is put (`${data}/comfyui/extensions/quickai_bridge`). The folders above it are made."),
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
            a("share", "`yes`, `review`, `never`", "Whether the value travels in a configuration, a saved and shareable runtime. `yes`: it does. `review`: it does, and a person who is given a file that has it is shown it and asked (it becomes a command-line word, an environment line or an address, or it widens what the program may do). `never`: it belongs to a computer or a person and is never in one. Without it the type decides: `int`, `float`, `bool`, `choice` and `port` are `yes`; `text`, `url` and `kv_lines` are `review`; `dir`, `file` and `runtime_path` are `never`; `hidden` and `model_ref` are not settings (the model has a section of its own). A field added later is private until its document says otherwise."),
            a("share_note", "text", "The sentence a person is told when this field is asked about or left out of a configuration (`Open WebUI may use the internet`). Only for a field that is not `yes`."),
            a("applies", "`on_start`, `spawn_only`, `live`", "When a change takes effect: at the next start; at the next start, and only when QuickAI starts the engine itself; at once. Default `on_start`."),
            a("bind", "table, below", "How the value reaches the engine."),
            a("derive", "table of conditions", "Named facts for the rest of the program, from the field's value (`gpu_offload = \"value > 0\"`)."),
            a("identity", "bool", "Whether the value changes what runs, for telling two shared configurations apart (the community keeps one copy of each). `false` for a port: two configurations that differ only in it run the same model the same way. Default true."),
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
            a("arg_by_value", "table of lists of text", "For a `choice` field: the arguments each option adds, as plain words (`low = [\"--lowvram\"]`); an option that is not listed adds none."),
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
            a("lookup", "list of tables, below", "The ways the window can look models up online: a button of the model box for each. Without one the window has none."),
            a("bundle", "list of tables, below", "What is fetched together with a model's weights: a vision model's projector. A result that has a file for every part of a bundle is offered in it, as one download."),
            a("mode", "list of tables, below", "The ways the engine is started for a kind of model (`embedding`). A catalog names one by its `id`, and the record beside a fetched model remembers it."),
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
        intro: "How a model gets into the folder.",
        source: Some("ModelAcquire"),
        attributes: &[
            a("use", "text", "`file`: it is downloaded or linked as a file. Required."),
            a("dir", "template", "Where it goes."),
            a("place", "text", "For an engine that finds its files by the folder they are in (ComfyUI): the folder of `dir` the weights of a model go in (`diffusion_models`), a plain name. The recommended list may say another for a model, and `[components.<kind>] place` says where each kind of companion goes."),
            a("places", "list of text", "The other folders of `dir` a file can be put in by hand, plain names (`checkpoints`, `controlnet`): what the window offers a person who gives a link or a file of their own and says where it goes, besides `place` and the folder of each `[components.<kind>]`."),
        ],
    },
    Group {
        heading: "`[[models.lookup]]`",
        intro: "One way of looking models up online: the primitive that does it and what it is told. The window draws a \"Look up online\" button for it; nothing is asked of the network until the button is pressed. Every key besides `id` and `use` is the primitive's own and is checked by it: one the look-up does not take is a warning, and the list of tables merges by `id`, so a document of the person's can change the address of a look-up and leave the rest.",
        source: Some("ModelLookup"),
        attributes: &[
            a("id", "text", "Its name, with no spaces (`library`), unique in the document. Required."),
            a("use", "text", "The look-up: `ollama_library` (needs `lookup.ollama_library@1`) or `huggingface_gguf` (needs `lookup.huggingface_gguf@1`). Another name is an error that lists the ones this version has. Required."),
        ],
    },
    Group {
        heading: "`[[models.lookup]]` `use = \"ollama_library\"`",
        intro: "A search of Ollama's library of models, the one at ollama.com, for models that run on this machine. The site publishes no list a program can read, so the look-up reads the page of the site's own search, and says so when the page is not one it understands. It finds the library's own models (a person's uploads are left out) with the sizes each is offered in; a size is pulled as `name:size`, a model with no size shown by its name. The library gives no size in bytes, so a button says how big a size is by its count of parameters times `bytes_per_parameter`, with a `~`.",
        source: None,
        attributes: &[
            a("site", "address", "The root of the site (`https://ollama.com`, the default). Another root, `http` or `https`, is how a test points the look-up at a fake, or how a mirror is used."),
            a("bytes_per_parameter", "number", "Bytes a parameter takes in what a name pulls, to tell how big a size is (0.05 to 4; default 0.6, about 4.8 bits, which is what most of the library is quantized to)."),
            a("show_variants", "whole number", "The most size buttons a model gets (1 to 64; default 16), the smallest first."),
            a("types", "list of text", "The ids of the model types this engine runs (`chat`, `vision`); a model of another type is shown under `other`. Without it every type is run."),
        ],
    },
    Group {
        heading: "`[[models.lookup]]` `use = \"huggingface_gguf\"`",
        intro: "A search of the GGUF repositories of Hugging Face, the most downloaded first, for the words typed or for none. For each repository it looks into, it reads the files and their sizes, and offers the quantizations of the weights as buttons that download them; a file that a `[[models.bundle]]` part matches is not weights but a companion of them. A repository that needs a login is left out and counted in the status line.",
        source: None,
        attributes: &[
            a("site", "address", "The root of the hub (`https://huggingface.co`, the default); files are fetched from `<site>/<repository>/resolve/main/<path>`. Another root is how a test points the look-up at a fake."),
            a("popular", "whole number", "How many repositories of the plain list it looks into (1 to 50; default 30)."),
            a("keyword", "whole number", "How many it looks into for the words typed (1 to 50; default 20)."),
            a("similar", "whole number", "How many it looks into for each single word of a phrase typed, to find what the whole phrase misses (1 to 50; default 20)."),
            a("pipelines", "list of text", "The pipelines of the hub (`text-to-video`, `text-to-image`: lower case letters, digits and `-`; at most eight) the plain list is made of: one search for each, `popular` of the repositories of each looked into. For an engine that runs one kind of model, whose plain list would otherwise be the most downloaded GGUF repositories of any kind, which are language models. Words typed are one search that no pipeline narrows. Default none."),
            a("words", "list of text", "Words the plain list is also made of: one search for each (at most four; `encoder`), `popular` of the repositories of each looked into, for what no pipeline tag finds (a text encoder has none). A search is a few words with single spaces, up to 100 characters, and no `:`. Default none."),
            a("alone", "list of text", "Kinds of file (the `kind` of a `[[models.bundle]]` part: `vae`, `text_encoder`) that are offered by themselves as well as with a model: each file of the repository that a part of that kind finds gets a button in a row of its own (`Get VAE only:`), the ones `prefer` likes and then the smallest, and is kept under its own name in the folder of its kind (`[components.<kind>] place`). A repository that holds only such files is offered by them. For an engine whose workflows name their files one by one. A kind no part finds is an error. Default none: a companion is fetched with the weights it goes with."),
            a("prefer", "list of text", "The quantizations to offer first, best first (default `Q4_K_M`, `Q4_K_S`, `Q5_K_M`, `Q4_0`, `Q8_0`, `Q6_K`, `Q3_K_M`). A variant of one (`UD-Q4_K_M`) counts as it; the best is shown in bold."),
            a("skip", "list of text", "A file whose name or path one of these patterns matches (`*` any run, `?` one character, any case) is not weights: shards (`*-?????-of-?????*`: five digits each side of `-of-`), draft models (`mtp/*`, `mtp-*`), importance matrices (`imatrix*`). Default those. A pattern is about the file, not about the repository: a model whose own name says `MTP` is still offered."),
            a("show_variants", "whole number", "The most buttons a model gets (1 to 64; default 4): the ones `prefer` likes, then the smallest, shown smallest first."),
            a("types", "list of text", "The ids of the model types this engine runs (`chat`, `code`, `vision`, `embedding`); a model of another type is shown under `other`. Without it every type is run."),
        ],
    },
    Group {
        heading: "`[[models.bundle]]`",
        intro: "A way to fetch a model with the files that go with it: the weights of a result and one file for each part. It is offered as another row of buttons on every result that has a file for every part, and fetches them as one download; the files are kept as the pairing says (docs/lookup-and-bundles.md, section 4), and what the engine does with them is `[components.<kind>]`. The list merges by `id`, so a document of the person's can change a bundle or add another.",
        source: Some("ModelBundle"),
        attributes: &[
            a("id", "text", "Its name, with no spaces (`vision`), unique in the document. Required."),
            a("label", "text", "The words of its row (`with projector` makes `Get with projector:`); default the id."),
            a("help", "text", "A sentence for the person who wonders what it is."),
            a("parts", "list of tables, below", "The files beside the weights, one kind each. A bundle with none is an error. Required."),
        ],
    },
    Group {
        heading: "`[[models.bundle]]` `parts`",
        intro: "One part of a bundle: a kind of file and how to find it among the files of a repository.",
        source: Some("BundlePart"),
        attributes: &[
            a("kind", "text", "The kind of component (`mmproj`). The engine must have a `[components.<kind>]` for it, else the document is refused and says which kinds it has. Required."),
            a("label", "text", "What the window calls it in a sentence (`the projector mmproj-F16.gguf`); default the kind."),
            a("match", "list of text", "Patterns a file's name or path matches (`mmproj*.gguf`, `*mmproj*`): `*` stands for any run of characters, `?` for one, case is ignored. A file that matches is a companion and is not offered as a model. Required."),
            a("prefer", "list of text", "Of several files that match, the quantizations to take first, best first (`F16`, `BF16`); else the smallest."),
        ],
    },
    Group {
        heading: "`[[models.mode]]`",
        intro: "A way the engine is started for a model: the words it adds to the command line, after the arguments of the fields and of the model's files and before the person's own extra arguments. A catalog (which is data) and the record beside a fetched model name a mode by its `id` and never carry words of their own, so what reaches the command line is always the engine document's. A mode a model names that the engine does not have adds nothing, and the log says so. The list merges by `id`.",
        source: Some("ModelMode"),
        attributes: &[
            a("id", "text", "Its name: lower case letters, digits and `.`, `_`, `-` (`embedding`), unique in the document. Required."),
            a("label", "text", "What the model picker says of a model that runs so (`embedding server`); default the id."),
            a("help", "text", "A sentence for the person who wonders why."),
            a("arg", "list of text", "The words added, as they are (no templates); none is an error. Required."),
        ],
    },
    Group {
        heading: "`[components.<kind>]`",
        intro: "How one kind of companion file reaches the program once a model that has one is started: the arguments it adds to the command line, after the fields of a lower `order` and before the person's own extra arguments. A model has a companion of a kind when the pairing file beside it names one that is still there. The rest of Phase 4's design for a component (`requires_flag`, `fields`, `live`, `bind.each`) is not read yet.",
        source: Some("Component"),
        attributes: &[
            a("order", "whole number", "Where its arguments stand among the fields': after those of a lower `order`, before those of a higher one. Default 80, which is before `extra_args` (90)."),
            a("bind", "table, below", "What it adds."),
            a("place", "text", "For an engine that finds its files by the folder they are in (ComfyUI): the folder of the models folder this kind of file is kept in (`text_encoders`), a plain name. A component needs `place` or `bind.arg`, and may have both."),
        ],
    },
    Group {
        heading: "`[components.<kind>.bind]`",
        intro: "The arguments a companion adds.",
        source: Some("ComponentBind"),
        attributes: &[
            a("arg", "list of templates", "The arguments, one entry each, over `${component.path}` (the file): `[\"--mmproj\", \"${component.path}\"]`. Required unless the component has a `place`."),
        ],
    },
    Group {
        heading: "`[flags]`",
        intro: "What the program's command line means, as far as telling two shared configurations apart needs it: the community keeps one copy of each configuration, whatever the order or the spelling of its extra arguments.",
        source: Some("Flags"),
        attributes: &[
            a("aliases", "table of text", "Other spellings of a flag, by the spelling written in its place: `\"-c\" = \"--ctx-size\"`. A short form, or a second long name, becomes the one it stands for; a flag that is not listed keeps its spelling."),
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
            a("hide", "list of patterns", "Names the engine lists that are its own bookkeeping, not models to pick (`[\"llamacpp:.*\"]`): each pattern must match the whole name. The picker, the recommendation and the cache never see them. A name the engine lists twice is shown once, whatever this says."),
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
        intro: "What the Connect an app panel says to a person who connects an app. Every text is a template over the engine's context (`${public_port}`, `${session.model_display}`).",
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
        heading: "`[[prerequisite]]`",
        intro: "Something the computer has to have for a configuration of the engine to work, how to tell whether it has it, and how to put it right. A configuration says which of them it needs (`[[needs]] system = \"<id>\"`); the packages are named here and nowhere else, because a package that is installed as the administrator must not come from a file that was given away. `quickai-run` looks at the computer before it fetches anything, says what is missing and the exact command that brings it, and installs the packages only when it is told to (`--install-system-packages`).",
        source: Some("Prerequisite"),
        attributes: &[
            a("id", "text", "What a configuration calls it. Letters, digits and `_`. Required."),
            a("label", "text", "What a sentence calls it (`a C compiler`), up to 120 characters. Required."),
            a("when", "condition", "Only on a computer where this holds (`os == 'linux'`); always when left out."),
            a("check", "table, below", "How to tell. Required."),
            a("fix", "table, below", "The packages that bring it, by package manager. Left out, the program only says what is missing, and `hint` what to do."),
            a("hint", "text", "What to tell the person when nothing here can put it right, or when it did not help, up to 600 characters."),
        ],
    },
    Group {
        heading: "`[[prerequisite]]` `check`, `use = \"program\"`",
        intro: "One of the programs is on the computer, in a folder of the `PATH`, or the one a variable names is.",
        source: None,
        attributes: &[
            a("use", "`program`", "The kind of check."),
            a("any_of", "list of text", "Names of programs, any of which will do (`[\"gcc\", \"clang\"]`). Plain names, with no path."),
            a("env", "text", "A variable that names the program instead (`CC`), as the tool that needs it reads it. When it is set and not empty it decides: the program it names must be there, and the packages of `fix` are not offered."),
        ],
    },
    Group {
        heading: "`[[prerequisite]]` `check`, `use = \"python_header\"`",
        intro: "A header file of Python is in the folder of Python's headers, for the Python the engine's environment is made with (the one the install will use, or the one the installed environment was made with). Only for an engine installed by `python_app`. The packages of `fix` are offered only when that Python is the system's.",
        source: None,
        attributes: &[
            a("use", "`python_header`", "The kind of check."),
            a("file", "text", "The file, by its name alone (`Python.h`)."),
        ],
    },
    Group {
        heading: "`[[prerequisite]]` `check`, `use = \"python_module\"`",
        intro: "A module of Python's own library can be imported by the Python the engine's environment is made with: `ensurepip`, which Debian and Ubuntu leave out of the Python and put in the package `python3.12-venv`, without which no environment can be made. Only for an engine installed by `python_app`; made before the install with the Python the install will use, and an environment that exists was made, so it passes. The packages of `fix` are offered only when that Python is the system's.",
        source: None,
        attributes: &[
            a("use", "`python_module`", "The kind of check."),
            a("module", "text", "The module, as Python names it (`ensurepip`)."),
        ],
    },
    Group {
        heading: "`[[prerequisite]]` `check`, `use = \"nvidia_driver\"`",
        intro: "The driver of an NVIDIA card is of this version or a newer, read from the driver's own file or from `nvidia-smi`. A driver is not installed by this program: say what to do in `hint`.",
        source: None,
        attributes: &[
            a("use", "`nvidia_driver`", "The kind of check."),
            a("at_least", "text", "The version, as numbers with dots between them (`580`, `580.65`), compared number by number."),
        ],
    },
    Group {
        heading: "`[[prerequisite]]` `check`, `use = \"python_script\"`",
        intro: "A script of the application's own files is run by the Python of the engine's environment, with the environment the engine is started with, and passes when it ends with status 0. What it prints is shown when it fails. It tests that the installed environment works, so it is run after the install and before the engine is started. Only for an engine installed by `python_app`.",
        source: None,
        attributes: &[
            a("use", "`python_script`", "The kind of check."),
            a("asset", "text", "The script, as a name of the application's own files, ending in `.py` (`comfyui/selftest/triton_kernel.py`)."),
            a("timeout", "duration", "How long it may take (default `120s`)."),
        ],
    },
    Group {
        heading: "`[[prerequisite]]` `fix`",
        intro: "The packages that bring a prerequisite, for each package manager that has them. Plain package names only. `${python.version}` (`3.12`) and `${python.digits}` (`312`) stand for the Python of the environment. Only for `program`, `python_module` and `python_header`.",
        source: Some("Fix"),
        attributes: &[
            a("apt", "list of text", "Debian and Ubuntu, installed with `apt-get` (its list of packages is refreshed first)."),
            a("dnf", "list of text", "Fedora and Red Hat, installed with `dnf`."),
            a("zypper", "list of text", "openSUSE, installed with `zypper`."),
            a("pacman", "list of text", "Arch, installed with `pacman`."),
            a("apk", "list of text", "Alpine, installed with `apk`."),
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
            a("archive", "address", "Where the source archive of a release is fetched from, for `assets` of `by = \"source\"`: `<archive>/<owner>/<name>/archive/refs/tags/<tag>.tar.gz`. Default `https://github.com`; as with `api`, only documents QuickAI trusts may name another."),
        ],
    },
    Group {
        heading: "`[acquire]` `feed`, `use = \"pypi\"`",
        intro: "The releases of a package of the Python Package Index, for an application that is installed from its wheel (`assets` of `by = \"wheel\"`). The index lists a sha256 for every file, so every build carries one.",
        source: None,
        attributes: &[
            a("use", "`pypi`", "The kind of feed."),
            a("package", "text", "The package, as the index names it (`open-webui`)."),
            a("show", "whole number", "How many releases that have a wheel to show. Default 5."),
            a("prereleases", "bool", "Whether a release the index calls a pre-release (`1.0rc1`) counts. Default false."),
            a("api", "address", "The root of the index's JSON API (`https://pypi.org`, the default). Another root, `http` or `https`, is how a test points the feed at a fake; only documents QuickAI trusts may name one."),
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
        heading: "`[acquire]` `assets`, `by = \"source\"`",
        intro: "The release's own source archive is the build, for an application that is installed from its source (`install` of `use = \"python_app\"`). Every release has one, on every platform; its digest is not listed by the host, so only the pin of the tested release can check it.",
        source: None,
        attributes: &[
            a("by", "`source`", "The kind of rule."),
            a("label", "template", "The directory a build is installed under, over `tag`, `stem` and `asset`. Default `${tag}`."),
        ],
    },
    Group {
        heading: "`[acquire]` `assets`, `by = \"wheel\"`",
        intro: "The release's wheel is the build, for an application that is installed from the package index (`feed` of `use = \"pypi\"`, `install` of `use = \"python_app\"` with a `console_script`): the one file of the release that is a wheel for every platform (`py3-none-any`). It is downloaded, checked against its sha256, and installed into an environment with what it needs.",
        source: None,
        attributes: &[
            a("by", "`wheel`", "The kind of rule."),
            a("label", "template", "The directory a build is installed under, over `tag`, `stem` and `asset`. Default `${tag}`."),
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
        heading: "`[acquire]` `install`, `use = \"python_app\"`",
        intro: "Install an application written in Python from its source: the source archive is checked and unpacked as `app/`, a Python makes a virtual environment `venv/` beside it, the `pip` steps that apply to this machine install into it (only as wheels: no package runs a build script on the person's machine), the `nodes` are unpacked into the source, and a launcher is written that runs the application with that environment. The launcher is the build's executable. Everything is made in a directory of its own that is renamed into place when it is whole, and can be stopped at any moment.",
        source: None,
        attributes: &[
            a("use", "`python_app`", "The kind of install."),
            a("binary", "`{ name }`", "The launcher the installer writes (`comfyui`; `comfyui.cmd` on Windows)."),
            a("entry", "text", "The script of the unpacked source that the launcher runs, relative to it (`main.py`). One of `entry` and `console_script` is needed."),
            a("console_script", "text", "For a build that is a wheel, which has no source to run a script of: the program the wheel puts in the environment (`open-webui`). The installer asks the environment which function it calls, and the launcher has the environment's Python call it; the script itself is never run, because it holds the path of the folder it was made in, and a build is moved to its name when it is whole."),
            a("python", "`{ versions }`", "The versions of Python to look for, best first (`[\"3.13\", \"3.12\"]`): on the `PATH`, in pyenv's and uv's folders, and through the Windows launcher. The person may name a Python of their own."),
            a("verify", "`none` or `digest`", "`digest`: compare the source archive with the sha256 this document pins for the tested release before anything is unpacked; another release is installed and said to be unchecked. Default `none`."),
            a("smoke", "list of text", "A command that must succeed on the finished build before it counts as installed, starting with the launcher: `[\"${binary}\", \"--quick-test-for-ci\"]`."),
            a("smoke_timeout", "duration", "How long the smoke test may take (default `120s`)."),
            a("space", "size", "How much free room an install needs (`30GiB`): the download and the environment together. Checked before anything is asked of the network."),
            a("pip", "list of tables, below", "The packages of the environment, in the order they are installed."),
            a("nodes", "list of tables, below", "Packs of extra nodes, unpacked into the source."),
            a("options", "list of tables, below", "Parts of the install that are the person's to choose: a box in the window, ticked or not, and the `pip` steps that belong to it."),
        ],
    },
    Group {
        heading: "`[[acquire.install.options]]`",
        intro: "One part of the install that the person chooses (ComfyUI-Manager), and what the build made with it does differently. Its steps are the `pip` steps that name it; the window offers it as a box, above the list of builds, and what was chosen is what the install does, what the line under the bar counts and what an install that stopped is gone on with. A caller that chooses nothing (the headless runner) gets the install without it.",
        source: Some("InstallOption"),
        attributes: &[
            a("id", "text", "Letters, digits and `_`; the `pip` steps that belong to it name it with `option`."),
            a("label", "text", "What the box says (`Install ComfyUI-Manager`)."),
            a("badge", "text", "A few words beside the label, in the colour of a recommendation (`highly recommended`)."),
            a("help", "text", "What the pointer on the box is told."),
            a("default", "bool", "Ticked until the person unticks it. Default false: a part nobody asked for is not installed."),
            a("launch_args", "list of text", "What the launcher of a build made with it passes to the program before the arguments it is given, one plain word each (`--enable-manager-legacy-ui`): the build is the one that knows what it has, so a build made without it is started without it."),
        ],
    },
    Group {
        heading: "`[[acquire.install.pip]]`",
        intro: "One `pip install`.",
        source: Some("PipStep"),
        attributes: &[
            a("id", "text", "Letters, digits and `_`; a document of the person's replaces a step by it."),
            a("label", "text", "What the log and the window call it (`PyTorch for AMD GPUs (ROCm 7.2)`)."),
            a("when", "condition", "Only on a machine where this holds (`has(gpu.vendors, 'amd')`); always when left out."),
            a("option", "text", "Only when the person has chosen this option of the install (the `id` of an `[[acquire.install.options]]`); always when left out."),
            a("packages", "list of text", "Packages, as requirement specifiers (`torch`, `torchvision==0.29.1`)."),
            a("requirements", "text", "A requirements file of the unpacked source, relative to it (`requirements.txt`)."),
            a("wheel", "bool", "Installs the wheel the build is (`assets` of `by = \"wheel\"`): the file that was downloaded and checked, with what it needs from the index. A build that is a wheel has exactly one such step, and it names no packages or requirements file."),
            a("index", "address", "The package index the packages come from instead of PyPI. It is the only one used: a second could offer a newer package built for something else."),
            a("weight", "whole number", "How much of the progress bar this step is, next to the others (default 1)."),
        ],
    },
    Group {
        heading: "`[[acquire.install.nodes]]`",
        intro: "One pack of extra nodes (a custom node of ComfyUI), pinned to one commit by the sha256 of its archive.",
        source: Some("NodePack"),
        attributes: &[
            a("id", "text", "Letters, digits and `_`."),
            a("label", "text", "What the log and the window call it."),
            a("when", "condition", "Only on a machine where this holds; always when left out."),
            a("url", "address", "A `.tar.gz` of one commit."),
            a("sha256", "text", "What the archive must hash to, as `sha256:` and 64 hexadecimal digits. Always checked."),
            a("into", "text", "Where it goes, relative to the unpacked source (`custom_nodes/ComfyUI-GGUF`)."),
            a("requirements", "text", "Its requirements file, relative to the pack; installed into the environment."),
            a("license", "text", "Its licence, for the About window."),
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
        heading: "`[[model_type]]`",
        intro: "A type of model, which is a section of the results of a look-up (Chat, Code, Vision, ...). A model is of the first type, in the order written, whose `match` holds, else of the `default` one; the sections stand in the order of `order`. The list merges by `id`, so a document of the person's can change a type or add one (a new one is tried after the embedded ones).",
        source: Some("ModelTypeDef"),
        attributes: &[
            a("id", "text", "Lower case letters, digits and `.`, `_`, `-` (`vision`). Required. An engine's `types` list names them, and `other` is where what an engine does not run is shown."),
            a("label", "text", "The heading of its section (`Vision`); default the id."),
            a("help", "text", "What it means, shown when the pointer rests on the heading."),
            a("order", "whole number", "Where its section stands: lowest first; default after the ones that say, in the order written."),
            a("default", "bool", "What a model is when no other type matches. At most one type is the default, across all vocabularies. Default false."),
            a("match", "table, below", "What makes a model of this type; any one hit is enough. A type that is not the default and has no `match` is a warning: nothing is ever of it."),
        ],
    },
    Group {
        heading: "`[model_type.match]`",
        intro: "What a type's rule looks at, each list in any case.",
        source: Some("TypeRuleDef"),
        attributes: &[
            a("capabilities", "list of text", "What the source says the model can do: an Ollama card's `vision`, `embedding`, `audio`."),
            a("pipelines", "list of text", "The pipeline tag of a Hugging Face repository (`image-text-to-text`, `feature-extraction`)."),
            a("tags", "list of text", "Another tag of the repository (`conversational`)."),
            a("words", "list of text", "A word of the model's name (`qwen3-coder` has `qwen3` and `coder`). A word of four letters or more also matches the start of one (`embed` is in `embeddinggemma`); a shorter one only itself (`vl` is not in `vllm`)."),
            a("companions", "list of text", "A kind of companion file found beside the model by an engine's bundles (`mmproj`): a repository with a projector is a vision model whatever its tags say."),
        ],
    },
    Group {
        heading: "`[[size_class]]`",
        intro: "A band of sizes, which is a button of the size filter of a look-up. A model's button belongs to the first class, from the smallest up, that its size is below. Write them from the smallest to the largest; the one with no `below` is last. The list merges by `id`.",
        source: Some("SizeClassDoc"),
        attributes: &[
            a("id", "text", "Lower case letters, digits and `.`, `_`, `-` (`small`). Required."),
            a("label", "text", "What the filter says (`Small (< 4 GB)`); default the id."),
            a("help", "text", "A sentence for the person who wonders what it is."),
            a("below", "size", "Where it ends: sizes below it are in the class (`4GiB`, `500MB`; `GB` counts thousands, `GiB` 1024s). Left out, the class takes every size beyond the last bound."),
        ],
    },
    Group {
        heading: "`[[task]]`",
        intro: "Something a person can want a model for, which is a button of the first page of the recommended list (Chat, Write code, Read pictures, ...; docs/catalog.md, section 1). A person chooses one or several, and the page shows the models of the list that do every one. A model does a task when the task's `match` holds for it or when it says so itself (its `tasks`); a task no model of the engine does has no button. The list merges by `id`.",
        source: Some("TaskDoc"),
        attributes: &[
            a("id", "text", "Lower case letters, digits and `.`, `_`, `-` (`pictures`). Required."),
            a("label", "text", "What its button says (`Read pictures`); default the id."),
            a("help", "text", "What it means, shown when the pointer rests on the button."),
            a("order", "whole number", "Where its button stands: lowest first; default after the ones that say, in the order written."),
            a("match", "table, below", "Which models do it; any one hit is enough."),
        ],
    },
    Group {
        heading: "`[task.match]`",
        intro: "What a task's rule looks at. An id that no vocabulary has matches nothing.",
        source: Some("TaskRuleDoc"),
        attributes: &[
            a("types", "list of text", "Ids of `[[model_type]]`. The first is the task's own: the page picks from the models of that type first, so that a person who asks for Chat is shown assistants before the code models that also chat."),
            a("input", "list of text", "Ids of `[[modality]]` the model reads (`image`)."),
            a("features", "list of text", "Features a model of the list has (`thinking`)."),
        ],
    },
    Group {
        heading: "`[[finding]]`",
        intro: "The sentence of one note that a person is told about a configuration (docs/configurations.md, section 7): `too_new`, `unknown_engine`, `build_missing` and the rest. The code decides when a note is made and how much it matters (it stops an import, asks a question, warns or only says); a document can reword it and cannot change that. A vocabulary that gives some of the notes gives all of them. The list merges by `id`, so a document of the person's can reword one.",
        source: Some("FindingDef"),
        attributes: &[
            a("id", "text", "The note it is for. One this version does not make is a warning."),
            a("text", "text", "The sentence. `{name}` is replaced by the word the note has for it (`{needs}`, `{this}`, `{model}`...); one the note does not have is an error, and the error lists the ones it has. Required."),
        ],
    },
    Group {
        heading: "`[profile]`",
        intro: "What a configuration file may and may not do.",
        source: Some("ProfileRules"),
        attributes: &[
            a("deny_env", "list of text", "Names of environment variables no configuration may set, in any case; a trailing `*` stands for any ending (`DYLD_*`). They add to a short list the application always refuses (`LD_*`, `DYLD_*`, `PATH`, `PYTHON*`), which no document can take away: a file from outside must not make a program load other code."),
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
        intro: "One example for the API panel: a curl command, and the same request in Python and JavaScript when they are given (the panel has a tab for each).",
        source: Some("ApiExample"),
        attributes: &[
            a("label", "text", "What the example shows (`Chat with an image`). Required."),
            a("when", "condition", "Show it only while this holds (`has(model.modalities.input, 'image')`)."),
            a("curl", "text", "The command, as text. Required."),
            a("python", "text", "The same request in Python, as text."),
            a("javascript", "text", "The same request in JavaScript, as text."),
        ],
    },
];

/// The page for vocabulary documents, as Markdown.
pub fn render_vocab() -> String {
    let mut out = String::new();
    out.push_str("# Vocabulary documents: modalities, APIs, types of model and sizes\n\n");
    out.push_str(
        "_Generated from the types in `crates/quickai-core/src/doc/vocab.rs` by `quickai config docs vocab`. \
         Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` \
         rewrites it, and a test fails while it is out of date._\n\n",
    );
    out.push_str(
        "A vocabulary document (`kind = \"vocab\"`) names what engines and the window have in common: \
         the kinds of input and output a model can have, the APIs an engine can serve, with where the \
         text is in the requests and replies of each, so that the Live tab can read the traffic of an \
         engine the code has never heard of, the types of model and the classes of size a look-up \
         sorts what it found by, and the sentences of the notes a person is told about a configuration. A vocabulary can name things and describe them; it cannot \
         start a program or reach a host. A document in the user's configuration directory with the same \
         `id` patches the embedded one: tables merge by key and `[[modality]]`, `[[api]]`, `[[model_type]]`, \
         `[[size_class]]`, `[[task]]` and `[[finding]]` entries by `id` \
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
         not both;\n\
         - a model type id is a name and is used once (also across documents), at most one type is the \
         `default`, and a list of its `match` has no empty entry;\n\
         - a size class id is a name and is used once, every `below` is a size, the bounds go up from one class \
         to the next, and the class with no bound is the last;\n\
         - a `[[finding]]` has the `id` of a note this version makes (else a warning), a `text` that is not empty and \
         uses only the words its note fills in, and is given once; the notes of the document all have a sentence \
         when it has any;\n\
         - a name of `deny_env` is the name of an environment variable, with `*` allowed at the end, and not `*` alone.\n\n\
         An engine that names an API in `[provides]` `apis` that no vocabulary defines does not load (when \
         there is a vocabulary at all), with the nearest name suggested.\n",
    );
    out
}

/// The tables of a catalog document (`kind = "catalog"`), by the types in `doc/catalog.rs`.
pub const CATALOG_GROUPS: &[Group] = &[
    Group {
        heading: "the document",
        intro: "The top level of a catalog document, besides the envelope (`schema`, `kind`, `id`). Every document of kind `catalog` is a page of the one list: the application ships several and a person adds their own.",
        source: Some("CatalogDoc"),
        attributes: &[
            a("title", "text", "What the page is called, for messages; default the document's `id`."),
            a("verified", "text", "The day its facts were last checked against the sites, `YYYY-MM-DD`; the window says the list is as old as its oldest page."),
            a("model", "list of tables, below", "The models of the page. The list merges by `id`, so a document with the same `id` as a shipped one patches it model by model."),
        ],
    },
    Group {
        heading: "`[[model]]`",
        intro: "One model of the list. A model id is unique across every catalog document: a second document that writes it is refused and the message names the first.",
        source: Some("ModelDoc"),
        attributes: &[
            a("id", "text", "Lower case letters, digits and `.`, `_`, `-`, beginning with a letter or a digit (`qwen3.5-4b`). Required."),
            a("name", "text", "What the window calls it; default the id."),
            a("by", "text", "Who made it."),
            a("type", "text", "An id of the vocabulary's `[[model_type]]` (`chat`, `code`, `vision`, ...); default the vocabulary's default type. One no vocabulary defines is an error, with the nearest one suggested."),
            a("note", "text", "What it is good for, in a sentence or two (a warning beyond 600 characters)."),
            a("tagline", "text", "What it is in a few words, the line under its name on the first page of the list (a warning beyond 80 characters)."),
            a("license", "text", "Its licence, as its makers name it."),
            a("params", "text", "How big it is as its makers say (`4B`, `30B-A3B`)."),
            a("context", "whole number", "The tokens of context it was made for."),
            a("input", "list of text", "What it reads, as ids of the vocabulary's `[[modality]]` (`text`, `image`, `audio`); default `text`."),
            a("features", "list of text", "What else is worth knowing, as short lower case words of 24 characters at most, eight at most (`thinking`, `tools`, `multilingual`). They are searched."),
            a("tasks", "list of text", "Ids of the vocabulary's `[[task]]` it does besides the ones the tasks' rules give it (`scans` for a vision model that reads documents well). One no vocabulary defines is an error."),
            a("start", "bool", "Whether \"Start here\" shows it, and whether the first page of the list picks it before the others. Default false."),
            a("bundled", "bool", "Whether the packages ship it, so that it counts as there when its file is in the folder of the models that come with the application. Default false."),
            a("hidden", "bool", "A person's document sets it to take a shipped model out of the list: it is not checked and takes no name. Default false."),
            a("run", "table of tables, below", "How each engine gets and runs it, `[model.run.<engine id>]`. An engine with no table does not offer the model; a model that no engine offers is a warning. An engine id that does not exist is an error."),
        ],
    },
    Group {
        heading: "`[model.run.<engine>]`",
        intro: "What one engine does with the model. Which keys are meant depends on what the engine accepts (`[provides]` `accepts`): an engine whose models are files (`formats`) takes `repo`, `rev`, `files`, `companions`, `mode` and `place`; an engine that pulls names (`refs`) takes `ref` and `size`. The other keys are errors, not ignored.",
        source: Some("RunDoc"),
        attributes: &[
            a("repo", "text", "Files: the repository they are in, `owner/name`. Required for files."),
            a("rev", "text", "Files: the commit they are fetched at; default `main`. The application's own documents give a commit, so that what was checked is what arrives."),
            a("mode", "text", "Files: one of the engine's `[[models.mode]]`, how it is started for this model (`embedding`). One the engine does not have is an error."),
            a("place", "text", "Files: the folder of the models folder the weights are kept in, for an engine that finds its files by the folder they are in (ComfyUI) and a model whose weights are not where it keeps weights (`checkpoints`). A plain name."),
            a("files", "list of tables, below", "Files: the weights on offer, one for each quantization. Required for files. They are shown smallest first."),
            a("companions", "table of tables, below", "Files: the files that go with the weights, by the kind of component they are (`mmproj = { file = ... }`). The engine must be able to attach each kind (`[components.<kind>]`) and have a `[[models.bundle]]` that pairs weights with exactly these kinds; the weights are then offered in a second row, `Get with projector`, as one download."),
            a("ref", "text", "A name: what the engine pulls (`qwen3.5:4b`). Required for a name."),
            a("size", "whole number", "A name: what the pull downloads, in bytes, as the registry says."),
        ],
    },
    Group {
        heading: "`files` and `companions`",
        intro: "A file of the repository.",
        source: Some("FileDoc"),
        attributes: &[
            a("file", "text", "Its path in the repository (`model-Q4_K_M.gguf`, `gguf/model.gguf`): relative, with no `..`, and of a format the engine takes. Required."),
            a("size", "whole number", "Its size in bytes; a download is refused if it comes to another size."),
            a("sha256", "text", "Its SHA-256, 64 hexadecimal digits: what a download is checked against while it arrives, and a file that hashes to something else is not kept."),
            a("label", "text", "What its button says; default the quantization its name says (`Q4_K_M`), else the name."),
            a("best", "bool", "Whether it is the one the list recommends of the weights (shown in bold); only one may be. Default: the first."),
            a("store_as", "text", "The name it is kept under in the models folder, when its own is another repository's file of the same name (two such files would overwrite each other): a plain file name with the same ending. Default: its own name."),
        ],
    },
];

/// The page for catalog documents, as Markdown.
pub fn render_catalog() -> String {
    let mut out = String::new();
    out.push_str("# Catalog documents: the list of recommended models\n\n");
    out.push_str(
        "_Generated from the types in `crates/quickai-core/src/doc/catalog.rs` by `quickai config docs catalog`. \
         Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` \
         rewrites it, and a test fails while it is out of date._\n\n",
    );
    out.push_str(
        "A catalog document (`kind = \"catalog\"`) says which models the application recommends, what is known of each \
         and, for each engine, how to get it and how to run it (docs/catalog.md). Catalog documents are data: \
         they can name files and say where they are, and they cannot start a program, add a word to a command \
         line or reach a host of their own (the hub files are fetched from is the one the engine's own look-up of \
         Hugging Face names). The application ships its list in several documents, one for each kind of model; \
         a document of the person's with the same `id` as a shipped one patches it model by model, and one with \
         a new `id` adds a page of their own. Tables merge by key and `[[model]]` entries by `id` \
         (docs/architecture.md, section 5.2). A key this version does not know is a warning; \
         `quickai config check --strict` makes warnings fail.\n\n",
    );
    render_groups(&mut out, CATALOG_GROUPS);
    out.push_str(
        "## What is checked\n\n\
         Each of these is an error that names the file and the place, and a document with an error in it \
         defines no models (the document below it, if any, stays in force):\n\n\
         - a model id is a name and is written once, in one document;\n\
         - `type` is a type the vocabulary defines, and `input` names only modalities it defines;\n\
         - `features` are lower case words of 24 characters at most, eight at most, and `context` is not 0;\n\
         - every `[model.run.<engine>]` names an engine that exists, and has the keys that engine's models need \
         and none of the others;\n\
         - `repo` is `owner/name`, `rev` is a commit or a branch, every `file` is a path inside the repository of a \
         format the engine takes, listed once, `size` is not 0 and `sha256` is 64 hexadecimal digits;\n\
         - at most one file is `best`;\n\
         - every companion kind is one the engine has a `[components.<kind>]` for, and the engine has a \
         `[[models.bundle]]` that pairs weights with exactly the kinds a model has;\n\
         - `mode` is one of the engine's `[[models.mode]]`;\n\
         - `ref` is a name the engine can pull.\n\n\
         A warning is a model no engine offers and a note of more than 600 characters.\n",
    );
    out
}

/// The tables of a configuration document (`kind = "profile"`), by the types in `doc/profile.rs`.
pub const PROFILE_GROUPS: &[Group] = &[
    Group {
        heading: "the document",
        intro: "The top level of a configuration, besides the envelope (`schema`, `kind`, `id`, `requires`, and `replace` for a file of the person's that stands in for one of the same `id` in a lower layer). The keys that are not tables come first in the file.",
        source: Some("ProfileDoc"),
        attributes: &[
            a("name", "text", "What the dropdown and the list call it, up to 60 characters; control and direction characters are taken out. Default: the `id`. The `id` is made from it (lower case, letters, digits and `-`) and is never a path."),
            a("meta", "table, below", "For the person who is given the file. Nothing in it changes what runs."),
            a("engine", "text", "The id of the engine it runs: one a person can choose, not a companion. Required. An engine this version does not have is told to the person who opens the file, and nothing is stored."),
            a("runtime", "table, below", "Which program, as who it is and not where it is."),
            a("model", "list of tables, below", "The models it uses, as who they are and where they can be had from, never as a path and never the model itself. The first is the one the engine loads."),
            a("needs", "list of tables, below", "What whoever runs the configuration has to give besides the file: a login, a setting that belongs to a computer, a variable of a program's environment. The file never carries them."),
            a("values", "table of tables", "The value of settings: `[values.<engine>]` holds the fields of that engine by their ids (`ctx_size = 16384`), and a companion's settings sit in a table of its own (`[values.openwebui]`). Only what differs from the default is written. A field whose `share` is `never`, and one the engine does not have, is told and ignored; a value that does not fit its field is told and the default is used."),
            a("app", "table, below", "Settings of the application that are part of a run."),
        ],
    },
    Group {
        heading: "`[meta]`",
        intro: "Who made the file and for what. Coarse on purpose: no name of a person or of a computer is ever written.",
        source: Some("MetaDoc"),
        attributes: &[
            a("note", "text", "A sentence or two from whoever made the file."),
            a("made_with", "text", "The application that wrote it (`QuickAI 0.1.0`)."),
            a("made_on", "`{ os, arch, gpu, memory_gb }`", "The kind of computer it was made on: `linux`, `windows` or `macos`; `x86_64` or `aarch64`; the makers of the graphics cards (`amd`, `nvidia`, `intel`); the memory in whole gigabytes. The person who opens the file is told where the computer differs from theirs."),
            a("community", "`{ id, version }`", "The configuration of quickai.everybodyai.dev the file was fetched from (`{ id = \"vision-chat-16gb\", version = 3 }`). QuickAI writes it when it imports one, and an export keeps it, so that a configuration made from another can say what it is based on. Nothing in it changes what runs."),
        ],
    },
    Group {
        heading: "`[meta]` `community`",
        intro: "Where on quickai.everybodyai.dev a configuration came from.",
        source: Some("CommunityDoc"),
        attributes: &[
            a("id", "text", "The configuration's id on the site."),
            a("version", "whole number", "Its version."),
        ],
    },
    Group {
        heading: "`[meta]` `made_on`",
        intro: "The kind of computer a file was made on.",
        source: Some("MadeOnDoc"),
        attributes: &[
            a("os", "text", "`linux`, `windows` or `macos`."),
            a("arch", "text", "`x86_64` or `aarch64`."),
            a("gpu", "list of text", "The makers of the graphics cards."),
            a("memory_gb", "whole number", "The memory, in whole gigabytes."),
        ],
    },
    Group {
        heading: "`[runtime]`",
        intro: "The program a configuration runs, and the programs that go with it. Left out, it is the program the computer uses by default.",
        source: Some("RuntimeDoc"),
        attributes: &[
            a("build", "text", "The label of a build in the manager's list (`v0.40.0`): the name of the directory it is installed in, with no folder in it. Choosing the configuration sets the engine's program to the installed build of that label, and to nothing else. One that is not installed is told, with the labels that are."),
            a("version", "text", "The version the program said it was, for the notes."),
        ],
    },
    Group {
        heading: "`[runtime.<program>]`",
        intro: "A program that goes with the engine (`[runtime.openwebui]` for Open WebUI beside Ollama), with the same two keys. `quickai-run` installs that build of it, and the window leaves the file's pin as it is when it saves. A key that is none of these is one this version does not know.",
        source: None,
        attributes: &[
            a("build", "text", "The label of the build of that program to install and run."),
            a("version", "text", "The version the program said it was, for the notes."),
        ],
    },
    Group {
        heading: "`[[model]]`",
        intro: "One model, by exactly one source: a name the engine takes (the key is one of the engine's `accepts.refs`: `ollama = \"qwen3:8b\"`), an entry of the recommended list (`catalog`, and `file` for one of its files), or a file (`file`) with, if it is to be fetched, where it can be had from (`from`, and `path` and `revision` in it, or `url`). A path of a computer is never a source, and the model is never in the file. Choosing a configuration in the window selects a model that is here and fetches nothing: one that is not here is told, with its size and where it can be had. `quickai-run` fetches it from where the file says, once whoever runs it has read where that is.",
        source: Some("ModelDoc"),
        attributes: &[
            a("role", "text", "What it is for (`chat`, `embedding`); letters, digits, `_` and `-`. Default `main`."),
            a("name", "text", "What people call it; nothing finds it by this."),
            a("catalog", "text", "The id of an entry of the recommended list."),
            a("file", "text", "A file name, with no folder in it: the file of a `catalog` entry, or a file of the models folder."),
            a("size", "whole number", "Its size in bytes, to recognise it by and to say what getting it costs."),
            a("sha256", "text", "Its SHA-256, 64 hexadecimal digits."),
            a("from", "text", "Where it can be got from, as a kind of place and a name in it (`huggingface:owner/repo`), for a file that is no entry of the recommended list. The window says it and never fetches from it by itself (the person asks, and looks it up); `quickai-run` fetches from it."),
            a("path", "text", "Where in the repository `from` names the file is (`split_files/vae/wan2.2_vae.safetensors`): names joined by `/`, none of them `..`, at most eight. Left out, the file is at the top and is called `file`. Needs `from`."),
            a("revision", "text", "The branch, tag or commit of the repository: letters, digits, `.`, `_` and `-`. Left out, `main`. A commit is the same file for ever. Needs `from`."),
            a("url", "text", "An address the file can be fetched from instead of `from`: `https://host/path`, with no sign-in in it (`http://` only for this computer's own). Not with `from`."),
            a("into", "text", "The folder of the models folder the file is kept in (`diffusion_models`, `vae`), for an engine that finds its files by the folder they are in: one name of lower case letters, digits, `_` and `-`. Left out, the folder the engine's document gives a file like this."),
        ],
    },
    Group {
        heading: "`[[needs]]`",
        intro: "One thing that whoever runs the configuration has to give, because the file cannot carry it, or that the computer has to have: exactly one of `token`, `setting`, `env` and `system`. `quickai-run` stops before it fetches anything, says which are missing and how to give them, and `quickai-run init` writes the file to fill in; it is given in an environment file, on the command line, or, for a login, in the usual variable. What the computer has to have is looked at on the computer, and the engine's document says how.",
        source: Some("NeedDoc"),
        attributes: &[
            a("token", "text", "A login the fetching needs: `huggingface` or `github`."),
            a("setting", "text", "A setting that belongs to a computer, as `<engine>.<field>` (`comfyui.models_dir`)."),
            a("env", "text", "A variable of a program's environment (`WEBUI_SECRET_KEY`)."),
            a("system", "text", "Something the computer has to have, as the id of a `[[prerequisite]]` of the engine's document (`c_compiler`): the document says how it is checked and what brings it, and the file says only that it is needed and why. An id the document does not know is refused when the file is read."),
            a("for", "text", "The program an `env` goes to (`openwebui`). Left out, the engine of the configuration."),
            a("secret", "bool", "Whether the value is a secret, that is never shown. A token always is one, and so is a variable whose name has KEY, TOKEN, SECRET or PASSWORD in it."),
            a("why", "text", "Said to whoever has not given it, up to 300 characters."),
        ],
    },
    Group {
        heading: "`[app]`",
        intro: "Settings of the application, not of an engine, that change a run.",
        source: Some("AppDoc"),
        attributes: &[
            a("traffic_proxy", "bool", "Whether the live-traffic proxy sits in front of the engine."),
        ],
    },
];

/// The page for configuration documents, as Markdown.
pub fn render_profile() -> String {
    let mut out = String::new();
    out.push_str("# Configuration documents: a runtime as a file\n\n");
    out.push_str(
        "_Generated from the types in `crates/quickai-core/src/doc/profile.rs` by `quickai config docs profile`. \
         Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` \
         rewrites it, and a test fails while it is out of date._\n\n",
    );
    out.push_str(
        "A configuration document (`kind = \"profile\"`) says what QuickAI runs and how: the engine, the model, which \
         build of the program, the settings and command-line arguments of the engine and of what goes with it. The \
         window has one active configuration, a dropdown chooses it, and a file can be given to someone else, who is \
         told what their computer lacks or does differently before anything is taken (docs/configurations.md). \
         Configurations are data: they can pick things the application already knows (an engine, a model, a build, \
         the value of a setting), say where a model file can be had from, and cannot name a program. They never \
         carry a model, a folder of a computer, a login or a secret. The application \
         ships `default`; a package may ship more in `<exe>/config/profiles/`; the person's own are files in \
         `profiles/` of their configuration directory, and one with the same `id` and `replace = true` stands in for \
         a shipped one. A key this version does not know is kept when the file is written again and told as a \
         warning; `quickai config check --strict` makes warnings fail.\n\n",
    );
    render_groups(&mut out, PROFILE_GROUPS);

    out.push_str(
        "## What travels: `share`\n\n\
         Which settings a configuration holds is not decided here but by the engine documents: each `[[field]]` has \
         a `share` (`yes`, `review` or `never`, see `quickai config docs`). When it says nothing the type decides:\n\n\
         | `type` | `share` |\n|---|---|\n",
    );
    for (ty, _, _) in TYPES {
        let field: Result<crate::doc::engine::Field, _> =
            toml::from_str(&format!("id = \"x\"\ntype = \"{}\"\n", type_name(*ty)));
        let share = match field.ok().and_then(|f| f.share()) {
            Some(share) => format!("`{}`", share.name()),
            None => "not a setting: the model has a section of its own".to_string(),
        };
        out.push_str(&format!("| `{}` | {share} |\n", type_name(*ty)));
    }
    out.push_str(
        "\nA setting that is `review` travels, and the person who is given a file that has it is shown it in full and \
         asked before it is taken: it becomes a command-line word, a line of environment or an address, or it widens \
         what a program may do. A setting that is `never` is a folder, a file or a program, or belongs to a computer; \
         it is not written and is ignored when it is read. The names of `deny_env` of the vocabulary (and `LD_*`, \
         `DYLD_*`, `PATH`, `PYTHON*`, which are always refused) cannot be set from a configuration, even when it is \
         ticked.\n\n\
         ## What is checked\n\n\
         Each of these is an error that names the file and the place, and a document with an error in it is not used \
         (the document below it, if any, stays in force):\n\n\
         - the file is TOML, no more than 256 KiB, with a `schema` this version reads and a `requires.app` it meets;\n\
         - `engine` is an id; `name` has no more than 60 characters;\n\
         - a `[[model]]` has exactly one source, of the kind the engine takes, and no name in it is a path (no `/` or \
         `\\\\` in a file, no `..`, no drive, no NUL, no control character);\n\
         - `runtime.build` is a label, not a path;\n\
         - a model's `path` is names joined by `/` with no `..`, its `revision` is a branch, tag or commit, its \
         `url` is `https://host/path` with no sign-in, its `into` is one folder name, and `from` and `url` are \
         not both given;\n\
         - a need is exactly one of `token`, `setting` and `env`;\n\
         - at most 512 settings, eight models and sixteen needs.\n\n\
         A warning, that leaves the document in use: a setting the engine does not have or does not carry, a value \
         that does not fit its field (the default is used), keys this version does not know.\n",
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
