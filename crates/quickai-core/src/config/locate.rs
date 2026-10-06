//! Finding the line of a value in the text of a document, for diagnostics.
//!
//! Loading works on parsed tables, which carry no positions. When a problem is reported, the text
//! is parsed again with `toml_edit`, which keeps them, and the path is followed down to its line
//! (docs/implementation-plan.md, C2). This costs nothing on the success path.

use toml_edit::{Document, Item, TableLike};

use crate::docpath::{DocPath, Seg};

/// 1-based line of a byte offset.
pub fn line_of_offset(text: &str, offset: usize) -> u32 {
    let end = offset.min(text.len());
    1 + text.as_bytes()[..end]
        .iter()
        .filter(|b| **b == b'\n')
        .count() as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Located {
    pub line: u32,
    /// False when only a prefix of the path was found (the value itself is missing).
    pub exact: bool,
}

/// Where `path` is in `text`: the line of the value, or of the deepest part of the path that the
/// text does contain. `None` when the text has none of it or cannot be parsed.
pub fn locate_in_text(text: &str, path: &DocPath) -> Option<Located> {
    let doc = Document::parse(text).ok()?;
    let mut here = Node::Keys(doc.as_table() as &dyn TableLike);
    let mut line = 1;
    let mut matched = 0;
    for seg in &path.0 {
        match step(&here, seg) {
            Some((next, offset)) => {
                matched += 1;
                if let Some(offset) = offset {
                    line = line_of_offset(text, offset);
                }
                here = next;
            }
            None => break,
        }
    }
    if matched == 0 && !path.is_empty() {
        return None;
    }
    Some(Located {
        line,
        exact: matched == path.0.len(),
    })
}

/// A place in the parsed text: something with keys (a table, an inline table) or any other item.
enum Node<'a> {
    Keys(&'a dyn TableLike),
    Item(&'a Item),
}

impl<'a> Node<'a> {
    fn keys(&self) -> Option<&'a dyn TableLike> {
        match self {
            Node::Keys(t) => Some(*t),
            Node::Item(i) => i.as_table_like(),
        }
    }
}

/// One step down: the next node and the byte offset to report for it.
fn step<'a>(node: &Node<'a>, seg: &Seg) -> Option<(Node<'a>, Option<usize>)> {
    match seg {
        Seg::Key(key) => {
            let (k, item) = node.keys()?.get_key_value(key)?;
            Some((Node::Item(item), k.span().map(|s| s.start)))
        }
        Seg::Id(id) => {
            let Node::Item(item) = node else { return None };
            elements(item).into_iter().find(|(element, _)| {
                element
                    .keys()
                    .and_then(|t| t.get("id"))
                    .and_then(Item::as_str)
                    == Some(id.as_str())
            })
        }
        Seg::Index(n) => {
            let Node::Item(item) = node else { return None };
            elements(item).into_iter().nth(*n)
        }
    }
}

/// The elements of an array of tables, or of an array of inline tables, with where each starts.
fn elements<'a>(item: &'a Item) -> Vec<(Node<'a>, Option<usize>)> {
    if let Some(tables) = item.as_array_of_tables() {
        return tables
            .iter()
            .map(|t| (Node::Keys(t as &dyn TableLike), t.span().map(|s| s.start)))
            .collect();
    }
    if let Some(array) = item.as_array() {
        return array
            .iter()
            .filter_map(|v| {
                let table = v.as_inline_table()?;
                Some((
                    Node::Keys(table as &dyn TableLike),
                    v.span().map(|s| s.start),
                ))
            })
            .collect();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "schema = 1\n\
kind = \"engine\"\n\
\n\
[engine]\n\
name = \"x\"\n\
\n\
[launch.spawn]\n\
program = \"p\"\n\
args = [\"--a\", \"--b\"]\n\
\n\
[[field]]\n\
id = \"port\"\n\
type = \"port\"\n\
\n\
[[field]]\n\
id = \"ctx_size\"\n\
type = \"int\"\n\
default = 4096\n\
[field.bind]\n\
arg = [\"--ctx-size\"]\n\
\n\
inline = [ { id = \"a\", v = 1 },\n\
           { id = \"b\", v = 2 } ]\n";

    fn at(path: DocPath) -> Option<Located> {
        locate_in_text(TEXT, &path)
    }

    fn key(path: &str) -> DocPath {
        DocPath(path.split('.').map(|s| Seg::Key(s.to_string())).collect())
    }

    #[test]
    fn a_key_is_found_on_its_own_line() {
        assert_eq!(
            at(key("kind")),
            Some(Located {
                line: 2,
                exact: true
            })
        );
        assert_eq!(
            at(key("engine.name")),
            Some(Located {
                line: 5,
                exact: true
            })
        );
        assert_eq!(
            at(key("launch.spawn.program")),
            Some(Located {
                line: 8,
                exact: true
            })
        );
        assert_eq!(
            at(key("launch.spawn.args")),
            Some(Located {
                line: 9,
                exact: true
            })
        );
    }

    #[test]
    fn an_element_is_found_by_id_or_position() {
        let ctx = DocPath::default()
            .key("field")
            .id("ctx_size")
            .key("default");
        assert_eq!(
            at(ctx),
            Some(Located {
                line: 18,
                exact: true
            })
        );
        let ctx_bind = DocPath::default()
            .key("field")
            .id("ctx_size")
            .key("bind")
            .key("arg");
        assert_eq!(
            at(ctx_bind),
            Some(Located {
                line: 20,
                exact: true
            })
        );
        let by_index = DocPath(vec![
            Seg::Key("field".into()),
            Seg::Index(0),
            Seg::Key("type".into()),
        ]);
        assert_eq!(
            at(by_index),
            Some(Located {
                line: 13,
                exact: true
            })
        );
        // the element itself points at its header
        let element = DocPath::default().key("field").id("port");
        assert_eq!(
            at(element),
            Some(Located {
                line: 11,
                exact: true
            })
        );
    }

    #[test]
    fn inline_tables_in_an_array_are_found_too() {
        // (the array sits under `[field.bind]`, the last table header before it)
        let second = DocPath::default()
            .key("field")
            .id("ctx_size")
            .key("bind")
            .key("inline")
            .id("b");
        assert_eq!(
            at(second),
            Some(Located {
                line: 23,
                exact: true
            })
        );
    }

    #[test]
    fn a_missing_value_points_at_what_is_there() {
        let missing = key("launch.spawn.stop");
        assert_eq!(
            at(missing),
            Some(Located {
                line: 7,
                exact: false
            })
        );
        let missing = DocPath::default().key("field").id("ctx_size").key("range");
        let found = at(missing).unwrap();
        assert!(!found.exact);
        assert_eq!(found.line, 15, "the element's header");
        assert_eq!(at(key("nothing.at.all")), None);
        assert_eq!(
            at(DocPath::default()),
            Some(Located {
                line: 1,
                exact: true
            })
        );
    }

    #[test]
    fn unparseable_text_has_no_location() {
        assert_eq!(locate_in_text("this is = = not toml", &key("a")), None);
    }

    #[test]
    fn offsets_become_lines() {
        assert_eq!(line_of_offset("a\nb\nc", 0), 1);
        assert_eq!(line_of_offset("a\nb\nc", 2), 2);
        assert_eq!(line_of_offset("a\nb\nc", 4), 3);
        assert_eq!(line_of_offset("a\nb\nc", 999), 3);
    }
}
