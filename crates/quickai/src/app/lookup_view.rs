//! The look-up of models online, as the window of every engine draws it (docs/lookup-and-bundles.md):
//! one state, one panel. The engine's document says which look-up there is
//! (`[[models.lookup]]`); what it finds arrives as entries (`quickai_core::lookup::present`) already
//! sorted into a type, with a button for each way to fetch it; this arranges them in sections by type,
//! keeps the size filter, and carries out the button that was pressed: a name pulled by the daemon
//! (Ollama) or files downloaded into the models folder (llama.cpp).
//!
//! Nothing is asked of a site until a button is pressed, and what a look-up says is kept until the
//! person closes it or the engine changes.

use crossbeam_channel::Receiver;
use eframe::egui;
use quickai_core::lookup::huggingface::relevance;
use quickai_core::lookup::present::{Download, Entry, Fetch, Offer};
use quickai_core::lookup::view::{arrange, Section};
use quickai_core::models::{bundles, lookup_rules, lookups};
use quickai_core::registry::engine::LookupKind;

use super::model_row::{self, Row, RowAction, RowLook};
use super::App;
use crate::downloader;
use crate::prims::hub::{spawn_hub_lookup, HubSpec};
use crate::prims::lookup::{spawn_library_lookup, LookupEvent};

/// What a look-up has said so far.
#[derive(Default)]
pub struct LookupView {
    /// The look-up that is running: its events are drained once a frame.
    rx: Option<Receiver<LookupEvent>>,
    /// The results are on show (the person has not closed them).
    pub open: bool,
    /// What to say about it: that it is looking, what it found, or why it could not.
    pub status: String,
    /// The look-up could not be made, and `status` says why (it is shown as the problem it is).
    pub failed: bool,
    pub entries: Vec<Entry>,
    /// The size class that is the only one shown; `None` shows every size.
    pub size: Option<String>,
    /// The words it looked up (none: the popular models), so that a box emptied of them goes back to its suggestions.
    pub words: String,
    /// What the look-up said of the words that the results do not say (a repository typed that QuickAI cannot run).
    pub note: Option<String>,
    /// The results are put in order of how well their names answer the words (a search of the hub, whose answers
    /// come one search after another), rather than in the order they came.
    ranked: bool,
    /// The one kind of model shown (`chat`); `None` shows every kind.
    pub kind: Option<String>,
    /// The order the person chose; `None` is the one a look-up starts in.
    pub order: Option<ResultOrder>,
}

/// The order of the results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultOrder {
    /// How well their names answer the words: the order they are kept in.
    Best,
    Downloads,
    Smallest,
}

impl ResultOrder {
    const ALL: [ResultOrder; 3] = [
        ResultOrder::Best,
        ResultOrder::Downloads,
        ResultOrder::Smallest,
    ];

    fn words(self) -> &'static str {
        match self {
            ResultOrder::Best => "Best match",
            ResultOrder::Downloads => "Most downloaded",
            ResultOrder::Smallest => "Smallest first",
        }
    }
}

/// The offer a result's Get fetches, of those the size filter shows: the one the look-up recommends, with what goes with it when
/// there is such a bundle, else the first.
fn main_of<'a>(entry: &'a Entry, shown: &[usize]) -> Option<&'a Offer> {
    let offers: Vec<&Offer> = shown.iter().map(|i| &entry.offers[*i]).collect();
    // the size the look-up recommends with what goes with it (a projector: the model then reads pictures, as the main
    // choice of the recommended list does), else that size alone, else the first
    offers
        .iter()
        .find(|o| o.recommended && o.bundle.is_some())
        .or_else(|| offers.iter().find(|o| o.recommended))
        .or_else(|| offers.first())
        .copied()
}

