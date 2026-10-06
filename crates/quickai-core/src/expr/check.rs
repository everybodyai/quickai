//! The load-time type check of an expression against a schema of the context.
//!
//! It finds what the parser cannot: a name that is not in the context (with a "did you mean"), a
//! function that does not exist or is called with the wrong number or kind of arguments, a string
//! compared with a number, a condition that is not a condition, and a regular expression that does
//! not compile. All problems of one expression are reported, not only the first.

use super::ast::{BinOp, Expr};
use super::funcs;
use super::types::{suggest, Type};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckError {
    pub message: String,
    pub hint: Option<String>,
}

impl CheckError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: None,
        }
    }

    fn with_hint(mut self, hint: Option<String>) -> Self {
        self.hint = hint;
        self
    }
}

/// The type the expression has, and every problem found. When there are problems the type is a best
/// guess (`Any` where the check could not tell).
pub fn check(expr: &Expr, schema: &Type) -> (Type, Vec<CheckError>) {
    let mut errors = Vec::new();
    let ty = infer(expr, schema, &mut errors);
    (ty, errors)
}

fn infer(expr: &Expr, schema: &Type, errors: &mut Vec<CheckError>) -> Type {
    match expr {
        Expr::Lit(value) => Type::of(value),
        Expr::List(items) => {
            let types: Vec<Type> = items.iter().map(|i| infer(i, schema, errors)).collect();
            match types.split_first() {
                None => Type::list_of(Type::Any),
                Some((first, rest)) => {
                    if rest.iter().all(|t| t == first) {
                        Type::list_of(first.clone())
                    } else {
                        Type::list_of(Type::Any)
                    }
                }
            }
        }
        Expr::Path(path) => match schema.resolve(path) {
            Ok(ty) => ty,
            Err(e) => {
                errors.push(CheckError {
                    message: e.message,
                    hint: e.hint,
                });
                Type::Any
            }
        },
        Expr::Not(inner) => {
            let ty = infer(inner, schema, errors);
            require_condition(&ty, "`not`", errors);
            Type::Bool
        }
        Expr::Neg(inner) => {
            let ty = infer(inner, schema, errors);
            if !ty.is_numeric() {
                errors.push(CheckError::new(format!(
                    "unary `-` needs a number, found {}",
                    ty.name()
                )));
                return Type::Any;
            }
            ty
        }
        Expr::Binary(op, left, right) => {
            let l = infer(left, schema, errors);
            let r = infer(right, schema, errors);
            binary(*op, &l, &r, errors)
        }
        Expr::Call { name, args } => call(name, args, schema, errors),
    }
}

fn require_condition(ty: &Type, what: &str, errors: &mut Vec<CheckError>) {
    if !ty.is_condition() {
        errors.push(
            CheckError::new(format!("{what} needs a condition, found {}", ty.name())).with_hint(
                Some("use a comparison, such as `x == 'a'` or `x > 0`".to_string()),
            ),
        );
    }
}

fn binary(op: BinOp, l: &Type, r: &Type, errors: &mut Vec<CheckError>) -> Type {
    match op {
        BinOp::And | BinOp::Or => {
            require_condition(l, &format!("`{}`", op.symbol()), errors);
            require_condition(r, &format!("`{}`", op.symbol()), errors);
        }
        BinOp::Eq | BinOp::Ne => {
            if !l.comparable(r) {
                errors.push(CheckError::new(format!(
                    "`{}` compares {} with {}, which can never be equal",
                    op.symbol(),
                    l.name(),
                    r.name()
                )));
            }
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let both_numbers = l.is_numeric() && r.is_numeric();
            let both_strings =
                matches!(l, Type::Str | Type::Any) && matches!(r, Type::Str | Type::Any);
            if !(both_numbers || both_strings)
                && !matches!(l, Type::Null)
                && !matches!(r, Type::Null)
            {
                errors.push(CheckError::new(format!(
                    "`{}` orders numbers or strings, found {} and {}",
                    op.symbol(),
                    l.name(),
                    r.name()
                )));
            }
        }
        BinOp::In | BinOp::NotIn => match r {
            Type::List(item) => {
                if !l.comparable(item) {
                    errors.push(CheckError::new(format!(
                        "`{}` looks for {} in {}",
                        op.symbol(),
                        l.name(),
                        r.name()
                    )));
                }
            }
            Type::Str => {
                if !matches!(l, Type::Str | Type::Any | Type::Null) {
                    errors.push(CheckError::new(format!(
                        "`{}` looks for {} in a string; a string can only contain a string",
                        op.symbol(),
                        l.name()
                    )));
                }
            }
            Type::Map(_) | Type::Any | Type::Null => {}
            other => errors.push(CheckError::new(format!(
                "`{}` needs a list, a string or a map on the right, found {}",
                op.symbol(),
                other.name()
            ))),
        },
    }
    Type::Bool
}

