//! Everything QuickAI can send, as data: each event with its level and purpose, and each field with its shape, the
//! kind of data it is and why it is there (VS Code annotates its telemetry the same way). An event or a field that is
//! not here is never queued, and a value of the wrong shape is refused (`check`). `list_json` is what
//! `quickai --telemetry` prints; `reference_page` is docs/reference/telemetry-events.md.

use serde_json::{json, Map, Value};

use super::event::{Batch, Common, Event, SCHEMA};
use super::level::Level;
use super::measure::{is_plain_name, MINUTES, STALL};

/// The shape of a field's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// One of these words.
    Choice(&'static [&'static str]),
    /// Several of these words.
    Choices(&'static [&'static str]),
    /// An id of a thing: letters, digits and `._:/+@-`, at most 160 (an engine, a build's label, a model's name).
    Name,
    /// Short text, at most 100 characters, cleaned (a thread's name, an exit).
    Label,
    /// Text, at most 2000 characters, cleaned (a crash's message).
    Text,
    /// Where in QuickAI's code: `file:line` or `file:line:column`.
    Location,
    /// The names of the functions of a backtrace, at most 64.
    Frames,
    /// A whole number of seconds.
    Seconds,
    /// Yes or no.
    Flag,
}

impl Shape {
    fn describe(self) -> String {
        match self {
            Shape::Choice(words) => format!("one of: {}", words.join(", ")),
            Shape::Choices(words) => format!("any of: {}", words.join(", ")),
            Shape::Name => "a name (letters, digits and ._:/+@-)".to_string(),
            Shape::Label => "short text, cleaned".to_string(),
            Shape::Text => "text, cleaned, at most 2000 characters".to_string(),
            Shape::Location => "a file of QuickAI's source and a line".to_string(),
            Shape::Frames => "function names, at most 64".to_string(),
            Shape::Seconds => "whole seconds".to_string(),
            Shape::Flag => "yes or no".to_string(),
        }
    }

    /// The shape for a program (the server checks against the list `quickai --telemetry` prints): its id, and the
    /// words a choice allows.
    fn id(self) -> (&'static str, Option<&'static [&'static str]>) {
        match self {
            Shape::Choice(words) => ("choice", Some(words)),
            Shape::Choices(words) => ("choices", Some(words)),
            Shape::Name => ("name", None),
            Shape::Label => ("label", None),
            Shape::Text => ("text", None),
            Shape::Location => ("location", None),
            Shape::Frames => ("frames", None),
            Shape::Seconds => ("seconds", None),
            Shape::Flag => ("flag", None),
        }
    }

    fn accepts(self, value: &Value) -> bool {
        let short = |s: &str, most: usize| {
            s.chars().count() <= most && !s.chars().any(|c| c.is_control() && c != '\n')
        };
        match (self, value) {
            (Shape::Choice(words), Value::String(s)) => words.contains(&s.as_str()),
            (Shape::Choices(words), Value::Array(items)) => {
                items.len() <= words.len()
                    && items
                        .iter()
                        .all(|i| i.as_str().is_some_and(|s| words.contains(&s)))
            }
            (Shape::Name, Value::String(s)) => is_plain_name(s),
            (Shape::Label, Value::String(s)) => !s.contains('\n') && short(s, 100),
            (Shape::Text, Value::String(s)) => short(s, super::clean::MAX_TEXT),
            (Shape::Location, Value::String(s)) => {
                let mut parts = s.rsplitn(3, ':');
                let last = parts.next().unwrap_or("");
                s.len() <= 300
                    && !s.contains(char::is_whitespace)
                    && last.chars().all(|c| c.is_ascii_digit())
                    && !last.is_empty()
            }
            (Shape::Frames, Value::Array(items)) => {
                items.len() <= 64
                    && items.iter().all(|i| {
                        i.as_str()
                            .is_some_and(|s| !s.contains('\n') && short(s, 300))
                    })
            }
            (Shape::Seconds, Value::Number(n)) => n.as_u64().is_some_and(|n| n <= 100_000_000),
            (Shape::Flag, Value::Bool(_)) => true,
            _ => false,
        }
    }
}

/// What kind of data a field is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// About the computer or the build.
    System,
    /// Which part of QuickAI was used.
    Use,
    /// The public name of a program or a model.
    PublicName,
    /// A measurement: a time, a count.
    Measure,
    /// What went wrong, cleaned.
    Error,
    /// Where in QuickAI's code.
    CallStack,
    /// A random id.
    Id,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::System => "system metadata",
            Kind::Use => "feature use",
            Kind::PublicName => "public name",
            Kind::Measure => "measurement",
            Kind::Error => "error, cleaned",
            Kind::CallStack => "call stack",
            Kind::Id => "pseudonymous id",
        }
    }
}

