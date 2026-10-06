//! The syntax tree of an expression (docs/architecture.md, section 7.1).
//!
//! The language is closed on purpose: literals, lists, dotted paths, `not`, unary minus, the binary
//! operators `and or == != < <= > >= in` and calls of named, pure functions. There is no
//! assignment, no loop, no recursion and no way to define a function, so every expression
//! terminates and every one can be checked before it is ever evaluated.

use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    NotIn,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::In => "in",
            BinOp::NotIn => "not in",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// `null`, `true`, `false`, a number or a string.
    Lit(Value),
    /// `[a, b, c]`
    List(Vec<Expr>),
    /// `model.size_bytes`: names looked up in the context, one map level per dot.
    Path(Vec<String>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `has(list, x)` or a dotted name such as `runtime.has_flag('--mmproj')`.
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

impl Expr {
    /// The dotted text of a path or call name, for messages.
    pub fn path_text(path: &[String]) -> String {
        path.join(".")
    }
}
