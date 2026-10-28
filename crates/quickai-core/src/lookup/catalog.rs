//! The recommended models, as the entries the look-up panel draws (docs/catalog.md): a model of the
//! list that an engine offers becomes an entry of its type with a button for every way to get it, one
//! row for the weights alone and one for each bundle the model has files for. Pure: the same list and
//! the same engine always give the same entries, and nothing here asks the network anything.

use crate::registry::catalog::{Catalog, CatalogFile, CatalogModel, FilesRun, ReferenceRun, Run};

use super::bundle::Bundle;
use super::huggingface;
use super::present::{
    class_of, companion_file_name, safe_file_name, Card, Download, Entry, Fetch, Offer, Piece,
};
use super::quant;
use super::size::format_bytes;
use super::types::SizeClassDef;

/// What the entries of an engine are made with.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    /// The id of the engine the entries are for: only the models that have a `run` for it are there.
    pub engine: &'a str,
    pub sizes: &'a [SizeClassDef],
    /// The engine's bundles, for the words of a row and the label of a part.
    pub bundles: &'a [Bundle],
    /// The hub the files are fetched from (no slash at the end); without one, no model that is
    /// fetched as files is offered.
    pub site: Option<&'a str>,
}

/// The entries of every model the engine offers, the smallest first (by the least a button fetches;
/// a model that says no size last), then by name.
pub fn entries(catalog: &Catalog, ctx: &Context<'_>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = catalog
        .for_engine(ctx.engine)
        .filter_map(|model| entry(model, ctx))
        .collect();
    entries.sort_by_cached_key(|e| {
        (
            e.offers
                .iter()
                .filter_map(|o| o.bytes)
                .min()
                .unwrap_or(u64::MAX),
            e.name.to_lowercase(),
        )
    });
    entries
}

/// The entry of one model for the engine, `None` when the engine does not offer it (or the hub it
/// would be fetched from is not known).
pub fn entry(model: &CatalogModel, ctx: &Context<'_>) -> Option<Entry> {
    let offers = match model.runs.get(ctx.engine)? {
        Run::Files(run) => files_offers(run, ctx)?,
        Run::Reference(run) => reference_offers(run, ctx),
    };
    Some(Entry {
        key: format!("catalog:{}", model.id),
        name: model.name.clone(),
        description: model.note.clone(),
        downloads: None,
        likes: None,
        tags: Vec::new(),
        model_type: Some(model.kind.clone()),
        offers,
        card: Some(Box::new(Card {
            by: model.by.clone(),
            license: model.license.clone(),
            params: model.params.clone(),
            context: model.context,
            input: model.input.clone(),
            features: model.features.clone(),
            tagline: model.tagline.clone(),
            tasks: model.tasks.clone(),
            start: model.start,
            bundled: model.bundled,
        })),
    })
}

/// The name a button of the weights says: the label the list gives, else the quantization the file's
/// name says, else the name.
fn label_of(file: &CatalogFile, all: &[CatalogFile]) -> String {
    if let Some(label) = &file.label {
        return label.clone();
    }
    let name = name_of(file);
    match quant::label(name) {
        // two files that say the same are told by their names
        Some(label)
            if all
                .iter()
                .filter(|f| {
                    f.label.is_none() && quant::label(name_of(f)).as_deref() == Some(&label)
                })
                .count()
                < 2 =>
        {
            label
        }
        _ => name.trim_end_matches(".gguf").to_string(),
    }
}

fn name_of(file: &CatalogFile) -> &str {
    super::glob::name_of(&file.path)
}

fn host_of(site: &str) -> &str {
    site.trim()
        .trim_end_matches('/')
        .split_once("://")
        .map_or(site, |(_, rest)| rest)
}

fn with_size(label: &str, bytes: Option<u64>) -> String {
    match bytes {
        Some(b) => format!("{label} · {}", format_bytes(b)),
        None => label.to_string(),
    }
}