/// Why an event is collected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// To find and fix what breaks.
    Health,
    /// To know how fast things are.
    Performance,
    /// To know what is used, and what to build or recommend.
    FeatureInsight,
}

impl Purpose {
    fn label(self) -> &'static str {
        match self {
            Purpose::Health => "health: to find and fix what breaks",
            Purpose::Performance => "performance: to know how fast things are",
            Purpose::FeatureInsight => {
                "feature insight: what is used, and what to build or recommend"
            }
        }
    }
}

/// A field of an event, or of the common properties.
#[derive(Clone, Copy, Debug)]
pub struct FieldSpec {
    pub name: &'static str,
    pub shape: Shape,
    pub kind: Kind,
    pub about: &'static str,
}

/// An event QuickAI can send.
#[derive(Clone, Copy, Debug)]
pub struct EventSpec {
    pub name: &'static str,
    pub level: Level,
    pub purpose: Purpose,
    pub about: &'static str,
    pub fields: &'static [FieldSpec],
}

const fn f(name: &'static str, shape: Shape, kind: Kind, about: &'static str) -> FieldSpec {
    FieldSpec {
        name,
        shape,
        kind,
        about,
    }
}

/// The version of what is collected that a person answers about. It goes up when an event or a field is added that a
/// person who said yes did not agree to, and the notice asks them again (people who chose Off are not asked again).
pub const CONSENT_VERSION: u32 = 1;

/// The ways QuickAI is packaged, as `common.app.package` names them.
pub const PACKAGES: &[&str] = &[
    "appimage", "tar", "snap", "msix", "zip", "dmg", "brew", "scoop", "choco", "aur", "other",
];
/// The systems.
pub const SYSTEMS: &[&str] = &["linux", "windows", "macos", "other"];
/// The makers of graphics adapters.
pub const GPU_MAKERS: &[&str] = &["nvidia", "amd", "intel", "apple"];
/// The windows whose opening is counted.
pub const WINDOWS: &[&str] = &[
    "add_model",
    "recommended",
    "details",
    "settings",
    "configurations",
    "help",
    "about",
    "telemetry_log",
    "report_problem",
];
/// The features whose use is counted.
pub const FEATURES: &[&str] = &[
    "live_traffic",
    "keep_chats",
    "remote_run",
    "open_webui",
    "configuration_import",
    "configuration_export",
    "chat",
];
const ENGINE_STATES: &[&str] = &["idle", "starting", "running", "stopping", "crashed"];
const REASONS: &[&str] = &[
    "checksum",
    "network",
    "disk_full",
    "http_4xx",
    "http_5xx",
    "other",
];
const SOURCES: &[&str] = &[
    "huggingface",
    "github",
    "ollama",
    "pypi",
    "pytorch",
    "other",
];

const ENGINE: FieldSpec = f(
    "engine",
    Shape::Name,
    Kind::PublicName,
    "the engine (llamacpp, ollama, comfyui)",
);
const BUILD: FieldSpec = f(
    "build",
    Shape::Name,
    Kind::PublicName,
    "the label of the engine's build (`b11889-vulkan`), `bundled`, or `custom` for a program of the person's own",
);
const MODEL: FieldSpec = f(
    "model",
    Shape::Name,
    Kind::PublicName,
    "`catalog:<id>` for a model of the recommended list, `hf:<owner>/<repo>` for one found through a look-up, \
     otherwise `other:<size in GB>`",
);

