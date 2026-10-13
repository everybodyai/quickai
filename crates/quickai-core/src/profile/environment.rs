//! What belongs to the computer that runs a configuration and not to the configuration
//! (docs/headless.md, section 6): the logins the fetching needs, a mirror of a site, the settings that
//! are one computer's (a folder on a big disk, a Python), the variables of a program's environment, and
//! where to get a file from when it is not where the configuration says.
//!
//! A configuration is given away, so it carries none of it (docs/configurations.md, rule 2). Whoever runs
//! it says it: in an *environment file* (`quickai-run init` writes the file to fill in), on the command
//! line, or, for the logins, in the variables every tool of the kind reads (`HF_TOKEN`). This module is
//! the pure half: the shape of that file, what its words mean, how layers of it are put together, how a
//! value is shown without its secrets, what a configuration still needs from it, and the template of the
//! file. Reading the file and the variables is the program's. Nothing here names an engine or a setting:
//! which fields belong to a computer is the `share` of the engines' documents, and which program takes
//! lines of environment is the field that binds them.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use super::{
    clean_text, family, fit, from_label, split_key, split_url_with, url_label, valid_env_name,
    valid_file_name, valid_from, Configuration, Need, NeedKind, Token,
};
use crate::doc::engine::{Field, FieldDefault, FieldType, Share};
use crate::doc::read_body;
use crate::plan::is_secret_name;
use crate::registry::Engine;
use crate::value::Value;

/// The version of the file this reads.
pub const SCHEMA: i64 = 1;

/// A value that is never shown: a login, a key. Its `Debug` and `Display` say that it is hidden, so
/// that nothing which prints a structure can print it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: impl Into<String>) -> Secret {
        Secret(text.into())
    }

    /// The text, for the one place that sends it where it belongs.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(hidden)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("(hidden)")
    }
}

/// How the files and the lists are fetched on this computer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetch {
    /// A Hugging Face login, for repositories that need one. Sent to that site and no other.
    pub huggingface_token: Option<Secret>,
    /// Another address for huggingface.co (a mirror), without a slash at the end.
    pub huggingface_endpoint: Option<String>,
    /// A GitHub login, for the list of builds (60 requests an hour without one).
    pub github_token: Option<Secret>,
}

/// Where a file is got from instead of where the configuration says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Where {
    /// A file on this computer, or a folder that holds it (by name).
    Path(String),
    /// An address to fetch it from.
    Url(String),
    /// A kind of place and a name in it (`huggingface:owner/repo`); the path and revision of the
    /// configuration stay.
    Place(String),
}

impl Where {
    /// Tells which of the three the words are. An address is `http://` or `https://` with a host and no
    /// sign-in; a place is `kind:owner/repo`; anything else non-empty is a path.
    pub fn classify(text: &str) -> Result<Where, String> {
        let text = text.trim();
        if text.is_empty() || text.contains('\0') {
            return Err("nothing to get it from".to_string());
        }
        let lower = text.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return if split_url_with(text, true).is_some() {
                Ok(Where::Url(text.to_string()))
            } else {
                Err(format!(
                    "`{}` is not an address: it is `https://host/path`, with no sign-in in it",
                    clean_text(text, 60, false)
                ))
            };
        }
        if valid_from(text) {
            return Ok(Where::Place(text.to_string()));
        }
        Ok(Where::Path(text.to_string()))
    }

    /// What a sentence calls it.
    pub fn label(&self) -> String {
        match self {
            Where::Path(path) => path.clone(),
            Where::Url(url) => url_label(url),
            Where::Place(place) => from_label(place),
        }
    }
}

/// One line of a program's environment.
#[derive(Clone, PartialEq, Eq)]
pub struct EnvLine {
    pub name: String,
    pub value: String,
}

impl fmt::Debug for EnvLine {
    /// The value of a variable whose name says it is a secret is not shown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if is_secret_name(&self.name) {
            write!(f, "{}=(hidden)", self.name)
        } else {
            write!(f, "{}={}", self.name, self.value)
        }
    }
}

/// What is said of this computer, from every place it can be said, put together.
#[derive(Clone, Default, PartialEq)]
pub struct Environment {
    pub fetch: Fetch,
    /// Settings by `<engine>.<field>`, held to the field they are for.
    pub values: BTreeMap<String, Value>,
    /// Lines of environment by program, in the order they were given.
    pub env: BTreeMap<String, Vec<EnvLine>>,
    /// Where to get a file from, by the name it is kept under.
    pub models: BTreeMap<String, Where>,
    /// Where each thing was said (`environment file x.toml`, `--set`), by the key `describe` gives it.
    pub origins: BTreeMap<String, String>,
}