fn files_offers(run: &FilesRun, ctx: &Context<'_>) -> Option<Vec<Offer>> {
    let site = ctx.site?;
    let source = format!("huggingface:{}", run.repo);
    let host = host_of(site);
    let piece = |kind: Option<(&str, &str)>, file: &CatalogFile| Piece {
        kind: kind.map(|(k, _)| k.to_string()),
        label: kind.map_or_else(|| "model".to_string(), |(_, l)| l.to_string()),
        url: huggingface::file_url(site, &run.repo, &run.rev, &file.path),
        store_as: match kind {
            None => file
                .store_as
                .clone()
                .unwrap_or_else(|| safe_file_name(name_of(file))),
            Some(_) => companion_file_name(&run.repo, name_of(file)),
        },
        name: name_of(file).to_string(),
        size: file.size,
        sha256: file.sha256.clone(),
        // the folder the list says for the weights of this model; the engine's own is put on later
        place: match kind {
            None => run.place.clone(),
            Some(_) => None,
        },
    };
    // what each companion is called in a sentence: the bundle's own word for the part
    let bundle = run
        .bundle
        .as_deref()
        .and_then(|id| ctx.bundles.iter().find(|b| b.id == id));
    let part_label = |kind: &str| -> String {
        bundle
            .and_then(|b| b.parts.iter().find(|p| p.kind == kind))
            .map_or_else(|| kind.to_string(), |p| p.label.clone())
    };

    let best = run.files.iter().position(|f| f.best).unwrap_or(0);
    let mut weights: Vec<(usize, &CatalogFile)> = run.files.iter().enumerate().collect();
    weights.sort_by_key(|(i, f)| (f.size.unwrap_or(u64::MAX), *i));

    let mut offers = Vec::new();
    for (i, file) in &weights {
        let recommended = *i == best;
        let download = Download {
            files: vec![piece(None, file)],
            bundle: None,
            source: source.clone(),
            mode: run.mode.clone(),
        };
        let bytes = file.size;
        offers.push(Offer {
            bundle: None,
            label: with_size(&label_of(file, &run.files), bytes),
            hover: hover(
                format!(
                    "Download {}{} from {host}/{}",
                    name_of(file),
                    bytes.map_or_else(String::new, |b| format!(" ({})", format_bytes(b))),
                    run.repo
                ),
                file.sha256.is_some(),
                recommended,
            ),
            bytes,
            estimated: false,
            class: class_of(ctx.sizes, bytes),
            recommended,
            fetch: Fetch::Files(download),
        });
    }
    if let Some(bundle_id) = &run.bundle {
        for (i, file) in &weights {
            let recommended = *i == best;
            let mut pieces = vec![piece(None, file)];
            let mut words = Vec::new();
            for (kind, companion) in &run.companions {
                let label = part_label(kind);
                pieces.push(piece(Some((kind, &label)), companion));
                words.push(format!("the {label} {}", name_of(companion)));
            }
            let download = Download {
                files: pieces,
                bundle: Some(bundle_id.clone()),
                source: source.clone(),
                mode: run.mode.clone(),
            };
            let bytes = download.total();
            let verified = download.files.iter().all(|p| p.sha256.is_some());
            offers.push(Offer {
                bundle: Some(bundle_id.clone()),
                label: with_size(&label_of(file, &run.files), bytes),
                hover: hover(
                    format!(
                        "Download {} and {}{} from {host}/{}",
                        name_of(file),
                        words.join(" and "),
                        bytes
                            .map_or_else(String::new, |b| format!(" ({} in all)", format_bytes(b))),
                        run.repo
                    ),
                    verified,
                    recommended,
                ),
                bytes,
                estimated: false,
                class: class_of(ctx.sizes, bytes),
                recommended,
                fetch: Fetch::Files(download),
            });
        }
    }
    // The weights with what goes with them come first, because a model that reads pictures does so only
    // with its projector and one download is what most people want; the weights alone stay as the
    // second row, because whether to fetch the extra file is the person's to choose.
    offers.sort_by_key(|o| o.bundle.is_none());
    Some(offers)
}

fn reference_offers(run: &ReferenceRun, ctx: &Context<'_>) -> Vec<Offer> {
    let tag = run
        .reference
        .rsplit_once(':')
        .map_or("latest", |(_, tag)| tag);
    vec![Offer {
        bundle: None,
        label: with_size(tag, run.size),
        hover: format!(
            "Pull {}{}: the engine downloads it and checks it. It is the one the list recommends.",
            run.reference,
            run.size
                .map_or_else(String::new, |b| format!(" ({})", format_bytes(b)))
        ),
        bytes: run.size,
        estimated: false,
        class: class_of(ctx.sizes, run.size),
        recommended: true,
        fetch: Fetch::Reference(run.reference.clone()),
    }]
}

fn hover(text: String, checked: bool, recommended: bool) -> String {
    let mut text = text;
    if checked {
        text.push_str(". It is checked against its SHA-256 when it arrives");
    }
    if recommended {
        text.push_str(". It is the one the list recommends");
    }
    text.push('.');
    text
}
