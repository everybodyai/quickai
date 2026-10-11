//! What a person is told before a configuration is used on a computer it was not made on, and what
//! is left out of one that is given away (docs/configurations.md, sections 7 and 8).
//!
//! Everything here is a function of its arguments: a file's text, the registry, and an `Inventory`
//! that the application fills in about the computer (what is installed, what is running, how much
//! memory there is). Reading a file, reviewing it and previewing an export never touch the network,
//! start a program or write a file; `tests/purity.rs` holds the core to that.

use std::collections::{BTreeMap, BTreeSet};

use crate::doc::engine::{FieldType, Share};
use crate::doc::{Envelope, Kind, SCHEMA_VERSION};
use crate::heuristics::{fit, memory_needed, Fit};
use crate::registry::catalog::Run;
use crate::registry::Registry;
use crate::value::Value;

use super::notes::{self, Level, Note};
use super::screen;
use super::state::{carried, Change, LeftOut};
use super::{
    clean_text, family, label_of, read, shown, Configuration, MadeOn, ModelPick, ModelSource,
    MAX_FILE_BYTES,
};

/// How many bytes as a person says it: `6.1 GB`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

// ---- what this computer has ------------------------------------------------------------------------

/// A file in the models folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHere {
    pub name: String,
    pub size: u64,
}

/// A model an engine listed by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedHere {
    pub name: String,
    pub size: Option<u64>,
}

/// Which models an engine has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ModelsHere {
    /// Not known: nothing is said about models.
    #[default]
    Unknown,
    /// An engine whose models are files: the files of its models folder.
    Files(Vec<FileHere>),
    /// An engine that keeps its own models: the names it listed last time (`None`: never).
    Named { listed: Option<Vec<NamedHere>> },
}

/// A build of a program that is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildHere {
    pub label: String,
    /// What is wrong with it for this computer's cards, in the manager's own sentence, when anything is.
    pub warning: Option<String>,
}

/// What one engine (or companion) has on this computer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineHere {
    /// A program is found for it: an installed build, the one the package ships, or one on the path.
    pub installed: bool,
    /// It is running now.
    pub running: bool,
    pub builds: Vec<BuildHere>,
    pub models: ModelsHere,
}

/// What a review of a configuration needs to know about the computer. The application fills it in.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    /// This application's version.
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub ram_bytes: u64,
    /// By the id of the engine, companions included.
    pub engines: BTreeMap<String, EngineHere>,
    /// Ports in use on this computer now, apart from the ones the working state already has.
    pub busy_ports: BTreeSet<u16>,
    /// The address of the engine each program that goes with it was first started with, by the id of the
    /// program: it goes on calling that one (docs/configurations.md, section 9).
    pub companion_first: BTreeMap<String, String>,
}

// ---- opening a file --------------------------------------------------------------------------------

/// A file that was read and can be looked at.
#[derive(Clone, Debug)]
pub struct Opened {
    pub configuration: Configuration,
    /// What is odd about the file itself (settings the engine has not, values that do not fit, keys
    /// this version does not know).
    pub notes: Vec<Note>,
}

fn not_a_configuration(why: impl Into<String>) -> Note {
    Note::new(
        "not_a_configuration",
        &[("why", clean_text(&why.into(), 160, false))],
    )
}

