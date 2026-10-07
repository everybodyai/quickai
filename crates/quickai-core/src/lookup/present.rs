//! From what a look-up found to what the window shows: the type of a model, the buttons that fetch
//! it and the bundles it can be fetched in (docs/lookup-and-bundles.md, sections 1 to 3). Pure: the
//! same candidate and rules always give the same entry.

use super::bundle::{self, Bundle, Part};
use super::candidate::{Candidate, Origin, RemoteFile};
use super::huggingface;
use super::quant;
use super::size::{format_bytes, parse_parameters};
use super::types::{classify, size_class, Facts, SizeClassDef, TypeDef};

/// How a look-up reads what it found, from its document (`[[models.lookup]]`).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The quantizations to offer first, best first (`Q4_K_M`).
    pub prefer: Vec<String>,
    /// A file whose name or path one of these patterns matches (`*-of-*`, as for a bundle's `match`) is
    /// not weights: shards, draft models, importance matrices.
    pub skip: Vec<String>,
    /// The most buttons a model gets in a row.
    pub show_variants: usize,
    /// Bytes per parameter of what a name pulls, to say how big a size tag is when the library does not.
    pub bytes_per_parameter: f64,
    /// The types this engine runs, by id; a model of another type is shown as `other`. Empty: all.
    pub types: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            prefer: Vec::new(),
            skip: Vec::new(),
            show_variants: 4,
            bytes_per_parameter: 0.6,
            types: Vec::new(),
        }
    }
}

/// What the entries are made with.
#[derive(Clone, Copy, Debug)]
pub struct Rules<'a> {
    pub types: &'a [TypeDef],
    pub sizes: &'a [SizeClassDef],
    pub bundles: &'a [Bundle],
    pub settings: &'a Settings,
}

/// What the entries are made with, owned: a look-up that runs on a thread of its own takes it along.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnedRules {
    pub types: Vec<TypeDef>,
    pub sizes: Vec<SizeClassDef>,
    pub bundles: Vec<Bundle>,
    pub settings: Settings,
}

impl OwnedRules {
    pub fn rules(&self) -> Rules<'_> {
        Rules {
            types: &self.types,
            sizes: &self.sizes,
            bundles: &self.bundles,
            settings: &self.settings,
        }
    }

    /// The entry of a candidate.
    pub fn present(&self, candidate: &Candidate) -> Entry {
        present(candidate, &self.rules())
    }
}

/// One result, as the window shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub name: String,
    pub description: String,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    /// Words the card shows beside the name: what an Ollama card says the model can do, the pipeline
    /// of a repository.
    pub tags: Vec<String>,
    /// The id of its type; `None` when there are no types.
    pub model_type: Option<String>,
    /// Every way to fetch it: the plain ones, then each bundle's.
    pub offers: Vec<Offer>,
    /// What a curated list knows of it (its maker, its licence, what it reads); a look-up knows none
    /// of that.
    pub card: Option<Box<Card>>,
}

/// What the recommended list says of a model besides what a look-up says (docs/catalog.md).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Card {
    pub by: String,
    pub license: String,
    pub params: String,
    /// The tokens of context it was made for.
    pub context: Option<u64>,
    /// What it reads, as modality ids (`text`, `image`, `audio`).
    pub input: Vec<String>,
    /// What else is worth knowing, as short words.
    pub features: Vec<String>,
    /// It is one of the models "Start here" shows.
    pub start: bool,
    /// The packages ship it.
    pub bundled: bool,
}

/// One button.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// The bundle it belongs to; `None` is the model alone.
    pub bundle: Option<String>,
    /// The words of the button: `Q4_K_M · 2.4 GB`, `4b · ~2.4 GB`, `latest`.
    pub label: String,
    /// What it will do, said in full.
    pub hover: String,
    /// What it fetches, in bytes, when that is known or can be told.
    pub bytes: Option<u64>,
    /// The size is a guess (a count of parameters times a typical weight).
    pub estimated: bool,
    /// The id of its size class.
    pub class: Option<String>,
    /// The one the look-up would pick for someone who does not know.
    pub recommended: bool,
    pub fetch: Fetch,
}

