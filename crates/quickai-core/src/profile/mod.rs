//! Configurations: a runtime as a file that can be chosen, saved and given to someone else
//! (docs/configurations.md).
//!
//! A configuration is a `profile` document (`doc::profile` is its shape). This module is what the
//! program does with one, all of it pure: `read` turns a document into a `Configuration` held to
//! what the engines' documents say (`Configuration::to_toml` is the way back), `state` captures the
//! working state into one and applies one to it, and `check` says what a person should be told
//! before one is used on a computer it was not made on. Nothing here names an engine or a setting:
//! which settings travel is the `share` of each field of the engine documents, and what is said
//! about them is the vocabulary's.

pub mod check;
pub mod notes;
pub mod screen;
pub mod state;

use std::collections::BTreeMap;

use crate::doc::engine::{Field, FieldType, Share};
use crate::doc::profile::{AppDoc, MadeOnDoc, MetaDoc, ModelDoc, ProfileDoc, RuntimeDoc};
use crate::doc::read_body;
use crate::docpath::DocPath;
use crate::plan::resolve::check_value;
use crate::registry::Engine;
use crate::value::Value;
use notes::Note;

/// The most a configuration file may be, in bytes. A larger one is not read.
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// The most characters a name may have.
pub const MAX_NAME_CHARS: usize = 60;
/// The most bytes a text of a setting may have.
pub const MAX_TEXT_BYTES: usize = 4096;
/// The most settings, and the most models, one file may hold.
pub const MAX_VALUES: usize = 512;
pub const MAX_MODELS: usize = 8;

/// The key a setting has in a selection and in the working state: `<engine>.<field>`.
pub fn value_key(engine: &str, field: &str) -> String {
    format!("{engine}.{field}")
}

/// Splits `<engine>.<field>` at its last dot (an engine's id may have dots, a field's cannot).
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.rsplit_once('.')
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Configuration {
    pub id: String,
    pub name: String,
    pub meta: Meta,
    pub engine: String,
    pub runtime: Runtime,
    pub models: Vec<ModelPick>,
    /// Settings by `<engine>.<field>`, only those the file gave and this version holds.
    pub values: BTreeMap<String, Value>,
    pub traffic_proxy: Option<bool>,
    /// Keys at the top of the file that this version does not know, kept so that saving does not lose
    /// what a newer version wrote.
    pub extra: toml::Table,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub note: Option<String>,
    pub made_with: Option<String>,
    pub made_on: Option<MadeOn>,
    /// Keys of `[meta]` this version does not know.
    pub extra: toml::Table,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MadeOn {
    pub os: Option<String>,
    pub arch: Option<String>,
    pub gpu: Vec<String>,
    pub memory_gb: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Runtime {
    pub build: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelPick {
    /// What it is for; `main` unless the file says.
    pub role: String,
    pub source: ModelSource,
    /// What people call it.
    pub name: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    /// Where it can be got from (`huggingface:owner/repo`), when it is a file that is no entry of the list
    /// and the application remembers where it was fetched from.
    pub from: Option<String>,
}

/// Who a model is, never where it is on a disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelSource {
    /// A name the engine lists and pulls (`ollama = "qwen3:8b"`): the kind is one of the engine's
    /// `accepts.refs`.
    Named { kind: String, name: String },
    /// An entry of the recommended list, and one of its files.
    Catalog { id: String, file: Option<String> },
    /// A file of the models folder, by its name.
    File { name: String },
}

impl ModelSource {
    /// What a sentence calls it: the name, the file or the id.
    pub fn label(&self) -> &str {
        match self {
            ModelSource::Named { name, .. } => name,
            ModelSource::Catalog { id, file } => file.as_deref().unwrap_or(id),
            ModelSource::File { name } => name,
        }
    }
}

impl ModelPick {
    /// The name a person is shown: the one the file gave, else the source's.
    pub fn display(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| self.source.label())
    }
}

/// Something that makes a document unusable: where it is and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fatal {
    pub path: DocPath,
    pub message: String,
    pub hint: Option<String>,
}

