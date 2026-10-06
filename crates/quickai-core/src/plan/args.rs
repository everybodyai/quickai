//! Splitting a line of arguments the way a text field means it.

/// Minimal shell-like split for a user-supplied "extra arguments" field:
/// splits on whitespace, except text wrapped in matching `"..."`/`'...'` is
/// kept as one argument (so a path containing spaces can be passed through).
/// No escape-sequence support — good enough for llama-server flags/values,
/// not a general shell parser. An empty line gives no arguments.
pub fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;
    for c in input.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_token = true;
            }
            None if c.is_whitespace() => {
                if in_token {
                    args.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            None => {
                current.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args("   "), Vec::<String>::new());
        assert_eq!(
            split_args("--threads 8   --flash-attn on"),
            vec!["--threads", "8", "--flash-attn", "on"]
        );
    }

    #[test]
    fn keeps_quoted_spaces_together() {
        assert_eq!(
            split_args(r#"--lora "/path/with spaces/adapter.gguf" --threads 4"#),
            vec!["--lora", "/path/with spaces/adapter.gguf", "--threads", "4"]
        );
        assert_eq!(split_args("'single quoted'"), vec!["single quoted"]);
    }

    #[test]
    fn an_empty_quoted_argument_is_an_argument() {
        assert_eq!(
            split_args(r#"--chat-template """#),
            vec!["--chat-template", ""]
        );
    }

    #[test]
    fn an_unclosed_quote_runs_to_the_end() {
        assert_eq!(split_args(r#"--a "b c"#), vec!["--a", "b c"]);
    }
}