/// What a button does.
#[derive(Clone, Debug, PartialEq)]
pub enum Fetch {
    /// The engine pulls it by name (`qwen3:4b`).
    Reference(String),
    /// QuickAI downloads files.
    Files(Download),
}

/// Files to fetch: the weights first, then the parts of a bundle.
#[derive(Clone, Debug, PartialEq)]
pub struct Download {
    pub files: Vec<Piece>,
    /// The bundle they are, or `None` for the weights alone.
    pub bundle: Option<String>,
    /// Where they come from, for the record that pairs them: `huggingface:owner/repo`.
    pub source: String,
    /// The engine's `[[models.mode]]` the model is started in (`embedding`), which the record beside
    /// the weights remembers.
    pub mode: Option<String>,
}

/// One file of a download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The kind of component it is; `None` is the weights.
    pub kind: Option<String>,
    /// What the window calls it: `model`, or the part's own word (`projector`).
    pub label: String,
    pub url: String,
    /// The name it is kept under in the models folder.
    pub store_as: String,
    /// The name it has in the repository.
    pub name: String,
    pub size: Option<u64>,
    /// What the bytes that arrive must hash to (lower case hexadecimal), when the source says.
    pub sha256: Option<String>,
}

impl Download {
    /// The bytes of all the files, when every one is known.
    pub fn total(&self) -> Option<u64> {
        self.files.iter().map(|f| f.size).sum()
    }

    /// The weights.
    pub fn weights(&self) -> Option<&Piece> {
        self.files.iter().find(|f| f.kind.is_none())
    }
}

/// A name safe to keep a file under in a folder: letters, digits, `.`, `-` and `_`, no leading dot.
pub fn safe_file_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = last
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "download.gguf".to_string()
    } else {
        cleaned
    }
}

/// The name a companion is kept under: its repository first, because the same file name
/// (`mmproj-F16.gguf`) is in nearly every repository and is the projector of a different model.
pub fn companion_file_name(repo: &str, name: &str) -> String {
    safe_file_name(&format!(
        "{}--{}",
        repo.replace('/', "--"),
        safe_file_name(name)
    ))
}

/// Makes the entry of a candidate.
pub fn present(candidate: &Candidate, rules: &Rules<'_>) -> Entry {
    let (offers, companions, tags) = match &candidate.origin {
        Origin::Library { name, sizes } => (
            library_offers(name, sizes, rules),
            Vec::new(),
            candidate.capabilities.clone(),
        ),
        Origin::Hub {
            site,
            repo,
            revision,
            files,
        } => {
            let (offers, kinds) = hub_offers(site, repo, revision, files, rules);
            (offers, kinds, candidate.pipeline.iter().cloned().collect())
        }
    };
    let facts = Facts {
        capabilities: &candidate.capabilities,
        pipeline: candidate.pipeline.as_deref(),
        tags: &candidate.tags,
        name: &candidate.name,
        companions: &companions,
    };
    let mut model_type = classify(rules.types, &facts).map(|t| t.id.clone());
    if let Some(found) = &model_type {
        let runs = rules.settings.types.is_empty() || rules.settings.types.contains(found);
        if !runs && rules.types.iter().any(|t| t.id == "other") {
            model_type = Some("other".to_string());
        }
    }
    Entry {
        key: candidate.key.clone(),
        name: candidate.name.clone(),
        description: candidate.description.clone(),
        downloads: candidate.downloads,
        likes: candidate.likes,
        tags,
        model_type,
        offers,
        card: None,
    }
}

pub(crate) fn class_of(sizes: &[SizeClassDef], bytes: Option<u64>) -> Option<String> {
    bytes
        .and_then(|b| size_class(sizes, b))
        .map(|c| c.id.clone())
}

