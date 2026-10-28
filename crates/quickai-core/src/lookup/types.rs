//! Model types and size classes: how a look-up sorts what it found (docs/lookup-and-bundles.md,
//! section 3). Both are data of the vocabulary (`[[model_type]]`, `[[size_class]]`); this is the pure
//! reading of them.

/// A kind of model a look-up sorts into: Chat, Code, Vision, Embedding, ...
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDef {
    pub id: String,
    pub label: String,
    pub help: String,
    /// Where its section stands: lower first, ties in the order of the document.
    pub order: i64,
    /// What a model is when no other type matches it. At most one type is.
    pub default: bool,
    pub rule: TypeMatch,
}

/// Something a person can want a model for: Chat, Write code, Read pictures, ...
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskDef {
    pub id: String,
    pub label: String,
    pub help: String,
    /// Where its button stands: lower first, ties in the order of the document.
    pub order: i64,
    /// Ids of model types; the first is the task's own.
    pub types: Vec<String>,
    /// Ids of modalities the model reads.
    pub input: Vec<String>,
    /// Features of a model of the list.
    pub features: Vec<String>,
}

impl TaskDef {
    /// Whether a model of `kind` that reads `input` and has `features` does it by the task's rule.
    pub fn holds(&self, kind: &str, input: &[String], features: &[String]) -> bool {
        self.types.iter().any(|t| t == kind)
            || self.input.iter().any(|i| input.contains(i))
            || self.features.iter().any(|f| features.contains(f))
    }

    /// The task's own type, whose models come first among the ones that do it.
    pub fn home(&self) -> Option<&str> {
        self.types.first().map(String::as_str)
    }
}

/// What makes a model one of a type. Every list is lower case; the rule holds when any list has a hit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeMatch {
    /// What a source says the model can do (an Ollama card's `vision`, `embedding`).
    pub capabilities: Vec<String>,
    /// The pipeline tag of a Hugging Face repository (`image-text-to-text`).
    pub pipelines: Vec<String>,
    /// Another tag of the repository.
    pub tags: Vec<String>,
    /// A word of the model's name (see `word_in`).
    pub words: Vec<String>,
    /// A kind of companion file found beside the model (`mmproj`).
    pub companions: Vec<String>,
}

impl TypeMatch {
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
            && self.pipelines.is_empty()
            && self.tags.is_empty()
            && self.words.is_empty()
            && self.companions.is_empty()
    }
}

/// What a source says about a model, which is what its type is decided by.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts<'a> {
    pub capabilities: &'a [String],
    pub pipeline: Option<&'a str>,
    pub tags: &'a [String],
    pub name: &'a str,
    pub companions: &'a [String],
}

/// The type of a model: the first of `types` (in the order of the document) other than the default
/// whose rule holds, else the default, else `None` (a vocabulary that has no types at all).
pub fn classify<'a>(types: &'a [TypeDef], facts: &Facts<'_>) -> Option<&'a TypeDef> {
    types
        .iter()
        .filter(|t| !t.default)
        .find(|t| holds(&t.rule, facts))
        .or_else(|| types.iter().find(|t| t.default))
}

/// Whether the rule holds for these facts. An empty rule holds for nothing (a type that wants to
/// catch the rest says `default`).
pub fn holds(rule: &TypeMatch, facts: &Facts<'_>) -> bool {
    let has = |list: &[String], wanted: &str| list.iter().any(|x| x.eq_ignore_ascii_case(wanted));
    if facts
        .capabilities
        .iter()
        .any(|c| has(&rule.capabilities, c))
    {
        return true;
    }
    if facts.pipeline.is_some_and(|p| has(&rule.pipelines, p)) {
        return true;
    }
    if facts.tags.iter().any(|t| has(&rule.tags, t)) {
        return true;
    }
    if facts.companions.iter().any(|c| has(&rule.companions, c)) {
        return true;
    }
    if !rule.words.is_empty() {
        let tokens = words_of(facts.name);
        return rule.words.iter().any(|w| word_in(w, &tokens));
    }
    false
}