impl fmt::Debug for Environment {
    /// The settings are named and not shown, since a setting can hold lines of environment with a secret in
    /// them; the logins are `Secret`s and say so themselves.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Environment")
            .field("fetch", &self.fetch)
            .field("values", &self.values.keys().collect::<Vec<_>>())
            .field("env", &self.env)
            .field("models", &self.models)
            .finish()
    }
}

/// Something wrong in an environment file or on a command line, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

fn problem(path: impl Into<String>, message: impl Into<String>) -> Problem {
    Problem {
        path: path.into(),
        message: message.into(),
    }
}

// ---- the file --------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct EnvironmentDoc {
    schema: Option<i64>,
    #[serde(default)]
    fetch: FetchDoc,
    #[serde(default)]
    values: BTreeMap<String, BTreeMap<String, toml::Value>>,
    #[serde(default)]
    env: BTreeMap<String, BTreeMap<String, toml::Value>>,
    #[serde(default)]
    models: BTreeMap<String, String>,
}

#[derive(Deserialize, Default)]
struct FetchDoc {
    huggingface_token: Option<String>,
    huggingface_endpoint: Option<String>,
    github_token: Option<String>,
}

/// The line a parse error is on.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

/// What the program of `engine_id` takes: its field of lines of environment, if it has one.
pub fn env_lines_field(engine: &Engine) -> Option<&Field> {
    engine
        .doc
        .fields
        .iter()
        .find(|f| f.bind.env_lines.is_some())
}

/// The members of the family of `engine_id`, or all engines when the id names none (so that a file can be
/// checked without a configuration).
fn members<'a>(engines: &'a BTreeMap<String, Engine>, engine_id: &str) -> Vec<&'a Engine> {
    let family = family(engines, engine_id);
    if family.is_empty() {
        engines.values().collect()
    } else {
        family
    }
}

/// Whether a text a person gave is the same as nothing: the template leaves what is not wanted empty.
fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

