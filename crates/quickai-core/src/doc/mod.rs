//! Documents: the envelope every one carries, the seven kinds, and the typed bodies.
//!
//! A document is a TOML file whose top level holds the envelope (`schema`, `kind`, `id`, and
//! optionally `extends`, `replace`, `requires`) and whose remaining keys are the body of its kind.
//! Merging works on generic tables (docs/implementation-plan.md, C1); the typed bodies here are
//! read from the merged table, and keys the build does not know are reported, not fatal (C3).

pub mod catalog;
pub mod duration;
pub mod engine;
pub mod reference;
pub mod vocab;

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;

use crate::docpath::{DocPath, RawSeg};

/// The document schema this build reads. A document with a larger number was written for a newer
/// application and is skipped with a diagnostic that says so.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Engine,
    Vocab,
    Catalog,
    Profile,
    Ui,
    Policy,
    Bundle,
}

/// What a document may define (docs/architecture.md, section 12.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Programs to run, hosts to contact, what the user is told: never from remote content.
    Privileged,
    /// Names, sources, selections, text.
    Data,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Engine,
        Kind::Vocab,
        Kind::Catalog,
        Kind::Profile,
        Kind::Ui,
        Kind::Policy,
        Kind::Bundle,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Engine => "engine",
            Kind::Vocab => "vocab",
            Kind::Catalog => "catalog",
            Kind::Profile => "profile",
            Kind::Ui => "ui",
            Kind::Policy => "policy",
            Kind::Bundle => "bundle",
        }
    }

    /// The name with its article, for sentences: "an engine", "a catalog".
    pub fn with_article(self) -> &'static str {
        match self {
            Kind::Engine => "an engine",
            Kind::Vocab => "a vocab",
            Kind::Catalog => "a catalog",
            Kind::Profile => "a profile",
            Kind::Ui => "a ui",
            Kind::Policy => "a policy",
            Kind::Bundle => "a bundle",
        }
    }

    pub fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The directory of a layer that holds documents of this kind.
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Engine => "engines",
            Kind::Vocab => "vocab",
            Kind::Catalog => "catalog",
            Kind::Profile => "profiles",
            Kind::Ui => "ui",
            Kind::Policy => "policy",
            Kind::Bundle => "bundle",
        }
    }

    pub fn tier(self) -> Tier {
        match self {
            Kind::Engine | Kind::Ui | Kind::Policy | Kind::Bundle => Tier::Privileged,
            Kind::Vocab | Kind::Catalog | Kind::Profile => Tier::Data,
        }
    }
}

/// A primitive a document needs, such as `spawn@1`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveRef {
    pub name: String,
    pub version: u32,
}

impl PrimitiveRef {
    pub fn parse(text: &str) -> Result<PrimitiveRef, String> {
        let Some((name, version)) = text.rsplit_once('@') else {
            return Err(format!(
                "`{text}` should look like `name@version`, for example `spawn@1`"
            ));
        };
        let version: u32 = version
            .parse()
            .map_err(|_| format!("`{text}`: the version after `@` is not a number"))?;
        if name.is_empty() {
            return Err(format!("`{text}` has no name before the `@`"));
        }
        Ok(PrimitiveRef {
            name: name.to_string(),
            version,
        })
    }
}

impl std::fmt::Display for PrimitiveRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub schema: u32,
    pub kind: Kind,
    pub id: String,
    /// Derive from another document of the same kind.
    pub extends: Option<String>,
    /// Replace the earlier document of the same `(kind, id)` instead of patching it.
    pub replace: bool,
    /// Minimum application version, when the document says so.
    pub requires_app: Option<String>,
    pub requires_primitives: Vec<PrimitiveRef>,
}

/// Keys of the envelope, removed from the table before the body is read.
const ENVELOPE_KEYS: [&str; 6] = ["schema", "kind", "id", "extends", "replace", "requires"];