impl Fatal {
    fn at(path: DocPath, message: impl Into<String>) -> Fatal {
        Fatal {
            path,
            message: message.into(),
            hint: None,
        }
    }

    fn hint(mut self, hint: impl Into<String>) -> Fatal {
        self.hint = Some(hint.into());
        self
    }
}

/// A configuration that was read, and what is odd about it that does not make it unusable.
#[derive(Clone, Debug)]
pub struct Read {
    pub configuration: Configuration,
    pub notes: Vec<Note>,
    /// Where the keys this version does not know are (every one is in `notes` as one `unknown_keys`).
    pub unknown: Vec<DocPath>,
}

/// The engine `id` and what goes with it (its companions), in the order of the selector. Empty when
/// there is no such engine, or it is a companion itself.
pub fn family<'a>(engines: &'a BTreeMap<String, Engine>, id: &str) -> Vec<&'a Engine> {
    let Some(head) = engines
        .get(id)
        .filter(|e| e.doc.engine.companion_of.is_none())
    else {
        return Vec::new();
    };
    let mut companions: Vec<&Engine> = engines
        .values()
        .filter(|e| e.doc.engine.companion_of.as_deref() == Some(id))
        .collect();
    companions.sort_by_key(|e| (e.doc.engine.order.unwrap_or(100), e.id.as_str()));
    let mut out = vec![head];
    out.extend(companions);
    out
}

/// What kind of file name a name is: the whole of it, no folder, no drive, nothing a shell would
/// take for more than a name.
pub fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 200
        && name != "."
        && !name.contains("..")
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(|c| c.is_control() || is_direction(c))
        && name == name.trim()
}

/// The label of a build: the name of the directory it is installed in.
pub fn valid_label(label: &str) -> bool {
    !label.is_empty() && label.chars().count() <= 100 && valid_file_name(label)
}

/// A name an engine pulls (`qwen3:8b`, `hf.co/user/repo:Q4_K_M`): text with no space or control
/// character, no folder tricks.
pub fn valid_named_ref(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 200
        && !name.starts_with('/')
        && !name.contains(['\\', '\0'])
        && !name.split('/').any(|part| part == ".." || part == ".")
        && !name.contains("//")
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || is_direction(c))
}

/// A kind of place and a name in it (`huggingface:owner/repo`): what a file says of where it can be got.
pub fn valid_from(text: &str) -> bool {
    let Some((kind, name)) = text.split_once(':') else {
        return false;
    };
    let plain = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    text.chars().count() <= 200
        && !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && name.split('/').count() == 2
        && name
            .split('/')
            .all(|part| plain(part) && part != "." && part != "..")
}

/// A place a model can be got from, as a sentence names it: `huggingface.co/owner/repo`.
pub fn from_label(from: &str) -> String {
    match from.split_once(':') {
        Some(("huggingface", name)) => format!("huggingface.co/{name}"),
        _ => from.to_string(),
    }
}

fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
}

