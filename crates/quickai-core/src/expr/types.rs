//! Types for the expression checker. An expression is type-checked when a document is loaded,
//! against a schema that says what each name in the context is, so a typo or a comparison of a
//! string with a number is a diagnostic with a file and line, not a surprise at run time.

use std::collections::BTreeMap;

use crate::value::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// Not known statically (a subtree the schema leaves open); compatible with everything.
    Any,
    Null,
    Bool,
    Int,
    Float,
    Str,
    List(Box<Type>),
    /// An object with known keys. Names inside it are looked up key by key.
    Map(BTreeMap<String, Type>),
}

/// Why a path could not be resolved, with a suggestion when a nearby name exists.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolveError {
    pub message: String,
    pub hint: Option<String>,
}

impl Type {
    pub fn list_of(item: Type) -> Type {
        Type::List(Box::new(item))
    }

    /// `Type::object([("port", Type::Int)])`.
    pub fn object<const N: usize>(fields: [(&str, Type); N]) -> Type {
        Type::Map(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn name(&self) -> String {
        match self {
            Type::Any => "any value".into(),
            Type::Null => "null".into(),
            Type::Bool => "a bool".into(),
            Type::Int => "an int".into(),
            Type::Float => "a float".into(),
            Type::Str => "a string".into(),
            Type::List(item) if **item == Type::Any => "a list".into(),
            Type::List(item) => format!(
                "a list of {}",
                item.name()
                    .trim_start_matches("a ")
                    .trim_start_matches("an ")
            ),
            Type::Map(_) => "a map".into(),
        }
    }

    pub fn is_any(&self) -> bool {
        matches!(self, Type::Any)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float | Type::Any)
    }

    /// May stand where a condition is expected: booleans, and lists and maps (their presence).
    pub fn is_condition(&self) -> bool {
        matches!(
            self,
            Type::Bool | Type::List(_) | Type::Map(_) | Type::Null | Type::Any
        )
    }

    /// Whether values of the two types could ever be equal, so `==` between them is not a mistake.
    pub fn comparable(&self, other: &Type) -> bool {
        if self.is_any() || other.is_any() {
            return true;
        }
        match (self, other) {
            (Type::Null, _) | (_, Type::Null) => true,
            (a, b) if a.is_numeric() && b.is_numeric() => true,
            (Type::Bool, Type::Bool) | (Type::Str, Type::Str) => true,
            (Type::List(a), Type::List(b)) => a.comparable(b),
            (Type::Map(_), Type::Map(_)) => true,
            _ => false,
        }
    }

    /// Whether a value of this type is acceptable where `expected` is wanted. Integers are
    /// acceptable as floats, `null` as anything (a value that is not there yet), `Any` both ways.
    pub fn assignable_to(&self, expected: &Type) -> bool {
        if self.is_any() || expected.is_any() {
            return true;
        }
        match (self, expected) {
            (Type::Null, _) => true,
            (Type::Int, Type::Float) => true,
            (Type::List(a), Type::List(b)) => a.assignable_to(b),
            (Type::Map(_), Type::Map(_)) => true,
            (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }

    /// The type a sample value has. Lists take the type of their first item.
    pub fn of(value: &Value) -> Type {
        match value {
            Value::Null => Type::Null,
            Value::Bool(_) => Type::Bool,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Str(_) => Type::Str,
            Value::List(items) => Type::list_of(items.first().map(Type::of).unwrap_or(Type::Any)),
            Value::Map(map) => {
                Type::Map(map.iter().map(|(k, v)| (k.clone(), Type::of(v))).collect())
            }
        }
    }

    /// The type of the name `path` in this schema.
    pub fn resolve(&self, path: &[String]) -> Result<Type, ResolveError> {
        let mut here = self;
        for (depth, step) in path.iter().enumerate() {
            match here {
                Type::Any => return Ok(Type::Any),
                Type::Map(fields) => match fields.get(step) {
                    Some(next) => here = next,
                    None => {
                        let parent = path[..depth].join(".");
                        let message = if parent.is_empty() {
                            format!("unknown name `{step}`")
                        } else {
                            format!("`{parent}` has no `{step}`")
                        };
                        let hint = suggest(step, fields.keys().map(String::as_str)).map(|s| {
                            if parent.is_empty() {
                                format!("did you mean `{s}`?")
                            } else {
                                format!("did you mean `{parent}.{s}`?")
                            }
                        });
                        return Err(ResolveError { message, hint });
                    }
                },
                other => {
                    let parent = path[..depth].join(".");
                    return Err(ResolveError {
                        message: format!("`{parent}` is {}, so it has no `{step}`", other.name()),
                        hint: None,
                    });
                }
            }
        }
        Ok(here.clone())
    }
}

/// The candidate closest to `word` when it is close enough to be a typo (edit distance up to 2, and
/// less than the word's length).
pub fn suggest<'a>(word: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        let d = edit_distance(word, candidate);
        if d <= 2 && d < word.chars().count().max(1) && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, c)| c.to_string())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(ca != cb);
            let insert = current[j] + 1;
            let delete = previous[j + 1] + 1;
            current.push(substitute.min(insert).min(delete));
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Type {
        Type::object([
            ("os", Type::Str),
            (
                "runtime",
                Type::object([("build", Type::Int), ("flags", Type::list_of(Type::Str))]),
            ),
            ("model", Type::Any),
        ])
    }

