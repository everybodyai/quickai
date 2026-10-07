//! The file-name patterns of the documents (`match = ["mmproj*.gguf"]`): `*` stands for any run of
//! characters (none included), `?` for exactly one, and nothing else is special. Case does not matter:
//! a repository that calls its projector `MMPROJ-F16.gguf` is the same as one that says `mmproj-f16.gguf`.

/// Whether `pattern` matches the whole of `text`.
pub fn matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    // the usual walk with one place to go back to: the last `*` and how much of the text it has eaten
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((star_at, eaten)) = star {
            p = star_at + 1;
            t = eaten + 1;
            star = Some((star_at, eaten + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// Whether the pattern matches a file given by its path in a repository: the whole path, or its last
/// component (`MTP/mtp-x.gguf` is matched by `mtp-*.gguf` and by `MTP/*`).
pub fn matches_path(pattern: &str, path: &str) -> bool {
    matches(pattern, path) || matches(pattern, name_of(path))
}

/// Whether any of the patterns matches the path.
pub fn matches_any<S: AsRef<str>>(patterns: &[S], path: &str) -> bool {
    patterns.iter().any(|p| matches_path(p.as_ref(), path))
}

/// The last component of a path written with `/` or `\`.
pub fn name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_is_any_run_and_a_question_mark_is_one_character() {
        for (pattern, text, expected) in [
            ("mmproj*.gguf", "mmproj-F16.gguf", true),
            ("mmproj*.gguf", "mmproj.gguf", true),
            ("mmproj*.gguf", "model-mmproj.gguf", false),
            ("*mmproj*.gguf", "gemma-mmproj-f16.gguf", true),
            ("*", "", true),
            ("*", "anything at all", true),
            ("", "", true),
            ("", "x", false),
            ("a?c", "abc", true),
            ("a?c", "ac", false),
            ("a?c", "abbc", false),
            ("*-of-*", "model-00001-of-00004.gguf", true),
            ("a*b*c", "a--b--c", true),
            ("a*b*c", "a--c--b", false),
            ("**x", "yx", true),
            ("x.gguf", "xagguf", false),
        ] {
            assert_eq!(matches(pattern, text), expected, "{pattern:?} on {text:?}");
        }
    }

    #[test]
    fn case_does_not_matter() {
        assert!(matches("mmproj*.GGUF", "MMPROJ-f16.gguf"));
        assert!(matches("Q4_K_M", "q4_k_m"));
    }

    #[test]
    fn a_path_is_matched_whole_or_by_its_last_component() {
        assert!(matches_path("mtp-*.gguf", "MTP/mtp-Qwen-Q4_0.gguf"));
        assert!(matches_path("mtp/*", "MTP/mtp-Qwen-Q4_0.gguf"));
        assert!(!matches_path("mmproj*", "MTP/mtp-Qwen-Q4_0.gguf"));
        assert_eq!(name_of("a/b\\c.gguf"), "c.gguf");
        assert_eq!(name_of("c.gguf"), "c.gguf");
    }

    #[test]
    fn a_hostile_pattern_is_not_slow() {
        // the walk goes back to one place only, so many stars do not blow up
        let pattern = "a*".repeat(200) + "b";
        let text = "a".repeat(2000);
        assert!(!matches(&pattern, &text));
    }
}
