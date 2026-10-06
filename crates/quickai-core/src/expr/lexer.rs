//! Turns expression text into tokens, each with the byte offset it starts at.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    True,
    False,
    Null,
    And,
    Or,
    Not,
    In,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Minus,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Int(i) => write!(f, "{i}"),
            Tok::Float(x) => write!(f, "{x}"),
            Tok::Str(s) => write!(f, "'{s}'"),
            Tok::Ident(s) => f.write_str(s),
            Tok::True => f.write_str("true"),
            Tok::False => f.write_str("false"),
            Tok::Null => f.write_str("null"),
            Tok::And => f.write_str("and"),
            Tok::Or => f.write_str("or"),
            Tok::Not => f.write_str("not"),
            Tok::In => f.write_str("in"),
            Tok::LParen => f.write_str("("),
            Tok::RParen => f.write_str(")"),
            Tok::LBracket => f.write_str("["),
            Tok::RBracket => f.write_str("]"),
            Tok::Comma => f.write_str(","),
            Tok::Dot => f.write_str("."),
            Tok::EqEq => f.write_str("=="),
            Tok::NotEq => f.write_str("!="),
            Tok::Lt => f.write_str("<"),
            Tok::Le => f.write_str("<="),
            Tok::Gt => f.write_str(">"),
            Tok::Ge => f.write_str(">="),
            Tok::Minus => f.write_str("-"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LexError {
    pub message: String,
    pub pos: usize,
}

/// Splits `src` into `(offset, token)` pairs.
///
/// Strings are `'...'` or `"..."`. Inside them `\\`, `\'`, `\"`, `\n` and `\t` are escapes; any
/// other backslash stays as it is, so a regular expression such as `"^hf\.co/"` reads naturally.
pub fn lex(src: &str) -> Result<Vec<(usize, Tok)>, LexError> {
    let bytes: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let (pos, c) = bytes[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push((pos, Tok::LParen));
                i += 1;
            }
            ')' => {
                out.push((pos, Tok::RParen));
                i += 1;
            }
            '[' => {
                out.push((pos, Tok::LBracket));
                i += 1;
            }
            ']' => {
                out.push((pos, Tok::RBracket));
                i += 1;
            }
            ',' => {
                out.push((pos, Tok::Comma));
                i += 1;
            }
            '.' => {
                out.push((pos, Tok::Dot));
                i += 1;
            }
            '-' => {
                out.push((pos, Tok::Minus));
                i += 1;
            }
            '=' => {
                if bytes.get(i + 1).map(|b| b.1) == Some('=') {
                    out.push((pos, Tok::EqEq));
                    i += 2;
                } else {
                    return Err(LexError {
                        message: "a single `=` is not an operator; compare with `==`".into(),
                        pos,
                    });
                }
            }
            '!' => {
                if bytes.get(i + 1).map(|b| b.1) == Some('=') {
                    out.push((pos, Tok::NotEq));
                    i += 2;
                } else {
                    return Err(LexError {
                        message: "`!` is not an operator; write `not`, or `!=` to compare".into(),
                        pos,
                    });
                }
            }
            '<' => {
                if bytes.get(i + 1).map(|b| b.1) == Some('=') {
                    out.push((pos, Tok::Le));
                    i += 2;
                } else {
                    out.push((pos, Tok::Lt));
                    i += 1;
                }
            }
            '>' => {
                if bytes.get(i + 1).map(|b| b.1) == Some('=') {
                    out.push((pos, Tok::Ge));
                    i += 2;
                } else {
                    out.push((pos, Tok::Gt));
                    i += 1;
                }
            }
            '\'' | '"' => {
                let quote = c;
                let mut text = String::new();
                let mut j = i + 1;
                loop {
                    let Some(&(_, ch)) = bytes.get(j) else {
                        return Err(LexError {
                            message: format!("this string is never closed (missing {quote})"),
                            pos,
                        });
                    };
                    if ch == quote {
                        j += 1;
                        break;
                    }
                    if ch == '\\' {
                        match bytes.get(j + 1).map(|b| b.1) {
                            Some('\\') => text.push('\\'),
                            Some('\'') => text.push('\''),
                            Some('"') => text.push('"'),
                            Some('n') => text.push('\n'),
                            Some('t') => text.push('\t'),
                            Some(other) => {
                                text.push('\\');
                                text.push(other);
                            }
                            None => {
                                return Err(LexError {
                                    message: format!(
                                        "this string is never closed (missing {quote})"
                                    ),
                                    pos,
                                })
                            }
                        }
                        j += 2;
                        continue;
                    }
                    text.push(ch);
                    j += 1;
                }
                out.push((pos, Tok::Str(text)));
                i = j;
            }
            c if c.is_ascii_digit() => {
                let mut j = i;
                while bytes.get(j).is_some_and(|b| b.1.is_ascii_digit()) {
                    j += 1;
                }
                let mut is_float = false;
                // A dot followed by a digit continues the number; a dot followed by anything else
                // is not part of it (`1.` is not a number).
                if bytes.get(j).map(|b| b.1) == Some('.')
                    && bytes.get(j + 1).is_some_and(|b| b.1.is_ascii_digit())
                {
                    is_float = true;
                    j += 1;
                    while bytes.get(j).is_some_and(|b| b.1.is_ascii_digit()) {
                        j += 1;
                    }
                }
                let text: String = bytes[i..j].iter().map(|b| b.1).collect();
                if is_float {
                    match text.parse::<f64>() {
                        Ok(x) => out.push((pos, Tok::Float(x))),
                        Err(_) => {
                            return Err(LexError {
                                message: format!("`{text}` is not a number"),
                                pos,
                            })
                        }
                    }
                } else {
                    match text.parse::<i64>() {
                        Ok(n) => out.push((pos, Tok::Int(n))),
                        Err(_) => {
                            return Err(LexError {
                                message: format!("`{text}` is too large to be a number"),
                                pos,
                            })
                        }
                    }
                }
                i = j;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut j = i;
                while bytes
                    .get(j)
                    .is_some_and(|b| b.1.is_ascii_alphanumeric() || b.1 == '_')
                {
                    j += 1;
                }
                let word: String = bytes[i..j].iter().map(|b| b.1).collect();
                let tok = match word.as_str() {
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    "in" => Tok::In,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    "null" => Tok::Null,
                    _ => Tok::Ident(word),
                };
                out.push((pos, tok));
                i = j;
            }
            other => {
                return Err(LexError {
                    message: format!("unexpected character `{other}`"),
                    pos,
                })
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|(_, t)| t).collect()
    }

    #[test]
    fn operators_keywords_and_names() {
        assert_eq!(
            toks("a.b == 1 and not c != 2.5 or d in [1, 'x']"),
            vec![
                Tok::Ident("a".into()),
                Tok::Dot,
                Tok::Ident("b".into()),
                Tok::EqEq,
                Tok::Int(1),
                Tok::And,
                Tok::Not,
                Tok::Ident("c".into()),
                Tok::NotEq,
                Tok::Float(2.5),
                Tok::Or,
                Tok::Ident("d".into()),
                Tok::In,
                Tok::LBracket,
                Tok::Int(1),
                Tok::Comma,
                Tok::Str("x".into()),
                Tok::RBracket,
            ]
        );
        assert_eq!(
            toks("< <= > >= - ( ) true false null"),
            vec![
                Tok::Lt,
                Tok::Le,
                Tok::Gt,
                Tok::Ge,
                Tok::Minus,
                Tok::LParen,
                Tok::RParen,
                Tok::True,
                Tok::False,
                Tok::Null
            ]
        );
    }

    #[test]
    fn offsets_point_at_the_token() {
        let t = lex("ab == 'x'").unwrap();
        assert_eq!(t.iter().map(|(p, _)| *p).collect::<Vec<_>>(), vec![0, 3, 6]);
    }

    #[test]
    fn strings_keep_backslashes_that_are_not_escapes() {
        assert_eq!(toks(r#""^hf\.co/""#), vec![Tok::Str(r"^hf\.co/".into())]);
        assert_eq!(toks(r"'it\'s'"), vec![Tok::Str("it's".into())]);
        assert_eq!(toks(r#""a\\b""#), vec![Tok::Str(r"a\b".into())]);
        assert_eq!(toks(r"'a\nb'"), vec![Tok::Str("a\nb".into())]);
        assert_eq!(toks("'--video-fps'"), vec![Tok::Str("--video-fps".into())]);
    }

    #[test]
    fn a_trailing_dot_is_not_part_of_a_number() {
        assert_eq!(toks("1."), vec![Tok::Int(1), Tok::Dot]);
        assert_eq!(toks("1.5"), vec![Tok::Float(1.5)]);
        assert_eq!(
            toks("10.20.30"),
            vec![Tok::Float(10.20), Tok::Dot, Tok::Int(30)]
        );
    }

    #[test]
    fn errors_say_what_and_where() {
        let e = lex("a = 1").unwrap_err();
        assert_eq!(e.pos, 2);
        assert!(e.message.contains("=="), "{}", e.message);
        let e = lex("!a").unwrap_err();
        assert!(e.message.contains("not"), "{}", e.message);
        let e = lex("'open").unwrap_err();
        assert_eq!(
            (e.pos, e.message.as_str()),
            (0, "this string is never closed (missing ')")
        );
        assert!(lex("a $ b").unwrap_err().message.contains('$'));
        assert!(lex("99999999999999999999")
            .unwrap_err()
            .message
            .contains("too large"));
        assert!(lex("'a\\").is_err());
    }
}