/// The second line of a result: who put it there, how often it was downloaded, its licence, and what it was made
/// from (`by mradermacher · 251 downloads · apache-2.0 · from mintujohnson/Llama-3.2-3B-Malayalam-Instruct`); a name of
/// a library says what it is.
pub(super) fn result_line(entry: &Entry) -> String {
    let Some((owner, _)) = entry.name.split_once('/') else {
        let first = super::shell::first_sentence(&entry.description);
        return if first.is_empty() {
            facts_line(entry)
        } else {
            first.to_string()
        };
    };
    let mut parts = vec![format!("by {owner}")];
    if let Some(count) = entry.downloads.filter(|n| *n > 0) {
        parts.push(format!("{} downloads", compact_count(count)));
    }
    let mut from = None;
    for part in entry.description.split(" · ").filter(|p| !p.is_empty()) {
        match part
            .strip_prefix("Quantized from ")
            .or_else(|| part.strip_prefix("Based on "))
        {
            Some(base) => from = Some(format!("from {base}")),
            None => parts.push(part.to_string()),
        }
    }
    parts.extend(from);
    parts.join(" · ")
}

impl LookupView {
    /// Whether results are on the screen to close.
    pub fn has_results(&self) -> bool {
        self.open && !self.entries.is_empty()
    }

    /// Whether a look-up is under way.
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }
}

/// Where the files of a download came from, as far as the application remembers it for the file it names
/// when the person gives the configuration away: the place, the path in the repository when it is not at the top
/// and the revision when it is not `main`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DownloadSource {
    pub place: String,
    pub path: Option<String>,
    pub revision: Option<String>,
}

impl DownloadSource {
    pub(super) fn of(download: &Download) -> DownloadSource {
        let weights = download.primary();
        DownloadSource {
            place: download.source.clone(),
            path: weights
                .filter(|p| !p.name.is_empty() && p.name != p.store_as)
                .map(|p| p.name.clone()),
            revision: weights
                .and_then(|p| p.url.split("/resolve/").nth(1))
                .and_then(|rest| rest.split('/').next())
                .filter(|r| *r != "main")
                .map(str::to_string),
        }
    }
}

impl App {
    /// Whether the engine's document declares a look-up of models online: only then is there a
    /// button for it.
    pub(super) fn can_look_up(&self) -> bool {
        self.engine_doc().is_some_and(|e| !lookups(e).is_empty())
    }

