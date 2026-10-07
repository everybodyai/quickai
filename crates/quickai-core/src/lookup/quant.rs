//! The quantization a GGUF file says it is (`Q4_K_M`, `UD-Q4_K_M`, `BF16`), read from its name, and
//! how a list of preferences ranks one.

use std::sync::OnceLock;

use regex::Regex;

fn pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)(?:^|[^a-z0-9])((?:ud-)?(?:iq\d(?:_[a-z0-9]+)*|q\d(?:_[a-z0-9]+)*|bf16|f16|f32|mxfp4(?:_moe)?|nvfp4|tq\d_\d))(?:$|[^a-z0-9_])",
        )
        .expect("the quantization pattern is valid")
    })
}

/// The quantization named by a file, upper case: `gemma-3-4b-it-Q4_K_M.gguf` is `Q4_K_M`,
/// `mmproj-model-f16.gguf` is `F16`, `Qwen3-27B-UD-Q4_K_M.gguf` is `UD-Q4_K_M`. `None` for a name
/// that says none.
pub fn label(file_name: &str) -> Option<String> {
    let stem = match file_name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("gguf") => stem,
        _ => file_name,
    };
    pattern()
        .captures(stem)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_ascii_uppercase())
}

/// Whether a label is what a preference asks for: the same, or a variant of it that a prefix adds
/// (`UD-Q4_K_M` is a `Q4_K_M`). `BF16` is not an `F16`.
pub fn matches(label: &str, preference: &str) -> bool {
    let (label, preference) = (label.to_ascii_uppercase(), preference.to_ascii_uppercase());
    label == preference || label.ends_with(&format!("-{preference}"))
}

/// How well a file's name matches the list: the place of the first preference it satisfies, and
/// `usize::MAX` when none, or no quantization is named.
pub fn rank(file_name: &str, prefer: &[String]) -> usize {
    let Some(label) = label(file_name) else {
        return usize::MAX;
    };
    prefer
        .iter()
        .position(|p| matches(&label, p))
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quantization_is_read_from_the_name() {
        for (file, expected) in [
            ("gemma-3-4b-it-Q4_K_M.gguf", Some("Q4_K_M")),
            ("gemma-3-4b-it-Q8_0.gguf", Some("Q8_0")),
            ("gemma-3-4b-it-f16.gguf", Some("F16")),
            ("mmproj-model-f16.gguf", Some("F16")),
            ("mmproj-BF16.gguf", Some("BF16")),
            ("Qwen3.8-27B-UD-Q4_K_M.gguf", Some("UD-Q4_K_M")),
            ("Qwen3.8-27B-UD-IQ1_M.gguf", Some("UD-IQ1_M")),
            ("Qwen3.8-27B-UD-Q2_K_XL.gguf", Some("UD-Q2_K_XL")),
            ("model.IQ3_XXS.gguf", Some("IQ3_XXS")),
            ("model_Q5_K_S.gguf", Some("Q5_K_S")),
            ("model-Q4_0_4_8.gguf", Some("Q4_0_4_8")),
            ("gpt-oss-20b-MXFP4.gguf", Some("MXFP4")),
            ("model-Q8_0-00001-of-00002.gguf", Some("Q8_0")),
            ("Qwen3-4B.gguf", None),
            ("imatrix_unsloth.gguf", None),
            ("Qwen3-4B", None),
            ("", None),
            // a q in a word is not a quantization
            ("quantized-model.gguf", None),
            ("Qwen3.gguf", None),
        ] {
            assert_eq!(label(file).as_deref(), expected, "{file}");
        }
    }

    #[test]
    fn a_preference_matches_a_variant_of_it_and_not_a_cousin() {
        assert!(matches("Q4_K_M", "q4_k_m"));
        assert!(matches("UD-Q4_K_M", "Q4_K_M"));
        assert!(!matches("BF16", "F16"));
        assert!(!matches("Q4_K_M_X", "Q4_K_M"));
        assert!(!matches("Q4_K_S", "Q4_K_M"));
    }

    #[test]
    fn files_are_ranked_by_the_place_of_the_first_preference_they_meet() {
        let prefer: Vec<String> = ["Q4_K_M", "Q5_K_M", "Q8_0"].map(String::from).to_vec();
        assert_eq!(rank("m-Q4_K_M.gguf", &prefer), 0);
        assert_eq!(rank("m-UD-Q4_K_M.gguf", &prefer), 0);
        assert_eq!(rank("m-Q8_0.gguf", &prefer), 2);
        assert_eq!(rank("m-IQ1_S.gguf", &prefer), usize::MAX);
        assert_eq!(rank("m.gguf", &prefer), usize::MAX);
        assert_eq!(rank("m-Q4_K_M.gguf", &[]), usize::MAX);
    }
}
