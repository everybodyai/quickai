//! The functions an expression may call. All are pure: the same arguments (and context) always give
//! the same answer, nothing is read from the machine. `is_file` is the one question about the
//! machine a document can ask, and the caller answers it: for the text of the model box, before the
//! rules are read, as `input_is_file` in the context (docs/phase3-test-plan.md, M01b).
//!
//! Arguments are coerced so that a value that is "not there yet" does not make an expression fail:
//! `null` reads as `false`, `0`, `""` or `[]` according to the parameter. (`model.size_bytes` is
//! `null` while no model is selected.)

use std::cmp::Ordering;

use super::types::Type;
use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Any,
    Bool,
    Num,
    Str,
    List,
}

impl Param {
    pub fn describe(self) -> &'static str {
        match self {
            Param::Any => "a value",
            Param::Bool => "a bool",
            Param::Num => "a number",
            Param::Str => "a string",
            Param::List => "a list",
        }
    }

    pub fn accepts(self, ty: &Type) -> bool {
        match self {
            Param::Any => true,
            Param::Bool => ty.is_condition(),
            Param::Num => ty.is_numeric() || matches!(ty, Type::Null),
            Param::Str => matches!(ty, Type::Str | Type::Any | Type::Null),
            Param::List => matches!(ty, Type::List(_) | Type::Any | Type::Null),
        }
    }
}

/// What a function returns, for the type checker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    Bool,
    Int,
    Str,
    /// An int when every number argument is an int, otherwise a float.
    Num,
}

impl Ret {
    pub fn to_type(self, args: &[Type]) -> Type {
        match self {
            Ret::Bool => Type::Bool,
            Ret::Int => Type::Int,
            Ret::Str => Type::Str,
            Ret::Num => {
                if args.iter().all(|t| matches!(t, Type::Int)) {
                    Type::Int
                } else if args.iter().any(|t| t.is_any()) {
                    Type::Any
                } else {
                    Type::Float
                }
            }
        }
    }
}

pub struct Func {
    pub name: &'static str,
    pub params: &'static [Param],
    pub ret: Ret,
    pub call: fn(&Value, &[Value]) -> Result<Value, String>,
}

pub static FUNCS: &[Func] = &[
    Func {
        name: "has",
        params: &[Param::List, Param::Any],
        ret: Ret::Bool,
        call: has,
    },
    Func {
        name: "len",
        params: &[Param::Any],
        ret: Ret::Int,
        call: len,
    },
    Func {
        name: "lower",
        params: &[Param::Str],
        ret: Ret::Str,
        call: lower,
    },
    Func {
        name: "matches",
        params: &[Param::Str, Param::Str],
        ret: Ret::Bool,
        call: matches,
    },
    Func {
        name: "starts_with",
        params: &[Param::Str, Param::Str],
        ret: Ret::Bool,
        call: starts_with,
    },
    Func {
        name: "is_url",
        params: &[Param::Str],
        ret: Ret::Bool,
        call: is_url,
    },
    Func {
        name: "is_file",
        params: &[Param::Str],
        ret: Ret::Bool,
        call: is_file,
    },
    Func {
        name: "path_after_host",
        params: &[Param::Str],
        ret: Ret::Str,
        call: path_after_host,
    },
    Func {
        name: "semver_ge",
        params: &[Param::Str, Param::Str],
        ret: Ret::Bool,
        call: semver_ge,
    },
    Func {
        name: "min",
        params: &[Param::Num, Param::Num],
        ret: Ret::Num,
        call: min,
    },
    Func {
        name: "max",
        params: &[Param::Num, Param::Num],
        ret: Ret::Num,
        call: max,
    },
    Func {
        name: "clamp",
        params: &[Param::Num, Param::Num, Param::Num],
        ret: Ret::Num,
        call: clamp,
    },
    Func {
        name: "recommend_ctx_size",
        params: &[Param::Num, Param::Num],
        ret: Ret::Int,
        call: recommend_ctx_size,
    },
    Func {
        name: "runtime.has_flag",
        params: &[Param::Str],
        ret: Ret::Bool,
        call: runtime_has_flag,
    },
    Func {
        name: "gpu.has",
        params: &[Param::Str],
        ret: Ret::Bool,
        call: gpu_has,
    },
];

