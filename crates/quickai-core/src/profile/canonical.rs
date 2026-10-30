//! One copy of each shared configuration (docs/reports-and-community.md, section 11.11): what a
//! configuration runs, written one way only, and the hash of it.
//!
//! Two files are the same configuration when they would run the same thing. The canonical form
//! leaves out what does not change what runs (the formatting, the id and the name, `[meta]`,
//! `requires`, the version the program said it was, a model's size and the name people call it, the
//! settings whose field says `identity = false`, a value equal to its field's literal default) and
//! writes the rest one way: values by their field's type, the models after the first sorted, the
//! lines of environment sorted by name, and the extra arguments grouped by flag and sorted. Where it
//! cannot be sure that two spellings run the same, it keeps the difference: missing a duplicate is
//! acceptable, merging two different configurations is not.
//!
//! The app and the server compute it with this code (the server runs it as WebAssembly), so the two
//! cannot disagree.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value as Json};
use sha2::{Digest, Sha256};

use super::state::field_of;
use super::{family, Configuration, ModelPick, ModelSource, NeedKind};
use crate::doc::engine::{Field, FieldDefault};
use crate::plan::args::split_args;
use crate::registry::Registry;
use crate::value::Value;

/// The number of the rules. When they change (a new alias, a new `identity = false`), it goes up and
/// every stored identity is computed again.
pub const CANON: u32 = 1;

/// What the hashes are taken over begins with these lines, so that no other hash of ours can be one
/// of them.
const IDENTITY_PREFIX: &str = "quickai-config-v1\n";
const FAMILY_PREFIX: &str = "quickai-config-family-v1\n";

/// Who a configuration is, whatever the spelling of its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The SHA-256 of the canonical form, in lower-case hex: two files with the same one run the same
    /// thing.
    pub identity: String,
    /// The SHA-256 of the engine and the models alone, in lower-case hex: configurations that run the
    /// same models with other settings are variants of each other.
    pub family: String,
    /// The canonical form: JSON with sorted keys and no spaces.
    pub canonical: String,
}

/// The identity of `configuration`, held to the engines' documents of `registry`.
pub fn identity(registry: &Registry, configuration: &Configuration) -> Identity {
    let models = models(&configuration.models);
    let mut form = Map::new();
    form.insert("canon".into(), json!(CANON));
    form.insert("engine".into(), json!(configuration.engine));
    if let Some(build) = &configuration.runtime.build {
        form.insert("build".into(), json!(build));
    }
    let companions: Map<String, Json> = configuration
        .runtime
        .companions
        .iter()
        .filter_map(|(id, runtime)| runtime.build.as_ref().map(|b| (id.clone(), json!(b))))
        .collect();
    if !companions.is_empty() {
        form.insert("companions".into(), Json::Object(companions));
    }
    if !models.is_empty() {
        form.insert("models".into(), Json::Array(models.clone()));
    }
    let values = values(registry, configuration);
    if !values.is_empty() {
        form.insert("values".into(), Json::Object(values));
    }
    let needs = needs(configuration);
    if !needs.is_empty() {
        form.insert("needs".into(), Json::Array(needs));
    }
    if let Some(proxy) = configuration.traffic_proxy {
        form.insert("traffic_proxy".into(), json!(proxy));
    }
    let canonical = Json::Object(form).to_string();

    let family_form = json!({
        "canon": CANON,
        "engine": configuration.engine,
        "models": models.iter().map(|m| m["id"].clone()).collect::<Vec<_>>(),
    });
    Identity {
        identity: hash(IDENTITY_PREFIX, &canonical),
        family: hash(FAMILY_PREFIX, &family_form.to_string()),
        canonical,
    }
}