    fn path(p: &str) -> Vec<String> {
        p.split('.').map(str::to_string).collect()
    }

    #[test]
    fn paths_resolve_through_maps_and_stop_at_any() {
        let s = schema();
        assert_eq!(s.resolve(&path("os")), Ok(Type::Str));
        assert_eq!(s.resolve(&path("runtime.build")), Ok(Type::Int));
        assert_eq!(s.resolve(&path("model.anything.at.all")), Ok(Type::Any));
    }

    #[test]
    fn an_unknown_name_gets_a_suggestion() {
        let s = schema();
        let e = s.resolve(&path("runtim.build")).unwrap_err();
        assert_eq!(e.message, "unknown name `runtim`");
        assert_eq!(e.hint.as_deref(), Some("did you mean `runtime`?"));
        let e = s.resolve(&path("runtime.bild")).unwrap_err();
        assert_eq!(e.message, "`runtime` has no `bild`");
        assert_eq!(e.hint.as_deref(), Some("did you mean `runtime.build`?"));
        let e = s.resolve(&path("zzzzzz")).unwrap_err();
        assert_eq!(e.hint, None);
        let e = s.resolve(&path("os.name")).unwrap_err();
        assert!(e.message.contains("is a string"), "{}", e.message);
    }

    #[test]
    fn comparable_and_assignable() {
        assert!(Type::Int.comparable(&Type::Float));
        assert!(Type::Str.comparable(&Type::Null));
        assert!(Type::Any.comparable(&Type::Bool));
        assert!(!Type::Str.comparable(&Type::Int));
        assert!(!Type::Bool.comparable(&Type::Str));
        assert!(Type::Int.assignable_to(&Type::Float));
        assert!(!Type::Float.assignable_to(&Type::Int));
        assert!(Type::Null.assignable_to(&Type::Str));
        assert!(Type::list_of(Type::Str).assignable_to(&Type::list_of(Type::Any)));
        assert!(!Type::list_of(Type::Str).assignable_to(&Type::list_of(Type::Int)));
        assert!(Type::Bool.is_condition() && Type::Map(Default::default()).is_condition());
        assert!(!Type::Str.is_condition() && !Type::Int.is_condition());
    }

    #[test]
    fn type_of_a_sample_value() {
        let v = crate::value::map([
            ("a", 1i64.into()),
            ("b", vec!["x"].into()),
            ("c", Value::List(vec![])),
        ]);
        assert_eq!(
            Type::of(&v),
            Type::object([
                ("a", Type::Int),
                ("b", Type::list_of(Type::Str)),
                ("c", Type::list_of(Type::Any))
            ])
        );
    }

    #[test]
    fn suggestions_need_to_be_close() {
        let names = ["port", "ctx_size", "n_gpu_layers"];
        assert_eq!(suggest("prot", names.iter().copied()), Some("port".into()));
        assert_eq!(
            suggest("ctx_siz", names.iter().copied()),
            Some("ctx_size".into())
        );
        assert_eq!(suggest("banana", names.iter().copied()), None);
        assert_eq!(suggest("p", names.iter().copied()), None);
    }

    #[test]
    fn names_read_naturally_in_messages() {
        assert_eq!(Type::Bool.name(), "a bool");
        assert_eq!(Type::list_of(Type::Str).name(), "a list of string");
        assert_eq!(Type::list_of(Type::Any).name(), "a list");
    }
}
