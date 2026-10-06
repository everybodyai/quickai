//! A recursive-descent parser for the expression language.
//!
//! Precedence, loosest first: `or`, `and`, prefix `not`, one comparison (`== != < <= > >= in`
//! and `not in`), unary minus, then literals, lists, parentheses, paths and calls. Comparisons do
//! not chain (`a < b < c` is an error: write `a < b and b < c`).

use super::ast::{BinOp, Expr};
use super::lexer::{lex, Tok};
use crate::value::Value;

/// Deeper nesting than any real document needs; the limit keeps hostile input from exhausting the
/// stack while parsing. It counts grammar levels, and one pair of parentheses costs about seven,
/// so this allows some thirty-five of them.
const MAX_DEPTH: usize = 256;

/// The most levels the finished tree may have. A long chain (`a and a and ...`) is parsed in a
/// loop, so it never recurses, but it builds a tree that is as tall as the chain is long and
/// checking, evaluating and dropping it do recurse. Real expressions are a few levels tall.
const MAX_HEIGHT: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the source text.
    pub pos: usize,
}

impl ParseError {
    /// `column 7: expected an operand, found `and``: what a diagnostic shows next to the text.
    pub fn describe(&self, src: &str) -> String {
        let column = src
            .char_indices()
            .take_while(|(i, _)| *i < self.pos)
            .count()
            + 1;
        format!("column {column}: {}", self.message)
    }
}

pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let tokens = lex(src).map_err(|e| ParseError {
        message: e.message,
        pos: e.pos,
    })?;
    let mut parser = Parser {
        tokens,
        at: 0,
        end: src.len(),
    };
    if parser.tokens.is_empty() {
        return Err(ParseError {
            message: "the expression is empty".into(),
            pos: 0,
        });
    }
    let (expr, _) = parser.or(0)?;
    if let Some((pos, tok)) = parser.tokens.get(parser.at) {
        return Err(ParseError {
            message: format!("unexpected `{tok}` after the end of the expression"),
            pos: *pos,
        });
    }
    Ok(expr)
}

/// A parsed subtree and its height.
type Parsed = Result<(Expr, usize), ParseError>;