    /// Looks `query` up (the most popular models when it is empty) in the first look-up the document
    /// declares. The first request is made here, by a button, and not before; a look-up that is
    /// running is forgotten, and what it finds is not heard.
    pub(super) fn start_lookup(&mut self, ctx: &egui::Context, query: &str) {
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let Some(lookup) = lookups(engine).first() else {
            return;
        };
        let rules = lookup_rules(&self.registry, engine, lookup);
        let query = query.trim().to_string();
        let ranked = matches!(lookup.kind, LookupKind::HuggingFace { .. }) && !query.is_empty();
        let words = query.clone();
        let wake = {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        let rx = match &lookup.kind {
            LookupKind::OllamaLibrary { site, .. } => {
                spawn_library_lookup(site.clone(), query, rules, wake)
            }
            LookupKind::HuggingFace {
                site,
                popular,
                keyword,
                similar,
                pipelines,
                words,
                ..
            } => spawn_hub_lookup(
                HubSpec {
                    site: site.clone(),
                    popular: *popular,
                    keyword: *keyword,
                    similar: *similar,
                    pipelines: pipelines.clone(),
                    words: words.clone(),
                },
                query,
                rules,
                wake,
            ),
        };
        self.lookup = LookupView {
            rx: Some(rx),
            open: true,
            status: "Looking...".into(),
            failed: false,
            entries: Vec::new(),
            size: self.lookup.size.take(),
            words,
            note: None,
            ranked,
            kind: None,
            order: None,
        };
        // an engine that keeps its files in folders of their own says which results it has already
        if self.runs_alone() {
            self.refresh_catalog_have();
        }
    }

    /// Drains what the look-up has said since the last frame.
    pub(super) fn poll_lookup(&mut self) {
        let Some(rx) = self.lookup.rx.take() else {
            return;
        };
        let lookup = &mut self.lookup;
        let mut done = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                LookupEvent::Status(sentence) => lookup.status = sentence,
                LookupEvent::Note(note) => lookup.note = Some(note),
                LookupEvent::Found(entry) if lookup.ranked => {
                    let at = ranked_place(&lookup.entries, &lookup.words, &entry);
                    lookup.entries.insert(at, entry);
                }
                LookupEvent::Found(entry) => lookup.entries.push(entry),
                LookupEvent::Failed => lookup.failed = true,
                LookupEvent::Done => done = true,
            }
        }
        if !done {
            lookup.rx = Some(rx);
        }
    }

    /// What a button of the results does: the engine pulls a name, or the files are downloaded.
    pub(super) fn fetch_offer(&mut self, fetch: &Fetch, ctx: &egui::Context) {
        match fetch {
            Fetch::Reference(reference) => {
                if self.lists_models() {
                    self.pull_named(reference, ctx);
                } else {
                    self.push_log(format!(
                        "[quickai] {} does not pull models by name: `{reference}` was not fetched.",
                        self.engine_name()
                    ));
                }
            }
            Fetch::Files(download) => self.fetch_files(download.clone(), ctx),
        }
    }

    /// Downloads the files of a button into the models folder, as one job.
    fn fetch_files(&mut self, download: Download, ctx: &egui::Context) {
        if self.download.is_some() {
            return;
        }
        let ctx = ctx.clone();
        self.bench_fetching(&download);
        self.download_source = Some(DownloadSource::of(&download));
        self.download = Some(downloader::spawn_files(
            download,
            self.models_dir.clone(),
            move || ctx.request_repaint(),
        ));
    }

    /// What the look-up said, and the models it found, in sections by type with a size filter: a
    /// button for each way to fetch one.
    pub(super) fn show_lookup_results(&mut self, ui: &mut egui::Ui) {
        if !self.lookup.open {
            return;
        }
        ui.add_space(4.0);
        // grey while it looks and when it has found, red when it could not look
        let colour = if self.lookup.failed {
            super::theme::palette().bad
        } else {
            super::theme::palette().muted
        };
        // A look-up that is over and found models says how many in the heading of what it found: its sentence is
        // read while it looks, when it found nothing and when it could not look. What it adds to the count (there
        // are more than it lists) stays, at the end of the row of sizes.
        let found =
            !self.lookup.entries.is_empty() && !self.lookup.running() && !self.lookup.failed;
        let more = found
            .then(|| {
                self.lookup
                    .status
                    .split_once(". ")
                    .map(|(_, more)| more.to_string())
            })
            .flatten();
        if !found {
            // what it says wraps at the width of the card: a sentence that is longer than the window (the reason a
            // look-up failed, with the proxy's words in it) would widen the cards and the page with it
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(&self.lookup.status)
                        .small()
                        .color(colour),
                );
                if self.lookup.running() {
                    ui.add(egui::Spinner::new().size(12.0));
                }
            });
        }
        if let Some(note) = &self.lookup.note {
            show_note(ui, note);
        }
        if self.lookup.entries.is_empty() {
            return;
        }
        let busy = self.download.is_some() || self.named.pulling();
        let types = self.registry.vocabulary().model_types().to_vec();
        let sizes = self.registry.vocabulary().size_classes().to_vec();
        let bundle_labels: Vec<BundleWords> =
            self.engine_doc().map(bundle_words).unwrap_or_default();
        let entries = std::mem::take(&mut self.lookup.entries);
        let mut size = self.lookup.size.clone();
        let mut chosen: Option<Fetch> = None;
        // An engine that keeps its files in folders of their own knows which of them are there: a button
        // whose files all are, at the size the hub says, is marked and does nothing. llama.cpp's results
        // are what they always were.
        let have = self.runs_alone().then(|| self.catalog.have.clone());
        let installed_fn = |offer: &Offer| {
            have.as_ref()
                .is_some_and(|have| super::catalog_view::installed(offer, have, &[]))
        };
        let ram = self.total_ram_bytes;
        let look = RowLook {
            installed: &installed_fn,
            ram,
            busy,
            bundles: &bundle_labels,
        };
        let p = super::theme::palette();
        let words = self.lookup.words.trim().to_string();
        let mut kind = self.lookup.kind.clone();
        let mut order = self.lookup.order.unwrap_or(if words.is_empty() {
            ResultOrder::Downloads
        } else {
            ResultOrder::Best
        });
        let sections = arrange(&entries, &types, size.as_deref());
        // the kinds that were found, to narrow the list to one
        if kind
            .as_deref()
            .is_some_and(|k| !sections.iter().any(|s| s.type_id == k))
        {
            kind = None;
        }

        // one heading: what was looked for and how many were found, the order and the size (on a line of their own when
        // the place is narrow: the results in ComfyUI's page)
        ui.add_space(8.0);
        let heading = if words.is_empty() {
            "Most downloaded".to_string()
        } else {
            format!("Results for \"{words}\"")
        };
        let found = format!("· {}", entries.len());
        let size_label = size
            .as_deref()
            .and_then(|id| sizes.iter().find(|c| c.id == id))
            .map_or_else(|| "Any".to_string(), |c| c.label.clone());
        let size_text = format!("Size: {size_label}");
        let sort_text = format!("Sort: {}", order.words());
        let gap = ui.spacing().item_spacing.x;
        let needed = super::theme::text_width(ui, &heading, egui::TextStyle::Body)
            + super::theme::text_width(ui, &found, egui::TextStyle::Body)
            + 12.0
            + gap
            + super::theme::combo_width(ui, &sort_text)
            + if sizes.is_empty() {
                0.0
            } else {
                super::theme::combo_width(ui, &size_text) + gap
            }
            + 4.0 * gap;
        let one_line = ui.available_width() >= needed;
        let mut controls = |ui: &mut egui::Ui| {
            if !sizes.is_empty() {
                egui::ComboBox::from_id_salt("lookup_size")
                    .selected_text(&size_text)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(size.is_none(), "Any").clicked() {
                            size = None;
                        }
                        for class in &sizes {
                            let on = size.as_deref() == Some(class.id.as_str());
                            let mut item = ui.selectable_label(on, &class.label);
                            if !class.help.is_empty() {
                                item = item.on_hover_text(&class.help);
                            }
                            if item.clicked() {
                                size = Some(class.id.clone());
                            }
                        }
                    });
            }
            egui::ComboBox::from_id_salt("lookup_order")
                .selected_text(&sort_text)
                .show_ui(ui, |ui| {
                    for choice in ResultOrder::ALL {
                        if choice == ResultOrder::Best && words.is_empty() {
                            continue;
                        }
                        if ui
                            .selectable_label(order == choice, choice.words())
                            .clicked()
                        {
                            order = choice;
                        }
                    }
                });
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&heading)
                    .color(p.text)
                    .family(super::theme::semibold(ui)),
            );
            ui.label(egui::RichText::new(&found).color(p.faint));
            if self.lookup.running() {
                ui.add(egui::Spinner::new().size(12.0));
            }
            if one_line {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    controls(ui)
                });
            }
        });
        if !one_line {
            ui.horizontal(|ui| controls(ui));
        }
        if let Some(more) = &more {
            ui.label(super::theme::muted(more));
        }
        // All is what the engine runs: a section that is folded (what it does not run) is shown by its own chip, unless
        // nothing else was found
        let any_open = sections.iter().any(|s| s.open);
        let in_all = |s: &&Section| s.open || !any_open;
        if sections.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                let count: usize = sections
                    .iter()
                    .filter(in_all)
                    .map(|s| s.entries.len())
                    .sum();
                if ui
                    .selectable_label(kind.is_none(), format!("All {count}"))
                    .clicked()
                {
                    kind = None;
                }
                for section in &sections {
                    let on = kind.as_deref() == Some(section.type_id.as_str());
                    let mut chip = ui.selectable_label(
                        on,
                        format!("{} {}", section.label, section.entries.len()),
                    );
                    if !section.help.is_empty() {
                        chip = chip.on_hover_text(&section.help);
                    }
                    if chip.clicked() {
                        kind = Some(section.type_id.clone());
                    }
                }
            });
        }
        ui.add_space(4.0);

        // the results in one list, in the order chosen: best first is the order they are kept in
        let mut shown: Vec<(usize, Vec<usize>)> = sections
            .iter()
            .filter(|s| {
                kind.as_deref()
                    .map_or_else(|| in_all(s), |k| s.type_id == k)
            })
            .flat_map(|s| s.entries.iter().map(|e| (e.entry, e.offers.clone())))
            .collect();
        shown.sort_by_key(|(i, _)| *i);
        match order {
            ResultOrder::Best => {}
            ResultOrder::Downloads => {
                shown.sort_by_key(|(i, _)| std::cmp::Reverse(entries[*i].downloads.unwrap_or(0)))
            }
            ResultOrder::Smallest => shown.sort_by_key(|(i, offers)| {
                main_of(&entries[*i], offers)
                    .and_then(|o| o.bytes)
                    .unwrap_or(u64::MAX)
            }),
        }
        // in the window of Add a model, the rest of its height but the line under them: as big as the person makes it;
        // in a page of the main window (a first run's, ComfyUI's), which scrolls itself, all of them: a list that
        // scrolled in a page that scrolls had the page's room and showed a part of it
        let room = if ui.ctx().viewport_id() == super::add_model::add_model_viewport() {
            (ui.available_height() - 40.0).max(160.0)
        } else {
            f32::INFINITY
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(room)
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() - 10.0);
                if shown.is_empty() {
                    ui.label(
                        egui::RichText::new("Nothing found in this size range.")
                            .small()
                            .color(p.muted),
                    );
                }
                for (i, offers) in &shown {
                    let entry = &entries[*i];
                    let Some(main) = main_of(entry, offers) else {
                        continue;
                    };
                    let row = Row {
                        entry,
                        main,
                        choices: offers.iter().map(|o| &entry.offers[*o]).collect(),
                        title: entry
                            .name
                            .split_once('/')
                            .map_or(entry.name.as_str(), |(_, name)| name)
                            .to_string(),
                        by: String::new(),
                        line: result_line(entry),
                        star: false,
                        abilities: false,
                        expandable: false,
                    };
                    if let Some(RowAction::Fetch(fetch)) = model_row::show(ui, &row, &look, false) {
                        chosen = Some(fetch);
                    }
                }
            });
        self.lookup.entries = entries;
        self.lookup.size = size;
        self.lookup.kind = kind;
        self.lookup.order = Some(order);
        if let Some(fetch) = chosen {
            self.fetch_offer(&fetch, ui.ctx());
        }
    }
}

