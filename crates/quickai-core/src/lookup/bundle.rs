//! Bundles: what is fetched together with a model's weights (docs/lookup-and-bundles.md, section 3).
//!
//! A bundle is a list of parts; a part is a kind of file the engine attaches to a model (`mmproj`, the
//! projector of a vision model) and how to find one among the files of a repository: the patterns its
//! name matches, and which of several to take. The bundle is offered for a repository when every part
//! finds a file.

use super::candidate::RemoteFile;
use super::glob;
use super::quant;

/// `[[models.bundle]]`, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    pub id: String,
    /// The words of the row of buttons: "with projector".
    pub label: String,
    pub help: String,
    pub parts: Vec<Part>,
}

/// One part of a bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The kind of component it is, which the engine's `[components.<kind>]` says how to attach.
    pub kind: String,
    /// What the window calls it: "projector".
    pub label: String,
    /// Patterns (`mmproj*.gguf`) a file's name or path matches.
    pub matches: Vec<String>,
    /// The quantizations to take first, best first (`F16`, `BF16`).
    pub prefer: Vec<String>,
}

impl Part {
    /// Whether the file is of this kind.
    pub fn is(&self, file: &RemoteFile) -> bool {
        glob::matches_any(&self.matches, &file.path)
    }

    /// The file of this kind to take: of those that are, the one whose quantization comes first in
    /// `prefer`, else the smallest, else the first.
    pub fn pick<'a>(&self, files: &'a [RemoteFile]) -> Option<&'a RemoteFile> {
        files.iter().filter(|f| self.is(f)).min_by(|a, b| {
            let key = |f: &RemoteFile| {
                (
                    quant::rank(f.name(), &self.prefer),
                    f.size.unwrap_or(u64::MAX),
                    f.path.clone(),
                )
            };
            key(a).cmp(&key(b))
        })
    }
}

/// Every pattern of every part of every bundle: what a file must match to be a companion and not
/// weights.
pub fn companion_patterns(bundles: &[Bundle]) -> Vec<String> {
    bundles
        .iter()
        .flat_map(|b| b.parts.iter().flat_map(|p| p.matches.iter().cloned()))
        .collect()
}

/// Whether a file is a companion of some bundle: not a model.
pub fn is_companion(bundles: &[Bundle], file: &RemoteFile) -> bool {
    bundles.iter().any(|b| b.parts.iter().any(|p| p.is(file)))
}

/// The kinds of companion found among the files, in the order the bundles list them, each once.
pub fn kinds_found(bundles: &[Bundle], files: &[RemoteFile]) -> Vec<String> {
    let mut kinds: Vec<String> = Vec::new();
    for part in bundles.iter().flat_map(|b| b.parts.iter()) {
        if files.iter().any(|f| part.is(f)) && !kinds.contains(&part.kind) {
            kinds.push(part.kind.clone());
        }
    }
    kinds
}

/// The files that make this bundle for these files: one per part, in the order of the parts; `None`
/// when a part finds none.
pub fn gather<'a>(
    bundle: &'a Bundle,
    files: &'a [RemoteFile],
) -> Option<Vec<(&'a Part, &'a RemoteFile)>> {
    bundle
        .parts
        .iter()
        .map(|part| part.pick(files).map(|file| (part, file)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: &str, matches: &[&str], prefer: &[&str]) -> Part {
        Part {
            kind: kind.into(),
            label: kind.into(),
            matches: matches.iter().map(|s| s.to_string()).collect(),
            prefer: prefer.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn files(list: &[(&str, u64)]) -> Vec<RemoteFile> {
        list.iter()
            .map(|(p, s)| RemoteFile::new(p, Some(*s)))
            .collect()
    }

    #[test]
    fn a_part_takes_the_preferred_quantization_and_then_the_smallest() {
        let projector = part("mmproj", &["mmproj*.gguf"], &["F16", "BF16"]);
        let repo = files(&[
            ("m-Q4_K_M.gguf", 100),
            ("mmproj-BF16.gguf", 931),
            ("mmproj-F32.gguf", 1800),
            ("mmproj-F16.gguf", 928),
        ]);
        assert_eq!(projector.pick(&repo).unwrap().path, "mmproj-F16.gguf");
        // without F16 the second preference
        let no_f16 = files(&[
            ("m-Q4_K_M.gguf", 100),
            ("mmproj-F32.gguf", 1800),
            ("mmproj-BF16.gguf", 931),
        ]);
        assert_eq!(projector.pick(&no_f16).unwrap().path, "mmproj-BF16.gguf");
        // preferences that name none of them: the smallest wins
        let by_size = part("mmproj", &["mmproj*.gguf"], &["Q8_0"]);
        assert_eq!(
            by_size.pick(&repo).unwrap().path,
            "mmproj-F16.gguf",
            "928 is smaller than 931 and 1800"
        );
        // no preference and no size: the first by path
        let blind = vec![
            RemoteFile::new("mmproj-b.gguf", None),
            RemoteFile::new("mmproj-a.gguf", None),
        ];
        assert_eq!(by_size.pick(&blind).unwrap().path, "mmproj-a.gguf");
        assert!(projector.pick(&files(&[("m.gguf", 1)])).is_none());
    }

    #[test]
    fn a_bundle_is_there_for_a_repository_only_when_every_part_is() {
        let two = Bundle {
            id: "both".into(),
            label: "with both".into(),
            help: String::new(),
            parts: vec![
                part("mmproj", &["mmproj*"], &[]),
                part("draft", &["*draft*"], &[]),
            ],
        };
        let only_projector = files(&[("m.gguf", 1), ("mmproj-f16.gguf", 2)]);
        assert!(gather(&two, &only_projector).is_none());
        let both = files(&[("m.gguf", 1), ("mmproj-f16.gguf", 2), ("draft-q8.gguf", 3)]);
        let got = gather(&two, &both).unwrap();
        assert_eq!(
            got.iter()
                .map(|(p, f)| (p.kind.as_str(), f.path.as_str()))
                .collect::<Vec<_>>(),
            [("mmproj", "mmproj-f16.gguf"), ("draft", "draft-q8.gguf")]
        );
    }

    #[test]
    fn companions_are_the_files_any_part_matches_and_their_kinds_are_listed_once() {
        let bundles = vec![
            Bundle {
                id: "a".into(),
                label: String::new(),
                help: String::new(),
                parts: vec![part("mmproj", &["mmproj*"], &[])],
            },
            Bundle {
                id: "b".into(),
                label: String::new(),
                help: String::new(),
                parts: vec![
                    part("mmproj", &["*mmproj*"], &[]),
                    part("draft", &["*draft*"], &[]),
                ],
            },
        ];
        let repo = files(&[("m.gguf", 1), ("mmproj-f16.gguf", 2), ("x/draft.gguf", 3)]);
        assert!(!is_companion(&bundles, &repo[0]));
        assert!(is_companion(&bundles, &repo[1]));
        assert!(is_companion(&bundles, &repo[2]));
        assert_eq!(kinds_found(&bundles, &repo), ["mmproj", "draft"]);
        assert_eq!(
            companion_patterns(&bundles),
            ["mmproj*", "*mmproj*", "*draft*"]
        );
        assert!(kinds_found(&[], &repo).is_empty());
    }
}