/// The words of a name, lower case: `unsloth/Qwen2.5-VL-7B-Instruct-GGUF` is `unsloth`, `qwen2`, `5`,
/// `vl`, `7b`, `instruct`, `gguf`.
pub fn words_of(name: &str) -> Vec<String> {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `word` is one of `tokens`; a word of four letters or more also matches the start of one
/// (`embed` is in `embeddinggemma`, `code` in `codestral`), a shorter one only itself (`vl` is not in
/// `vllm`).
pub fn word_in(word: &str, tokens: &[String]) -> bool {
    let word = word.to_lowercase();
    !word.is_empty()
        && tokens
            .iter()
            .any(|t| *t == word || (word.chars().count() >= 4 && t.starts_with(&word)))
}

/// The types in the order their sections are shown.
pub fn in_display_order(types: &[TypeDef]) -> Vec<&TypeDef> {
    let mut shown: Vec<(usize, &TypeDef)> = types.iter().enumerate().collect();
    shown.sort_by_key(|(index, t)| (t.order, *index));
    shown.into_iter().map(|(_, t)| t).collect()
}

/// A band of sizes: Small, Medium, Large.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizeClassDef {
    pub id: String,
    pub label: String,
    pub help: String,
    /// Sizes below this many bytes are in the class (and not in one of a lower bound); `None` is
    /// the class of everything larger.
    pub below: Option<u64>,
}

/// The class a size is in: the first, from the smallest bound up, that it is below; the class with
/// no bound takes the rest. `None` when there are no classes, or none that is open and the size is
/// beyond the last bound.
pub fn size_class(classes: &[SizeClassDef], bytes: u64) -> Option<&SizeClassDef> {
    let mut bounded: Vec<&SizeClassDef> = classes.iter().filter(|c| c.below.is_some()).collect();
    bounded.sort_by_key(|c| c.below);
    bounded
        .into_iter()
        .find(|c| c.below.is_some_and(|below| bytes < below))
        .or_else(|| classes.iter().find(|c| c.below.is_none()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_words_and_a_long_word_matches_a_start() {
        let tokens = words_of("unsloth/Qwen2.5-VL-7B-Instruct-GGUF");
        assert_eq!(
            tokens,
            ["unsloth", "qwen2", "5", "vl", "7b", "instruct", "gguf"]
        );
        assert!(word_in("vl", &tokens));
        assert!(word_in("VL", &tokens));
        assert!(word_in("instr", &tokens), "five letters match a start");
        assert!(
            !word_in("ins", &tokens),
            "three letters match whole words only"
        );
        assert!(!word_in("", &tokens));
        assert!(!word_in("v", &tokens));
        assert!(!word_in("vl", &words_of("vllm-thing")));
        assert!(word_in("embed", &words_of("embeddinggemma-2")));
        assert!(word_in("code", &words_of("Qwen3-Coder-30B")));
    }

    #[test]
    fn sizes_fall_in_the_class_they_are_below() {
        let class = |id: &str, below: Option<u64>| SizeClassDef {
            id: id.into(),
            label: id.into(),
            help: String::new(),
            below,
        };
        // given out of order: the order of the document does not matter
        let classes = [
            class("large", None),
            class("medium", Some(15)),
            class("small", Some(4)),
        ];
        let of = |bytes| size_class(&classes, bytes).map(|c| c.id.as_str());
        assert_eq!(of(0), Some("small"));
        assert_eq!(of(3), Some("small"));
        assert_eq!(
            of(4),
            Some("medium"),
            "the bound itself is in the next class"
        );
        assert_eq!(of(14), Some("medium"));
        assert_eq!(of(15), Some("large"));
        assert_eq!(of(u64::MAX), Some("large"));
        assert_eq!(size_class(&[], 5), None);
        // no open class: beyond the last bound is nothing
        assert_eq!(size_class(&[class("small", Some(4))], 9), None);
    }
}
