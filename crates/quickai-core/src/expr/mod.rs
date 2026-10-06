//! The closed expression language of docs/architecture.md, section 7.
//!
//! One small language serves every condition, default and string hole in a document, so documents
//! need no scripting. `parse` turns text into an `Expr`, `check` types it against a schema of the
//! context when a document is loaded, `eval` runs it against a context, and `Template` handles
//! strings with `${...}` holes.

pub mod ast;
pub mod check;
pub mod eval;
pub mod funcs;
pub mod lexer;
pub mod parser;
pub mod template;
pub mod types;

pub use ast::{BinOp, Expr};
pub use check::{check, CheckError};
pub use eval::{eval, EvalError};
pub use parser::{parse, ParseError};
pub use template::Template;
pub use types::Type;

#[cfg(test)]
mod fuzz {
    //! Random input must never panic or hang: every text either parses and then checks and
    //! evaluates to a result (a value or an error), or is refused with an error.

    use super::*;
    use crate::value::{map, Value};

    /// A small deterministic generator, so a failure can be reproduced from its seed.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }

        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[(self.next() as usize) % items.len()]
        }
    }

    const PIECES: &[&str] = &[
        "and",
        "or",
        "not",
        "in",
        "true",
        "false",
        "null",
        "(",
        ")",
        "[",
        "]",
        ",",
        ".",
        "==",
        "!=",
        "<",
        "<=",
        ">",
        ">=",
        "-",
        "0",
        "1",
        "42",
        "1.5",
        "'x'",
        "\"y\"",
        "os",
        "model",
        "model.size_bytes",
        "runtime.flags",
        "has",
        "len",
        "min",
        "max",
        "clamp",
        "matches",
        "lower",
        "x",
        "$",
        "{",
        "}",
        "'",
        "\"",
        "\\",
        "!",
        "=",
        " ",
        " ",
        " ",
        "é",
        "9999999999999999999",
        "recommend_ctx_size",
        "runtime.has_flag",
        "gpu.has",
        "semver_ge",
        "is_url",
        "starts_with",
    ];

    fn sample_context() -> Value {
        map([
            ("os", "linux".into()),
            ("model", map([("size_bytes", 1024u64.into())])),
            ("runtime", map([("flags", vec!["--a"].into())])),
            ("gpu", map([("vendors", vec!["nvidia"].into())])),
            ("x", 1i64.into()),
        ])
    }

    #[test]
    fn random_text_never_panics() {
        let ctx = sample_context();
        let schema = Type::of(&ctx);
        for seed in 0..4000u64 {
            let mut rng = Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xDEADBEEF);
            let len = 1 + (rng.next() % 14) as usize;
            let text: String = (0..len)
                .map(|_| *rng.pick(PIECES))
                .collect::<Vec<_>>()
                .join(" ");
            if let Ok(expr) = parse(&text) {
                let _ = check(&expr, &schema);
                let _ = eval(&expr, &ctx);
            }
            if let Ok(template) = Template::parse(&text) {
                let _ = template.check(&schema);
                let _ = template.render(&ctx);
            }
        }
    }

    #[test]
    fn pathological_inputs_are_refused_quickly() {
        // a long chain is refused (it would build a tree too tall to walk safely) ...
        let too_long = vec!["true"; 5000].join(" and ");
        let e = parse(&too_long).unwrap_err();
        assert!(e.message.contains("too complex"), "{}", e.message);
        let too_long_or = vec!["false"; 5000].join(" or ");
        assert!(parse(&too_long_or).is_err());
        // ... while a long but sensible one is fine
        let long = vec!["true"; 150].join(" and ");
        let expr = parse(&long).unwrap();
        assert_eq!(eval(&expr, &Value::Null).unwrap(), Value::Bool(true));
        assert!(parse(&"(".repeat(100_000)).is_err());
        assert!(parse(&"-".repeat(100_000)).is_err());
        assert!(parse(&format!("{}1", "not ".repeat(100_000))).is_err());
        assert!(parse(&format!("[{}", "[".repeat(100_000))).is_err());
        assert!(Template::parse(&"${".repeat(10_000)).is_err());
    }
}