fn valid_role(role: &str) -> bool {
    !role.is_empty()
        && role.chars().count() <= 32
        && role
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

/// The characters that change which way text runs, which can make a name read as another.
fn is_direction(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Text for people to read: control and direction characters taken out (a newline stays when
/// `lines`), and the ends trimmed. A text longer than `max` characters is cut.
pub fn clean_text(text: &str, max: usize, lines: bool) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !is_direction(*c) && (!c.is_control() || (lines && matches!(c, '\n'))))
        .collect();
    cleaned.trim().chars().take(max).collect()
}

/// Reads the body of a `profile` document. `engines` is what this build has, which the values are
/// held to. A document that cannot be used at all gives the places that are wrong; one that can be
/// used gives the configuration and a note for each thing in it that is odd (a setting the engine
/// does not have, a value that does not fit, keys this version does not know).
pub fn read(
    id: &str,
    body: &toml::Table,
    engines: &BTreeMap<String, Engine>,
) -> Result<Read, Vec<Fatal>> {
    let typed = match read_body::<ProfileDoc>(body) {
        Ok(typed) => typed,
        Err(e) => return Err(vec![Fatal::at(e.path, e.message)]),
    };
    let doc = typed.value;
    let mut fatal: Vec<Fatal> = Vec::new();
    let mut notes: Vec<Note> = Vec::new();

    let name = match &doc.name {
        None => id.to_string(),
        Some(name) if name.chars().count() > MAX_NAME_CHARS => {
            fatal.push(
                Fatal::at(
                    DocPath::default().key("name"),
                    format!("the name has more than {MAX_NAME_CHARS} characters"),
                )
                .hint("a name is what the list shows; the note is the place for more"),
            );
            id.to_string()
        }
        Some(name) => {
            let cleaned = clean_text(name, MAX_NAME_CHARS, false);
            if cleaned.is_empty() {
                id.to_string()
            } else {
                cleaned
            }
        }
    };

    let engine_id = doc.engine.trim();
    if engine_id.is_empty()
        || !engine_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        fatal.push(Fatal::at(
            DocPath::default().key("engine"),
            format!("`{}` is not the id of an engine", doc.engine),
        ));
    }

    let runtime = read_runtime(doc.runtime.as_ref(), &mut fatal);
    let models = read_models(&doc.models, engines.get(engine_id), &mut fatal);
    let meta = read_meta(&doc.meta, body, &mut fatal);

    // ---- the values ----
    let members = family(engines, engine_id);
    let mut values: BTreeMap<String, Value> = BTreeMap::new();
    let count: usize = doc.values.values().map(BTreeMap::len).sum();
    if count > MAX_VALUES {
        fatal.push(Fatal::at(
            DocPath::default().key("values"),
            format!("the file sets {count} settings; a configuration has at most {MAX_VALUES}"),
        ));
    } else {
        for (owner, table) in &doc.values {
            let engine = members.iter().find(|e| e.id == *owner).copied();
            for (field_id, raw) in table {
                let at = DocPath::default().key("values").key(owner).key(field_id);
                let key = value_key(owner, field_id);
                let Some(engine) = engine else {
                    notes.push(
                        Note::new("unknown_setting", &[("setting", key.clone())])
                            .at(at)
                            .about(key),
                    );
                    continue;
                };
                let Some(field) = engine.doc.fields.iter().find(|f| f.id == *field_id) else {
                    notes.push(
                        Note::new("unknown_setting", &[("setting", key.clone())])
                            .at(at)
                            .about(key),
                    );
                    continue;
                };
                if field.share().is_none_or(|s| s == Share::Never) {
                    notes.push(
                        Note::new("private_setting", &[("setting", key.clone())])
                            .at(at)
                            .about(key),
                    );
                    continue;
                }
                match fit(field, &Value::from_toml(raw)) {
                    Ok(value) => {
                        values.insert(key, value);
                    }
                    Err(why) => notes.push(
                        Note::new(
                            "bad_value",
                            &[
                                ("field", label_of(field)),
                                ("value", shown(&Value::from_toml(raw))),
                                ("why", why),
                            ],
                        )
                        .at(at)
                        .about(key),
                    ),
                }
            }
        }
    }

    let traffic_proxy = doc.app.as_ref().and_then(|a: &AppDoc| a.traffic_proxy);

    // ---- the keys this version does not know ----
    // those at the top are kept when the file is written again; every one is told
    let mut extra = toml::Table::new();
    for path in &typed.unknown {
        let text = path.to_string();
        if !text.contains(['.', '[']) {
            if let Some(value) = body.get(&text) {
                extra.insert(text, value.clone());
            }
        }
    }
    let mut unknown_names: Vec<String> = typed.unknown.iter().map(DocPath::to_string).collect();
    unknown_names.sort();
    if !unknown_names.is_empty() {
        notes.push(Note::new(
            "unknown_keys",
            &[("keys", unknown_names.join(", "))],
        ));
    }

    if !fatal.is_empty() {
        return Err(fatal);
    }
    Ok(Read {
        configuration: Configuration {
            id: id.to_string(),
            name,
            meta,
            engine: engine_id.to_string(),
            runtime,
            models,
            values,
            traffic_proxy,
            extra,
        },
        notes,
        unknown: typed.unknown,
    })
}