/// Where an entry that was found goes among those found before it, which are best first: after every one whose name
/// answers the words as well or better, so that equals keep the order the site gave them (the most downloaded first).
pub(super) fn ranked_place(entries: &[Entry], words: &str, entry: &Entry) -> usize {
    let rank = relevance(words, &entry.name);
    entries
        .iter()
        .position(|e| relevance(words, &e.name) < rank)
        .unwrap_or(entries.len())
}

/// What the look-up said of the words, in a quiet box above the results: a sentence, not an alarm.
fn show_note(ui: &mut egui::Ui, note: &str) {
    let p = super::theme::palette();
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                super::icons::paint(ui.painter(), r, super::icons::Icon::Info, p.muted);
                ui.add(egui::Label::new(egui::RichText::new(note).small().color(p.body)).wrap());
            });
        });
    ui.add_space(4.0);
}

/// What a window says of a bundle of an engine: its name, the words of its row, what it is, and what
/// its parts are called.
#[derive(Clone, Debug)]
pub(super) struct BundleWords {
    pub id: String,
    /// `with projector`.
    pub label: String,
    pub help: String,
    /// `projector`.
    pub parts: Vec<String>,
}

/// The bundles of an engine, as the rows of a window name them.
pub(super) fn bundle_words(engine: &quickai_core::registry::Engine) -> Vec<BundleWords> {
    bundles(engine)
        .iter()
        .map(|b| BundleWords {
            id: b.id.clone(),
            label: b.label.clone(),
            help: b.help.clone(),
            parts: b.parts.iter().map(|p| p.label.clone()).collect(),
        })
        .collect()
}