pub fn find(name: &str) -> Option<&'static Func> {
    FUNCS.iter().find(|f| f.name == name)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    FUNCS.iter().map(|f| f.name)
}

// ---- coercion -------------------------------------------------------------------------------------

fn text(v: &Value) -> Result<&str, String> {
    match v {
        Value::Str(s) => Ok(s),
        Value::Null => Ok(""),
        other => Err(format!("expected a string, found {}", other.type_name())),
    }
}

fn list(v: &Value) -> Result<&[Value], String> {
    match v {
        Value::List(l) => Ok(l),
        Value::Null => Ok(&[]),
        other => Err(format!("expected a list, found {}", other.type_name())),
    }
}

fn number(v: &Value) -> Result<Value, String> {
    match v {
        Value::Int(_) | Value::Float(_) => Ok(v.clone()),
        Value::Null => Ok(Value::Int(0)),
        other => Err(format!("expected a number, found {}", other.type_name())),
    }
}

// ---- equality and ordering (shared with the evaluator) ------------------------------------------------

/// Equality as the language defines it: integers and floats compare by value, other types only
/// equal themselves, and values of different types are simply not equal.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            a.as_f64() == b.as_f64()
        }
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| values_equal(v, w)))
        }
        _ => a == b,
    }
}

/// Order of two numbers or two strings; `None` for anything else.
pub fn order(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            a.as_f64()?.partial_cmp(&b.as_f64()?)
        }
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

// ---- the functions ---------------------------------------------------------------------------------------

fn has(_: &Value, args: &[Value]) -> Result<Value, String> {
    let items = list(&args[0])?;
    Ok(Value::Bool(items.iter().any(|i| values_equal(i, &args[1]))))
}

fn len(_: &Value, args: &[Value]) -> Result<Value, String> {
    let n = match &args[0] {
        Value::Null => 0,
        Value::Str(s) => s.chars().count(),
        Value::List(l) => l.len(),
        Value::Map(m) => m.len(),
        other => {
            return Err(format!(
                "`len` needs a string, list or map, found {}",
                other.type_name()
            ))
        }
    };
    Ok(Value::Int(n as i64))
}

fn lower(_: &Value, args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(text(&args[0])?.to_lowercase()))
}

/// Compiles `pattern` with a size limit, so a hostile pattern cannot use unbounded memory.
pub fn compile_regex(pattern: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("invalid regular expression `{pattern}`: {e}"))
}

fn matches(_: &Value, args: &[Value]) -> Result<Value, String> {
    let re = compile_regex(text(&args[1])?)?;
    Ok(Value::Bool(re.is_match(text(&args[0])?)))
}

fn starts_with(_: &Value, args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(text(&args[0])?.starts_with(text(&args[1])?)))
}

/// Whether the text is an `http://` or `https://` address (the rule the model box uses).
pub fn looks_like_url(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn is_url(_: &Value, args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(looks_like_url(text(&args[0])?)))
}

/// Whether the text names a file on this machine. The caller works that out for the one text a
/// rule is about (the model box's input) and says so in the context as `input_is_file`; of any
/// other text nothing is known and the answer is no.
fn is_file(ctx: &Value, args: &[Value]) -> Result<Value, String> {
    let asked = text(&args[0])?;
    let about = ctx.lookup(&["input"]).and_then(Value::as_str);
    let is_file = ctx
        .lookup(&["input_is_file"])
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Value::Bool(is_file && about.is_some_and(|a| a == asked)))
}

