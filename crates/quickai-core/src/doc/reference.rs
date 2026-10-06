//! The reference page for engine documents (docs/implementation-plan.md, WP2.7).
//!
//! What a document may say is defined by the types in `doc/engine.rs`; this module describes each
//! key in a sentence and renders the whole as Markdown, which is checked in as
//! `docs/reference/engine-fields.md`. `quickai config docs` prints the same text. A test compares
//! the two, another compares the keys listed here with the fields of the types, so a key added to a
//! type without a line here (or the reverse) fails the build rather than leaving the page behind.

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
            a("egress", "list of text", "The hosts the engine itself contacts, which the privacy table lists."),
        ],
    },
    Group {
        heading: "`[provides]`",
        intro: "What a running engine offers.",
        source: Some("Provides"),
        attributes: &[
            a("apis", "list of text", "The ids of the APIs it serves."),
            a("web_ui", "`{ path, label }`", "Its built-in web page, if it has one; the button that opens it carries `label`."),
            a("accepts", "`{ formats }`", "The model file formats it loads."),
            a("model_name_matters", "bool", "Whether a request naming another model loads that one."),
            a("default_port", "whole number", "The port it uses when nothing says otherwise."),
        ],
    },
    Group {
        heading: "`[locate]`",
        intro: "Finding the executable without touching the network. `strategies` is a list, tried in order, and the first that yields a path wins. Each entry has `use` and its own keys:",
        source: Some("Locate"),
        attributes: &[
            a("strategies", "list of tables", "`{ use = \"setting\", field }` takes the path a field holds; `{ use = \"next_to_exe\", file }` looks beside the application's executable; `{ use = \"dev_tree\", path }` looks below the source checkout the application was built in."),
        ],
    },
    Group {
        heading: "`[launch]`",
        intro: "How the engine is started.",
        source: Some("Launch"),
        attributes: &[
            a("strategies", "list of text", "The ways to bring it up, in order. This version knows `spawn`."),
            a("spawn", "table", "The program to start, below."),
            a("ready", "table", "How to tell it is up, below."),
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
            a("group", "bool", "Start it in its own process group (not used yet)."),
            a("stop", "table", "How to stop it: `{ policy = \"kill\" }`."),
        ],
    },
    Group {
        heading: "`[[launch.spawn.env]]`",
        intro: "One environment variable: `value` or `prepend`, not both.",
        source: Some("EnvEntry"),
        attributes: &[
            a("name", "text", "The variable."),
            a("value", "template", "Set it to this."),
            a("prepend", "template", "Put this in front of what the variable already holds, joined with `sep`."),
            a("sep", "text", "The joiner for `prepend`. Default `:`."),
            a("when", "condition", "Only while this holds."),
            a("locked", "bool", "A user's own environment may not override it."),
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
            a("default", "value or `{ fn, args }`", "The value when nothing was chosen: a literal, or a named function over context values (`recommend_ctx_size`)."),
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
        ],
    },
    Group {
        heading: "`[[field]]` `ui`",
        intro: "How the window shows a field.",
        source: Some("FieldUi"),
        attributes: &[
            a("section", "text", "Where it goes: `primary` (the row under the model), `advanced` (inside Settings), or a section of your own. Default `advanced`."),
            a("order", "whole number", "Its place in the section, lowest first; those without come last."),
            a("widget", "text", "A widget other than the type's usual one (`runtime_manager`)."),
            a("hint", "text", "Greyed text shown in an empty text box."),
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
        intro: "`use = \"archive\"`: unpack a `.zip` or `.tar.gz` and find the executable inside.",
        source: None,
        attributes: &[
            a("use", "`archive`", "The kind of install."),
            a("binary", "`{ name, search_depth }`", "The executable's file name without a platform suffix, and how many directory levels below the build's directory to look for it (default 4, at most 16)."),
            a("chmod", "bool", "Mark the executable as executable (Unix)."),
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
    (FieldType::Dir, "text, a directory", "a one-line box"),
    (FieldType::File, "text, a file", "a one-line box"),
    (FieldType::Url, "text, an address", "a one-line box"),
    (
        FieldType::RuntimePath,
        "text, a file or a download address",
        "the runtime manager when `ui.widget` says so, else a one-line box",
    ),
    (
        FieldType::KvLines,
        "text, one `KEY=VALUE` per line",
        "a one-line box (a multi-line one is not built yet)",
    ),
    (FieldType::ModelRef, "text, a model", "a one-line box"),
    (
        FieldType::Hidden,
        "any",
        "not drawn: the value is only stored",
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
    for group in GROUPS {
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