struct Parser {
    tokens: Vec<(usize, Tok)>,
    at: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.at).map(|t| &t.1)
    }

    fn peek_at(&self, offset: usize) -> Option<&Tok> {
        self.tokens.get(self.at + offset).map(|t| &t.1)
    }

    fn pos(&self) -> usize {
        self.tokens.get(self.at).map(|t| t.0).unwrap_or(self.end)
    }

    fn bump(&mut self) -> Option<Tok> {
        let tok = self.tokens.get(self.at).map(|t| t.1.clone());
        if tok.is_some() {
            self.at += 1;
        }
        tok
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            pos: self.pos(),
        })
    }

    fn found(&self) -> String {
        match self.peek() {
            Some(tok) => format!("`{tok}`"),
            None => "the end of the expression".to_string(),
        }
    }

    /// The height of a node one level above children of height `child`, or an error when the tree
    /// would become taller than `MAX_HEIGHT`.
    fn grow(&self, child: usize) -> Result<usize, ParseError> {
        let height = child + 1;
        if height > MAX_HEIGHT {
            return self.error(format!(
                "the expression is too complex (more than {MAX_HEIGHT} levels deep)"
            ));
        }
        Ok(height)
    }

    fn enter(&self, depth: usize) -> Result<(), ParseError> {
        if depth > MAX_DEPTH {
            return self.error("the expression is nested too deeply");
        }
        Ok(())
    }

    fn or(&mut self, depth: usize) -> Parsed {
        self.enter(depth)?;
        let (mut left, mut height) = self.and(depth + 1)?;
        while self.peek() == Some(&Tok::Or) {
            self.bump();
            let (right, h) = self.and(depth + 1)?;
            height = self.grow(height.max(h))?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok((left, height))
    }

    fn and(&mut self, depth: usize) -> Parsed {
        self.enter(depth)?;
        let (mut left, mut height) = self.not(depth + 1)?;
        while self.peek() == Some(&Tok::And) {
            self.bump();
            let (right, h) = self.not(depth + 1)?;
            height = self.grow(height.max(h))?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok((left, height))
    }

    fn not(&mut self, depth: usize) -> Parsed {
        self.enter(depth)?;
        if self.peek() == Some(&Tok::Not) {
            self.bump();
            let (inner, h) = self.not(depth + 1)?;
            return Ok((Expr::Not(Box::new(inner)), self.grow(h)?));
        }
        self.comparison(depth + 1)
    }

    fn comparison(&mut self, depth: usize) -> Parsed {
        self.enter(depth)?;
        let (left, lh) = self.unary(depth + 1)?;
        let Some(op) = self.comparison_op() else {
            return Ok((left, lh));
        };
        let (right, rh) = self.unary(depth + 1)?;
        if self.comparison_op().is_some() {
            return self
                .error("comparison operators cannot be chained; join comparisons with `and`");
        }
        Ok((
            Expr::Binary(op, Box::new(left), Box::new(right)),
            self.grow(lh.max(rh))?,
        ))
    }

    /// Consumes a comparison operator if one is next.
    fn comparison_op(&mut self) -> Option<BinOp> {
        let op = match self.peek()? {
            Tok::EqEq => BinOp::Eq,
            Tok::NotEq => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::Le => BinOp::Le,
            Tok::Gt => BinOp::Gt,
            Tok::Ge => BinOp::Ge,
            Tok::In => BinOp::In,
            Tok::Not if self.peek_at(1) == Some(&Tok::In) => {
                self.bump();
                BinOp::NotIn
            }
            _ => return None,
        };
        self.bump();
        Some(op)
    }

    fn unary(&mut self, depth: usize) -> Parsed {
        self.enter(depth)?;
        if self.peek() == Some(&Tok::Minus) {
            self.bump();
            let (inner, h) = self.unary(depth + 1)?;
            return Ok((Expr::Neg(Box::new(inner)), self.grow(h)?));
        }
        self.primary(depth + 1)
    }

    fn primary(&mut self, depth: usize) -> Parsed {
        self.enter(depth)?;
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.bump();
                Ok((Expr::Lit(Value::Int(n)), 1))
            }
            Some(Tok::Float(x)) => {
                self.bump();
                Ok((Expr::Lit(Value::Float(x)), 1))
            }
            Some(Tok::Str(s)) => {
                self.bump();
                Ok((Expr::Lit(Value::Str(s)), 1))
            }
            Some(Tok::True) => {
                self.bump();
                Ok((Expr::Lit(Value::Bool(true)), 1))
            }
            Some(Tok::False) => {
                self.bump();
                Ok((Expr::Lit(Value::Bool(false)), 1))
            }
            Some(Tok::Null) => {
                self.bump();
                Ok((Expr::Lit(Value::Null), 1))
            }
            Some(Tok::LParen) => {
                self.bump();
                let (inner, height) = self.or(depth + 1)?;
                if self.peek() != Some(&Tok::RParen) {
                    return self.error(format!("expected `)`, found {}", self.found()));
                }
                self.bump();
                Ok((inner, height))
            }
            Some(Tok::LBracket) => {
                self.bump();
                let mut items = Vec::new();
                let mut height = 1;
                if self.peek() != Some(&Tok::RBracket) {
                    loop {
                        let (item, h) = self.or(depth + 1)?;
                        height = height.max(h);
                        items.push(item);
                        match self.peek() {
                            Some(Tok::Comma) => {
                                self.bump();
                            }
                            Some(Tok::RBracket) => break,
                            _ => {
                                return self.error(format!(
                                    "expected `,` or `]` in the list, found {}",
                                    self.found()
                                ))
                            }
                        }
                    }
                }
                self.bump();
                Ok((Expr::List(items), self.grow(height)?))
            }
            Some(Tok::Ident(first)) => {
                self.bump();
                let mut path = vec![first];
                while self.peek() == Some(&Tok::Dot) {
                    self.bump();
                    match self.bump() {
                        Some(Tok::Ident(next)) => path.push(next),
                        other => {
                            self.at -= usize::from(other.is_some());
                            return self.error(format!(
                                "expected a name after `.`, found {}",
                                self.found()
                            ));
                        }
                    }
                }
                if self.peek() == Some(&Tok::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    let mut height = 1;
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            let (arg, h) = self.or(depth + 1)?;
                            height = height.max(h);
                            args.push(arg);
                            match self.peek() {
                                Some(Tok::Comma) => {
                                    self.bump();
                                }
                                Some(Tok::RParen) => break,
                                _ => {
                                    return self.error(format!(
                                        "expected `,` or `)` in the call, found {}",
                                        self.found()
                                    ))
                                }
                            }
                        }
                    }
                    self.bump();
                    return Ok((
                        Expr::Call {
                            name: path.join("."),
                            args,
                        },
                        self.grow(height)?,
                    ));
                }
                Ok((Expr::Path(path), 1))
            }
            _ => self.error(format!("expected a value, found {}", self.found())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Expr {
        parse(src).unwrap_or_else(|e| panic!("{src}: {}", e.describe(src)))
    }

    fn path(parts: &[&str]) -> Expr {
        Expr::Path(parts.iter().map(|s| s.to_string()).collect())
    }

    fn lit(v: impl Into<Value>) -> Expr {
        Expr::Lit(v.into())
    }

    fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    #[test]
    fn literals_paths_and_lists() {
        assert_eq!(p("42"), lit(42i64));
        assert_eq!(p("1.5"), lit(1.5));
        assert_eq!(p("'x'"), lit("x"));
        assert_eq!(p("true"), lit(true));
        assert_eq!(p("null"), Expr::Lit(Value::Null));
        assert_eq!(p("model.size_bytes"), path(&["model", "size_bytes"]));
        assert_eq!(p("[1, 'a']"), Expr::List(vec![lit(1i64), lit("a")]));
        assert_eq!(p("[]"), Expr::List(vec![]));
        assert_eq!(p("-3"), Expr::Neg(Box::new(lit(3i64))));
    }

    #[test]
    fn precedence_is_or_and_not_comparison() {
        // a or b and not c == 1   =>   a or (b and (not (c == 1)))
        assert_eq!(
            p("a or b and not c == 1"),
            bin(
                BinOp::Or,
                path(&["a"]),
                bin(
                    BinOp::And,
                    path(&["b"]),
                    Expr::Not(Box::new(bin(BinOp::Eq, path(&["c"]), lit(1i64))))
                )
            )
        );
        // parentheses override
        assert_eq!(
            p("(a or b) and c"),
            bin(
                BinOp::And,
                bin(BinOp::Or, path(&["a"]), path(&["b"])),
                path(&["c"])
            )
        );
    }

    #[test]
    fn in_and_not_in() {
        assert_eq!(
            p("os in ['linux', 'macos']"),
            bin(
                BinOp::In,
                path(&["os"]),
                Expr::List(vec![lit("linux"), lit("macos")])
            )
        );
        assert_eq!(
            p("x not in y"),
            bin(BinOp::NotIn, path(&["x"]), path(&["y"]))
        );
        // a prefix `not` before an `in` comparison negates the comparison
        assert_eq!(
            p("not x in y"),
            Expr::Not(Box::new(bin(BinOp::In, path(&["x"]), path(&["y"]))))
        );
    }

    #[test]
    fn calls_with_plain_and_dotted_names() {
        assert_eq!(
            p("has(model.modalities.input, 'video')"),
            Expr::Call {
                name: "has".into(),
                args: vec![path(&["model", "modalities", "input"]), lit("video")]
            }
        );
        assert_eq!(
            p("runtime.has_flag('--video-fps')"),
            Expr::Call {
                name: "runtime.has_flag".into(),
                args: vec![lit("--video-fps")]
            }
        );
        assert_eq!(
            p("f()"),
            Expr::Call {
                name: "f".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn the_expressions_of_the_example_documents_parse() {
        for src in [
            "has(model.modalities.input, 'video') and runtime.has_flag('--video-ffmpeg-dir')",
            "os == 'linux' and arch == 'x86_64'",
            "gpu.known and not gpu.has('nvidia')",
            "not field.use_default_store",
            "value == 'off'",
            "value > 0",
            "session.running and ui.show_api",
            r#"matches(input, "^hf\.co/")"#,
            "is_url(input)",
            "model",
            "model.components",
            "true",
            "recommend_ctx_size(model.size_bytes, sys.ram_bytes)",
        ] {
            p(src);
        }
    }

    #[test]
    fn errors_name_the_problem_and_its_column() {
        let cases: &[(&str, &str, usize)] = &[
            ("", "empty", 0),
            (
                "a and",
                "expected a value, found the end of the expression",
                5,
            ),
            ("a b", "unexpected `b` after the end", 2),
            ("(a", "expected `)`", 2),
            ("[1 2]", "expected `,` or `]`", 3),
            ("f(1 2)", "expected `,` or `)`", 4),
            ("a.", "expected a name after `.`", 2),
            ("a < b < c", "cannot be chained", 8),
            ("and a", "expected a value, found `and`", 0),
        ];
        for (src, needle, pos) in cases {
            let e = parse(src).unwrap_err();
            assert!(e.message.contains(needle), "{src}: {}", e.message);
            assert_eq!(e.pos, *pos, "{src}");
        }
        assert_eq!(
            parse("a b").unwrap_err().describe("a b"),
            "column 3: unexpected `b` after the end of the expression"
        );
    }

    #[test]
    fn height_accumulates_across_nesting() {
        // ten levels of parentheses, each with a 30-term chain: 300 levels in all
        let mut src = "a".to_string();
        for _ in 0..10 {
            src = format!("({src}{})", " and a".repeat(30));
        }
        let e = parse(&src).unwrap_err();
        assert!(e.message.contains("too complex"), "{}", e.message);
        // the same shape at a sensible size is fine
        let mut src = "a".to_string();
        for _ in 0..3 {
            src = format!("({src}{})", " and a".repeat(5));
        }
        parse(&src).unwrap();
    }

    #[test]
    fn deep_nesting_is_refused_not_crashed_on() {
        let deep = format!("{}1{}", "(".repeat(500), ")".repeat(500));
        let e = parse(&deep).unwrap_err();
        assert!(e.message.contains("nested too deeply"), "{}", e.message);
        let nots = format!("{}a", "not ".repeat(500));
        assert!(parse(&nots).is_err());
        let negs = format!("{}1", "-".repeat(500));
        assert!(parse(&negs).is_err());
    }
}