impl Envelope {
    /// Reads the envelope from a parsed document and returns it with the body that is left.
    /// Problems are messages; the caller turns them into diagnostics with file and line.
    pub fn split(mut table: toml::Table) -> Result<(Envelope, toml::Table), Vec<EnvelopeProblem>> {
        let mut problems = Vec::new();
        let mut take = |key: &str| table.remove(key);

        let schema = match take("schema") {
            Some(toml::Value::Integer(n)) if n >= 1 => n as u32,
            Some(_) => {
                problems.push(EnvelopeProblem::at(
                    "schema",
                    "`schema` must be a whole number, 1 or more",
                ));
                0
            }
            None => {
                problems.push(
                    EnvelopeProblem::at("schema", "the document has no `schema` number")
                        .hint("start the file with `schema = 1`"),
                );
                0
            }
        };
        let kind = match take("kind") {
            Some(toml::Value::String(s)) => match Kind::parse(&s) {
                Some(kind) => Some(kind),
                None => {
                    let names: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
                    problems.push(
                        EnvelopeProblem::at("kind", format!("unknown kind `{s}`"))
                            .hint(format!("the kinds are {}", names.join(", "))),
                    );
                    None
                }
            },
            Some(_) => {
                problems.push(EnvelopeProblem::at("kind", "`kind` must be a string"));
                None
            }
            None => {
                problems.push(
                    EnvelopeProblem::at("kind", "the document has no `kind`")
                        .hint("for example `kind = \"engine\"`"),
                );
                None
            }
        };
        let id = match take("id") {
            Some(toml::Value::String(s)) if is_id(&s) => Some(s),
            Some(toml::Value::String(s)) => {
                problems.push(
                    EnvelopeProblem::at("id", format!("`{s}` is not a valid id"))
                        .hint("use letters, digits, `_`, `-` and `.`"),
                );
                None
            }
            Some(_) => {
                problems.push(EnvelopeProblem::at("id", "`id` must be a string"));
                None
            }
            None => {
                problems.push(EnvelopeProblem::at("id", "the document has no `id`"));
                None
            }
        };
        let extends = match take("extends") {
            None => None,
            Some(toml::Value::String(s)) if is_id(&s) => Some(s),
            Some(_) => {
                problems.push(EnvelopeProblem::at(
                    "extends",
                    "`extends` must be the id of another document",
                ));
                None
            }
        };
        let replace = match take("replace") {
            None => false,
            Some(toml::Value::Boolean(b)) => b,
            Some(_) => {
                problems.push(EnvelopeProblem::at(
                    "replace",
                    "`replace` must be true or false",
                ));
                false
            }
        };
        let (mut requires_app, mut requires_primitives) = (None, Vec::new());
        match take("requires") {
            None => {}
            Some(toml::Value::Table(requires)) => {
                match requires.get("app") {
                    None => {}
                    Some(toml::Value::String(s)) => requires_app = Some(s.clone()),
                    Some(_) => problems.push(EnvelopeProblem::at(
                        "requires.app",
                        "must be a string such as \">=0.2\"",
                    )),
                }
                match requires.get("primitives") {
                    None => {}
                    Some(toml::Value::Array(items)) => {
                        for item in items {
                            match item.as_str().map(PrimitiveRef::parse) {
                                Some(Ok(p)) => requires_primitives.push(p),
                                Some(Err(message)) => problems
                                    .push(EnvelopeProblem::at("requires.primitives", message)),
                                None => problems.push(EnvelopeProblem::at(
                                    "requires.primitives",
                                    "each entry must be a string such as \"spawn@1\"",
                                )),
                            }
                        }
                    }
                    Some(_) => problems.push(EnvelopeProblem::at(
                        "requires.primitives",
                        "must be a list of strings",
                    )),
                }
            }
            Some(_) => problems.push(EnvelopeProblem::at(
                "requires",
                "`requires` must be a table",
            )),
        }

        if schema > SCHEMA_VERSION {
            problems.push(
                EnvelopeProblem::at(
                    "schema",
                    format!("this document uses schema {schema}, newer than this application reads ({SCHEMA_VERSION})"),
                )
                .hint("update QuickAI, or remove the document"),
            );
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok((
            Envelope {
                schema,
                kind: kind.expect("checked above"),
                id: id.expect("checked above"),
                extends,
                replace,
                requires_app,
                requires_primitives,
            },
            table,
        ))
    }

    pub fn is_envelope_key(key: &str) -> bool {
        ENVELOPE_KEYS.contains(&key)
    }
}

fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeProblem {
    pub path: String,
    pub message: String,
    pub hint: Option<String>,
}

impl EnvelopeProblem {
    fn at(path: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_string(),
            message: message.into(),
            hint: None,
        }
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// A typed body together with the keys that were present but are not part of this build's schema.
#[derive(Debug)]
pub struct Typed<T> {
    pub value: T,
    /// Where the unknown keys are, as paths in the merged document.
    pub unknown: Vec<DocPath>,
}

/// What went wrong reading a body: where, and in the deserializer's words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyError {
    pub path: DocPath,
    pub message: String,
}

fn raw_segments(path: &serde_ignored::Path) -> Vec<RawSeg> {
    fn walk(path: &serde_ignored::Path, out: &mut Vec<RawSeg>) {
        use serde_ignored::Path;
        match path {
            Path::Root => {}
            Path::Seq { parent, index } => {
                walk(parent, out);
                out.push(RawSeg::Index(*index));
            }
            Path::Map { parent, key } => {
                walk(parent, out);
                out.push(RawSeg::Key(key.clone()));
            }
            Path::Some { parent }
            | Path::NewtypeStruct { parent }
            | Path::NewtypeVariant { parent } => walk(parent, out),
        }
    }
    let mut out = Vec::new();
    walk(path, &mut out);
    out
}

fn raw_segments_of_error(path: &serde_path_to_error::Path) -> Vec<RawSeg> {
    path.iter()
        .filter_map(|seg| match seg {
            serde_path_to_error::Segment::Seq { index } => Some(RawSeg::Index(*index)),
            serde_path_to_error::Segment::Map { key } => Some(RawSeg::Key(key.clone())),
            serde_path_to_error::Segment::Enum { .. } | serde_path_to_error::Segment::Unknown => {
                None
            }
        })
        .collect()
}

/// Reads the body table as `T`. Keys `T` does not know are collected, not rejected; a value of the
/// wrong shape is an error that names its place.
pub fn read_body<T: DeserializeOwned>(body: &toml::Table) -> Result<Typed<T>, BodyError> {
    let root = toml::Value::Table(body.clone());
    let mut unknown: Vec<Vec<RawSeg>> = Vec::new();
    let mut record = |path: serde_ignored::Path| unknown.push(raw_segments(&path));
    let result = {
        let tracked = serde_ignored::Deserializer::new(root.clone(), &mut record);
        serde_path_to_error::deserialize::<_, T>(tracked)
    };
    match result {
        Ok(value) => Ok(Typed {
            value,
            unknown: unknown
                .iter()
                .map(|raw| DocPath::from_raw(&root, raw))
                .collect(),
        }),
        Err(error) => Err(BodyError {
            path: DocPath::from_raw(&root, &raw_segments_of_error(error.path())),
            message: error.inner().message().to_string(),
        }),
    }
}

/// The ids of a table's array of tables, for messages and cross-references.
pub fn ids_of(table: &toml::Table, key: &str) -> BTreeMap<String, usize> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| item.get("id")?.as_str().map(|id| (id.to_string(), i)))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::engine::EngineDoc;

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn the_envelope_is_split_from_the_body() {
        let (env, body) = Envelope::split(table(
            r#"
            schema = 1
            kind = "engine"
            id = "llamacpp"
            extends = "base"
            replace = true
            requires = { app = ">=0.2", primitives = ["spawn@1", "catalog.hf_gguf@2"] }
            [engine]
            name = "llama.cpp"
            "#,
        ))
        .unwrap();
        assert_eq!(env.schema, 1);
        assert_eq!(env.kind, Kind::Engine);
        assert_eq!(env.id, "llamacpp");
        assert_eq!(env.extends.as_deref(), Some("base"));
        assert!(env.replace);
        assert_eq!(env.requires_app.as_deref(), Some(">=0.2"));
        assert_eq!(
            env.requires_primitives,
            vec![
                PrimitiveRef {
                    name: "spawn".into(),
                    version: 1
                },
                PrimitiveRef {
                    name: "catalog.hf_gguf".into(),
                    version: 2
                }
            ]
        );
        assert!(body.contains_key("engine"));
        assert!(!body.contains_key("schema") && !body.contains_key("requires"));
    }

    #[test]
    fn envelope_problems_are_all_reported_with_hints() {
        let problems = Envelope::split(table("[engine]\nname='x'\n")).unwrap_err();
        let paths: Vec<&str> = problems.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, vec!["schema", "kind", "id"]);
        assert!(problems[0].hint.as_deref().unwrap().contains("schema = 1"));

        let problems = Envelope::split(table("schema=1\nkind='engin'\nid='x'\n")).unwrap_err();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message, "unknown kind `engin`");
        assert!(problems[0]
            .hint
            .as_deref()
            .unwrap()
            .contains("engine, vocab"));

        let problems =
            Envelope::split(table("schema=1\nkind='engine'\nid='has space'\n")).unwrap_err();
        assert!(problems[0].message.contains("not a valid id"));

        let problems = Envelope::split(table("schema=7\nkind='engine'\nid='x'\n")).unwrap_err();
        assert!(problems[0].message.contains("newer than this application"));

        let problems = Envelope::split(table(
            "schema=1\nkind='engine'\nid='x'\nrequires={primitives=['spawn']}\n",
        ))
        .unwrap_err();
        assert!(
            problems[0].message.contains("name@version"),
            "{}",
            problems[0].message
        );
    }

    #[test]
    fn kinds_have_names_directories_and_tiers() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.name()), Some(kind));
        }
        assert_eq!(Kind::Engine.dir(), "engines");
        assert_eq!(Kind::Profile.dir(), "profiles");
        assert_eq!(Kind::Engine.tier(), Tier::Privileged);
        assert_eq!(Kind::Policy.tier(), Tier::Privileged);
        assert_eq!(Kind::Catalog.tier(), Tier::Data);
        assert_eq!(Kind::Vocab.tier(), Tier::Data);
        assert_eq!(Kind::parse("nope"), None);
    }

    #[test]
    fn a_body_is_read_and_unknown_keys_are_named() {
        let body = table(
            r#"
            [engine]
            name = "x"
            mystery = 1
            [launch.spawn]
            program = "p"
            bogus = true
            [[field]]
            id = "port"
            type = "port"
            surprise = "yes"
            [future]
            a = 1
            "#,
        );
        let typed = read_body::<EngineDoc>(&body).unwrap();
        assert_eq!(typed.value.engine.name, "x");
        let mut unknown: Vec<String> = typed.unknown.iter().map(DocPath::to_string).collect();
        unknown.sort();
        assert_eq!(
            unknown,
            vec![
                "engine.mystery",
                "field[port].surprise",
                "future",
                "launch.spawn.bogus"
            ]
        );
    }

    #[test]
    fn a_value_of_the_wrong_shape_names_its_place() {
        let body = table(
            r#"
            [engine]
            name = "x"
            [[field]]
            id = "port"
            type = "port"
            [[field]]
            id = "ctx_size"
            type = "quaternion"
            "#,
        );
        let e = read_body::<EngineDoc>(&body).unwrap_err();
        assert_eq!(e.path.to_string(), "field[ctx_size].type");
        assert!(e.message.contains("unknown variant"), "{}", e.message);

        let e = read_body::<EngineDoc>(&table("[provides]\napis = []\n")).unwrap_err();
        assert!(
            e.message.contains("missing field `engine`"),
            "{}",
            e.message
        );
    }

    #[test]
    fn primitive_references_parse_and_print() {
        let p = PrimitiveRef::parse("install.archive@1").unwrap();
        assert_eq!(p.to_string(), "install.archive@1");
        assert!(PrimitiveRef::parse("spawn").is_err());
        assert!(PrimitiveRef::parse("spawn@x").is_err());
        assert!(PrimitiveRef::parse("@1").is_err());
    }

    #[test]
    fn ids_of_lists_an_array_of_tables() {
        let t = table("[[field]]\nid='a'\n[[field]]\nid='b'\n[[field]]\nx=1\n");
        let ids = ids_of(&t, "field");
        assert_eq!(ids.get("a"), Some(&0));
        assert_eq!(ids.get("b"), Some(&1));
        assert_eq!(ids.len(), 2);
        assert!(ids_of(&t, "nothing").is_empty());
    }
}
