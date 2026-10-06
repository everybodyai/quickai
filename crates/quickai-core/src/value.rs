//! The values the expression language works on, and the context it reads them from.
//!
//! A `Value` is JSON's model plus a distinction between integers and floats: what a TOML document,
//! a `settings.json` entry or an engine's JSON reply can hold. The expression context is one
//! `Value::Map`, so a path such as `model.modalities.input` is a walk down maps.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::List(_) => "list",
            Value::Map(_) => "map",
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
            _ => None,
        }
    }

    /// Integers and floats alike, as a float.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// How a value acts as a condition (`when = "model"` means "a model is selected"): `null` and
    /// `false` are false, a list or map is true when it has entries, and so are a string that is
    /// not empty and a number that is not zero. The type checker only lets booleans, lists and maps
    /// stand where a condition is expected, so the last two rules are a safety net, not a feature.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
        }
    }

    /// Walks `path` down nested maps. `None` when a step is missing or not a map.
    pub fn lookup(&self, path: &[&str]) -> Option<&Value> {
        let mut here = self;
        for step in path {
            here = here.as_map()?.get(*step)?;
        }
        Some(here)
    }

    /// Sets `path` to `value`, creating the maps on the way.
    pub fn set_path(&mut self, path: &[&str], value: Value) {
        let Some((first, rest)) = path.split_first() else {
            *self = value;
            return;
        };
        if !matches!(self, Value::Map(_)) {
            *self = Value::Map(BTreeMap::new());
        }
        if let Value::Map(map) = self {
            map.entry((*first).to_string())
                .or_insert(Value::Null)
                .set_path(rest, value);
        }
    }

    pub fn from_toml(value: &toml::Value) -> Value {
        match value {
            toml::Value::String(s) => Value::Str(s.clone()),
            toml::Value::Integer(i) => Value::Int(*i),
            toml::Value::Float(f) => Value::Float(*f),
            toml::Value::Boolean(b) => Value::Bool(*b),
            toml::Value::Datetime(d) => Value::Str(d.to_string()),
            toml::Value::Array(items) => Value::List(items.iter().map(Value::from_toml).collect()),
            toml::Value::Table(table) => Value::Map(
                table
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::from_toml(v)))
                    .collect(),
            ),
        }
    }

    pub fn from_json(value: &serde_json::Value) -> Value {
        match value {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => Value::Int(i),
                None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            serde_json::Value::String(s) => Value::Str(s.clone()),
            serde_json::Value::Array(items) => {
                Value::List(items.iter().map(Value::from_json).collect())
            }
            serde_json::Value::Object(map) => Value::Map(
                map.iter()
                    .map(|(k, v)| (k.clone(), Value::from_json(v)))
                    .collect(),
            ),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Int(i) => serde_json::Value::from(*i),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Str(s) => serde_json::Value::String(s.clone()),
            Value::List(items) => {
                serde_json::Value::Array(items.iter().map(Value::to_json).collect())
            }
            Value::Map(map) => serde_json::Value::Object(
                map.iter().map(|(k, v)| (k.clone(), v.to_json())).collect(),
            ),
        }
    }
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

/// How a value reads inside a string (`"--port ${listen_port}"`) or as one command-line argument:
/// integers without a decimal point, floats the way Rust prints them (`4` for 4.0, `0.5`), booleans
/// `true` and `false`, `null` as nothing, a list as its items joined by commas, a map as JSON.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => Ok(()),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Str(s) => f.write_str(s),
            Value::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                Ok(())
            }
            Value::Map(_) => write!(f, "{}", self.to_json()),
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::Int(i64::try_from(v).unwrap_or(i64::MAX))
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<u16> for Value {
    fn from(v: u16) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::List(v.into_iter().map(Into::into).collect())
    }
}

/// Builds a `Value::Map` from pairs: `map([("a", 1.into())])`.
pub fn map<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_what_a_command_line_wants() {
        assert_eq!(Value::Int(8080).to_string(), "8080");
        assert_eq!(Value::Float(4.0).to_string(), "4");
        assert_eq!(Value::Float(0.5).to_string(), "0.5");
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::Null.to_string(), "");
        assert_eq!(Value::from(vec!["a", "b"]).to_string(), "a,b");
        assert_eq!(Value::Str("x y".into()).to_string(), "x y");
    }

    #[test]
    fn lookup_and_set_path_walk_maps() {
        let mut ctx = Value::Null;
        ctx.set_path(&["model", "size_bytes"], 10u64.into());
        ctx.set_path(&["model", "id"], "qwen".into());
        ctx.set_path(&["os"], "linux".into());
        assert_eq!(ctx.lookup(&["model", "size_bytes"]), Some(&Value::Int(10)));
        assert_eq!(ctx.lookup(&["os"]), Some(&Value::Str("linux".into())));
        assert_eq!(ctx.lookup(&["model", "missing"]), None);
        assert_eq!(ctx.lookup(&["os", "deeper"]), None);
        assert_eq!(ctx.lookup(&[]), Some(&ctx));
    }

    #[test]
    fn conversions_from_toml_and_json_keep_integers_and_floats_apart() {
        let toml: toml::Value =
            toml::from_str("a = 1\nb = 1.5\nc = [1, 'x']\n[d]\ne = true\n").unwrap();
        let v = Value::from_toml(&toml);
        assert_eq!(v.lookup(&["a"]), Some(&Value::Int(1)));
        assert_eq!(v.lookup(&["b"]), Some(&Value::Float(1.5)));
        assert_eq!(
            v.lookup(&["c"]),
            Some(&Value::List(vec![Value::Int(1), Value::Str("x".into())]))
        );
        assert_eq!(v.lookup(&["d", "e"]), Some(&Value::Bool(true)));

        let json: serde_json::Value =
            serde_json::from_str(r#"{"n": 3, "f": 2.5, "s": null}"#).unwrap();
        let v = Value::from_json(&json);
        assert_eq!(v.lookup(&["n"]), Some(&Value::Int(3)));
        assert_eq!(v.lookup(&["f"]), Some(&Value::Float(2.5)));
        assert_eq!(v.lookup(&["s"]), Some(&Value::Null));
        assert_eq!(Value::from_json(&v.to_json()), v);
    }

    #[test]
    fn numbers_convert_the_way_the_language_expects() {
        assert_eq!(Value::Float(3.0).as_i64(), Some(3));
        assert_eq!(Value::Float(3.5).as_i64(), None);
        assert_eq!(Value::Int(3).as_f64(), Some(3.0));
        assert_eq!(Value::Str("3".into()).as_f64(), None);
    }

    #[test]
    fn truthiness_is_presence() {
        assert!(Value::Bool(true).truthy());
        assert!(!Value::Bool(false).truthy());
        assert!(!Value::Null.truthy());
        assert!(!Value::List(vec![]).truthy());
        assert!(Value::List(vec![Value::Null]).truthy());
        assert!(!Value::Map(Default::default()).truthy());
        assert!(map([("id", "qwen".into())]).truthy());
        assert!(!Value::Str(String::new()).truthy());
        assert!(!Value::Int(0).truthy());
        assert!(Value::Int(2).truthy());
    }
}