/// What people call a field.
pub fn label_of(field: &Field) -> String {
    field.label.clone().unwrap_or_else(|| field.id.clone())
}

/// A value as a sentence shows it.
pub fn shown(value: &Value) -> String {
    match value {
        Value::Str(s) => {
            let one_line: String = s.chars().take(60).collect();
            if s.chars().count() > 60 {
                format!("{one_line}...")
            } else {
                one_line
            }
        }
        other => other.to_string(),
    }
}

/// Whether `value` can be the value of `field`, and the value to hold if it can: a number is
/// held to the field's range, a choice to its options, a text to its pattern and to the size a
/// text may have. The error is a phrase for a sentence.
pub fn fit(field: &Field, value: &Value) -> Result<Value, String> {
    check_value(field.ty, value)?;
    match field.ty {
        FieldType::Int | FieldType::Port | FieldType::Float => {
            let number = value.as_f64().ok_or("not a number")?;
            if let Some((lo, hi)) = field.range {
                if number < lo || number > hi {
                    return Err(format!("more than {hi} or less than {lo}"));
                }
            }
            if field.ty == FieldType::Float {
                return Ok(Value::Float(number));
            }
            Ok(Value::Int(value.as_i64().ok_or("not a whole number")?))
        }
        FieldType::Choice => {
            let chosen = value.as_str().unwrap_or_default();
            if field.options.iter().any(|o| o == chosen) {
                Ok(value.clone())
            } else {
                Err(format!("not one of {}", field.options.join(", ")))
            }
        }
        FieldType::Bool => Ok(value.clone()),
        FieldType::Hidden | FieldType::ModelRef => Err("not a setting".to_string()),
        _ => {
            let text = value.as_str().unwrap_or_default();
            if text.len() > MAX_TEXT_BYTES {
                return Err(format!("longer than {MAX_TEXT_BYTES} characters"));
            }
            if text.contains('\0') {
                return Err("holds a character that cannot be in a setting".to_string());
            }
            Ok(value.clone())
        }
    }
}

fn read_runtime(doc: Option<&RuntimeDoc>, fatal: &mut Vec<Fatal>) -> Runtime {
    let Some(doc) = doc else {
        return Runtime::default();
    };
    let build = doc.build.as_ref().and_then(|build| {
        if valid_label(build) {
            Some(build.clone())
        } else {
            fatal.push(
                Fatal::at(
                    DocPath::default().key("runtime").key("build"),
                    format!(
                        "`{}` is not the label of a build",
                        clean_text(build, 40, false)
                    ),
                )
                .hint("a label is a name like `v0.40.0`, with no folder in it"),
            );
            None
        }
    });
    Runtime {
        build,
        version: doc
            .version
            .as_ref()
            .map(|v| clean_text(v, 64, false))
            .filter(|v| !v.is_empty()),
    }
}

