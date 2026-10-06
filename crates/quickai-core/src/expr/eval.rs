//! Evaluates an expression against a context (a `Value::Map`).
//!
//! Evaluation always terminates: the language has no loops or recursion, so the work is bounded by
//! the size of the expression. A name that is not in the context reads as `null` ("not there yet",
//! for example no model selected), which is false as a condition and equal only to `null`.

use std::cmp::Ordering;

use super::ast::{BinOp, Expr};
use super::funcs;
use crate::value::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct EvalError {
    pub message: String,
}

fn fail<T>(message: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError {
        message: message.into(),
    })
}

pub fn eval(expr: &Expr, ctx: &Value) -> Result<Value, EvalError> {
    match expr {
        Expr::Lit(value) => Ok(value.clone()),
        Expr::List(items) => items
            .iter()
            .map(|i| eval(i, ctx))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Expr::Path(path) => {
            let steps: Vec<&str> = path.iter().map(String::as_str).collect();
            Ok(ctx.lookup(&steps).cloned().unwrap_or(Value::Null))
        }
        Expr::Not(inner) => Ok(Value::Bool(!eval(inner, ctx)?.truthy())),
        Expr::Neg(inner) => match eval(inner, ctx)? {
            Value::Int(i) => match i.checked_neg() {
                Some(n) => Ok(Value::Int(n)),
                None => fail("the number is too large to negate"),
            },
            Value::Float(f) => Ok(Value::Float(-f)),
            Value::Null => Ok(Value::Int(0)),
            other => fail(format!(
                "unary `-` needs a number, found {}",
                other.type_name()
            )),
        },
        Expr::Binary(op, left, right) => binary(*op, left, right, ctx),
        Expr::Call { name, args } => {
            let Some(func) = funcs::find(name) else {
                return fail(format!("unknown function `{name}`"));
            };
            if args.len() != func.params.len() {
                return fail(format!(
                    "`{name}` takes {} arguments, found {}",
                    func.params.len(),
                    args.len()
                ));
            }
            let values = args
                .iter()
                .map(|a| eval(a, ctx))
                .collect::<Result<Vec<_>, _>>()?;
            (func.call)(ctx, &values).map_err(|message| EvalError {
                message: format!("`{name}`: {message}"),
            })
        }
    }
}