fn library_offers(name: &str, sizes: &[String], rules: &Rules<'_>) -> Vec<Offer> {
    let settings = rules.settings;
    if sizes.is_empty() {
        return vec![Offer {
            bundle: None,
            label: "latest".to_string(),
            hover: format!("Pull {name} (the default size)"),
            bytes: None,
            estimated: false,
            class: None,
            recommended: false,
            fetch: Fetch::Reference(name.to_string()),
        }];
    }
    let mut offers: Vec<Offer> = sizes
        .iter()
        .map(|size| {
            let bytes = parse_parameters(size)
                .map(|parameters| (parameters * settings.bytes_per_parameter).round() as u64);
            let reference = format!("{name}:{size}");
            let (label, hover) = match bytes {
                Some(b) => (
                    format!("{size} · ~{}", format_bytes(b)),
                    format!(
                        "Pull {reference} (about {} at the size it is usually pulled in; the exact size is known once it is pulled)",
                        format_bytes(b)
                    ),
                ),
                None => (size.clone(), format!("Pull {reference}")),
            };
            Offer {
                bundle: None,
                label,
                hover,
                bytes,
                estimated: bytes.is_some(),
                class: class_of(rules.sizes, bytes),
                recommended: false,
                fetch: Fetch::Reference(reference),
            }
        })
        .collect();
    // smallest first; a size that cannot be told stays where the card put it, after the others
    offers.sort_by_key(|o| o.bytes.unwrap_or(u64::MAX));
    offers.truncate(settings.show_variants.max(1));
    offers
}

/// One kind of weights file of a repository: the quantization it is, when the name says.
struct Variant<'a> {
    label: String,
    file: &'a RemoteFile,
    rank: usize,
}