fn read_meta(doc: &MetaDoc, body: &toml::Table, _fatal: &mut Vec<Fatal>) -> Meta {
    let mut extra = toml::Table::new();
    if let Some(toml::Value::Table(meta)) = body.get("meta") {
        for (key, value) in meta {
            if !matches!(key.as_str(), "note" | "made_with" | "made_on") {
                extra.insert(key.clone(), value.clone());
            }
        }
    }
    let text = |value: &Option<String>, max: usize, lines: bool| {
        value
            .as_ref()
            .map(|v| clean_text(v, max, lines))
            .filter(|v| !v.is_empty())
    };
    Meta {
        note: text(&doc.note, 1000, true),
        made_with: text(&doc.made_with, 100, false),
        made_on: doc.made_on.as_ref().map(|m: &MadeOnDoc| MadeOn {
            os: text(&m.os, 32, false),
            arch: text(&m.arch, 32, false),
            gpu: m
                .gpu
                .iter()
                .take(8)
                .map(|g| clean_text(g, 32, false))
                .filter(|g| !g.is_empty())
                .collect(),
            memory_gb: m.memory_gb,
        }),
        extra,
    }
}

fn read_models(
    docs: &[ModelDoc],
    engine: Option<&Engine>,
    fatal: &mut Vec<Fatal>,
) -> Vec<ModelPick> {
    if docs.len() > MAX_MODELS {
        fatal.push(Fatal::at(
            DocPath::default().key("model"),
            format!("a configuration has at most {MAX_MODELS} models"),
        ));
        return Vec::new();
    }
    let accepted: Option<&[String]> = engine.map(|e| {
        e.doc
            .provides
            .accepts
            .as_ref()
            .map(|a| a.refs.as_slice())
            .unwrap_or(&[])
    });
    let mut out = Vec::new();
    for (index, doc) in docs.iter().enumerate() {
        let at = DocPath::default().key("model").index(index);
        // the kinds of named reference: the keys of the table that are none of the others
        let mut named: Vec<(&String, &toml::Value)> = Vec::new();
        let mut broken = false;
        for (kind, value) in &doc.refs {
            if accepted.is_some_and(|kinds| !kinds.contains(kind)) {
                fatal.push(Fatal::at(
                    at.clone().key(kind),
                    format!(
                        "`{kind}` is not a kind of name this engine takes; it takes {}",
                        accepted_text(accepted)
                    ),
                ));
                broken = true;
                continue;
            }
            named.push((kind, value));
        }
        let sources = usize::from(!named.is_empty())
            + usize::from(doc.catalog.is_some())
            + usize::from(doc.file.is_some() && doc.catalog.is_none());
        if broken {
            continue;
        }
        if named.len() > 1 || sources != 1 {
            fatal.push(
                Fatal::at(
                    at.clone(),
                    "a model has exactly one source: a name the engine takes, a `catalog` id (with its `file`), or a `file`",
                )
                .hint("never a path"),
            );
            continue;
        }
        let source = if let Some((kind, value)) = named.first() {
            match value.as_str() {
                Some(name) if valid_named_ref(name) => ModelSource::Named {
                    kind: (*kind).clone(),
                    name: name.to_string(),
                },
                _ => {
                    fatal.push(Fatal::at(
                        at.clone().key(kind.as_str()),
                        "that is not a name an engine pulls",
                    ));
                    continue;
                }
            }
        } else if let Some(id) = &doc.catalog {
            if !crate::registry::catalog::valid_id(id) {
                fatal.push(Fatal::at(
                    at.clone().key("catalog"),
                    format!(
                        "`{}` is not the id of a model of the list",
                        clean_text(id, 40, false)
                    ),
                ));
                continue;
            }
            match &doc.file {
                Some(file) if !valid_file_name(file) => {
                    fatal.push(Fatal::at(
                        at.clone().key("file"),
                        "a file is a name, with no folder in it",
                    ));
                    continue;
                }
                file => ModelSource::Catalog {
                    id: id.clone(),
                    file: file.clone(),
                },
            }
        } else if let Some(file) = &doc.file {
            if !valid_file_name(file) {
                fatal.push(Fatal::at(
                    at.clone().key("file"),
                    "a file is a name, with no folder in it",
                ));
                continue;
            }
            ModelSource::File { name: file.clone() }
        } else {
            continue;
        };
        let role = match &doc.role {
            None => "main".to_string(),
            Some(role) if valid_role(role) => role.clone(),
            Some(_) => {
                fatal.push(Fatal::at(
                    at.clone().key("role"),
                    "a role is a word of letters, digits, `_` and `-`",
                ));
                continue;
            }
        };
        let sha256 = match &doc.sha256 {
            Some(sum) if valid_sha256(sum) => Some(sum.to_ascii_lowercase()),
            Some(_) => {
                fatal.push(Fatal::at(
                    at.clone().key("sha256"),
                    "a SHA-256 is 64 hexadecimal digits",
                ));
                continue;
            }
            None => None,
        };
        let from = match &doc.from {
            Some(from) if valid_from(from) => Some(from.clone()),
            Some(_) => {
                fatal.push(
                    Fatal::at(
                        at.clone().key("from"),
                        "`from` is a kind of place and a name in it",
                    )
                    .hint("for example `huggingface:owner/repo`"),
                );
                continue;
            }
            None => None,
        };
        out.push(ModelPick {
            role,
            source,
            name: doc
                .name
                .as_ref()
                .map(|n| clean_text(n, 120, false))
                .filter(|n| !n.is_empty()),
            size: doc.size,
            sha256,
            from,
        });
    }
    out
}