fn hash(prefix: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---- the models ------------------------------------------------------------------------------------

/// Who a model is: its file's SHA-256 when the file says it, else where it is had from.
pub fn model_identity(pick: &ModelPick) -> String {
    if let Some(sum) = &pick.sha256 {
        return format!("sha256:{sum}");
    }
    match &pick.source {
        ModelSource::Catalog {
            id,
            file: Some(file),
        } => format!("catalog:{id}/{file}"),
        ModelSource::Catalog { id, file: None } => format!("catalog:{id}"),
        // Ollama's names do not care about case, and a name without a tag is its `latest`
        ModelSource::Named { kind, name } if kind == "ollama" => {
            let name = name.to_ascii_lowercase();
            if name.contains(':') {
                format!("ollama:{name}")
            } else {
                format!("ollama:{name}:latest")
            }
        }
        ModelSource::Named { kind, name } => format!("{kind}:{name}"),
        ModelSource::File { name } => match (&pick.from, &pick.url) {
            (Some(from), _) => {
                let path = pick.path.as_deref().unwrap_or(name);
                match pick.revision.as_deref().filter(|r| *r != "main") {
                    Some(revision) => format!("{from}/{path}@{revision}"),
                    None => format!("{from}/{path}"),
                }
            }
            (None, Some(url)) => format!("url:{url}"),
            (None, None) => format!("file:{name}"),
        },
    }
}

/// The first model stays first, since the engine loads it; the others are sorted by role, then by
/// who they are.
fn models(picks: &[ModelPick]) -> Vec<Json> {
    let mut out: Vec<Json> = picks
        .iter()
        .map(|pick| {
            let mut model = Map::new();
            model.insert("role".into(), json!(pick.role));
            model.insert("id".into(), json!(model_identity(pick)));
            if let Some(into) = &pick.into {
                model.insert("into".into(), json!(into));
            }
            Json::Object(model)
        })
        .collect();
    if out.len() > 2 {
        out[1..].sort_by(|a, b| {
            (a["role"].as_str(), a["id"].as_str()).cmp(&(b["role"].as_str(), b["id"].as_str()))
        });
    }
    out
}

// ---- the settings ----------------------------------------------------------------------------------

fn values(registry: &Registry, configuration: &Configuration) -> Map<String, Json> {
    let members = family(registry.engine_map(), &configuration.engine);
    let mut out = Map::new();
    for (key, value) in &configuration.values {
        // a configuration that was read holds only settings its engines have
        let Some((engine, field)) = field_of(registry, key) else {
            continue;
        };
        if !members.iter().any(|m| m.id == engine.id) || field.identity == Some(false) {
            continue;
        }
        if is_default(field, value) {
            continue;
        }
        let written = match value {
            Value::Str(text) if field.bind.arg_split => {
                let words = split_args(text);
                if words.is_empty() {
                    continue;
                }
                arguments(&words, &engine.doc.flags.aliases)
            }
            Value::Str(text) if field.bind.env_lines.is_some() => match environment(text) {
                Some(lines) => lines,
                None => continue,
            },
            other => typed(other),
        };
        out.insert(key.clone(), written);
    }
    out
}

/// Whether the value is the field's literal default. A default the program works out (the context
/// size of llama.cpp follows the model) is never one: the file that spells it out fixes it.
fn is_default(field: &Field, value: &Value) -> bool {
    match &field.default {
        Some(FieldDefault::Literal(literal)) => {
            super::fit(field, &Value::from_toml(literal)).is_ok_and(|d| d == *value)
        }
        _ => false,
    }
}

fn typed(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::Float(f) => json!(f),
        Value::Str(s) => json!(s),
        Value::List(items) => Json::Array(items.iter().map(typed).collect()),
        Value::Map(map) => Json::Object(map.iter().map(|(k, v)| (k.clone(), typed(v))).collect()),
    }
}

/// Lines of environment as the program receives them: `NAME=value`, blank and comment lines dropped,
/// a name given twice with its last value, sorted by name. A line that is no `NAME=value` is kept as
/// it was written, in its order. Nothing at all is `None`.
fn environment(text: &str) -> Option<Json> {
    let lines = crate::kvlines::parse(text, &[], false);
    let mut entries = lines.entries.clone();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let odd: Vec<&str> = text
        .lines()
        .enumerate()
        .filter(|(index, _)| lines.problems.iter().any(|p| p.line == index + 1))
        .map(|(_, line)| line.trim())
        .collect();
    if entries.is_empty() && odd.is_empty() {
        return None;
    }
    let mut out = Map::new();
    out.insert(
        "lines".into(),
        Json::Array(entries.iter().map(|(n, v)| json!([n, v])).collect()),
    );
    if !odd.is_empty() {
        out.insert("odd".into(), json!(odd));
    }
    Some(Json::Object(out))
}

/// A word that is a flag: it begins with `-`, and is not a negative number.
fn is_flag(word: &str) -> bool {
    let Some(rest) = word.strip_prefix('-') else {
        return false;
    };
    !rest.is_empty() && !rest.starts_with(|c: char| c.is_ascii_digit() || c == '.')
}