/// What an entry says besides the name, on one line: `tools · vision · 3.3M downloads`.
pub(super) fn facts_line(entry: &Entry) -> String {
    let mut facts: Vec<String> = Vec::new();
    if let Some(card) = &entry.card {
        facts.extend(
            [&card.by, &card.params, &card.license]
                .into_iter()
                .filter(|f| !f.is_empty())
                .cloned(),
        );
        if let Some(context) = card.context {
            facts.push(format!("{} context", compact_count(context)));
        }
    }
    facts.extend(entry.tags.iter().cloned());
    if let Some(count) = entry.downloads {
        if count > 0 {
            facts.push(format!("{} downloads", compact_count(count)));
        }
    }
    if let Some(count) = entry.likes {
        if count > 0 {
            facts.push(format!("{} likes", compact_count(count)));
        }
    }
    facts.join(" · ")
}

/// 433, 10.5K, 3.3M, 1.2B.
pub(super) fn compact_count(count: u64) -> String {
    let (value, unit) = match count {
        0..=999 => return count.to_string(),
        1_000..=999_999 => (count as f64 / 1_000.0, "K"),
        1_000_000..=999_999_999 => (count as f64 / 1_000_000.0, "M"),
        _ => (count as f64 / 1_000_000_000.0, "B"),
    };
    let text = format!("{value:.1}");
    format!("{}{unit}", text.strip_suffix(".0").unwrap_or(&text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::lookup::present::Piece;

    fn piece(url: &str, name: &str, store_as: &str) -> Piece {
        Piece {
            kind: None,
            label: "model".into(),
            url: url.into(),
            store_as: store_as.into(),
            name: name.into(),
            size: Some(1),
            sha256: None,
            place: None,
        }
    }

    /// Y09: what the window remembers of a download is where its weights are in the repository when that is not the
    /// top, and the revision when it is not `main`: what an export needs to be fetched from the same file.
    #[test]
    fn p3_y09_a_download_remembers_the_path_in_the_repository_and_the_revision() {
        let download = |p: Piece| Download {
            files: vec![p],
            bundle: None,
            source: "huggingface:acme/Wan".into(),
            mode: None,
        };
        let deep = DownloadSource::of(&download(piece(
            "https://huggingface.co/acme/Wan/resolve/abc123/VAE/Wan2.2_VAE.safetensors",
            "VAE/Wan2.2_VAE.safetensors",
            "Wan2.2_VAE.safetensors",
        )));
        assert_eq!(deep.place, "huggingface:acme/Wan");
        assert_eq!(deep.path.as_deref(), Some("VAE/Wan2.2_VAE.safetensors"));
        assert_eq!(deep.revision.as_deref(), Some("abc123"));
        let top = DownloadSource::of(&download(piece(
            "https://huggingface.co/acme/Wan/resolve/main/m.gguf",
            "m.gguf",
            "m.gguf",
        )));
        assert_eq!((top.path, top.revision), (None, None));
    }

    #[test]
    fn results_of_a_search_go_best_first_and_equals_keep_the_order_they_came_in() {
        let found = |name: &str| Entry {
            key: name.to_string(),
            name: name.to_string(),
            description: String::new(),
            downloads: None,
            likes: None,
            tags: Vec::new(),
            model_type: None,
            offers: Vec::new(),
            card: None,
        };
        let words = "mintujohnson/Llama-3.2-3B-Malayalam-Instruct";
        // as a search of each word brings them: the most downloaded of a word first
        let arrived = [
            "mradermacher/qwen3-32b-malayalam-v1-GGUF",
            "mradermacher/gemma3-malayalam-instruct-GGUF",
            "mradermacher/Llama-3.2-3B-Malayalam-Instruct-GGUF",
            "unsloth/Llama-3.2-3B-Instruct-GGUF",
            "sivaharibyte/Llama-3.2-3B-Malayalam-Instruct-Q4_K_M-GGUF",
        ];
        let mut entries: Vec<Entry> = Vec::new();
        for name in arrived {
            let entry = found(name);
            let at = ranked_place(&entries, words, &entry);
            entries.insert(at, entry);
        }
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "mradermacher/Llama-3.2-3B-Malayalam-Instruct-GGUF",
                "sivaharibyte/Llama-3.2-3B-Malayalam-Instruct-Q4_K_M-GGUF",
                "unsloth/Llama-3.2-3B-Instruct-GGUF",
                "mradermacher/gemma3-malayalam-instruct-GGUF",
                "mradermacher/qwen3-32b-malayalam-v1-GGUF",
            ]
        );
    }

    #[test]
    fn counts_are_written_short() {
        for (count, shown) in [
            (0, "0"),
            (433, "433"),
            (999, "999"),
            (1_000, "1K"),
            (10_455, "10.5K"),
            (999_949, "999.9K"),
            (3_301_841, "3.3M"),
            (88_321_876, "88.3M"),
            (2_000_000_000, "2B"),
        ] {
            assert_eq!(compact_count(count), shown, "{count}");
        }
    }
}