fn accepted_text(accepted: Option<&[String]>) -> String {
    match accepted {
        Some(kinds) if !kinds.is_empty() => kinds.join(", "),
        _ => "files".to_string(),
    }
}

// ---- writing ---------------------------------------------------------------------------------------

/// Whether the text of a file says `replace = true`: it stands in for a document of the same id in a
/// lower layer instead of patching it.
pub fn replaces(text: &str) -> bool {
    toml::from_str::<toml::Table>(text)
        .ok()
        .and_then(|table| crate::doc::Envelope::split(table).ok())
        .is_some_and(|(envelope, _)| envelope.replace)
}

/// The lowest application version that reads what this build writes: `>=0.1` for 0.1.7.
pub fn requires_for(version: &str) -> String {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    format!(">={major}.{minor}")
}

impl Configuration {
    /// The file. `engines` orders the settings as the engines' forms do; `replace` marks a file that
    /// stands in for a document of the same id in a lower layer (the packages' or the application's).
    pub fn to_toml(
        &self,
        engines: &BTreeMap<String, Engine>,
        app_version: &str,
        replace: bool,
    ) -> String {
        use toml_edit::{value, Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table};

        let mut doc = DocumentMut::new();
        doc["schema"] = value(1);
        doc["kind"] = value("profile");
        doc["id"] = value(self.id.as_str());
        if replace {
            doc["replace"] = value(true);
        }
        let mut requires = InlineTable::new();
        requires.insert("app", requires_for(app_version).into());
        doc["requires"] = Item::Value(requires.into());
        doc["name"] = value(self.name.as_str());
        doc["engine"] = value(self.engine.as_str());
        for (key, item) in &self.extra {
            if let Some(item) = extra_item(key, item) {
                doc[key.as_str()] = item;
            }
        }

        // the tables, after every plain key
        let has_meta = self.meta.note.is_some()
            || self.meta.made_with.is_some()
            || self.meta.made_on.is_some()
            || !self.meta.extra.is_empty();
        if has_meta {
            let mut meta = Table::new();
            if let Some(note) = &self.meta.note {
                meta["note"] = value(note.as_str());
            }
            if let Some(made_with) = &self.meta.made_with {
                meta["made_with"] = value(made_with.as_str());
            }
            if let Some(on) = &self.meta.made_on {
                let mut inline = InlineTable::new();
                if let Some(os) = &on.os {
                    inline.insert("os", os.as_str().into());
                }
                if let Some(arch) = &on.arch {
                    inline.insert("arch", arch.as_str().into());
                }
                if !on.gpu.is_empty() {
                    let mut gpus = Array::new();
                    for gpu in &on.gpu {
                        gpus.push(gpu.as_str());
                    }
                    inline.insert("gpu", gpus.into());
                }
                if let Some(gb) = on.memory_gb {
                    inline.insert("memory_gb", (gb as i64).into());
                }
                meta["made_on"] = Item::Value(inline.into());
            }
            for (key, item) in &self.meta.extra {
                if let Some(item) = extra_item(key, item) {
                    meta[key.as_str()] = item;
                }
            }
            doc["meta"] = Item::Table(meta);
        }
        if self.runtime.build.is_some() || self.runtime.version.is_some() {
            let mut runtime = Table::new();
            if let Some(build) = &self.runtime.build {
                runtime["build"] = value(build.as_str());
            }
            if let Some(version) = &self.runtime.version {
                runtime["version"] = value(version.as_str());
            }
            doc["runtime"] = Item::Table(runtime);
        }
        if !self.models.is_empty() {
            let mut models = ArrayOfTables::new();
            for pick in &self.models {
                let mut model = Table::new();
                model["role"] = value(pick.role.as_str());
                if let Some(name) = &pick.name {
                    model["name"] = value(name.as_str());
                }
                match &pick.source {
                    ModelSource::Named { kind, name } => {
                        model[kind.as_str()] = value(name.as_str())
                    }
                    ModelSource::Catalog { id, file } => {
                        model["catalog"] = value(id.as_str());
                        if let Some(file) = file {
                            model["file"] = value(file.as_str());
                        }
                    }
                    ModelSource::File { name } => model["file"] = value(name.as_str()),
                }
                if let Some(size) = pick.size {
                    model["size"] = value(size as i64);
                }
                if let Some(sum) = &pick.sha256 {
                    model["sha256"] = value(sum.as_str());
                }
                if let Some(from) = &pick.from {
                    model["from"] = value(from.as_str());
                }
                models.push(model);
            }
            doc["model"] = Item::ArrayOfTables(models);
        }
        // settings by owner, each in the order of the engine's fields
        let mut owners: BTreeMap<&str, Vec<(&str, &Value)>> = BTreeMap::new();
        for (key, v) in &self.values {
            if let Some((owner, field)) = split_key(key) {
                owners.entry(owner).or_default().push((field, v));
            }
        }
        if !owners.is_empty() {
            let mut values = Table::new();
            values.set_implicit(true);
            let mut order: Vec<&str> = owners.keys().copied().collect();
            order.sort_by_key(|owner| (*owner != self.engine, *owner));
            for owner in order {
                let mut fields = owners.remove(owner).unwrap_or_default();
                if let Some(engine) = engines.get(owner) {
                    let position = |field: &str| {
                        engine
                            .doc
                            .fields
                            .iter()
                            .position(|f| f.id == field)
                            .unwrap_or(usize::MAX)
                    };
                    fields.sort_by_key(|(field, _)| (position(field), field.to_string()));
                }
                let mut table = Table::new();
                for (field, v) in fields {
                    table[field] = to_item(v);
                }
                values[owner] = Item::Table(table);
            }
            doc["values"] = Item::Table(values);
        }
        if let Some(proxy) = self.traffic_proxy {
            let mut app = Table::new();
            app["traffic_proxy"] = value(proxy);
            doc["app"] = Item::Table(app);
        }
        doc.to_string()
    }
}

/// A key this version does not know, as an item of the file being written.
fn extra_item(key: &str, value: &toml::Value) -> Option<toml_edit::Item> {
    let one = toml::to_string(&toml::Table::from_iter([(key.to_string(), value.clone())])).ok()?;
    let parsed = one.parse::<toml_edit::DocumentMut>().ok()?;
    parsed.get(key).cloned()
}

fn to_item(v: &Value) -> toml_edit::Item {
    use toml_edit::value;
    match v {
        Value::Bool(b) => value(*b),
        Value::Int(i) => value(*i),
        Value::Float(f) => value(*f),
        Value::Str(s) => value(s.as_str()),
        other => value(other.to_string()),
    }
}