fn call(name: &str, args: &[Expr], schema: &Type, errors: &mut Vec<CheckError>) -> Type {
    let arg_types: Vec<Type> = args.iter().map(|a| infer(a, schema, errors)).collect();
    let Some(func) = funcs::find(name) else {
        errors.push(
            CheckError::new(format!("unknown function `{name}`"))
                .with_hint(suggest(name, funcs::names()).map(|s| format!("did you mean `{s}`?"))),
        );
        return Type::Any;
    };
    if args.len() != func.params.len() {
        errors.push(CheckError::new(format!(
            "`{name}` takes {} argument{}, found {}",
            func.params.len(),
            if func.params.len() == 1 { "" } else { "s" },
            args.len()
        )));
        return func.ret.to_type(&arg_types);
    }
    for (i, (param, ty)) in func.params.iter().zip(&arg_types).enumerate() {
        if !param.accepts(ty) {
            errors.push(CheckError::new(format!(
                "argument {} of `{name}` must be {}, found {}",
                i + 1,
                param.describe(),
                ty.name()
            )));
        }
    }
    // A pattern written out in the document can be checked now, not when it is first used.
    if name == "matches" {
        if let Some(Expr::Lit(crate::value::Value::Str(pattern))) = args.get(1) {
            if let Err(message) = funcs::compile_regex(pattern) {
                errors.push(CheckError::new(message));
            }
        }
    }
    func.ret.to_type(&arg_types)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::parser::parse;

    fn schema() -> Type {
        Type::object([
            ("os", Type::Str),
            ("arch", Type::Str),
            ("listen_port", Type::Int),
            (
                "sys",
                Type::object([("ram_bytes", Type::Int), ("cpus", Type::Int)]),
            ),
            (
                "gpu",
                Type::object([("known", Type::Bool), ("vendors", Type::list_of(Type::Str))]),
            ),
            (
                "runtime",
                Type::object([("build", Type::Int), ("flags", Type::list_of(Type::Str))]),
            ),
            (
                "model",
                Type::object([
                    ("size_bytes", Type::Int),
                    (
                        "modalities",
                        Type::object([("input", Type::list_of(Type::Str))]),
                    ),
                    ("components", Type::Any),
                ]),
            ),
        ])
    }

    fn run(src: &str) -> (Type, Vec<CheckError>) {
        check(&parse(src).unwrap(), &schema())
    }

    fn ok(src: &str) -> Type {
        let (ty, errors) = run(src);
        assert!(errors.is_empty(), "{src}: {errors:?}");
        ty
    }

    fn messages(src: &str) -> Vec<String> {
        run(src).1.into_iter().map(|e| e.message).collect()
    }

    #[test]
    fn well_formed_expressions_get_their_type() {
        assert_eq!(ok("os == 'linux' and arch == 'x86_64'"), Type::Bool);
        assert_eq!(ok("gpu.known and not gpu.has('nvidia')"), Type::Bool);
        assert_eq!(
            ok("has(model.modalities.input, 'video') and runtime.has_flag('--video-fps')"),
            Type::Bool
        );
        assert_eq!(
            ok("recommend_ctx_size(model.size_bytes, sys.ram_bytes)"),
            Type::Int
        );
        assert_eq!(ok("listen_port"), Type::Int);
        assert_eq!(ok("min(sys.cpus, 8)"), Type::Int);
        assert_eq!(ok("min(sys.cpus, 8.5)"), Type::Float);
        assert_eq!(ok("os in ['linux', 'macos']"), Type::Bool);
        assert_eq!(
            ok("model"),
            Type::object([
                ("components", Type::Any),
                (
                    "modalities",
                    Type::object([("input", Type::list_of(Type::Str))])
                ),
                ("size_bytes", Type::Int)
            ])
        );
        assert_eq!(ok("model.components"), Type::Any);
        assert_eq!(ok("sys.ram_bytes > 1000 and sys.cpus >= 4"), Type::Bool);
    }

    #[test]
    fn a_misspelt_name_is_found_with_a_suggestion() {
        let (_, errors) = run("runtim.build > 1");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "unknown name `runtim`");
        assert_eq!(errors[0].hint.as_deref(), Some("did you mean `runtime`?"));
        let (_, errors) = run("model.size_byte > 1");
        assert_eq!(errors[0].message, "`model` has no `size_byte`");
        assert_eq!(
            errors[0].hint.as_deref(),
            Some("did you mean `model.size_bytes`?")
        );
    }

    #[test]
    fn functions_are_checked_for_existence_arity_and_argument_types() {
        let (_, errors) = run("recomend_ctx_size(1, 2)");
        assert_eq!(errors[0].message, "unknown function `recomend_ctx_size`");
        assert_eq!(
            errors[0].hint.as_deref(),
            Some("did you mean `recommend_ctx_size`?")
        );
        assert_eq!(
            messages("has(model.modalities.input)"),
            vec!["`has` takes 2 arguments, found 1"]
        );
        assert_eq!(
            messages("lower()"),
            vec!["`lower` takes 1 argument, found 0"]
        );
        assert_eq!(
            messages("lower(sys.cpus)"),
            vec!["argument 1 of `lower` must be a string, found an int"]
        );
        assert_eq!(
            messages("has(os, 'x')"),
            vec!["argument 1 of `has` must be a list, found a string"]
        );
    }

    #[test]
    fn types_that_can_never_match_are_rejected() {
        assert_eq!(
            messages("os == 5"),
            vec!["`==` compares a string with an int, which can never be equal"]
        );
        assert_eq!(
            messages("gpu.known == 'yes'"),
            vec!["`==` compares a bool with a string, which can never be equal"]
        );
        assert_eq!(
            messages("os < 5"),
            vec!["`<` orders numbers or strings, found a string and an int"]
        );
        assert_eq!(
            messages("-os"),
            vec!["unary `-` needs a number, found a string"]
        );
        assert_eq!(
            messages("5 in os"),
            vec!["`in` looks for an int in a string; a string can only contain a string"]
        );
        assert_eq!(
            messages("5 in sys.cpus"),
            vec!["`in` needs a list, a string or a map on the right, found an int"]
        );
        assert_eq!(
            messages("'x' in [1, 2]"),
            vec!["`in` looks for a string in a list of int"]
        );
    }

    #[test]
    fn conditions_must_be_conditions() {
        let (_, errors) = run("os and gpu.known");
        assert_eq!(errors[0].message, "`and` needs a condition, found a string");
        assert!(errors[0].hint.as_deref().unwrap().contains("comparison"));
        assert_eq!(
            messages("not sys.cpus"),
            vec!["`not` needs a condition, found an int"]
        );
        // lists and maps are conditions (their presence), as the example documents use them
        ok("model and model.components");
        ok("not model.modalities.input");
    }

    #[test]
    fn every_problem_of_an_expression_is_reported() {
        assert_eq!(
            messages("runtim.build > 1 and nope(1) and os == 5").len(),
            3
        );
    }

    #[test]
    fn a_literal_regular_expression_is_compiled_at_load_time() {
        let m = messages("matches(os, '(')");
        assert_eq!(m.len(), 1);
        assert!(m[0].contains("invalid regular expression"), "{}", m[0]);
        ok(r#"matches(os, "^lin")"#);
        // a pattern that is computed cannot be checked early; that is fine
        ok("matches(os, arch)");
    }
}