fn binary(op: BinOp, left: &Expr, right: &Expr, ctx: &Value) -> Result<Value, EvalError> {
    // `and` and `or` stop as soon as the answer is known.
    match op {
        BinOp::And => {
            return Ok(Value::Bool(
                eval(left, ctx)?.truthy() && eval(right, ctx)?.truthy(),
            ))
        }
        BinOp::Or => {
            return Ok(Value::Bool(
                eval(left, ctx)?.truthy() || eval(right, ctx)?.truthy(),
            ))
        }
        _ => {}
    }
    let l = eval(left, ctx)?;
    let r = eval(right, ctx)?;
    match op {
        BinOp::Eq => Ok(Value::Bool(funcs::values_equal(&l, &r))),
        BinOp::Ne => Ok(Value::Bool(!funcs::values_equal(&l, &r))),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            if l.is_null() || r.is_null() {
                return Ok(Value::Bool(false));
            }
            let Some(ordering) = funcs::order(&l, &r) else {
                return fail(format!(
                    "`{}` orders numbers or strings, found {} and {}",
                    op.symbol(),
                    l.type_name(),
                    r.type_name()
                ));
            };
            Ok(Value::Bool(match op {
                BinOp::Lt => ordering == Ordering::Less,
                BinOp::Le => ordering != Ordering::Greater,
                BinOp::Gt => ordering == Ordering::Greater,
                _ => ordering != Ordering::Less,
            }))
        }
        BinOp::In | BinOp::NotIn => {
            let found = match &r {
                Value::List(items) => items.iter().any(|i| funcs::values_equal(i, &l)),
                Value::Str(haystack) => match &l {
                    Value::Str(needle) => haystack.contains(needle.as_str()),
                    Value::Null => false,
                    other => {
                        return fail(format!(
                            "`in` looks for a string in a string, found {}",
                            other.type_name()
                        ))
                    }
                },
                Value::Map(map) => l.as_str().is_some_and(|k| map.contains_key(k)),
                Value::Null => false,
                other => {
                    return fail(format!(
                        "`in` needs a list, a string or a map on the right, found {}",
                        other.type_name()
                    ))
                }
            };
            Ok(Value::Bool(if op == BinOp::In { found } else { !found }))
        }
        BinOp::And | BinOp::Or => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::parser::parse;
    use crate::value::map;

    fn ctx() -> Value {
        map([
            ("os", "linux".into()),
            ("arch", "x86_64".into()),
            ("listen_port", 8080u16.into()),
            (
                "sys",
                map([
                    ("ram_bytes", (32u64 * 1024 * 1024 * 1024).into()),
                    ("cpus", 8u32.into()),
                ]),
            ),
            (
                "gpu",
                map([("known", true.into()), ("vendors", vec!["nvidia"].into())]),
            ),
            (
                "runtime",
                map([
                    ("flags", vec!["--mmproj", "--lora"].into()),
                    ("build", 11007u32.into()),
                ]),
            ),
            (
                "model",
                map([
                    ("size_bytes", (1024u64 * 1024 * 1024).into()),
                    ("modalities", map([("input", vec!["text", "image"].into())])),
                ]),
            ),
            ("value", 999u32.into()),
            ("smallest", Value::Int(i64::MIN)),
        ])
    }

    fn run(src: &str) -> Value {
        eval(&parse(src).unwrap(), &ctx()).unwrap_or_else(|e| panic!("{src}: {}", e.message))
    }

    fn fails(src: &str) -> String {
        eval(&parse(src).unwrap(), &ctx()).unwrap_err().message
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(run("os == 'linux' and arch == 'x86_64'"), Value::Bool(true));
        assert_eq!(
            run("os == 'windows' or arch == 'x86_64'"),
            Value::Bool(true)
        );
        assert_eq!(run("not gpu.known"), Value::Bool(false));
        assert_eq!(run("value > 0"), Value::Bool(true));
        assert_eq!(run("value >= 999 and value <= 999"), Value::Bool(true));
        assert_eq!(run("sys.cpus < 4"), Value::Bool(false));
        assert_eq!(run("1 == 1.0"), Value::Bool(true));
        assert_eq!(run("'a' < 'b'"), Value::Bool(true));
        assert_eq!(run("-1 < 0"), Value::Bool(true));
        assert_eq!(run("- -2"), Value::Int(2));
    }

    #[test]
    fn membership() {
        assert_eq!(run("os in ['linux', 'macos']"), Value::Bool(true));
        assert_eq!(run("os not in ['windows']"), Value::Bool(true));
        assert_eq!(run("'image' in model.modalities.input"), Value::Bool(true));
        assert_eq!(run("'inu' in os"), Value::Bool(true));
        assert_eq!(run("'sys' in model"), Value::Bool(false));
        assert_eq!(run("'modalities' in model"), Value::Bool(true));
        assert_eq!(run("'x' in missing.thing"), Value::Bool(false));
    }

    #[test]
    fn calls_see_the_context() {
        assert_eq!(
            run("has(model.modalities.input, 'image')"),
            Value::Bool(true)
        );
        assert_eq!(run("runtime.has_flag('--mmproj')"), Value::Bool(true));
        assert_eq!(run("runtime.has_flag('--video-fps')"), Value::Bool(false));
        assert_eq!(
            run("gpu.known and not gpu.has('nvidia')"),
            Value::Bool(false)
        );
        assert_eq!(
            run("recommend_ctx_size(model.size_bytes, sys.ram_bytes)"),
            Value::Int(16384)
        );
    }

    #[test]
    fn missing_names_are_null_and_null_is_quiet() {
        assert_eq!(run("missing"), Value::Null);
        assert_eq!(run("missing.deeper"), Value::Null);
        assert_eq!(run("missing == null"), Value::Bool(true));
        assert_eq!(run("missing > 1"), Value::Bool(false));
        assert_eq!(run("missing and true"), Value::Bool(false));
        assert_eq!(run("not missing"), Value::Bool(true));
        // no model selected: the size is null and reads as 0, so the default context size results
        assert_eq!(
            run("recommend_ctx_size(nomodel.size_bytes, sys.ram_bytes)"),
            Value::Int(4096)
        );
        assert_eq!(run("has(nothing, 'x')"), Value::Bool(false));
    }

    #[test]
    fn presence_conditions_work_on_lists_and_maps() {
        assert_eq!(run("model and true"), Value::Bool(true));
        assert_eq!(run("not model.modalities.input"), Value::Bool(false));
        assert_eq!(run("[] and true"), Value::Bool(false));
    }

    #[test]
    fn and_or_short_circuit() {
        // the right side would be an error if it were evaluated
        assert_eq!(run("false and (1 < 'x')"), Value::Bool(false));
        assert_eq!(run("true or (1 < 'x')"), Value::Bool(true));
    }

    #[test]
    fn runtime_errors_say_what_went_wrong() {
        assert!(fails("1 < 'x'").contains("orders numbers or strings"));
        assert!(fails("-'x'").contains("needs a number"));
        assert!(fails("5 in 5").contains("needs a list, a string or a map"));
        assert!(fails("5 in 'abc'").contains("looks for a string in a string"));
        assert!(fails("matches('x', '(')").contains("invalid regular expression"));
        assert!(fails("has(1, 2)").contains("`has`"));
        assert!(fails("nope(1)").contains("unknown function"));
        assert!(fails("lower(1, 2)").contains("takes 1 arguments"));
        assert!(fails("-smallest").contains("too large"));
    }
}