/// Reads the text of a file from outside: nothing is taken from it that the registry does not know,
/// and a file that cannot be used gives the note that says why. A file larger than 256 KiB is not read.
pub fn open_text(registry: &Registry, text: &str, app_version: &str) -> Result<Opened, Note> {
    if text.len() > MAX_FILE_BYTES {
        return Err(not_a_configuration(format!(
            "it is larger than {} KiB",
            MAX_FILE_BYTES / 1024
        )));
    }
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(e) => {
            return Err(not_a_configuration(format!(
                "it is not valid TOML: {}",
                e.message().lines().next().unwrap_or_default()
            )))
        }
    };
    let (envelope, body) = match Envelope::split(table) {
        Ok(split) => split,
        Err(problems) => {
            if let Some(newer) = problems
                .iter()
                .find(|p| p.path == "schema" && p.message.contains("newer than this application"))
            {
                let wanted = newer
                    .message
                    .split_whitespace()
                    .nth(4)
                    .unwrap_or("a newer schema");
                return Err(Note::new(
                    "too_new",
                    &[
                        ("needs", format!("a document schema of {wanted}")),
                        ("this", format!("{SCHEMA_VERSION}")),
                    ],
                ));
            }
            let first = &problems[0];
            return Err(not_a_configuration(format!(
                "{}: {}",
                first.path, first.message
            )));
        }
    };
    if envelope.kind != Kind::Profile {
        return Err(not_a_configuration(format!(
            "it is {}, not a configuration",
            envelope.kind.with_article()
        )));
    }
    if let Some(required) = &envelope.requires_app {
        match (
            semver::VersionReq::parse(required),
            crate::expr::funcs::parse_version(app_version),
        ) {
            (Ok(req), Ok(version)) if !req.matches(&version) => {
                return Err(Note::new(
                    "too_new",
                    &[
                        ("needs", format!("QuickAI {required}")),
                        ("this", app_version.to_string()),
                    ],
                ))
            }
            (Err(_), _) => {
                return Err(not_a_configuration(format!(
                    "`requires.app` is `{}`, which is not a version requirement",
                    clean_text(required, 40, false)
                )))
            }
            _ => {}
        }
    }
    match read(&envelope.id, &body, registry.engine_map()) {
        Ok(read) => Ok(Opened {
            configuration: read.configuration,
            notes: read.notes,
        }),
        Err(fatal) => {
            let first = &fatal[0];
            Err(not_a_configuration(format!(
                "{}: {}",
                first.path, first.message
            )))
        }
    }
}

// ---- looking at it ---------------------------------------------------------------------------------

/// What a person is told about a configuration on this computer, most important first.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub notes: Vec<Note>,
}

impl Report {
    /// The note that stops it, when one does.
    pub fn stop(&self) -> Option<&Note> {
        self.notes.iter().find(|n| n.level == Level::Stops)
    }

    /// The things to ask about: each is a setting the person is asked to tick.
    pub fn asks(&self) -> impl Iterator<Item = &Note> {
        self.notes
            .iter()
            .filter(|n| n.level == Level::Asks && n.subject.is_some())
    }

    pub fn has(&self, id: &str) -> bool {
        self.notes.iter().any(|n| n.id == id)
    }

    pub fn of(&self, id: &str) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.id == id).collect()
    }

    /// The note that lists what would change, when anything would: up to six, then how many more.
    pub fn will_change(changes: &[Change]) -> Option<Note> {
        if changes.is_empty() {
            return None;
        }
        let mut list: Vec<String> = changes.iter().take(6).map(Change::describe).collect();
        if changes.len() > 6 {
            list.push(format!("and {} more", changes.len() - 6));
        }
        Some(Note::new("will_change", &[("list", list.join("; "))]))
    }
}

fn friendly_os(os: &str) -> String {
    match os.to_ascii_lowercase().as_str() {
        "linux" => "Linux".into(),
        "windows" => "Windows".into(),
        "macos" => "macOS".into(),
        other => other.to_string(),
    }
}

