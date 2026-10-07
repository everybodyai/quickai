//! The person's extra environment, as a text field of `KEY=VALUE` lines (docs/phase3-test-plan.md,
//! group E; plan decision C31). The engine's own variables come first and the protected ones
//! (`OLLAMA_HOST`, `OLLAMA_MODELS`, `OLLAMA_NO_CLOUD`, and whatever else the document locks) can
//! never be overridden from here: a line naming one is a problem shown under the field, and is not
//! passed on.
//!
//! This is the reading of the text and nothing else; what the lines mean for an engine (which names
//! are protected, where they go among the other variables) is `plan::env`'s.

/// A line that was not used, with why, in words for the person who typed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvProblem {
    /// Counted from 1, blank lines and comments included, as an editor counts them.
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KvLines {
    /// In the order typed; a later line for the same name replaces an earlier one in place.
    pub entries: Vec<(String, String)>,
    pub problems: Vec<KvProblem>,
}

impl KvLines {
    /// The problems as the message under the field, one sentence on each line of the text:
    /// `Line 2: ...` and then, on the next line, `Line 3: ...`. What shows them can put each under
    /// the text box without reading the sentences.
    pub fn problems_text(&self) -> String {
        self.problems
            .iter()
            .map(|p| format!("Line {}: {}.", p.line, p.message))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Reads `text`. Blank lines and lines starting with `#` are ignored; the value is everything
/// after the first `=`, with surrounding spaces trimmed (quotes are part of the value: this is not
/// a shell); a name is `[A-Za-z_][A-Za-z0-9_]*`. `protected` names are refused (compared without
/// regard to case when `ignore_case` is set, which is how Windows treats them).
pub fn parse(text: &str, protected: &[&str], ignore_case: bool) -> KvLines {
    parse_with(text, protected, ignore_case, ignore_case)
}

/// `parse` with the two uses of case told apart: a protected name is refused whatever its case when
/// `protected_ignore_case` is set (a lower-case spelling of one does nothing to a program on Linux,
/// and nobody who types it means well), and two names that differ only in case are one variable
/// when `names_ignore_case` is set (Windows), so the later line replaces the earlier.
pub fn parse_with(
    text: &str,
    protected: &[&str],
    protected_ignore_case: bool,
    names_ignore_case: bool,
) -> KvLines {
    let same_as = |ignore_case: bool| {
        move |a: &str, b: &str| {
            if ignore_case {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        }
    };
    let (same_protected, same_name) = (same_as(protected_ignore_case), same_as(names_ignore_case));
    let mut out = KvLines::default();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut problem = |message: String| out.problems.push(KvProblem { line, message });
        let Some((name, value)) = trimmed.split_once('=') else {
            problem(format!("`{trimmed}` is not NAME=VALUE"));
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            problem("there is no name before the =".to_string());
            continue;
        }
        if !is_name(name) {
            problem(format!(
                "`{name}` is not a name an environment variable can have (letters, digits and _, and not a digit first)"
            ));
            continue;
        }
        if let Some(own) = protected.iter().find(|p| same_protected(p, name)) {
            problem(if *own == name {
                format!("{name} is set by QuickAI and cannot be changed here")
            } else {
                format!("{name} is {own}, which is set by QuickAI and cannot be changed here")
            });
            continue;
        }
        match out
            .entries
            .iter_mut()
            .find(|(known, _)| same_name(known, name))
        {
            Some(entry) => entry.1 = value.to_string(),
            None => out.entries.push((name.to_string(), value.to_string())),
        }
    }
    out
}

/// A name an environment variable can have in a line of this field.
pub fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(text: &str) -> Vec<(String, String)> {
        parse(text, &[], false).entries
    }

    #[test]
    fn a_value_is_everything_after_the_first_equals_sign_trimmed_and_quotes_are_part_of_it() {
        assert_eq!(
            entries("A==b\nB = \"quoted value\" \nC=\tx y\t\n"),
            [
                ("A".to_string(), "=b".to_string()),
                ("B".to_string(), "\"quoted value\"".to_string()),
                ("C".to_string(), "x y".to_string()),
            ]
        );
    }

    #[test]
    fn an_indented_comment_is_a_comment_and_a_hash_inside_a_value_is_not() {
        assert_eq!(
            entries("   # not a variable=1\nURL=http://h/#frag"),
            [("URL".to_string(), "http://h/#frag".to_string())]
        );
    }

    #[test]
    fn names_are_ascii_letters_digits_and_underscores_and_not_a_digit_first() {
        for good in ["A", "_", "a1", "OLLAMA_NUM_PARALLEL", "_9"] {
            assert!(is_name(good), "{good}");
        }
        for bad in ["", "1A", "A-B", "A B", "A.B", "NÅME", "A=B"] {
            assert!(!is_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_problems_read_as_one_text_for_the_field_a_sentence_to_a_line() {
        let out = parse("fine=1\nnope\n=x", &[], false);
        assert_eq!(
            out.problems_text(),
            "Line 2: `nope` is not NAME=VALUE.\nLine 3: there is no name before the =."
        );
        assert_eq!(parse("fine=1", &[], false).problems_text(), "");
    }

    #[test]
    fn a_name_that_is_protected_in_another_spelling_says_which_one_it_is() {
        let out = parse("ollama_host=x", &["OLLAMA_HOST"], true);
        assert_eq!(
            out.problems[0].message,
            "ollama_host is OLLAMA_HOST, which is set by QuickAI and cannot be changed here"
        );
        assert!(out.entries.is_empty());
    }
}
