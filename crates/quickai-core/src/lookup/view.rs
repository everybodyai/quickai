//! How the entries of a look-up are laid out: in sections by type, each entry with the buttons that
//! pass the size filter. Pure, so the window only draws what this says (docs/lookup-and-bundles.md,
//! section 1).

use super::present::Entry;
use super::types::{in_display_order, TypeDef};

/// The type whose section is folded when the results open: what the engine does not run, and what
/// does not describe itself.
pub const FOLDED_TYPE: &str = "other";

/// One section of the results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The id of the type; empty for the one section of a vocabulary with no types.
    pub type_id: String,
    /// Its heading, without the count; empty when there is no heading to draw.
    pub label: String,
    pub help: String,
    /// Whether it is open when the results appear.
    pub open: bool,
    pub entries: Vec<Shown>,
}

/// An entry of a section, with the offers that pass the filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// Its place in the list of entries.
    pub entry: usize,
    /// Its offers that pass, as places in `Entry::offers`, in the order they have.
    pub offers: Vec<usize>,
}

/// The sections of `entries`: the types in the order they are shown, each with the entries of its type
/// in the order they arrived, and the offers of each that are in the size class `size` (all of them
/// when it is `None`). An entry with no offer left is not shown; a section with no entry is not there.
/// An entry of a type the vocabulary does not have goes in the section of `other` when there is one.
pub fn arrange(entries: &[Entry], types: &[TypeDef], size: Option<&str>) -> Vec<Section> {
    arrange_where(entries, types, size, &|_, _| true)
}

/// `arrange` for the entries `keep` says yes to, told the place of an entry and the entry (the
/// places in `entries` do not move).
pub fn arrange_where(
    entries: &[Entry],
    types: &[TypeDef],
    size: Option<&str>,
    keep: &dyn Fn(usize, &Entry) -> bool,
) -> Vec<Section> {
    let passes = |index: usize, entry: &Entry| -> Vec<usize> {
        if !keep(index, entry) {
            return Vec::new();
        }
        entry
            .offers
            .iter()
            .enumerate()
            .filter(|(_, o)| size.is_none_or(|s| o.class.as_deref() == Some(s)))
            .map(|(i, _)| i)
            .collect()
    };
    if types.is_empty() {
        let shown: Vec<Shown> = entries
            .iter()
            .enumerate()
            .filter_map(|(entry, e)| {
                let offers = passes(entry, e);
                (!offers.is_empty()).then_some(Shown { entry, offers })
            })
            .collect();
        return if shown.is_empty() {
            Vec::new()
        } else {
            vec![Section {
                type_id: String::new(),
                label: String::new(),
                help: String::new(),
                open: true,
                entries: shown,
            }]
        };
    }
    let known = |id: &str| types.iter().any(|t| t.id == id);
    in_display_order(types)
        .into_iter()
        .filter_map(|t| {
            let shown: Vec<Shown> = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| match e.model_type.as_deref() {
                    Some(id) if known(id) => id == t.id,
                    _ => t.id == FOLDED_TYPE,
                })
                .filter_map(|(entry, e)| {
                    let offers = passes(entry, e);
                    (!offers.is_empty()).then_some(Shown { entry, offers })
                })
                .collect();
            (!shown.is_empty()).then(|| Section {
                type_id: t.id.clone(),
                label: t.label.clone(),
                help: t.help.clone(),
                open: t.id != FOLDED_TYPE,
                entries: shown,
            })
        })
        .collect()
}

/// Which entries of a list are on show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// Every entry, in sections by type.
    #[default]
    All,
    /// The ones the list says to start with.
    Start,
    /// The entries of one type, by its id.
    Type(String),
}

/// What a person chose to look at in a list of entries: where, what words and what size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Browse {
    pub scope: Scope,
    /// Words that must each be somewhere in what an entry says of itself, whatever the case.
    pub words: String,
    /// The id of the size class whose buttons are shown; `None` shows every size.
    pub size: Option<String>,
}

/// How many entries each choice of `Scope` would show with the words and the size as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub start: usize,
    /// The types that have an entry, in the order their sections are shown, with how many.
    pub types: Vec<(String, usize)>,
}

/// What `browse` makes of a list: the sections to draw and the counts for the choices beside them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Browsed {
    pub sections: Vec<Section>,
    pub counts: Counts,
}

/// The text an entry is searched in, lower case: what it is called, who made it, what it is for, its
/// licence, its size, what it reads and its features, and the words of its type.
pub fn search_text(entry: &Entry, types: &[TypeDef]) -> String {
    let mut text = vec![entry.name.clone(), entry.description.clone()];
    text.extend(entry.tags.iter().cloned());
    if let Some(card) = &entry.card {
        text.push(card.by.clone());
        text.push(card.license.clone());
        text.push(card.params.clone());
        text.extend(card.input.iter().cloned());
        text.extend(card.features.iter().cloned());
    }
    if let Some(id) = &entry.model_type {
        text.push(id.clone());
        if let Some(t) = types.iter().find(|t| &t.id == id) {
            text.push(t.label.clone());
        }
    }
    text.join(" ").to_lowercase()
}

/// Whether every word is somewhere in `text` (already lower case). A word that ends in `s` is also found
/// without it: a person who types `images` means what the list says as `image`. No words: yes.
fn has_words(text: &str, words: &str) -> bool {
    words.split_whitespace().all(|w| {
        let w = w.to_lowercase();
        text.contains(&w)
            || (w.len() > 3 && w.strip_suffix('s').is_some_and(|stem| text.contains(stem)))
    })
}

/// Lays a list out for a choice of scope, words and size: the sections to draw, and for each scope
/// how many entries it would show. An entry counts for a scope when it is in it, says every word and
/// has a button in the size class.
pub fn browse(entries: &[Entry], types: &[TypeDef], what: &Browse) -> Browsed {
    let texts: Vec<String> = entries.iter().map(|e| search_text(e, types)).collect();
    let matches = |i: usize, entry: &Entry| {
        has_words(&texts[i], &what.words)
            && what
                .size
                .as_deref()
                .is_none_or(|s| entry.offers.iter().any(|o| o.class.as_deref() == Some(s)))
    };
    let is_start = |e: &Entry| e.card.as_ref().is_some_and(|c| c.start);
    let kind_of = |e: &Entry| -> String {
        match e.model_type.as_deref() {
            Some(id) if types.iter().any(|t| t.id == id) => id.to_string(),
            _ => FOLDED_TYPE.to_string(),
        }
    };

    let shown: Vec<(usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter(|(i, e)| matches(*i, e))
        .collect();
    let counts = Counts {
        all: shown.len(),
        start: shown.iter().filter(|(_, e)| is_start(e)).count(),
        types: in_display_order(types)
            .into_iter()
            .map(|t| {
                let n = shown.iter().filter(|(_, e)| kind_of(e) == t.id).count();
                (t.id.clone(), n)
            })
            .filter(|(_, n)| *n > 0)
            .collect(),
    };

    let in_scope = |e: &Entry| match &what.scope {
        Scope::All => true,
        Scope::Start => is_start(e),
        Scope::Type(id) => &kind_of(e) == id,
    };
    let sections = arrange_where(entries, types, what.size.as_deref(), &|i, e| {
        in_scope(e) && matches(i, e)
    });
    Browsed { sections, counts }
}