/// The part of an address after its host, without the slashes around it: `https://huggingface.co/a/b/`
/// is `a/b`. A text that is not an address, or has nothing after the host, is `""`.
pub fn path_after_host(_: &Value, args: &[Value]) -> Result<Value, String> {
    let address = text(&args[0])?.trim();
    let rest = address
        .split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once('/'))
        .map(|(_, path)| path)
        .unwrap_or("");
    let path = rest.split(['?', '#']).next().unwrap_or("");
    Ok(Value::Str(path.trim_matches('/').to_string()))
}

/// Reads `0.35.1`, `v0.35` or `12` as a version, padding what is missing with zeros.
pub fn parse_version(text: &str) -> Result<semver::Version, String> {
    let trimmed = text.trim().trim_start_matches(['v', 'V']);
    let core_end = trimmed.find(['-', '+']).unwrap_or(trimmed.len());
    let (core, rest) = trimmed.split_at(core_end);
    let mut parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(format!("`{text}` is not a version"));
    }
    while parts.len() < 3 {
        parts.push("0");
    }
    semver::Version::parse(&format!("{}{rest}", parts.join(".")))
        .map_err(|_| format!("`{text}` is not a version"))
}

fn semver_ge(_: &Value, args: &[Value]) -> Result<Value, String> {
    let a = parse_version(text(&args[0])?)?;
    let b = parse_version(text(&args[1])?)?;
    Ok(Value::Bool(a >= b))
}

fn min(_: &Value, args: &[Value]) -> Result<Value, String> {
    let (a, b) = (number(&args[0])?, number(&args[1])?);
    let pick_a = order(&a, &b) != Some(Ordering::Greater);
    Ok(if pick_a { a } else { b })
}

fn max(_: &Value, args: &[Value]) -> Result<Value, String> {
    let (a, b) = (number(&args[0])?, number(&args[1])?);
    let pick_a = order(&a, &b) != Some(Ordering::Less);
    Ok(if pick_a { a } else { b })
}

fn clamp(_: &Value, args: &[Value]) -> Result<Value, String> {
    let (x, lo, hi) = (number(&args[0])?, number(&args[1])?, number(&args[2])?);
    if order(&lo, &hi) == Some(Ordering::Greater) {
        return Err("`clamp` needs the lower bound not above the upper bound".into());
    }
    Ok(if order(&x, &lo) == Some(Ordering::Less) {
        lo
    } else if order(&x, &hi) == Some(Ordering::Greater) {
        hi
    } else {
        x
    })
}

fn recommend_ctx_size(_: &Value, args: &[Value]) -> Result<Value, String> {
    let size = number(&args[0])?.as_f64().unwrap_or(0.0).max(0.0) as u64;
    let ram = number(&args[1])?.as_f64().unwrap_or(0.0).max(0.0) as u64;
    Ok(Value::Int(i64::from(
        crate::heuristics::recommend_ctx_size(size, ram),
    )))
}

fn runtime_has_flag(ctx: &Value, args: &[Value]) -> Result<Value, String> {
    let flag = text(&args[0])?;
    let flags = ctx
        .lookup(&["runtime", "flags"])
        .and_then(Value::as_list)
        .unwrap_or(&[]);
    Ok(Value::Bool(
        flags.iter().any(|f| f.as_str().is_some_and(|s| s == flag)),
    ))
}