fn hub_offers(
    site: &str,
    repo: &str,
    revision: &str,
    files: &[RemoteFile],
    rules: &Rules<'_>,
) -> (Vec<Offer>, Vec<String>) {
    let settings = rules.settings;
    let kinds = bundle::kinds_found(rules.bundles, files);
    let skipped = |f: &RemoteFile| super::glob::matches_any(&settings.skip, &f.path);
    let weights: Vec<&RemoteFile> = files
        .iter()
        .filter(|f| f.path.to_lowercase().ends_with(".gguf"))
        .filter(|f| !bundle::is_companion(rules.bundles, f))
        .filter(|f| !skipped(f))
        .collect();
    if weights.is_empty() {
        return (Vec::new(), kinds);
    }

    // a label is the quantization the name says; a file that says none, or says what another does, is
    // told by its name without what all the names begin with (`APEX-Compact` for `Laguna-XS-2.1-APEX-Compact`)
    let labels: Vec<Option<String>> = weights.iter().map(|f| quant::label(f.name())).collect();
    let stems: Vec<&str> = weights.iter().map(|f| stem_of(f.name())).collect();
    let common = common_prefix(&stems);
    let mut variants: Vec<Variant<'_>> = weights
        .iter()
        .zip(&labels)
        .zip(&stems)
        .map(|((file, label), stem)| {
            let clash = label.is_some() && labels.iter().filter(|l| *l == label).count() > 1;
            Variant {
                label: match label {
                    Some(l) if !clash => l.clone(),
                    _ => stem[common..].to_string(),
                },
                file,
                rank: quant::rank(file.name(), &settings.prefer),
            }
        })
        .collect();
    // the ones to show: those the preferences like, then the smallest of the rest
    variants.sort_by_key(|v| (v.rank, v.file.size.unwrap_or(u64::MAX), v.file.path.clone()));
    variants.truncate(settings.show_variants.max(1));
    let best = variants
        .iter()
        .map(|v| v.rank)
        .min()
        .filter(|r| *r != usize::MAX);
    variants.sort_by_key(|v| (v.file.size.unwrap_or(u64::MAX), v.file.path.clone()));

    let source = format!("huggingface:{repo}");
    let piece = |part: Option<&Part>, file: &RemoteFile| Piece {
        kind: part.map(|p| p.kind.clone()),
        label: part.map_or_else(|| "model".to_string(), |p| p.label.clone()),
        url: huggingface::file_url(site, repo, revision, &file.path),
        store_as: match part {
            None => safe_file_name(file.name()),
            Some(_) => companion_file_name(repo, file.name()),
        },
        name: file.name().to_string(),
        size: file.size,
        sha256: None,
    };

    let mut offers = Vec::new();
    for variant in &variants {
        let download = Download {
            files: vec![piece(None, variant.file)],
            bundle: None,
            source: source.clone(),
            mode: None,
        };
        let bytes = variant.file.size;
        let recommended = best == Some(variant.rank);
        offers.push(Offer {
            bundle: None,
            label: with_size(&variant.label, bytes),
            hover: preferred(
                match bytes {
                    Some(b) => format!("Download {} ({})", variant.file.name(), format_bytes(b)),
                    None => format!("Download {}", variant.file.name()),
                },
                recommended,
            ),
            bytes,
            estimated: false,
            class: class_of(rules.sizes, bytes),
            recommended,
            fetch: Fetch::Files(download),
        });
    }
    for b in rules.bundles {
        let Some(found) = bundle::gather(b, files) else {
            continue;
        };
        for variant in &variants {
            let mut pieces = vec![piece(None, variant.file)];
            let mut words = Vec::new();
            for (part, file) in &found {
                pieces.push(piece(Some(part), file));
                words.push(format!("the {} {}", part.label, file.name()));
            }
            let download = Download {
                files: pieces,
                bundle: Some(b.id.clone()),
                source: source.clone(),
                mode: None,
            };
            let bytes = download.total();
            let recommended = best == Some(variant.rank);
            offers.push(Offer {
                bundle: Some(b.id.clone()),
                label: with_size(&variant.label, bytes),
                hover: preferred(
                    format!(
                        "Download {} and {}{}",
                        variant.file.name(),
                        words.join(" and "),
                        bytes
                            .map(|b| format!(" ({} in all)", format_bytes(b)))
                            .unwrap_or_default()
                    ),
                    recommended,
                ),
                bytes,
                estimated: false,
                class: class_of(rules.sizes, bytes),
                recommended,
                fetch: Fetch::Files(download),
            });
        }
    }
    (offers, kinds)
}

/// What a button says when it is the preferred one: it is bold, and this is why.
fn preferred(text: String, yes: bool) -> String {
    if yes {
        format!("{text}. It is the one this look-up prefers.")
    } else {
        text
    }
}

/// A file name without its `.gguf`.
fn stem_of(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".gguf") {
        &name[..name.len() - ".gguf".len()]
    } else {
        name
    }
}

/// How much the names begin with in common, counted up to and including the last separator in it, so
/// that what is left of each is a whole word; 0 for a single name or when nothing is left of one.
fn common_prefix(stems: &[&str]) -> usize {
    let Some(first) = stems.first() else {
        return 0;
    };
    if stems.len() < 2 {
        return 0;
    }
    let mut end = first.len();
    for other in &stems[1..] {
        end = end.min(
            first
                .bytes()
                .zip(other.bytes())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    let head = &first.as_bytes()[..end];
    let cut = head
        .iter()
        .rposition(|b| matches!(b, b'-' | b'_' | b'.' | b' '))
        .map_or(0, |i| i + 1);
    // a name that is all prefix would be left with nothing
    if stems.iter().any(|s| s.len() <= cut) || !first.is_char_boundary(cut) {
        0
    } else {
        cut
    }
}

fn with_size(label: &str, bytes: Option<u64>) -> String {
    match bytes {
        Some(b) => format!("{label} · {}", format_bytes(b)),
        None => label.to_string(),
    }
}