/// The properties sent once with every batch.
pub const COMMON: &[FieldSpec] = &[
    f(
        "install",
        Shape::Name,
        Kind::Id,
        "random, made the first time anything is sent; Settings shows it and can replace it",
    ),
    f(
        "session",
        Shape::Name,
        Kind::Id,
        "random for each start of the app, never kept",
    ),
    f(
        "app.version",
        Shape::Name,
        Kind::System,
        "QuickAI's version",
    ),
    f(
        "app.channel",
        Shape::Choice(&["official", "source"]),
        Kind::System,
        "a build EverybodyAI released, or one built from source",
    ),
    f(
        "app.commit",
        Shape::Name,
        Kind::System,
        "the commit it was built from",
    ),
    f(
        "app.target",
        Shape::Name,
        Kind::System,
        "the kind of computer it was built for (`x86_64-unknown-linux-gnu`)",
    ),
    f(
        "app.package",
        Shape::Choice(PACKAGES),
        Kind::System,
        "how it was installed",
    ),
    f("os", Shape::Choice(SYSTEMS), Kind::System, "the system"),
    f(
        "os_version",
        Shape::Label,
        Kind::System,
        "the system's version",
    ),
    f(
        "arch",
        Shape::Name,
        Kind::System,
        "the processor's architecture",
    ),
    f(
        "memory_gb",
        Shape::Choice(&["8", "16", "32", "64", "128"]),
        Kind::System,
        "the memory, rounded to 8, 16, 32, 64 or 128 GB and more",
    ),
    f(
        "gpus",
        Shape::Choices(GPU_MAKERS),
        Kind::System,
        "the makers of the graphics adapters",
    ),
    f(
        "language",
        Shape::Label,
        Kind::System,
        "the system's language, without its region",
    ),
    f(
        "level",
        Shape::Choice(&["off", "crash", "error", "all"]),
        Kind::System,
        "the telemetry level in force",
    ),
];

/// Every event QuickAI can send.
pub const EVENTS: &[EventSpec] = &[
    EventSpec {
        name: "crash.panic",
        level: Level::Crash,
        purpose: Purpose::Health,
        about: "QuickAI stopped because of an error. Written when it happens, sent at the next start.",
        fields: &[
            f("message", Shape::Text, Kind::Error, "the error's message, cleaned"),
            f("location", Shape::Location, Kind::CallStack, "where in QuickAI's code it happened"),
            f("thread", Shape::Label, Kind::CallStack, "the name of the thread it happened in"),
            f("backtrace", Shape::Frames, Kind::CallStack, "the names of the functions that led to it"),
            ENGINE,
            f("engine_state", Shape::Choice(ENGINE_STATES), Kind::Use, "whether the engine was running"),
            f("open_windows", Shape::Choices(WINDOWS), Kind::Use, "which windows were open"),
            f("minutes_open", Shape::Choice(&MINUTES), Kind::Measure, "how long the app had been open"),
        ],
    },
    EventSpec {
        name: "error.engine_exit",
        level: Level::Error,
        purpose: Purpose::Health,
        about: "An engine stopped by itself, while loading a model or while serving.",
        fields: &[
            ENGINE,
            BUILD,
            MODEL,
            f("phase", Shape::Choice(&["loading", "serving"]), Kind::Use, "what the engine was doing"),
            f("status", Shape::Label, Kind::Error, "how it ended (`exit 1`, `signal 9`, `unknown`)"),
            f("after_seconds", Shape::Seconds, Kind::Measure, "how long after Start"),
        ],
    },
    EventSpec {
        name: "error.download",
        level: Level::Error,
        purpose: Purpose::Health,
        about: "A download failed: its kind, the site and the reason, never the file or the address.",
        fields: &[
            f("what", Shape::Choice(&["model", "build", "program"]), Kind::Use, "what was being downloaded"),
            f("source", Shape::Choice(SOURCES), Kind::PublicName, "the site it came from"),
            f("reason", Shape::Choice(REASONS), Kind::Error, "why it failed"),
        ],
    },
    EventSpec {
        name: "error.stall",
        level: Level::Error,
        purpose: Purpose::Health,
        about: "The window stopped answering for five seconds or more.",
        fields: &[f("seconds", Shape::Choice(&STALL), Kind::Measure, "for how long")],
    },
    EventSpec {
        name: "session.unclean_exit",
        level: Level::Error,
        purpose: Purpose::Health,
        about: "The last session ended without closing (a crash of the system's graphics, a forced kill, a power cut). \
                Sent at the next start; never asked about.",
        fields: &[],
    },
    EventSpec {
        name: "session.start",
        level: Level::All,
        purpose: Purpose::FeatureInsight,
        about: "QuickAI started.",
        fields: &[f("first_run", Shape::Flag, Kind::Use, "whether it is the first start, with no model yet")],
    },
    EventSpec {
        name: "session.end",
        level: Level::All,
        purpose: Purpose::FeatureInsight,
        about: "QuickAI was closed.",
        fields: &[f("minutes_open", Shape::Choice(&MINUTES), Kind::Measure, "how long it was open")],
    },
    EventSpec {
        name: "window.open",
        level: Level::All,
        purpose: Purpose::FeatureInsight,
        about: "A window was opened.",
        fields: &[f("window", Shape::Choice(WINDOWS), Kind::Use, "which")],
    },
    EventSpec {
        name: "feature.use",
        level: Level::All,
        purpose: Purpose::FeatureInsight,
        about: "A feature was used or switched on.",
        fields: &[f("feature", Shape::Choice(FEATURES), Kind::Use, "which")],
    },
    EventSpec {
        name: "model.add",
        level: Level::All,
        purpose: Purpose::FeatureInsight,
        about: "A model was added.",
        fields: &[
            f("how", Shape::Choice(&["recommended", "lookup", "link", "import", "pull"]), Kind::Use, "how it was added"),
            MODEL,
        ],
    },
    EventSpec {
        name: "run.start",
        level: Level::All,
        purpose: Purpose::FeatureInsight,
        about: "Start was pressed.",
        fields: &[ENGINE, BUILD, MODEL, f("where", Shape::Choice(&["local", "remote"]), Kind::Use, "this computer or another")],
    },
    EventSpec {
        name: "run.ready",
        level: Level::All,
        purpose: Purpose::Performance,
        about: "The engine answered after Start, with its model loaded.",
        fields: &[ENGINE, MODEL, f("seconds", Shape::Seconds, Kind::Measure, "how long that took")],
    },
];

