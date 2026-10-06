//! Merging two versions of the same document (docs/architecture.md, section 5.2).
//!
//! Tables merge key by key. Arrays replace, except arrays of tables in which every element has a
//! string `id`: those merge by `id`, so a user can add or change one field of the llama.cpp engine
//! without copying the whole file.

use toml::{Table, Value};

/// Merges `over` into `base`: what `over` says wins, what it leaves out stays.
pub fn merge_tables(base: &mut Table, over: Table) {
    for (key, new) in over {
        match base.get_mut(&key) {
            Some(old) => merge_values(old, new),
            None => {
                base.insert(key, new);
            }
        }
    }
}

fn merge_values(old: &mut Value, new: Value) {
    match (old, new) {
        (Value::Table(old), Value::Table(new)) => merge_tables(old, new),
        (Value::Array(old), Value::Array(new)) if mergeable_by_id(old) && mergeable_by_id(&new) => {
            for element in new {
                let id = id_of(&element).map(str::to_string);
                match id.and_then(|id| old.iter_mut().find(|e| id_of(e) == Some(id.as_str()))) {
                    Some(existing) => merge_values(existing, element),
                    None => old.push(element),
                }
            }
        }
        (old, new) => *old = new,
    }
}

fn id_of(value: &Value) -> Option<&str> {
    value.get("id")?.as_str()
}

/// A non-empty array whose every element is a table with a string `id`. (An empty array replaces,
/// so a document can clear a list.)
fn mergeable_by_id(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(|e| e.is_table() && id_of(e).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    fn merged(base: &str, over: &str) -> Table {
        let mut b = t(base);
        merge_tables(&mut b, t(over));
        b
    }

    #[test]
    fn scalars_are_replaced_and_untouched_keys_stay() {
        let m = merged("a = 1\nb = 2\n", "b = 3\nc = 4\n");
        assert_eq!(m, t("a = 1\nb = 3\nc = 4\n"));
    }

    #[test]
    fn tables_merge_key_by_key() {
        let m = merged(
            "[launch.spawn]\nprogram = 'p'\nargs = ['x']\n[launch.ready]\ntimeout = '1s'\n",
            "[launch.spawn]\nprogram = 'q'\n",
        );
        assert_eq!(
            m,
            t("[launch.spawn]\nprogram = 'q'\nargs = ['x']\n[launch.ready]\ntimeout = '1s'\n")
        );
    }

    #[test]
    fn plain_arrays_replace() {
        let m = merged("args = ['a', 'b']\n", "args = ['c']\n");
        assert_eq!(m, t("args = ['c']\n"));
        // an empty array clears
        let m = merged("args = ['a']\n", "args = []\n");
        assert_eq!(m, t("args = []\n"));
    }

    #[test]
    fn arrays_of_tables_with_ids_merge_by_id() {
        let m = merged(
            "[[field]]\nid = 'port'\ndefault = 8080\n[[field]]\nid = 'ctx'\ndefault = 4096\nrange = [1, 2]\n",
            "[[field]]\nid = 'ctx'\ndefault = 8192\n[[field]]\nid = 'extra'\ndefault = 1\n",
        );
        assert_eq!(
            m,
            t("[[field]]\nid = 'port'\ndefault = 8080\n[[field]]\nid = 'ctx'\ndefault = 8192\nrange = [1, 2]\n[[field]]\nid = 'extra'\ndefault = 1\n")
        );
    }

    #[test]
    fn arrays_of_tables_without_ids_replace() {
        let m = merged(
            "[[env]]\nname = 'A'\n[[env]]\nname = 'B'\n",
            "[[env]]\nname = 'C'\n",
        );
        assert_eq!(m, t("[[env]]\nname = 'C'\n"));
    }

    #[test]
    fn a_table_replacing_a_scalar_or_the_reverse_replaces() {
        let m = merged("a = 1\n", "[a]\nb = 2\n");
        assert_eq!(m, t("[a]\nb = 2\n"));
        let m = merged("[a]\nb = 2\n", "a = 1\n");
        assert_eq!(m, t("a = 1\n"));
    }

    #[test]
    fn merging_by_id_reaches_into_the_elements() {
        let m = merged(
            "[[field]]\nid = 'ctx'\n[field.bind]\narg = ['--ctx-size', '${value}']\norder = 5\n",
            "[[field]]\nid = 'ctx'\n[field.bind]\norder = 7\n",
        );
        assert_eq!(
            m,
            t("[[field]]\nid = 'ctx'\n[field.bind]\narg = ['--ctx-size', '${value}']\norder = 7\n")
        );
    }
}