fn computer(os: &str, arch: &str) -> String {
    [friendly_os(os), arch.to_string()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A model with its size as a sentence names it: `qwen3-vl:8b (6.1 GB)`.
fn with_size(name: &str, size: Option<u64>) -> String {
    match size {
        Some(size) if size > 0 => format!("{name} ({})", human_bytes(size)),
        _ => name.to_string(),
    }
}

/// Where a model can be had from, as a sentence names it, when it is known.
fn source_of(registry: &Registry, engine: &str, pick: &ModelPick) -> Option<String> {
    match &pick.source {
        ModelSource::Named { .. } => registry.engine(engine).map(|e| e.doc.engine.name.clone()),
        ModelSource::Catalog { id, .. } => {
            registry
                .catalog()
                .model(id)
                .and_then(|m| match m.runs.get(engine)? {
                    Run::Files(run) => Some(format!("huggingface.co/{}", run.repo)),
                    Run::Reference(_) => None,
                })
        }
        ModelSource::File { .. } => pick.from.as_deref().map(super::from_label),
    }
}

/// The size the file or the catalog gives for the pick, in bytes.
fn size_of(registry: &Registry, engine: &str, pick: &ModelPick) -> Option<u64> {
    if pick.size.is_some() {
        return pick.size;
    }
    if let ModelSource::Catalog { id, file } = &pick.source {
        if let Some(Run::Files(run)) = registry.catalog().model(id)?.runs.get(engine) {
            return run
                .files
                .iter()
                .find(|f| {
                    file.as_deref()
                        .is_none_or(|name| f.path.rsplit('/').next() == Some(name))
                })
                .and_then(|f| f.size);
        }
    }
    None
}

/// What this computer says about a model: here, not here (and where it can be had from), or not
/// to be told.
fn model_notes(registry: &Registry, configuration: &Configuration, here: &Inventory) -> Vec<Note> {
    let mut out = Vec::new();
    let engine = &configuration.engine;
    let Some(pick) = configuration.models.first() else {
        return out;
    };
    let Some(on_hand) = here.engines.get(engine) else {
        return out;
    };
    let size = size_of(registry, engine, pick);
    let named = pick.display().to_string();
    let label = with_size(&named, size);
    let subject = pick.source.label().to_string();
    let present =
        match (&pick.source, &on_hand.models) {
            (ModelSource::Named { name, .. }, ModelsHere::Named { listed }) => listed
                .as_ref()
                .map(|list| list.iter().any(|m| &m.name == name)),
            (
                ModelSource::File { name }
                | ModelSource::Catalog {
                    file: Some(name), ..
                },
                ModelsHere::Files(files),
            ) => Some(files.iter().any(|f| {
                &f.name == name && size.is_none_or(|wanted| wanted == 0 || f.size == wanted)
            })),
            _ => None,
        };
    let can_be_got = source_of(registry, engine, pick);
    match present {
        Some(true) => {}
        Some(false) | None => {
            let listed_while_stopped = matches!(
                (&pick.source, &on_hand.models),
                (ModelSource::Named { .. }, ModelsHere::Named { .. })
            ) && !on_hand.running;
            if matches!(on_hand.models, ModelsHere::Unknown) {
                // nothing to compare with
            } else if listed_while_stopped {
                let name = registry
                    .engine(engine)
                    .map_or_else(|| engine.clone(), |e| e.doc.engine.name.clone());
                out.push(
                    Note::new(
                        "model_unseen",
                        &[("model", named.clone()), ("engine", name)],
                    )
                    .about(subject.clone()),
                );
            } else if let Some(from) = can_be_got {
                out.push(
                    Note::new("model_to_get", &[("model", label.clone()), ("from", from)])
                        .about(subject.clone()),
                );
            } else {
                out.push(
                    Note::new("model_unknown", &[("model", label.clone())]).about(subject.clone()),
                );
            }
        }
    }
    if let Some(size) = size {
        if fit(size, here.ram_bytes) == Fit::TooBig {
            out.push(
                Note::new(
                    "memory",
                    &[
                        ("model", named),
                        ("need", human_bytes(memory_needed(size))),
                        ("have", human_bytes(here.ram_bytes)),
                    ],
                )
                .about(subject),
            );
        }
    }
    out
}

/// The ports a configuration sets or leaves to their defaults, with the field that holds each.
fn ports(registry: &Registry, configuration: &Configuration) -> Vec<(String, String, u16, bool)> {
    let mut out = Vec::new();
    for member in family(registry.engine_map(), &configuration.engine) {
        for field in &member.doc.fields {
            if field.ty != FieldType::Port {
                continue;
            }
            let key = super::value_key(&member.id, &field.id);
            let chosen = configuration.values.get(&key).and_then(Value::as_i64);
            let default = match &field.default {
                Some(crate::doc::engine::FieldDefault::Literal(toml::Value::Integer(n))) => {
                    Some(*n)
                }
                _ => None,
            };
            if let Some(port) = chosen.or(default).and_then(|p| u16::try_from(p).ok()) {
                let name = format!("{} {}", member.doc.engine.name, label_of(field));
                out.push((key, name, port, chosen.is_some()));
            }
        }
    }
    out
}

/// What a person should be told about `configuration` on the computer `here`. `outside` is a file
/// that came from someone else: its free text and the settings that widen what a program may do are
/// asked about. `notes` are what was odd about the file itself (`Opened::notes`).
pub fn review(
    registry: &Registry,
    configuration: &Configuration,
    notes: &[Note],
    here: &Inventory,
    outside: bool,
) -> Report {
    let members = family(registry.engine_map(), &configuration.engine);
    if members.is_empty() {
        return Report {
            notes: vec![Note::new(
                "unknown_engine",
                &[("engine", configuration.engine.clone())],
            )],
        };
    }
    let mut out: Vec<Note> = notes.to_vec();

    // another kind of computer
    if let Some(on) = &configuration.meta.made_on {
        let differs = |made: &Option<String>, here: &str| {
            made.as_deref()
                .is_some_and(|m| !m.eq_ignore_ascii_case(here))
        };
        if differs(&on.os, &here.os) || differs(&on.arch, &here.arch) {
            out.push(Note::new(
                "other_system",
                &[
                    (
                        "made",
                        computer(
                            on.os.as_deref().unwrap_or(""),
                            on.arch.as_deref().unwrap_or(""),
                        ),
                    ),
                    ("this", computer(&here.os, &here.arch)),
                ],
            ));
        }
    }

    // the program
    if let (Some(label), Some(on_hand)) = (
        &configuration.runtime.build,
        here.engines.get(&configuration.engine),
    ) {
        match on_hand.builds.iter().find(|b| &b.label == label) {
            Some(build) => {
                if let Some(text) = &build.warning {
                    out.push(
                        Note::new("other_gpu", &[("text", text.clone())]).about(label.clone()),
                    );
                }
            }
            None if on_hand.builds.is_empty() => {
                out.push(Note::new("build_none", &[("build", label.clone())]).about(label.clone()));
            }
            None => {
                let have: Vec<&str> = on_hand.builds.iter().map(|b| b.label.as_str()).collect();
                out.push(
                    Note::new(
                        "build_missing",
                        &[("build", label.clone()), ("have", have.join(", "))],
                    )
                    .about(label.clone()),
                );
            }
        }
    }

    out.extend(model_notes(registry, configuration, here));

    // the ports
    let all = ports(registry, configuration);
    for (key, name, port, chosen) in &all {
        if *chosen && here.busy_ports.contains(port) {
            out.push(
                Note::new(
                    "port_taken",
                    &[("field", name.clone()), ("port", port.to_string())],
                )
                .about(key.clone()),
            );
        }
    }
    let mut seen: BTreeMap<u16, Vec<&str>> = BTreeMap::new();
    for (_, name, port, _) in &all {
        seen.entry(*port).or_default().push(name);
    }
    for (port, names) in seen.into_iter().filter(|(_, names)| names.len() > 1) {
        out.push(Note::new(
            "ports_clash",
            &[("fields", names.join(", ")), ("port", port.to_string())],
        ));
    }

    // what goes with the engine, switched on and not installed
    for companion in members.iter().skip(1) {
        let wanted = configuration
            .values
            .get(&super::value_key(&companion.id, "enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if wanted
            && here
                .engines
                .get(&companion.id)
                .is_some_and(|c| !c.installed)
        {
            out.push(
                Note::new(
                    "companion_missing",
                    &[("companion", companion.doc.engine.name.clone())],
                )
                .about(companion.id.clone()),
            );
        }
    }

    // a program that goes with the engine keeps the address it was first given: when the configuration puts
    // the engine at another, it is told
    if let Some(head) = members.first() {
        let public = head.doc.fields.iter().find(|f| {
            f.bind.port == Some(crate::doc::engine::PortRole::Public) && f.ty == FieldType::Port
        });
        let port = public.and_then(|field| {
            configuration
                .values
                .get(&super::value_key(&head.id, &field.id))
                .and_then(Value::as_i64)
                .or(match &field.default {
                    Some(crate::doc::engine::FieldDefault::Literal(toml::Value::Integer(n))) => {
                        Some(*n)
                    }
                    _ => None,
                })
                .or(head.doc.provides.default_port.map(i64::from))
        });
        if let Some(port) = port.and_then(|p| u16::try_from(p).ok()) {
            let now = crate::plan::parent_url(port);
            for companion in members.iter().skip(1) {
                let wanted = configuration
                    .values
                    .get(&super::value_key(&companion.id, "enabled"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if let (true, Some(first)) = (wanted, here.companion_first.get(&companion.id)) {
                    if *first != now {
                        out.push(
                            Note::new(
                                "companion_address",
                                &[
                                    ("companion", companion.doc.engine.name.clone()),
                                    ("engine", head.doc.engine.name.clone()),
                                    ("first", first.clone()),
                                    ("now", now.clone()),
                                ],
                            )
                            .about(companion.id.clone()),
                        );
                    }
                }
            }
        }
    }

    // the settings themselves
    let deny = registry.vocabulary().deny_env();
    for (key, field) in carried(registry, &configuration.engine) {
        let Some(value) = configuration.values.get(&key) else {
            continue;
        };
        let owner = key.rsplit_once('.').map_or("", |(owner, _)| owner);
        let engine = registry.engine(owner);
        let engine_name = engine.map_or_else(|| owner.to_string(), |e| e.doc.engine.name.clone());
        let label = label_of(field);
        let free = matches!(
            field.ty,
            FieldType::Text | FieldType::Url | FieldType::KvLines
        );
        if outside && field.share() == Some(Share::Review) {
            let note = if free {
                Note::new(
                    "free_text",
                    &[("field", label.clone()), ("engine", engine_name.clone())],
                )
            } else {
                Note::new(
                    "wider_access",
                    &[
                        ("field", label.clone()),
                        ("engine", engine_name.clone()),
                        ("value", shown(value)),
                    ],
                )
            };
            out.push(note.about(key.clone()).said(field.share_note.as_deref()));
        }
        if let (Value::Str(text), Some(lines), Some(engine)) =
            (value, field.bind.env_lines.as_ref(), engine)
        {
            for name in screen::env_lines(engine, &lines.protect, text, deny).refused {
                out.push(
                    Note::new("refused_line", &[("name", name), ("field", label.clone())])
                        .about(key.clone()),
                );
            }
        }
        if let (true, Value::Str(text)) = (free, value) {
            if let Some(path) = screen::names_a_path(text) {
                out.push(
                    Note::new("names_a_path", &[("field", label), ("path", path)])
                        .about(key.clone()),
                );
            }
        }
    }

    notes::sort(&mut out);
    Report { notes: out }
}

/// The configuration with only what the person agreed to: a setting that is asked about and was not
/// ticked (`ticked` holds the keys) is not in it, and the lines of environment that a file may not
/// set are taken out of the text that is. What is stored from an import is this.
pub fn accepted(
    registry: &Registry,
    configuration: &Configuration,
    ticked: &BTreeSet<String>,
) -> (Configuration, Vec<Note>) {
    let mut out = configuration.clone();
    let mut notes = Vec::new();
    let deny = registry.vocabulary().deny_env();
    for (key, field) in carried(registry, &configuration.engine) {
        let Some(value) = out.values.get(&key).cloned() else {
            continue;
        };
        if field.share() == Some(Share::Review) && !ticked.contains(&key) {
            out.values.remove(&key);
            continue;
        }
        let owner = key.rsplit_once('.').map_or("", |(owner, _)| owner);
        if let (Value::Str(text), Some(lines), Some(engine)) = (
            &value,
            field.bind.env_lines.as_ref(),
            registry.engine(owner),
        ) {
            let seen = screen::env_lines(engine, &lines.protect, text, deny);
            for name in seen.refused {
                notes.push(
                    Note::new(
                        "refused_line",
                        &[("name", name), ("field", label_of(field))],
                    )
                    .about(key.clone()),
                );
            }
            out.values.insert(key, Value::Str(seen.text));
        }
    }
    (out, notes)
}

// ---- giving it away --------------------------------------------------------------------------------

/// What the person says about a file they give away, and what the application adds: coarse, and
/// nothing that names a person or a computer.
#[derive(Clone, Debug, Default)]
pub struct ExportMeta {
    /// Typed by whoever exports.
    pub note: Option<String>,
    /// `QuickAI 0.1.0`.
    pub made_with: String,
    pub made_on: MadeOn,
}

/// A configuration ready to be written for someone else, and what was left out of it.
#[derive(Clone, Debug)]
pub struct Exported {
    pub configuration: Configuration,
    pub left_out: Vec<LeftOut>,
    /// What the person is told about the file before it is written: what stays out and what it names.
    pub notes: Vec<Note>,
}

/// Makes `captured` into a file to give away: the lines and words that look like secrets are taken
/// out of the texts, what stays with the computer is counted, and the meta is the coarse kind of
/// computer and nothing else.
pub fn export(
    registry: &Registry,
    captured: super::state::Captured,
    id: &str,
    name: &str,
    meta: ExportMeta,
) -> Exported {
    let mut configuration = captured.configuration;
    let mut left_out = captured.left_out;
    configuration.id = id.to_string();
    configuration.name = clean_text(name, super::MAX_NAME_CHARS, false);
    configuration.meta.note = meta
        .note
        .map(|n| clean_text(&n, 1000, true))
        .filter(|n| !n.is_empty());
    configuration.meta.made_with = Some(meta.made_with).filter(|m| !m.is_empty());
    configuration.meta.made_on = Some(meta.made_on);

    let mut notes = Vec::new();
    for (key, field) in carried(registry, &configuration.engine) {
        let Some(Value::Str(text)) = configuration.values.get(&key).cloned() else {
            continue;
        };
        let (kept, count) = if field.bind.env_lines.is_some() {
            screen::drop_secret_lines(&text)
        } else if field.bind.arg_split {
            screen::drop_secret_args(&text)
        } else {
            (text.clone(), 0)
        };
        if count > 0 {
            left_out.push(LeftOut::Secrets {
                key: key.clone(),
                count,
            });
            if kept.trim().is_empty() {
                configuration.values.remove(&key);
            } else {
                configuration
                    .values
                    .insert(key.clone(), Value::Str(kept.clone()));
            }
        }
        if let Some(path) = screen::names_a_path(&kept) {
            notes.push(
                Note::new(
                    "names_a_path",
                    &[("field", label_of(field)), ("path", path)],
                )
                .about(key.clone()),
            );
        }
    }
    let private = left_out
        .iter()
        .filter(|l| matches!(l, LeftOut::Private { .. }))
        .count();
    if private > 0 {
        notes.push(Note::new(
            "left_out_private",
            &[("count", private.to_string())],
        ));
    }
    let secrets: usize = left_out
        .iter()
        .map(|l| match l {
            LeftOut::Secrets { count, .. } => *count,
            _ => 0,
        })
        .sum();
    if secrets > 0 {
        notes.push(Note::new(
            "left_out_secrets",
            &[("count", secrets.to_string())],
        ));
    }
    for item in &left_out {
        if let LeftOut::ModelSource { name } = item {
            notes
                .push(Note::new("left_out_source", &[("model", name.clone())]).about(name.clone()));
        }
    }
    notes::sort(&mut notes);
    Exported {
        configuration,
        left_out,
        notes,
    }
}