impl Environment {
    /// Reads the text of an environment file. `origin` is how its words are told later (`environment file
    /// x.toml`). Returns the environment, the keys this version does not know (to be told as warnings),
    /// or every place that is wrong. An empty value is not set.
    pub fn read(
        text: &str,
        origin: &str,
        engines: &BTreeMap<String, Engine>,
        engine_id: &str,
    ) -> Result<(Environment, Vec<String>), Vec<Problem>> {
        let table: toml::Table = match toml::from_str(text) {
            Ok(table) => table,
            Err(e) => {
                let line = e.span().map_or(1, |s| line_of(text, s.start));
                return Err(vec![problem(
                    format!("{origin}:{line}"),
                    format!("not a TOML file: {}", e.message()),
                )]);
            }
        };
        let typed = match read_body::<EnvironmentDoc>(&table) {
            Ok(typed) => typed,
            Err(e) => return Err(vec![problem(format!("{origin}: {}", e.path), e.message)]),
        };
        let doc = typed.value;
        let at = |path: &str| format!("{origin}: {path}");
        let mut problems = Vec::new();
        if doc.schema.is_some_and(|s| s != SCHEMA) {
            problems.push(problem(
                at("schema"),
                format!(
                    "this file is of a version this QuickAI does not read (`schema = {}`); it reads {SCHEMA}",
                    doc.schema.unwrap_or_default()
                ),
            ));
        }
        let mut found = Environment::default();
        let say = |found: &mut Environment, key: String| {
            found.origins.insert(key, origin.to_string());
        };

        // [fetch]
        if let Some(token) = doc.fetch.huggingface_token.filter(|t| !blank(t)) {
            found.fetch.huggingface_token = Some(Secret::new(token.trim()));
            say(&mut found, "fetch.huggingface_token".into());
        }
        if let Some(token) = doc.fetch.github_token.filter(|t| !blank(t)) {
            found.fetch.github_token = Some(Secret::new(token.trim()));
            say(&mut found, "fetch.github_token".into());
        }
        if let Some(endpoint) = doc.fetch.huggingface_endpoint.filter(|t| !blank(t)) {
            match endpoint_of(&endpoint) {
                Ok(endpoint) => {
                    found.fetch.huggingface_endpoint = Some(endpoint);
                    say(&mut found, "fetch.huggingface_endpoint".into());
                }
                Err(why) => problems.push(problem(at("fetch.huggingface_endpoint"), why)),
            }
        }

        // [values.<engine>]
        let family = members(engines, engine_id);
        for (owner, fields) in &doc.values {
            let Some(engine) = family.iter().find(|e| e.id == *owner) else {
                problems.push(problem(
                    at(&format!("values.{owner}")),
                    format!(
                        "`{owner}` is not the engine of this configuration or a program that goes with it ({})",
                        family.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                ));
                continue;
            };
            for (field_id, raw) in fields {
                let path = at(&format!("values.{owner}.{field_id}"));
                let Some(field) = engine.doc.fields.iter().find(|f| f.id == *field_id) else {
                    problems.push(problem(
                        path,
                        format!(
                            "{} has no setting called `{field_id}`",
                            engine.doc.engine.name
                        ),
                    ));
                    continue;
                };
                if matches!(field.ty, FieldType::Hidden | FieldType::ModelRef) {
                    problems.push(problem(path, "that is not a setting a file can give"));
                    continue;
                }
                if matches!(raw, toml::Value::String(s) if blank(s)) {
                    continue;
                }
                match fit(field, &Value::from_toml(raw)) {
                    Ok(value) => {
                        let key = format!("{owner}.{field_id}");
                        found.values.insert(key.clone(), value);
                        say(&mut found, format!("values.{key}"));
                    }
                    Err(why) => problems.push(problem(path, why)),
                }
            }
        }

        // [env.<program>]
        for (owner, lines) in &doc.env {
            let Some(engine) = family.iter().find(|e| e.id == *owner) else {
                problems.push(problem(
                    at(&format!("env.{owner}")),
                    format!(
                        "`{owner}` is not the engine of this configuration or a program that goes with it ({})",
                        family.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                ));
                continue;
            };
            if env_lines_field(engine).is_none() {
                problems.push(problem(
                    at(&format!("env.{owner}")),
                    format!("{} takes no lines of environment", engine.doc.engine.name),
                ));
                continue;
            }
            for (name, raw) in lines {
                let path = at(&format!("env.{owner}.{name}"));
                if !valid_env_name(name) {
                    problems.push(problem(
                        path,
                        "that is not the name of a variable: letters, digits and `_`, not starting with a digit",
                    ));
                    continue;
                }
                let text = match raw {
                    toml::Value::String(s) => s.clone(),
                    toml::Value::Integer(i) => i.to_string(),
                    toml::Value::Boolean(b) => b.to_string(),
                    toml::Value::Float(f) => f.to_string(),
                    _ => {
                        problems.push(problem(
                            path,
                            "a variable has a text, a number or true/false",
                        ));
                        continue;
                    }
                };
                if blank(&text) {
                    continue;
                }
                if text.contains(['\n', '\r', '\0']) {
                    problems.push(problem(path, "a variable's value is one line"));
                    continue;
                }
                found.env.entry(owner.clone()).or_default().push(EnvLine {
                    name: name.clone(),
                    value: text,
                });
                say(&mut found, format!("env.{owner}.{name}"));
            }
        }

        // [models]
        for (file, place) in &doc.models {
            let path = at(&format!("models.\"{file}\""));
            if !valid_file_name(file) {
                problems.push(problem(path, "a file is a name, with no folder in it"));
                continue;
            }
            if blank(place) {
                continue;
            }
            match Where::classify(place) {
                Ok(place) => {
                    found.models.insert(file.clone(), place);
                    say(&mut found, format!("models.{file}"));
                }
                Err(why) => problems.push(problem(path, why)),
            }
        }

        if !problems.is_empty() {
            return Err(problems);
        }
        let mut unknown: Vec<String> = typed
            .unknown
            .iter()
            .map(|p| {
                format!("{origin}: `{p}` is not used by this version of QuickAI and is ignored")
            })
            .collect();
        unknown.sort();
        Ok((found, unknown))
    }

    /// Puts `later` over this: what it says wins, its lines of environment follow ours.
    pub fn merge(&mut self, later: Environment) {
        if later.fetch.huggingface_token.is_some() {
            self.fetch.huggingface_token = later.fetch.huggingface_token;
        }
        if later.fetch.huggingface_endpoint.is_some() {
            self.fetch.huggingface_endpoint = later.fetch.huggingface_endpoint;
        }
        if later.fetch.github_token.is_some() {
            self.fetch.github_token = later.fetch.github_token;
        }
        self.values.extend(later.values);
        for (program, lines) in later.env {
            self.env.entry(program).or_default().extend(lines);
        }
        self.models.extend(later.models);
        self.origins.extend(later.origins);
    }

    /// `--set <engine>.<field>=<text>`: one setting, the text read as the field's type.
    pub fn set_text(
        &mut self,
        engines: &BTreeMap<String, Engine>,
        engine_id: &str,
        key: &str,
        text: &str,
        origin: &str,
    ) -> Result<(), String> {
        let family = members(engines, engine_id);
        let (owner, field_id) = split_key(key).ok_or_else(|| {
            format!("`{key}` is not `<engine>.<field>`: the id of the engine, a dot, and the id of its setting")
        })?;
        let engine = family.iter().find(|e| e.id == owner).ok_or_else(|| {
            format!(
                "`{owner}` is not the engine of this configuration or a program that goes with it ({})",
                family.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(", ")
            )
        })?;
        let field = engine
            .doc
            .fields
            .iter()
            .find(|f| f.id == field_id && !matches!(f.ty, FieldType::Hidden | FieldType::ModelRef))
            .ok_or_else(|| {
                format!(
                    "{} has no setting called `{field_id}` (it has {})",
                    engine.doc.engine.name,
                    engine
                        .doc
                        .fields
                        .iter()
                        .filter(|f| !matches!(f.ty, FieldType::Hidden | FieldType::ModelRef))
                        .map(|f| f.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        let value = coerce(field, text).map_err(|why| format!("`{key}`: {why}"))?;
        let value = fit(field, &value).map_err(|why| format!("`{key}`: {why}"))?;
        self.values.insert(key.to_string(), value);
        self.origins
            .insert(format!("values.{key}"), origin.to_string());
        Ok(())
    }

    /// `--env [program:]NAME=VALUE`: a line of a program's environment, after the ones already given.
    pub fn add_env_line(
        &mut self,
        engines: &BTreeMap<String, Engine>,
        engine_id: &str,
        spec: &str,
        origin: &str,
    ) -> Result<(), String> {
        let (left, value) = spec
            .split_once('=')
            .ok_or_else(|| format!("`{spec}` is not `NAME=VALUE` (or `program:NAME=VALUE`)"))?;
        let (program, name) = match left.split_once(':') {
            Some((program, name)) => (program, name),
            None => (engine_id, left),
        };
        if !valid_env_name(name) {
            return Err(format!(
                "`{}` is not the name of a variable: letters, digits and `_`, not starting with a digit",
                clean_text(name, 40, false)
            ));
        }
        if value.contains(['\n', '\r', '\0']) {
            return Err(format!("the value of {name} is one line"));
        }
        let family = members(engines, engine_id);
        let engine = family.iter().find(|e| e.id == program).ok_or_else(|| {
            format!(
                "`{program}` is not the engine of this configuration or a program that goes with it ({})",
                family.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(", ")
            )
        })?;
        if env_lines_field(engine).is_none() {
            return Err(format!(
                "{} takes no lines of environment, so {name} cannot be given to it",
                engine.doc.engine.name
            ));
        }
        self.env
            .entry(program.to_string())
            .or_default()
            .push(EnvLine {
                name: name.to_string(),
                value: value.to_string(),
            });
        self.origins
            .insert(format!("env.{program}.{name}"), origin.to_string());
        Ok(())
    }

    /// `--model-source <file>=<where>`.
    pub fn add_model_source(&mut self, spec: &str, origin: &str) -> Result<(), String> {
        let (file, place) = spec
            .split_once('=')
            .ok_or_else(|| format!("`{spec}` is not `FILE=WHERE`: the name the file is kept under, then a path, an address or `huggingface:owner/repo`"))?;
        if !valid_file_name(file) {
            return Err(format!(
                "`{}` is not a file name (no folder in it)",
                clean_text(file, 60, false)
            ));
        }
        let place = Where::classify(place).map_err(|why| format!("`{file}`: {why}"))?;
        self.models.insert(file.to_string(), place);
        self.origins
            .insert(format!("models.{file}"), origin.to_string());
        Ok(())
    }

    /// A login or an address given by a variable or a flag.
    pub fn set_fetch(&mut self, key: &str, text: &str, origin: &str) -> Result<(), String> {
        if blank(text) {
            return Ok(());
        }
        match key {
            "huggingface_token" => self.fetch.huggingface_token = Some(Secret::new(text.trim())),
            "github_token" => self.fetch.github_token = Some(Secret::new(text.trim())),
            "huggingface_endpoint" => {
                self.fetch.huggingface_endpoint = Some(endpoint_of(text)?);
            }
            other => return Err(format!("`{other}` is not something fetching takes")),
        }
        self.origins
            .insert(format!("fetch.{key}"), origin.to_string());
        Ok(())
    }

    /// Puts what is said here over `values`, the settings the configuration gave: the settings first, then
    /// the lines of environment, which follow what the configuration's own field holds. Names the program
    /// that takes no lines when there is no field for them.
    pub fn apply_to(
        &self,
        engines: &BTreeMap<String, Engine>,
        values: &mut BTreeMap<String, Value>,
    ) -> Result<(), String> {
        for (key, value) in &self.values {
            values.insert(key.clone(), value.clone());
        }
        for (program, lines) in &self.env {
            let engine = engines
                .get(program)
                .ok_or_else(|| format!("there is no program called `{program}`"))?;
            let field = env_lines_field(engine).ok_or_else(|| {
                format!("{} takes no lines of environment", engine.doc.engine.name)
            })?;
            let key = format!("{program}.{}", field.id);
            let mut text = values
                .get(&key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_end()
                .to_string();
            for line in lines {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&format!("{}={}", line.name, line.value));
            }
            values.insert(key, Value::Str(text));
        }
        Ok(())
    }

    /// What has been said, one line each, with the value shown unless it is a secret, and where it was said.
    pub fn describe(&self, engines: &BTreeMap<String, Engine>) -> Vec<Described> {
        let origin = |key: &str| self.origins.get(key).cloned().unwrap_or_default();
        let mut out = Vec::new();
        for (key, secret) in [
            ("huggingface_token", true),
            ("github_token", true),
            ("huggingface_endpoint", false),
        ] {
            let shown = match key {
                "huggingface_token" => self.fetch.huggingface_token.as_ref().map(|_| String::new()),
                "github_token" => self.fetch.github_token.as_ref().map(|_| String::new()),
                _ => self.fetch.huggingface_endpoint.clone(),
            };
            if let Some(shown) = shown {
                out.push(Described {
                    key: format!("fetch.{key}"),
                    shown: if secret { None } else { Some(shown) },
                    origin: origin(&format!("fetch.{key}")),
                });
            }
        }
        for (key, value) in &self.values {
            let secret = is_secret_name(key);
            let hidden_by_field = split_key(key)
                .and_then(|(owner, field)| {
                    let engine = engines.get(owner)?;
                    let field = engine.doc.fields.iter().find(|f| f.id == field)?;
                    // a block of lines may hold a secret in any of them
                    (field.bind.env_lines.is_some()).then_some(())
                })
                .is_some();
            let shown = if secret {
                None
            } else if hidden_by_field {
                let lines = value.as_str().unwrap_or_default().lines().count();
                Some(format!("{lines} line{}", if lines == 1 { "" } else { "s" }))
            } else {
                // a path is shown whole: it is what the person wants to check
                Some(match value {
                    Value::Str(text) => clean_text(text, 300, false),
                    other => other.to_string(),
                })
            };
            out.push(Described {
                key: format!("values.{key}"),
                shown,
                origin: origin(&format!("values.{key}")),
            });
        }
        for (program, lines) in &self.env {
            for line in lines {
                let key = format!("env.{program}.{}", line.name);
                out.push(Described {
                    shown: if is_secret_name(&line.name) {
                        None
                    } else {
                        Some(clean_text(&line.value, 60, false))
                    },
                    origin: origin(&key),
                    key,
                });
            }
        }
        for (file, place) in &self.models {
            out.push(Described {
                key: format!("models.{file}"),
                shown: Some(place.label()),
                origin: origin(&format!("models.{file}")),
            });
        }
        out
    }

    /// The things the configuration asks for that nothing here (or in the settings it was given, once
    /// the environment is put over them) says. `effective` is those settings. What the computer has to
    /// have (`NeedKind::System`) is not here: nobody gives it, the program looks at the computer.
    pub fn unmet<'a>(
        &self,
        configuration: &'a Configuration,
        effective: &BTreeMap<String, Value>,
    ) -> Vec<&'a Need> {
        configuration
            .needs
            .iter()
            .filter(|need| !self.meets(need, effective))
            .collect()
    }

    fn meets(&self, need: &Need, effective: &BTreeMap<String, Value>) -> bool {
        match &need.kind {
            NeedKind::Token(Token::Huggingface) => self.fetch.huggingface_token.is_some(),
            NeedKind::Token(Token::Github) => self.fetch.github_token.is_some(),
            NeedKind::Setting { key } => effective.get(key).is_some_and(|v| match v {
                Value::Str(s) => !blank(s),
                Value::Null => false,
                _ => true,
            }),
            NeedKind::Env { program, name } => self
                .env
                .get(program)
                .is_some_and(|lines| lines.iter().any(|l| l.name == *name && !blank(&l.value))),
            // looked at on the computer, not said by whoever runs the configuration
            NeedKind::System { .. } => true,
        }
    }
}

/// One thing that was said, for the sentence that tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Described {
    /// `values.comfyui.models_dir`, `env.openwebui.WEBUI_SECRET_KEY`, `fetch.huggingface_token`, `models.<file>`.
    pub key: String,
    /// What it is, or `None` for a secret.
    pub shown: Option<String>,
    /// Where it was said (`environment file x.toml`, `--set`, `HF_TOKEN`).
    pub origin: String,
}

/// An address for a site, without the slash at its end.
fn endpoint_of(text: &str) -> Result<String, String> {
    let text = text.trim().trim_end_matches('/');
    if split_url_with(text, true).is_some_and(|(_, _, tail)| tail.is_empty()) {
        Ok(text.to_string())
    } else {
        Err(format!(
            "`{}` is not the address of a site: `https://host`, with no path and no sign-in",
            clean_text(text, 60, false)
        ))
    }
}

/// A text a person typed, read as the value of `field`: a number for a number, `true` or `false` for a
/// switch, an option for a choice, else the text itself.
pub fn coerce(field: &Field, text: &str) -> Result<Value, String> {
    let word = text.trim();
    match field.ty {
        FieldType::Int | FieldType::Port => word
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| format!("`{word}` is not a whole number")),
        FieldType::Float => word
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| format!("`{word}` is not a number")),
        FieldType::Bool => match word.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
            "false" | "no" | "off" | "0" => Ok(Value::Bool(false)),
            _ => Err(format!("`{word}` is not true or false")),
        },
        FieldType::Choice => Ok(Value::Str(word.to_string())),
        _ => Ok(Value::Str(text.to_string())),
    }
}