/// The words of an extra-arguments field, one way only. `--name=value` is `--name value`, and a
/// spelling the engine's document lists as an alias is the flag it stands for. Then each flag takes
/// the words after it up to the next flag, and the groups are sorted by flag; repeats of one flag
/// keep their order among themselves (`--lora a --lora b` may not mean `--lora b --lora a`), and a
/// flag given twice is not made one. When a word comes before the first flag, nothing is reordered.
pub fn arguments(words: &[String], aliases: &BTreeMap<String, String>) -> Json {
    let spelled = |flag: &str| {
        aliases
            .get(flag)
            .cloned()
            .unwrap_or_else(|| flag.to_string())
    };
    let mut normal: Vec<String> = Vec::with_capacity(words.len());
    for word in words {
        if !is_flag(word) {
            normal.push(word.clone());
            continue;
        }
        match word.split_once('=') {
            Some((name, value)) if name.starts_with("--") => {
                normal.push(spelled(name));
                normal.push(value.to_string());
            }
            _ => normal.push(spelled(word)),
        }
    }
    if !normal.first().is_some_and(|w| is_flag(w)) {
        return json!({ "exact": normal });
    }
    let mut groups: Vec<Vec<String>> = Vec::new();
    for word in normal {
        if is_flag(&word) {
            groups.push(vec![word]);
        } else if let Some(group) = groups.last_mut() {
            group.push(word);
        }
    }
    // a stable sort: repeats of one flag stay in the order they were given
    groups.sort_by(|a, b| a[0].cmp(&b[0]));
    json!({ "groups": groups })
}

fn needs(configuration: &Configuration) -> Vec<Json> {
    let mut out: Vec<Vec<String>> = configuration
        .needs
        .iter()
        .map(|need| match &need.kind {
            NeedKind::Token(token) => vec!["token".to_string(), token.word().to_string()],
            NeedKind::Setting { key } => vec!["setting".to_string(), key.clone()],
            NeedKind::Env { program, name } => {
                vec!["env".to_string(), program.clone(), name.clone()]
            }
            NeedKind::System { id } => vec!["system".to_string(), id.clone()],
        })
        .collect();
    out.sort();
    out.dedup();
    out.into_iter().map(|n| json!(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        split_args(text)
    }

    fn aliases() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("-c".to_string(), "--ctx-size".to_string()),
            ("-fa".to_string(), "--flash-attn".to_string()),
        ])
    }

    #[test]
    fn regrouped_reordered_and_respelled_arguments_are_one() {
        let a = arguments(&words("-c 8192 --temp=0.7 --flash-attn on"), &aliases());
        let b = arguments(&words("-fa on --temp 0.7 --ctx-size 8192"), &aliases());
        assert_eq!(a, b);
    }

    #[test]
    fn a_word_before_the_first_flag_keeps_the_order() {
        let a = arguments(&words("model.gguf --temp 0.7"), &aliases());
        let b = arguments(&words("--temp 0.7 model.gguf"), &aliases());
        assert_ne!(a, b);
    }

    #[test]
    fn repeats_of_one_flag_keep_their_order() {
        let a = arguments(&words("--lora a --lora b"), &aliases());
        let b = arguments(&words("--lora b --lora a"), &aliases());
        assert_ne!(a, b);
        let c = arguments(&words("--temp 1 --lora a --lora b"), &aliases());
        let d = arguments(&words("--lora a --temp 1 --lora b"), &aliases());
        assert_eq!(c, d);
    }

    #[test]
    fn a_negative_number_is_a_word() {
        let a = arguments(&words("--seed -1 --temp 0.5"), &aliases());
        assert_eq!(
            a,
            json!({ "groups": [["--seed", "-1"], ["--temp", "0.5"]] })
        );
    }

    #[test]
    fn quoting_is_gone() {
        let a = arguments(&words("--alias \"a b\""), &aliases());
        let b = arguments(&words("--alias 'a b'"), &aliases());
        assert_eq!(a, b);
    }

    #[test]
    fn environment_lines_are_sorted_and_the_last_value_wins() {
        let a = environment("B=2\n# a comment\n\nA=1\nB=3\n");
        let b = environment("A=1\nB=3");
        assert_eq!(a, b);
        assert_eq!(environment("# only a comment\n"), None);
        assert_ne!(environment("A=1\nnot a line"), environment("A=1"));
    }

    #[test]
    fn ollama_names_ignore_case_and_add_latest() {
        let pick = |name: &str| ModelPick {
            role: "main".into(),
            source: ModelSource::Named {
                kind: "ollama".into(),
                name: name.into(),
            },
            name: None,
            size: None,
            sha256: None,
            from: None,
            path: None,
            revision: None,
            url: None,
            into: None,
        };
        assert_eq!(
            model_identity(&pick("Qwen3")),
            model_identity(&pick("qwen3:latest"))
        );
        assert_ne!(
            model_identity(&pick("qwen3:8b")),
            model_identity(&pick("qwen3:4b"))
        );
    }
}