/// The row of `EVENTS` named `name`.
pub fn spec(name: &str) -> Option<&'static EventSpec> {
    EVENTS.iter().find(|e| e.name == name)
}

/// Whether `event` is one the schema has, with known fields of the right shapes, at a well-formed hour.
pub fn check(event: &Event) -> Result<(), String> {
    let spec =
        spec(&event.event).ok_or_else(|| format!("no event {} in the schema", event.event))?;
    if !is_hour(&event.time) {
        return Err(format!(
            "{}: the time {:?} is not an hour",
            event.event, event.time
        ));
    }
    for (key, value) in &event.fields {
        let field = spec
            .fields
            .iter()
            .find(|f| f.name == key)
            .ok_or_else(|| format!("{}: no field {key}", event.event))?;
        if !field.shape.accepts(value) {
            return Err(format!(
                "{}: {key} is not {}",
                event.event,
                field.shape.describe()
            ));
        }
    }
    Ok(())
}

/// Whether the common properties are of the shapes `COMMON` says.
pub fn check_common(common: &Common) -> Result<(), String> {
    let as_json = serde_json::to_value(common).map_err(|e| e.to_string())?;
    for field in COMMON {
        let value = field
            .name
            .split('.')
            .try_fold(&as_json, |v, key| v.get(key))
            .ok_or_else(|| format!("common: no {}", field.name))?;
        // the memory is a number; the schema lists its steps as words
        let value = match value {
            Value::Number(n) if field.name == "memory_gb" => Value::String(n.to_string()),
            other => other.clone(),
        };
        if !field.shape.accepts(&value) {
            return Err(format!(
                "common: {} is not {}",
                field.name,
                field.shape.describe()
            ));
        }
    }
    Ok(())
}

/// Whether a whole batch is one the schema allows.
pub fn check_batch(batch: &Batch) -> Result<(), String> {
    if batch.schema != SCHEMA {
        return Err(format!("schema {} is not {SCHEMA}", batch.schema));
    }
    check_common(&batch.common)?;
    batch.events.iter().try_for_each(check)
}