// ---- the file to fill in -----------------------------------------------------------------------------

/// A file the configuration fetches, for the template to tell about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHint {
    /// The name it is kept under.
    pub name: String,
    /// Where the configuration says it comes from, as a sentence.
    pub source: String,
    pub size: Option<u64>,
}

fn comment_lines(out: &mut String, text: &str) {
    for line in wrap(text, 96) {
        out.push_str("# ");
        out.push_str(&line);
        out.push('\n');
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.len() + 1 + word.len() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

fn toml_string(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// What a field's default looks like in the file to fill in: its literal, or nothing.
fn example_value(field: &Field) -> String {
    match (&field.default, field.ty) {
        (Some(FieldDefault::Literal(toml::Value::Integer(i))), _) => i.to_string(),
        (Some(FieldDefault::Literal(toml::Value::Float(f))), _) => f.to_string(),
        (Some(FieldDefault::Literal(toml::Value::Boolean(b))), _) => b.to_string(),
        (Some(FieldDefault::Literal(toml::Value::String(s))), FieldType::Choice) => toml_string(s),
        (_, FieldType::Int | FieldType::Port | FieldType::Float) => "0".to_string(),
        (_, FieldType::Bool) => "false".to_string(),
        _ => "\"\"".to_string(),
    }
}

fn default_note(field: &Field) -> Option<String> {
    match &field.default {
        Some(FieldDefault::Literal(toml::Value::String(s))) if !s.is_empty() => {
            Some(format!("default: {s}"))
        }
        Some(FieldDefault::Literal(toml::Value::Integer(i))) => Some(format!("default: {i}")),
        Some(FieldDefault::Literal(toml::Value::Boolean(b))) => Some(format!("default: {b}")),
        Some(FieldDefault::Computed { .. }) => {
            Some("default: worked out from this computer".into())
        }
        _ => None,
    }
}

/// The settings of a program that belong to a computer or have to be read before they are taken: the
/// ones a configuration does not carry freely (`share` is `never` or `review`), and not the lines of
/// environment, which have a table of their own.
pub fn computer_fields(engine: &Engine) -> Vec<&Field> {
    engine
        .doc
        .fields
        .iter()
        .filter(|f| {
            !matches!(f.ty, FieldType::Hidden | FieldType::ModelRef)
                && f.bind.env_lines.is_none()
                && f.share().is_some_and(|s| s != Share::Yes)
        })
        .collect()
}

/// The text of the file that whoever runs `configuration` fills in. What the configuration needs is
/// written as a line to complete; everything else the documents say belongs to a computer is there in
/// comments, to be taken out of them. `files` are the files the configuration fetches.
pub fn template(
    engines: &BTreeMap<String, Engine>,
    configuration: &Configuration,
    files: &[FileHint],
) -> String {
    let family = members(engines, &configuration.engine);
    let needed_setting = |key: &str| {
        configuration
            .needs
            .iter()
            .find(|n| matches!(&n.kind, NeedKind::Setting { key: k } if k == key))
    };
    let mut out = String::new();
    out.push_str(&format!(
        "# The environment of a computer for the configuration \"{}\" ({}).\n#\n",
        clean_text(&configuration.name, 60, false),
        configuration.id
    ));
    comment_lines(
        &mut out,
        "This file holds what belongs to the computer that runs the configuration and not to the \
         configuration, which is given away: where files are kept, a login, the variables of a program's \
         environment, and where to get a file from when it is not where the configuration says. It is yours: \
         keep it private (it is written so that only you can read it), and do not give it away with the \
         configuration.",
    );
    out.push_str("#\n");
    comment_lines(
        &mut out,
        "Use it with `quickai-run check` and `quickai-run up`, as `--environment <this file>`. What is on the \
         command line wins over this file, and this file over the configuration. A line that starts with # \
         is not set, and a value left empty is not set either.",
    );
    out.push_str("\nschema = 1\n");

    // [fetch]
    out.push_str("\n[fetch]\n");
    for (key, what, token) in [
        (
            "huggingface_token",
            "A Hugging Face login, for a repository that needs one (also read from the variable HF_TOKEN). It is sent to huggingface.co and to no other site.",
            Some(Token::Huggingface),
        ),
        (
            "huggingface_endpoint",
            "Another address for huggingface.co, a mirror of it (also read from HF_ENDPOINT), for example https://hf-mirror.example",
            None,
        ),
        (
            "github_token",
            "A GitHub login: the list of builds is asked of GitHub, which allows 60 requests an hour without one (also read from GITHUB_TOKEN).",
            Some(Token::Github),
        ),
    ] {
        let need = token.and_then(|t| {
            configuration
                .needs
                .iter()
                .find(|n| n.kind == NeedKind::Token(t))
        });
        comment_lines(&mut out, what);
        if let Some(need) = need {
            comment_lines(
                &mut out,
                &format!(
                    "NEEDED by this configuration{}",
                    need.why.as_deref().map_or(String::new(), |w| format!(": {w}"))
                ),
            );
            out.push_str(&format!("{key} = \"\"\n"));
        } else {
            out.push_str(&format!("# {key} = \"\"\n"));
        }
    }

    // [values.<program>]
    for engine in &family {
        let fields = computer_fields(engine);
        let needed: Vec<&Field> = engine
            .doc
            .fields
            .iter()
            .filter(|f| needed_setting(&format!("{}.{}", engine.id, f.id)).is_some())
            .collect();
        if fields.is_empty() && needed.is_empty() {
            continue;
        }
        out.push_str(&format!("\n[values.{}]\n", engine.id));
        out.push_str(&format!(
            "# Settings of {} that belong to this computer.\n",
            engine.doc.engine.name
        ));
        let mut all: Vec<&Field> = fields;
        for field in needed {
            if !all.iter().any(|f| f.id == field.id) {
                all.push(field);
            }
        }
        for field in all {
            let need = needed_setting(&format!("{}.{}", engine.id, field.id));
            let label = field.label.clone().unwrap_or_else(|| field.id.clone());
            let mut text = label;
            if let Some(help) = &field.help {
                text.push_str(": ");
                text.push_str(help);
            }
            comment_lines(&mut out, &text);
            if field.ty == FieldType::Choice && !field.options.is_empty() {
                comment_lines(&mut out, &format!("one of: {}", field.options.join(", ")));
            }
            if let Some(default) = default_note(field) {
                comment_lines(&mut out, &default);
            }
            match need {
                Some(need) => {
                    comment_lines(
                        &mut out,
                        &format!(
                            "NEEDED by this configuration{}",
                            need.why
                                .as_deref()
                                .map_or(String::new(), |w| format!(": {w}"))
                        ),
                    );
                    out.push_str(&format!("{} = {}\n", field.id, example_value(field)));
                }
                None => out.push_str(&format!("# {} = {}\n", field.id, example_value(field))),
            }
        }
    }

    // [env.<program>]
    for engine in &family {
        let Some(field) = env_lines_field(engine) else {
            continue;
        };
        let needs: Vec<&Need> = configuration
            .needs
            .iter()
            .filter(|n| matches!(&n.kind, NeedKind::Env { program, .. } if *program == engine.id))
            .collect();
        out.push_str(&format!("\n[env.{}]\n", engine.id));
        comment_lines(
            &mut out,
            &format!(
                "Variables of the environment of {}, one `NAME = \"value\"` each.{}",
                engine.doc.engine.name,
                field
                    .help
                    .as_deref()
                    .map_or(String::new(), |h| format!(" {h}"))
            ),
        );
        if let Some(lines) = &field.bind.env_lines {
            if !lines.protect.is_empty() {
                comment_lines(
                    &mut out,
                    &format!(
                        "These are not to be set from here, the document of {} decides them: {}.",
                        engine.doc.engine.name,
                        lines.protect.join(", ")
                    ),
                );
            }
        }
        for need in needs {
            if let NeedKind::Env { name, .. } = &need.kind {
                comment_lines(
                    &mut out,
                    &format!(
                        "NEEDED by this configuration{}",
                        need.why
                            .as_deref()
                            .map_or(String::new(), |w| format!(": {w}"))
                    ),
                );
                out.push_str(&format!("{name} = \"\"\n"));
            }
        }
        out.push_str("# NAME = \"value\"\n");
    }

    // [models]
    out.push_str("\n[models]\n");
    comment_lines(
        &mut out,
        "Where to get a file from on this computer instead of where the configuration says: a path of the \
         file (or of a folder that holds it), an address, or huggingface:owner/repo. Each file the \
         configuration fetches is here with where it says it is, to be taken out of the comment. A file \
         that is in the models folder at the size it should be is not fetched at all.",
    );
    if files.is_empty() {
        out.push_str("# (this configuration fetches no file)\n");
    }
    for file in files {
        let size = file.size.map_or(String::new(), |s| {
            format!(" ({})", super::check::human_bytes(s))
        });
        out.push_str(&format!(
            "# {} = \"\"      # from {}{size}\n",
            toml_string(&file.name),
            file.source
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_prints_as_hidden_whatever_prints_it() {
        let secret = Secret::new("hf_abcdef");
        assert_eq!(format!("{secret:?}"), "Secret(hidden)");
        assert_eq!(format!("{secret}"), "(hidden)");
        let fetch = Fetch {
            huggingface_token: Some(secret),
            ..Fetch::default()
        };
        assert!(!format!("{fetch:?}").contains("hf_abcdef"));
    }

    #[test]
    fn the_three_kinds_of_place_are_told_apart() {
        assert_eq!(
            Where::classify("/srv/models/x.safetensors").unwrap(),
            Where::Path("/srv/models/x.safetensors".into())
        );
        assert_eq!(
            Where::classify("relative/x.gguf").unwrap(),
            Where::Path("relative/x.gguf".into())
        );
        assert_eq!(
            Where::classify("https://mirror.example/x.gguf").unwrap(),
            Where::Url("https://mirror.example/x.gguf".into())
        );
        assert_eq!(
            Where::classify("huggingface:acme/Thing").unwrap(),
            Where::Place("huggingface:acme/Thing".into())
        );
        assert!(Where::classify("https://user:pw@host/x").is_err());
        assert!(Where::classify("").is_err());
    }
}
