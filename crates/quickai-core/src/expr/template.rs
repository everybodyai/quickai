//! Strings with `${expression}` holes: command-line arguments, paths, JSON bodies, messages.
//!
//! A template whose whole text is one hole (`"${listen_port}"`) keeps the value's type, so a
//! number stays a number inside a JSON body. Anything else renders to a string. `$${` writes a
//! literal `${`.

use super::ast::Expr;
use super::check::{check, CheckError};
use super::eval::{eval, EvalError};
use super::parser::{parse, ParseError};
use super::types::Type;
use crate::value::Value;

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Text(String),
    Hole { source: String, expr: Expr },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    parts: Vec<Part>,
}

impl Template {
    pub fn parse(src: &str) -> Result<Template, ParseError> {
        let chars: Vec<(usize, char)> = src.char_indices().collect();
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut i = 0;
        while i < chars.len() {
            let (pos, c) = chars[i];
            if c == '$' {
                let next = chars.get(i + 1).map(|c| c.1);
                let after = chars.get(i + 2).map(|c| c.1);
                if next == Some('$') && after == Some('{') {
                    // `$${` is a literal `${`
                    text.push_str("${");
                    i += 3;
                    continue;
                }
                if next == Some('{') {
                    let body_start = i + 2;
                    let Some(body_end) = closing_brace(&chars, body_start) else {
                        return Err(ParseError {
                            message: "this `${` is never closed with `}`".into(),
                            pos,
                        });
                    };
                    let source: String = chars[body_start..body_end].iter().map(|c| c.1).collect();
                    let expr = parse(&source).map_err(|e| ParseError {
                        message: e.message,
                        // position inside the whole template
                        pos: chars.get(body_start).map(|c| c.0).unwrap_or(pos) + e.pos,
                    })?;
                    if !text.is_empty() {
                        parts.push(Part::Text(std::mem::take(&mut text)));
                    }
                    parts.push(Part::Hole { source, expr });
                    i = body_end + 1;
                    continue;
                }
            }
            text.push(c);
            i += 1;
        }
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        Ok(Template { parts })
    }

    /// Whether the text has no holes at all.
    pub fn is_literal(&self) -> bool {
        self.parts.iter().all(|p| matches!(p, Part::Text(_)))
    }

    /// The source text of the hole when the whole template is exactly one (`"${model.main.path}"`),
    /// which is the one case where "no value" is a problem rather than an empty piece of text.
    pub fn lone_hole(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [Part::Hole { source, .. }] => Some(source),
            _ => None,
        }
    }

    /// The expressions of the holes, in order.
    pub fn exprs(&self) -> impl Iterator<Item = &Expr> {
        self.parts.iter().filter_map(|p| match p {
            Part::Hole { expr, .. } => Some(expr),
            Part::Text(_) => None,
        })
    }

    /// The holes' problems, each tagged with the hole's source text.
    pub fn check(&self, schema: &Type) -> Vec<CheckError> {
        let mut out = Vec::new();
        for part in &self.parts {
            if let Part::Hole { source, expr } = part {
                for mut e in check(expr, schema).1 {
                    e.message = format!("in `${{{source}}}`: {}", e.message);
                    out.push(e);
                }
            }
        }
        out
    }

    /// The text with every hole filled in. A template that is exactly one hole yields that hole's
    /// value unchanged; every other template yields a string.
    pub fn render(&self, ctx: &Value) -> Result<Value, EvalError> {
        if let [Part::Hole { expr, .. }] = self.parts.as_slice() {
            return eval(expr, ctx);
        }
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(t) => out.push_str(t),
                Part::Hole { expr, .. } => out.push_str(&eval(expr, ctx)?.to_string()),
            }
        }
        Ok(Value::Str(out))
    }

    /// `render`, as text.
    pub fn render_string(&self, ctx: &Value) -> Result<String, EvalError> {
        self.render(ctx).map(|v| v.to_string())
    }
}