/// `2026-10-30T09:00:00Z`: an hour, as `measure::hour_utc` writes it.
fn is_hour(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 20
        && text.ends_with(":00:00Z")
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
}

fn field_json(field: &FieldSpec) -> Value {
    let (id, words) = field.shape.id();
    let mut o = json!({
        "name": field.name,
        "shape": field.shape.describe(),
        "type": id,
        "kind": field.kind.label(),
        "about": field.about,
    });
    if let Some(words) = words {
        o["values"] = words.into();
    }
    o
}

/// Every event this build can send, with its level, purpose and fields: what `quickai --telemetry` prints.
pub fn list_json() -> Value {
    let events: Vec<Value> = EVENTS
        .iter()
        .map(|e| {
            let mut o = Map::new();
            o.insert("name".into(), e.name.into());
            o.insert("level".into(), e.level.id().into());
            o.insert("purpose".into(), e.purpose.label().into());
            o.insert("about".into(), e.about.into());
            o.insert(
                "fields".into(),
                e.fields.iter().map(field_json).collect::<Vec<_>>().into(),
            );
            Value::Object(o)
        })
        .collect();
    json!({
        "schema": SCHEMA,
        "common": COMMON.iter().map(field_json).collect::<Vec<_>>(),
        "events": events,
    })
}

/// docs/reference/telemetry-events.md: the same list, for people.
pub fn reference_page() -> String {
    let mut out = String::from(
        "# Telemetry events\n\n\
         _Generated from the schema in `crates/quickai-core/src/telemetry/schema.rs` by `quickai config docs telemetry`; \
         `quickai --telemetry` prints the same as JSON. Do not edit by hand._\n\n\
         This is everything QuickAI can send EverybodyAI. Nothing is sent until you choose a telemetry level, and a \
         level sends only the events of its own level and those below it: **Crashes only** sends `crash` events, \
         **Errors and crashes** adds `error` events, **All** adds the rest. Text is cleaned before it is sent: a home \
         folder, user and computer names, addresses, email addresses and anything that looks like a password or a \
         token are replaced. See [the privacy policy](https://quickai.everybodyai.dev/privacy).\n\n\
         ## Sent with every batch\n\n| Property | Shape | Kind | What it is |\n|---|---|---|---|\n",
    );
    for field in COMMON {
        out.push_str(&row(field));
    }
    out.push_str("\nThe server adds the country the request came from, as two letters, and never stores the address.\n");
    for level in [Level::Crash, Level::Error, Level::All] {
        out.push_str(&format!(
            "\n## {}: events of level `{}`\n",
            level.label(),
            level.id()
        ));
        for e in EVENTS.iter().filter(|e| e.level == level) {
            out.push_str(&format!(
                "\n### `{}`\n\n{} Purpose: {}.\n",
                e.name,
                e.about,
                e.purpose.label()
            ));
            if e.fields.is_empty() {
                out.push_str("\nNo fields.\n");
            } else {
                out.push_str("\n| Field | Shape | Kind | What it is |\n|---|---|---|---|\n");
                for field in e.fields {
                    out.push_str(&row(field));
                }
            }
        }
    }
    out
}

