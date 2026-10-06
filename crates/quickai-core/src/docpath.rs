//! Names for places inside a document: `field[ctx_size].bind.arg`.
//!
//! Diagnostics point at a value by path, and the path has to mean the same thing in the merged
//! document and in each file that contributed to it. An element of an array of tables that has an
//! `id` is therefore named by that id (`field[ctx_size]`), not by its position, which differs from
//! one file to the next.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    /// A key of a table.
    Key(String),
    /// The element of an array of tables whose `id` is this.
    Id(String),
    /// The element at this position of an array.
    Index(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DocPath(pub Vec<Seg>);

impl DocPath {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, seg: Seg) {
        self.0.push(seg);
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.0.push(Seg::Key(key.into()));
        self
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.0.push(Seg::Id(id.into()));
        self
    }

    /// The element at position `n` of an array that has no ids (`env[0]`).
    pub fn index(mut self, n: usize) -> Self {
        self.0.push(Seg::Index(n));
        self
    }

    /// Names the places on the way down `root` to `raw` (a path as `serde_ignored` or
    /// `serde_path_to_error` report it: keys and positions), using ids for the elements of arrays
    /// of tables that have one.
    pub fn from_raw(root: &toml::Value, raw: &[RawSeg]) -> DocPath {
        let mut here = Some(root);
        let mut out = Vec::new();
        for seg in raw {
            match seg {
                RawSeg::Key(key) => {
                    out.push(Seg::Key(key.clone()));
                    here = here.and_then(|v| v.get(key.as_str()));
                }
                RawSeg::Index(index) => {
                    let element = here.and_then(|v| v.as_array()).and_then(|a| a.get(*index));
                    match element
                        .and_then(|e| e.get("id"))
                        .and_then(toml::Value::as_str)
                    {
                        Some(id) => out.push(Seg::Id(id.to_string())),
                        None => out.push(Seg::Index(*index)),
                    }
                    here = element;
                }
            }
        }
        DocPath(out)
    }
}

/// A step of a path as the serde helper crates report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawSeg {
    Key(String),
    Index(usize),
}

impl fmt::Display for DocPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            match seg {
                Seg::Key(k) => {
                    if i > 0 {
                        f.write_str(".")?;
                    }
                    f.write_str(k)?;
                }
                Seg::Id(id) => write!(f, "[{id}]")?,
                Seg::Index(n) => write!(f, "[{n}]")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> toml::Value {
        toml::from_str(
            r#"
            [launch.spawn]
            args = ["a"]

            [[field]]
            id = "port"
            [[field]]
            id = "ctx_size"
            [field.bind]
            arg = ["--ctx-size"]

            [[plain]]
            x = 1
            "#,
        )
        .unwrap()
    }

    fn raw(parts: &[&str]) -> Vec<RawSeg> {
        parts
            .iter()
            .map(|p| match p.parse::<usize>() {
                Ok(n) => RawSeg::Index(n),
                Err(_) => RawSeg::Key(p.to_string()),
            })
            .collect()
    }

    #[test]
    fn elements_with_an_id_are_named_by_it() {
        let path = DocPath::from_raw(&doc(), &raw(&["field", "1", "bind", "arg"]));
        assert_eq!(path.to_string(), "field[ctx_size].bind.arg");
        assert_eq!(
            path.0,
            vec![
                Seg::Key("field".into()),
                Seg::Id("ctx_size".into()),
                Seg::Key("bind".into()),
                Seg::Key("arg".into())
            ]
        );
    }

    #[test]
    fn elements_without_an_id_keep_their_position() {
        let path = DocPath::from_raw(&doc(), &raw(&["plain", "0", "x"]));
        assert_eq!(path.to_string(), "plain[0].x");
        let path = DocPath::from_raw(&doc(), &raw(&["launch", "spawn", "args", "0"]));
        assert_eq!(path.to_string(), "launch.spawn.args[0]");
    }

    #[test]
    fn a_path_that_leaves_the_document_is_still_named() {
        let path = DocPath::from_raw(&doc(), &raw(&["nothing", "here"]));
        assert_eq!(path.to_string(), "nothing.here");
        assert!(DocPath::default().is_empty());
        assert_eq!(
            DocPath::default().key("a").id("b").key("c").to_string(),
            "a[b].c"
        );
    }
}