fn gpu_has(ctx: &Value, args: &[Value]) -> Result<Value, String> {
    let vendor = text(&args[0])?.to_ascii_lowercase();
    let vendors = ctx
        .lookup(&["gpu", "vendors"])
        .and_then(Value::as_list)
        .unwrap_or(&[]);
    Ok(Value::Bool(vendors.iter().any(|v| {
        v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&vendor))
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::map;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        (find(name).unwrap().call)(&Value::Null, args)
    }

    #[test]
    fn has_len_lower_starts_with() {
        let l: Value = vec!["image", "video"].into();
        assert_eq!(
            call("has", &[l.clone(), "video".into()]),
            Ok(Value::Bool(true))
        );
        assert_eq!(call("has", &[l, "audio".into()]), Ok(Value::Bool(false)));
        assert_eq!(
            call("has", &[Value::Null, "x".into()]),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            call("has", &[vec![1i64, 2].into(), 2.0.into()]),
            Ok(Value::Bool(true))
        );
        assert_eq!(call("len", &["héllo".into()]), Ok(Value::Int(5)));
        assert_eq!(call("len", &[vec!["a"].into()]), Ok(Value::Int(1)));
        assert_eq!(call("len", &[Value::Null]), Ok(Value::Int(0)));
        assert!(call("len", &[Value::Int(3)]).is_err());
        assert_eq!(call("lower", &["AbC".into()]), Ok("abc".into()));
        assert_eq!(
            call("starts_with", &["--mmproj".into(), "--".into()]),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn matches_uses_regular_expressions_and_reports_bad_ones() {
        assert_eq!(
            call("matches", &["hf.co/user/repo".into(), r"^hf\.co/".into()]),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            call(
                "matches",
                &[
                    "qwen3:4b".into(),
                    "^[a-z0-9._/-]+(:[A-Za-z0-9._-]+)?$".into()
                ]
            ),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            call("matches", &["x".into(), "^y".into()]),
            Ok(Value::Bool(false))
        );
        let e = call("matches", &["x".into(), "(".into()]).unwrap_err();
        assert!(e.contains("invalid regular expression"), "{e}");
    }

    #[test]
    fn path_after_host_is_what_follows_the_host_without_the_slashes_around_it() {
        for (address, want) in [
            (
                "https://huggingface.co/Qwen/Qwen3-4B-GGUF",
                "Qwen/Qwen3-4B-GGUF",
            ),
            (
                "https://huggingface.co/Qwen/Qwen3-4B-GGUF/",
                "Qwen/Qwen3-4B-GGUF",
            ),
            ("  http://h.example/a/b?x=1#frag ", "a/b"),
            ("https://huggingface.co", ""),
            ("https://huggingface.co/", ""),
            ("not an address", ""),
            ("", ""),
        ] {
            assert_eq!(
                call("path_after_host", &[address.into()]),
                Ok(Value::Str(want.into())),
                "{address}"
            );
        }
    }

    #[test]
    fn is_file_is_answered_by_the_caller_for_the_text_it_is_about_and_no_other() {
        let about = |input: &str, yes: bool| {
            map([("input", input.into()), ("input_is_file", Value::Bool(yes))])
        };
        let ask = |ctx: &Value, text: &str| (find("is_file").unwrap().call)(ctx, &[text.into()]);
        assert_eq!(
            ask(&about("/m.gguf", true), "/m.gguf"),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            ask(&about("/m.gguf", false), "/m.gguf"),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            ask(&about("/m.gguf", true), "/other"),
            Ok(Value::Bool(false)),
            "nothing is known of any other text"
        );
        assert_eq!(ask(&Value::Null, "/m.gguf"), Ok(Value::Bool(false)));
    }

    #[test]
    fn is_url_follows_the_model_box_rule() {
        for (s, want) in [
            ("https://example.com/m.gguf", true),
            ("HTTP://example.com", true),
            ("  https://x ", true),
            ("/home/me/m.gguf", false),
            (r"C:\m.gguf", false),
            ("ftp://x", false),
        ] {
            assert_eq!(call("is_url", &[s.into()]), Ok(Value::Bool(want)), "{s}");
        }
    }

    #[test]
    fn versions_are_read_leniently() {
        assert_eq!(
            call("semver_ge", &["0.35.1".into(), "0.35.0".into()]),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            call("semver_ge", &["v0.35".into(), "0.35.1".into()]),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            call("semver_ge", &["1".into(), "0.99.9".into()]),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            call("semver_ge", &["0.4.1-dev".into(), "0.4.1".into()]),
            Ok(Value::Bool(false))
        );
        assert!(call("semver_ge", &["nope".into(), "1".into()]).is_err());
        assert!(call("semver_ge", &["1..2".into(), "1".into()]).is_err());
        assert!(call("semver_ge", &["1.2.3.4".into(), "1".into()]).is_err());
    }

    #[test]
    fn min_max_clamp_keep_integers_integers() {
        assert_eq!(call("min", &[3i64.into(), 5i64.into()]), Ok(Value::Int(3)));
        assert_eq!(
            call("max", &[3i64.into(), 5.5.into()]),
            Ok(Value::Float(5.5))
        );
        assert_eq!(
            call("clamp", &[10i64.into(), 0i64.into(), 5i64.into()]),
            Ok(Value::Int(5))
        );
        assert_eq!(
            call("clamp", &[(-1i64).into(), 0i64.into(), 5i64.into()]),
            Ok(Value::Int(0))
        );
        assert_eq!(
            call("clamp", &[3i64.into(), 0i64.into(), 5i64.into()]),
            Ok(Value::Int(3))
        );
        assert!(call("clamp", &[3i64.into(), 5i64.into(), 0i64.into()]).is_err());
        assert_eq!(call("min", &[Value::Null, 4i64.into()]), Ok(Value::Int(0)));
        assert!(call("min", &["a".into(), 1i64.into()]).is_err());
    }

    #[test]
    fn recommend_ctx_size_matches_the_application() {
        const GB: i64 = 1024 * 1024 * 1024;
        assert_eq!(
            call("recommend_ctx_size", &[GB.into(), (32 * GB).into()]),
            Ok(Value::Int(16384))
        );
        // no model selected: null size is the default
        assert_eq!(
            call("recommend_ctx_size", &[Value::Null, (32 * GB).into()]),
            Ok(Value::Int(4096))
        );
    }

    #[test]
    fn context_functions_read_the_context() {
        let ctx = map([
            (
                "runtime",
                map([("flags", vec!["--mmproj", "--lora"].into())]),
            ),
            ("gpu", map([("vendors", vec!["Nvidia"].into())])),
        ]);
        let run = |name: &str, arg: &str| (find(name).unwrap().call)(&ctx, &[arg.into()]);
        assert_eq!(run("runtime.has_flag", "--mmproj"), Ok(Value::Bool(true)));
        assert_eq!(
            run("runtime.has_flag", "--video-fps"),
            Ok(Value::Bool(false))
        );
        assert_eq!(run("gpu.has", "nvidia"), Ok(Value::Bool(true)));
        assert_eq!(run("gpu.has", "amd"), Ok(Value::Bool(false)));
        // no flags known at all
        assert_eq!(
            (find("runtime.has_flag").unwrap().call)(&Value::Null, &["--x".into()]),
            Ok(Value::Bool(false))
        );
    }

    #[test]
    fn equality_and_order_follow_the_language_rules() {
        assert!(values_equal(&Value::Int(1), &Value::Float(1.0)));
        assert!(!values_equal(&Value::Int(1), &Value::Str("1".into())));
        assert!(values_equal(&Value::Null, &Value::Null));
        assert!(values_equal(&vec!["a"].into(), &vec!["a"].into()));
        assert_eq!(
            order(&Value::Int(1), &Value::Float(1.5)),
            Some(Ordering::Less)
        );
        assert_eq!(order(&"a".into(), &"b".into()), Some(Ordering::Less));
        assert_eq!(order(&Value::Null, &Value::Int(1)), None);
        assert_eq!(order(&"a".into(), &Value::Int(1)), None);
    }

    #[test]
    fn every_function_has_a_unique_name_and_a_matching_param_count() {
        let mut seen = std::collections::BTreeSet::new();
        for f in FUNCS {
            assert!(seen.insert(f.name), "duplicate function {}", f.name);
            assert!(!f.params.is_empty(), "{}", f.name);
        }
    }
}