fn row(field: &FieldSpec) -> String {
    format!(
        "| `{}` | {} | {} | {} |\n",
        field.name,
        field.shape.describe().replace('|', "\\|"),
        field.kind.label(),
        field.about.replace('|', "\\|")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn common() -> Common {
        Common {
            install: "3f1c9a2e-7b44-4d0e-9c1a-0b6f1e2a3c4d".into(),
            session: "5b0e2c7a-1111-4222-8333-444455556666".into(),
            app: super::super::AppIdentity {
                version: "0.3.0".into(),
                channel: "official".into(),
                commit: "b978bde".into(),
                target: "x86_64-unknown-linux-gnu".into(),
                package: "appimage".into(),
            },
            os: "linux".into(),
            os_version: "24.04".into(),
            arch: "x86_64".into(),
            memory_gb: 32,
            gpus: vec!["amd".into()],
            language: "en".into(),
            level: "all".into(),
        }
    }

    #[test]
    fn every_event_has_a_level_that_sends_and_unique_names() {
        let mut names = std::collections::BTreeSet::new();
        for e in EVENTS {
            assert!(names.insert(e.name), "{} twice", e.name);
            assert_ne!(e.level, Level::Off, "{} would never be sent", e.name);
            let mut fields = std::collections::BTreeSet::new();
            for field in e.fields {
                assert!(fields.insert(field.name), "{}.{} twice", e.name, field.name);
                assert!(
                    !["event", "time"].contains(&field.name),
                    "{}.{} is a key of the event itself",
                    e.name,
                    field.name
                );
            }
        }
    }

    #[test]
    fn an_event_of_the_schema_passes_and_anything_else_is_refused() {
        let ok = Event::new("run.start", 1_793_351_564)
            .with("engine", "llamacpp")
            .with("build", "b11889-vulkan")
            .with("model", "catalog:qwen3-8b-q4_k_m")
            .with("where", "local");
        assert_eq!(check(&ok), Ok(()));
        assert!(
            check(&Event::new("run.secret", 0)).is_err(),
            "an unknown event"
        );
        assert!(
            check(&ok.clone().with("prompt", "hello")).is_err(),
            "an unknown field"
        );
        assert!(
            check(&ok.clone().with("where", "moon")).is_err(),
            "a word that is not a choice"
        );
        assert!(
            check(&ok.clone().with("model", "/home/grace/my model.gguf")).is_err(),
            "a path is not a name"
        );
        let mut late = ok.clone();
        late.time = "2026-10-30T09:12:44Z".into();
        assert!(check(&late).is_err(), "a moment, not an hour");
        let ready = Event::new("run.ready", 0)
            .with("engine", "ollama")
            .with("seconds", -3);
        assert!(check(&ready).is_err(), "seconds are whole and not negative");
    }

    #[test]
    fn a_crash_takes_text_a_location_and_frames() {
        let crash = Event::new("crash.panic", 0)
            .with("message", "called `Option::unwrap()` on a `None` value")
            .with("location", "crates/quickai/src/app/catalog_view.rs:412:31")
            .with("thread", "main")
            .with(
                "backtrace",
                vec![
                    "quickai::app::catalog_view::CatalogView::show",
                    "quickai::app::shell::<impl quickai::app::App>::show_window",
                ],
            )
            .with("open_windows", vec!["recommended"])
            .with("minutes_open", "30-60");
        assert_eq!(check(&crash), Ok(()));
        assert!(check(&crash.clone().with("location", "somewhere")).is_err());
        assert!(check(&crash.with("open_windows", vec!["recommended", "desktop"])).is_err());
    }

    #[test]
    fn the_common_properties_are_checked_too() {
        assert_eq!(check_common(&common()), Ok(()));
        let mut odd = common();
        odd.memory_gb = 31;
        assert!(check_common(&odd).is_err(), "memory is sent in steps");
        let mut odd = common();
        odd.gpus = vec!["NVIDIA GeForce RTX 4090".into()];
        assert!(
            check_common(&odd).is_err(),
            "makers only, never a model of card"
        );
        let mut odd = common();
        odd.app.package = "my-distro".into();
        assert!(check_common(&odd).is_err());
        let batch = Batch::new(
            "b".into(),
            common(),
            vec![Event::new("session.start", 0).with("first_run", true)],
        );
        assert_eq!(check_batch(&batch), Ok(()));
    }

    #[test]
    fn the_list_names_every_event_and_field() {
        let list = list_json();
        assert_eq!(list["events"].as_array().map(Vec::len), Some(EVENTS.len()));
        assert_eq!(list["common"].as_array().map(Vec::len), Some(COMMON.len()));
        // what the server checks by: a type for every field, and the words of every choice
        assert_eq!(list["common"][0]["type"], "name");
        assert_eq!(list["common"][13]["name"], "level");
        assert_eq!(
            list["common"][13]["values"],
            json!(["off", "crash", "error", "all"])
        );
        let page = reference_page();
        for e in EVENTS {
            assert!(page.contains(&format!("### `{}`", e.name)));
        }
    }
}