/// Index of the `}` that closes a hole starting at `from`, ignoring braces inside quoted strings.
fn closing_brace(chars: &[(usize, char)], from: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut i = from;
    while i < chars.len() {
        let c = chars[i].1;
        match quote {
            Some(q) => {
                if c == '\\' {
                    i += 1; // skip the escaped character
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                '}' => return Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::map;

    fn ctx() -> Value {
        map([
            ("listen_port", 8080u16.into()),
            (
                "runtime",
                map([
                    ("dir", "/opt/llama".into()),
                    ("binary", "/opt/llama/llama-server".into()),
                ]),
            ),
            (
                "model",
                map([("main", map([("path", "/m/q.gguf".into())]))]),
            ),
            ("input", "hf.co/user/repo".into()),
        ])
    }

    fn render(src: &str) -> Value {
        Template::parse(src).unwrap().render(&ctx()).unwrap()
    }

    #[test]
    fn text_with_holes_renders_to_a_string() {
        assert_eq!(
            render("--model ${model.main.path}"),
            Value::Str("--model /m/q.gguf".into())
        );
        assert_eq!(
            render("http://127.0.0.1:${listen_port}/v1"),
            Value::Str("http://127.0.0.1:8080/v1".into())
        );
        assert_eq!(
            render("${runtime.dir}/${runtime.binary}"),
            Value::Str("/opt/llama//opt/llama/llama-server".into())
        );
        assert_eq!(render("plain"), Value::Str("plain".into()));
        assert_eq!(render(""), Value::Str(String::new()));
    }

    #[test]
    fn a_lone_hole_keeps_its_type() {
        assert_eq!(render("${listen_port}"), Value::Int(8080));
        assert_eq!(render("${listen_port > 1}"), Value::Bool(true));
        assert_eq!(render("${missing}"), Value::Null);
        // a number inside other text is text
        assert_eq!(render("${listen_port} "), Value::Str("8080 ".into()));
    }

    #[test]
    fn holes_may_hold_calls_and_quoted_braces() {
        assert_eq!(render("${lower('ABC')}"), Value::Str("abc".into()));
        assert_eq!(render("${matches(input, '^hf\\.co/')}"), Value::Bool(true));
        assert_eq!(render("x${'}'}y"), Value::Str("x}y".into()));
    }

    #[test]
    fn dollar_dollar_brace_is_a_literal() {
        assert_eq!(render("$${not_a_hole}"), Value::Str("${not_a_hole}".into()));
        assert_eq!(render("cost: $5"), Value::Str("cost: $5".into()));
        assert_eq!(render("a$"), Value::Str("a$".into()));
    }

    #[test]
    fn unclosed_and_broken_holes_are_errors_with_a_position() {
        let e = Template::parse("abc ${x").unwrap_err();
        assert!(e.message.contains("never closed"), "{}", e.message);
        assert_eq!(e.pos, 4);
        let e = Template::parse("abc ${1 +}").unwrap_err();
        assert!(
            e.message.contains("unexpected") || e.message.contains("character"),
            "{}",
            e.message
        );
        assert!(e.pos >= 6, "{}", e.pos);
        assert!(Template::parse("${}").is_err());
    }

    #[test]
    fn checking_reports_each_hole_with_its_source() {
        let schema = Type::object([("listen_port", Type::Int)]);
        let t = Template::parse("--port ${listen_port} --x ${lisen_port}").unwrap();
        let errors = t.check(&schema);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].message,
            "in `${lisen_port}`: unknown name `lisen_port`"
        );
        assert_eq!(
            errors[0].hint.as_deref(),
            Some("did you mean `listen_port`?")
        );
        assert!(Template::parse("no holes")
            .unwrap()
            .check(&schema)
            .is_empty());
        assert!(Template::parse("no holes").unwrap().is_literal());
        assert!(!t.is_literal());
        assert_eq!(t.exprs().count(), 2);
    }

    #[test]
    fn a_lone_hole_is_recognised() {
        assert_eq!(
            Template::parse("${model.main.path}").unwrap().lone_hole(),
            Some("model.main.path")
        );
        assert_eq!(Template::parse("${a}${b}").unwrap().lone_hole(), None);
        assert_eq!(Template::parse("--x ${a}").unwrap().lone_hole(), None);
        assert_eq!(Template::parse("plain").unwrap().lone_hole(), None);
    }

    #[test]
    fn render_errors_propagate() {
        let t = Template::parse("${1 < 'x'}").unwrap();
        assert!(t.render(&ctx()).is_err());
    }
}
