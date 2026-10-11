//! Looking at the free text of a setting before it is taken from a file, or given away in one
//! (docs/configurations.md, section 8): the lines of environment a file may not set, the folders it
//! names, and the words and lines that look like secrets. Pure text work, shared by applying a
//! configuration, checking one and exporting one.

use crate::plan::args::split_args;
use crate::plan::env::protected_names;
use crate::plan::is_secret_name;
use crate::profile::notes::env_name_denied;
use crate::registry::Engine;

/// A text after it was looked at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screened {
    /// What is left of it: the lines that were refused are gone, the rest is as it was written.
    pub text: String,
    /// The names of the variables whose lines were refused, in the order they stood.
    pub refused: Vec<String>,
}

/// The name of the variable a line of `KEY=VALUE` sets, when it is such a line.
fn line_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    trimmed.split_once('=').map(|(name, _)| name.trim())
}

/// `text` as lines of environment, without the lines that set a name a file may not set (the
/// vocabulary's `deny` and the code's own floor) or a name the engine protects (`protect`, and what
/// its document locks): a configuration from outside must not make a program load other code or take
/// away the isolation an engine runs in.
pub fn env_lines(engine: &Engine, protect: &[String], text: &str, deny: &[String]) -> Screened {
    let protected = protected_names(engine, protect);
    let mut kept: Vec<&str> = Vec::new();
    let mut refused = Vec::new();
    for line in text.lines() {
        match line_name(line) {
            Some(name)
                if env_name_denied(name, deny)
                    || protected.iter().any(|p| p.eq_ignore_ascii_case(name)) =>
            {
                refused.push(name.to_string());
            }
            _ => kept.push(line),
        }
    }
    Screened {
        text: kept.join("\n"),
        refused,
    }
}

/// `text` as lines of environment without the lines whose name looks like a secret (`HF_TOKEN`,
/// `OLLAMA_API_KEY`), and how many there were.
pub fn drop_secret_lines(text: &str) -> (String, usize) {
    let mut kept: Vec<&str> = Vec::new();
    let mut dropped = 0;
    for line in text.lines() {
        match line_name(line) {
            Some(name) if is_secret_name(name) => dropped += 1,
            _ => kept.push(line),
        }
    }
    (kept.join("\n"), dropped)
}

/// `text` as command-line words without the flags that look like secrets (`--api-key`, `--token`) and
/// the word after each, and how many flags there were. A flag written `--api-key=abc` goes whole.
pub fn drop_secret_args(text: &str) -> (String, usize) {
    let words = split_args(text);
    let mut kept: Vec<String> = Vec::new();
    let mut dropped = 0;
    let mut skip_value = false;
    for word in &words {
        if skip_value {
            skip_value = false;
            if !word.starts_with('-') {
                continue;
            }
        }
        if let Some(flag) = word.strip_prefix('-') {
            let flag = flag.trim_start_matches('-');
            let (name, has_value) = match flag.split_once('=') {
                Some((name, _)) => (name, true),
                None => (flag, false),
            };
            if is_secret_name(name) {
                dropped += 1;
                skip_value = !has_value;
                continue;
            }
        }
        kept.push(quote(word));
    }
    (kept.join(" "), dropped)
}

/// A word as it is written in a line that `split_args` reads back: quoted when it has a space or is
/// empty.
fn quote(word: &str) -> String {
    if word.is_empty() || word.contains(char::is_whitespace) {
        if word.contains('"') {
            format!("'{word}'")
        } else {
            format!("\"{word}\"")
        }
    } else {
        word.to_string()
    }
}

/// The first thing in `text` that names a folder or a file by its place on a disk: an absolute path
/// (`/home/alice/x`, `C:\Users\x`, `~/x`), as a word of its own or after a `=`. Addresses
/// (`http://host/x`) are not paths.
pub fn names_a_path(text: &str) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        // a line of environment: the value; else the words of an argument line
        let mut parts: Vec<String> = match line.split_once('=') {
            Some((name, value)) if crate::kvlines::is_name(name.trim()) => {
                vec![value.trim().to_string()]
            }
            _ => split_args(line),
        };
        // `--model=/x/y`
        let inner: Vec<String> = parts
            .iter()
            .filter_map(|p| {
                p.split_once('=')
                    .filter(|(flag, _)| flag.starts_with('-'))
                    .map(|(_, v)| v.to_string())
            })
            .collect();
        parts.extend(inner);
        candidates.extend(parts);
    }
    candidates.into_iter().find(|word| is_absolute_path(word))
}

fn is_absolute_path(word: &str) -> bool {
    let word = word.trim_matches(['"', '\'']);
    if word.contains("://") {
        return false;
    }
    if let Some(rest) = word.strip_prefix('/') {
        // `/` alone, or a query-like word, is not a place
        return !rest.is_empty() && !rest.starts_with(char::is_whitespace);
    }
    if word.starts_with("~/") || word.starts_with("\\\\") {
        return true;
    }
    let mut chars = word.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(drive), Some(':'), Some('\\' | '/')) if drive.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_lines_and_flags_are_taken_out_whole() {
        let (kept, n) = drop_secret_lines("A=1\n# note\nHF_TOKEN=abc\nOLLAMA_API_KEY = k\nB=2");
        assert_eq!(kept, "A=1\n# note\nB=2");
        assert_eq!(n, 2);

        let (kept, n) = drop_secret_args("--threads 8 --api-key hunter2 --ctx-size 4096");
        assert_eq!(kept, "--threads 8 --ctx-size 4096");
        assert_eq!(n, 1);
        let (kept, n) = drop_secret_args("--api-key=hunter2 --threads 8 --hf-token");
        assert_eq!(kept, "--threads 8");
        assert_eq!(n, 2);
        // a word with a space keeps its quotes
        let (kept, _) = drop_secret_args(r#"--chat-template "a b" --password x"#);
        assert_eq!(kept, r#"--chat-template "a b""#);
        // a flag after a secret flag is not its value
        let (kept, n) = drop_secret_args("--token --threads 8");
        assert_eq!(kept, "--threads 8");
        assert_eq!(n, 1);
    }

    #[test]
    fn a_place_on_a_disk_is_found_in_a_line_or_an_argument() {
        for text in [
            "--lora /home/alice/adapter.gguf",
            "--model=/opt/m/x.gguf",
            "HF_HOME=/home/alice/hf",
            "--log-file C:\\Users\\alice\\log.txt",
            "SOME=~/cache",
            "--a \"/path with space\"",
        ] {
            assert!(names_a_path(text).is_some(), "{text}");
        }
        for text in [
            "--threads 8 --flash-attn on",
            "OLLAMA_ORIGINS=http://localhost:3000",
            "OLLAMA_FLASH_ATTENTION=1",
            "# /home/alice/not-a-setting",
            "--path /",
            "",
        ] {
            assert_eq!(names_a_path(text), None, "{text}");
        }
    }
}
