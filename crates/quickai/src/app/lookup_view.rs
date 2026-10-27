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
use quickai_core::lookup::present::{Download, Entry, Fetch, Offer};
use quickai_core::lookup::view::{arrange, Section};
use quickai_core::models::{bundles, lookup_rules, lookups};
use quickai_core::registry::engine::LookupKind;

use super::App;
use crate::downloader;
use crate::hardware::Fit;
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
        let fit_fn = |_: &Offer| Fit::Comfortable;
        let decor = have.as_ref().map(|_| Decor {
            installed: &installed_fn,
            fit: &fit_fn,
            ram: 0,
        });
        egui::CollapsingHeader::new(format!("Results ({})", entries.len()))
            .default_open(true)
            .show(ui, |ui| {
                super::theme::compact(ui);
                if !sizes.is_empty() || more.is_some() {
                    ui.horizontal_wrapped(|ui| {
                        if !sizes.is_empty() {
                            ui.label(egui::RichText::new("Size:").small());
                            if ui.selectable_label(size.is_none(), "Any").clicked() {
                                size = None;
                            }
                        }
                        for class in &sizes {
                            let on = size.as_deref() == Some(class.id.as_str());
                            let mut chip = ui.selectable_label(on, &class.label);
                            if !class.help.is_empty() {
                                chip = chip.on_hover_text(&class.help);
                            }
                            if chip.clicked() {
                                size = Some(class.id.clone());
                            }
                        }
                        if let Some(more) = &more {
                            ui.add_space(8.0);
                            ui.label(super::theme::muted(more));
                        }
                    });
                }
                let sections = arrange(&entries, &types, size.as_deref());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(260.0)
                    .show(ui, |ui| {
                        if sections.is_empty() {
                            ui.label(
                                egui::RichText::new("Nothing found in this size range.")
                                    .small()
                                    .color(super::theme::palette().muted),
                            );
                        }
                        for section in &sections {
                            show_section(
                                ui,
                                section,
                                &entries,
                                &bundle_labels,
                                busy,
                                &mut chosen,
                                decor.as_ref(),
                            );
                        }
                    });
            });
        self.lookup.entries = entries;
        self.lookup.size = size;
        if let Some(fetch) = chosen {
            self.fetch_offer(&fetch, ui.ctx());
        }
    }
}

/// What a window that knows more than a look-up does adds to an entry: which buttons fetch what is
/// already there, and how the size of each sits in this machine's memory.
pub(super) struct Decor<'a> {
    pub installed: &'a dyn Fn(&Offer) -> bool,
    pub fit: &'a dyn Fn(&Offer) -> Fit,
    /// The memory of this machine, for the sentence that says it.
    pub ram: u64,
}

/// One section: the models of a type, each with its rows of buttons.
pub(super) fn show_section(
    ui: &mut egui::Ui,
    section: &Section,
    entries: &[Entry],
    bundle_labels: &[BundleWords],
    busy: bool,
    chosen: &mut Option<Fetch>,
    decor: Option<&Decor<'_>>,
) {
    let mut body = |ui: &mut egui::Ui| {
        for shown in &section.entries {
            let entry = &entries[shown.entry];
            show_entry(ui, entry, &shown.offers, bundle_labels, busy, chosen, decor);
        }
    };
    if section.label.is_empty() {
        body(ui);
        return;
    }
    let response =
        egui::CollapsingHeader::new(format!("{} ({})", section.label, section.entries.len()))
            .id_salt(("lookup_section", &section.type_id))
            .default_open(section.open)
            .show(ui, |ui| body(ui));
    if !section.help.is_empty() {
        response.header_response.on_hover_text(&section.help);
    }
}

/// One model: its name, what is said of it, and a row of buttons for each way of fetching it.
/// A check mark, drawn: the fonts egui ships have no `✓`, and a missing symbol is shown as a box.
fn check_mark(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let at = |x: f32, y: f32| {
        egui::pos2(
            rect.min.x + x * rect.width(),
            rect.min.y + y * rect.height(),
        )
    };
    let stroke = egui::Stroke::new(1.6, color);
    ui.painter()
        .line_segment([at(0.05, 0.55), at(0.38, 0.9)], stroke);
    ui.painter()
        .line_segment([at(0.38, 0.9), at(0.95, 0.1)], stroke);
}

pub(super) fn show_entry(
    ui: &mut egui::Ui,
    entry: &Entry,
    shown: &[usize],
    bundle_labels: &[BundleWords],
    busy: bool,
    chosen: &mut Option<Fetch>,
    decor: Option<&Decor<'_>>,
) {
    let weak = ui.visuals().weak_text_color();
    // the rows, in the order the entry has them (a look-up puts the model alone first, the list the bundles)
    let mut groups: Vec<Row<'_>> = Vec::new();
    for offer in shown.iter().map(|i| &entry.offers[*i]) {
        let group = row_of(offer);
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    let has_bundles = entry.offers.iter().any(|o| o.bundle.is_some());
    let has_only = entry.offers.iter().any(|o| o.only().is_some());
    // what a row of buttons says in front of them, what resting the pointer on those words says, and the offers it has
    let parts_of = |group: Row<'_>| -> (String, String, Vec<&Offer>) {
        let in_group: Vec<&Offer> = shown
            .iter()
            .map(|i| &entry.offers[*i])
            .filter(|o| row_of(o) == group)
            .collect();
        let entry_bundles: Vec<&str> = entry
            .offers
            .iter()
            .filter_map(|o| o.bundle.as_deref())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let (label, hover) = match group {
            Row::Only(_) => (only_label(&in_group), only_hover(&in_group)),
            // beside a file of a kind by itself the model's own row says that it is the model
            Row::Model if has_only && !has_bundles => ("Get model:".to_string(), String::new()),
            Row::Model => (
                row_label(None, has_bundles, &in_group, bundle_labels),
                row_hover(None, &entry_bundles, bundle_labels),
            ),
            Row::Bundle(id) => (
                row_label(Some(id), has_bundles, &in_group, bundle_labels),
                row_hover(Some(id), &entry_bundles, bundle_labels),
            ),
        };
        (label, hover, in_group)
    };
    // one row of buttons: the words and the buttons of a row, in whatever row they are put
    let mut show_group = |ui: &mut egui::Ui, group: Row<'_>| {
        let (label, hover, in_group) = parts_of(group);
        let words = ui.label(egui::RichText::new(label).small());
        if !hover.is_empty() {
            words.on_hover_text(hover);
        }
        for offer in in_group {
            let installed = decor.is_some_and(|d| (d.installed)(offer));
            let fit = decor.map(|d| (d.fit)(offer));
            let mut text = egui::RichText::new(&offer.label);
            if offer.recommended {
                text = text.strong();
            }
            if fit == Some(Fit::TooBig) {
                text = text.color(weak);
            }
            let hover = if installed {
                "Already in your models folder.".to_string()
            } else {
                match (fit, decor) {
                    (Some(Fit::TooBig), Some(d)) => format!(
                        "{} This is more than the memory of this computer ({}).",
                        offer.hover,
                        crate::hardware::format_bytes(d.ram)
                    ),
                    (Some(Fit::Tight), Some(d)) => format!(
                        "{} It will fit in the memory of this computer ({}), with little to spare.",
                        offer.hover,
                        crate::hardware::format_bytes(d.ram)
                    ),
                    _ => offer.hover.clone(),
                }
            };
            if ui
                .add_enabled(!busy && !installed, egui::Button::new(text).small())
                .on_hover_text(hover)
                .clicked()
            {
                *chosen = Some(offer.fetch.clone());
            }
        }
    };
    // A model that has one row of buttons has them after its name, when the name, what is said of it and the whole row
    // of buttons fit on one line, and under it when they do not: a line of a model with its sizes beside it is a row of
    // a list, and a line of its own for the buttons was a third of the height of the list; but a row that is broken
    // in the middle, a button left alone on the next line, is worse than the line of its own.
    let alone = (groups.len() == 1).then(|| groups[0]).filter(|group| {
        let small = egui::TextStyle::Small.resolve(ui.style());
        let body = egui::TextStyle::Body.resolve(ui.style());
        let gap = ui.spacing().item_spacing.x;
        let width = |text: &str, font: &egui::FontId| {
            ui.painter()
                .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        };
        let facts = facts_line(entry);
        let mut need = width(&entry.name, &body) + gap + 8.0;
        if !facts.is_empty() {
            need += width(&facts, &small) + gap;
        }
        if let Some(decor) = decor {
            let offers: Vec<&Offer> = entry.offers.iter().collect();
            if offers.iter().any(|o| (decor.installed)(o)) {
                need += 10.0 + width("Installed", &small) + 2.0 * gap;
            }
            if !offers.is_empty() && offers.iter().all(|o| (decor.fit)(o) == Fit::TooBig) {
                need += width("Too big for this computer", &small) + gap;
            }
        }
        let (label, _, offers) = parts_of(*group);
        need += width(&label, &small) + gap;
        let padding = 2.0 * ui.spacing().button_padding.x;
        for offer in offers {
            need += width(&offer.label, &small) + padding + gap;
        }
        need <= ui.available_width()
    });
    ui.push_id(&entry.key, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&entry.name).strong());
            let facts = facts_line(entry);
            if !facts.is_empty() {
                ui.label(
                    egui::RichText::new(facts)
                        .small()
                        .color(super::theme::palette().muted),
                );
            }
            if let Some(decor) = decor {
                let offers: Vec<&Offer> = entry.offers.iter().collect();
                if offers.iter().any(|o| (decor.installed)(o)) {
                    let green = super::theme::palette().ok;
                    check_mark(ui, green);
                    ui.label(egui::RichText::new("Installed").small().color(green));
                }
                if !offers.is_empty() && offers.iter().all(|o| (decor.fit)(o) == Fit::TooBig) {
                    ui.label(
                        egui::RichText::new("Too big for this computer")
                            .small()
                            .color(super::theme::palette().warn),
                    );
                }
            }
            if let Some(group) = alone {
                ui.add_space(8.0);
                show_group(ui, group);
            }
        });
        if let Some(card) = &entry.card {
            let chips = card_chips(card);
            if !chips.is_empty() {
                ui.label(egui::RichText::new(chips.join(" · ")).small().color(weak));
            }
        }
        if !entry.description.is_empty() {
            // the whole sentence, on as many lines as it takes: a note that ends in an ellipsis
            // leaves a person looking for the rest of it, and a list cannot be scrolled sideways
            ui.add(egui::Label::new(egui::RichText::new(&entry.description).small()).wrap());
        }
        if alone.is_none() {
            for group in groups.iter().copied() {
                ui.horizontal_wrapped(|ui| show_group(ui, group));
            }
        }
        ui.add_space(6.0);
    });
}

/// The row of buttons an offer is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row<'a> {
    /// The weights alone.
    Model,
    /// The weights with the files of a bundle, by its id.
    Bundle(&'a str),
    /// A file of a kind of its own (a VAE, a text encoder), fetched without any model, by its kind.
    Only(&'a str),
}

fn row_of(offer: &Offer) -> Row<'_> {
    if let Some(piece) = offer.only() {
        return Row::Only(piece.kind.as_deref().unwrap_or_default());
    }
    match offer.bundle.as_deref() {
        Some(id) => Row::Bundle(id),
        None => Row::Model,
    }
}

/// The words in front of the row of a kind of file by itself: `Get VAE only:`.
fn only_label(offers: &[&Offer]) -> String {
    let what = offers
        .first()
        .and_then(|o| o.only())
        .map_or("file", |piece| piece.label.as_str());
    format!("Get {what} only:")
}

/// What a person who rests the pointer on those words is told: that it is the file and not a model, and
/// where it is kept.
fn only_hover(offers: &[&Offer]) -> String {
    let Some(piece) = offers.first().and_then(|o| o.only()) else {
        return String::new();
    };
    let place = piece
        .place
        .as_deref()
        .map(|p| format!(" in the {p} folder"))
        .unwrap_or_default();
    format!(
        "Just the {}, without a model: it is kept{place} under the name it has, and a workflow that needs one names it by that name.",
        piece.label
    )
}

/// What a curated entry says of itself besides its name, in short words: what it reads and what
/// else is worth knowing (`sees images`, `thinking`).
pub(super) fn card_chips(card: &quickai_core::lookup::present::Card) -> Vec<String> {
    let mut chips: Vec<String> = card
        .input
        .iter()
        .filter_map(|m| match m.as_str() {
            "image" => Some("sees images".to_string()),
            "audio" => Some("hears audio".to_string()),
            "video" => Some("reads video".to_string()),
            _ => None,
        })
        .collect();
    chips.extend(card.features.iter().cloned());
    chips
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

/// The words in front of a row: `Pull:` for names, `Get:` for files, and for a model that has
/// bundles `Get model only:` and `Get with projector:`.
pub(super) fn row_label(
    group: Option<&str>,
    has_bundles: bool,
    offers: &[&Offer],
    words: &[BundleWords],
) -> String {
    let verb = match offers.first().map(|o| &o.fetch) {
        Some(Fetch::Reference(_)) => "Pull",
        _ => "Get",
    };
    match group {
        None if has_bundles => format!("{verb} model only:"),
        None => format!("{verb}:"),
        Some(id) => {
            let label = words
                .iter()
                .find(|b| b.id == id)
                .map_or(id, |b| b.label.as_str());
            format!("{verb} {label}:")
        }
    }
}

/// What a person who rests the pointer on a row's words is told. The row of a bundle says what its
/// extra files are for; the row of the weights alone says what it leaves out, and that leaving it out
/// is theirs to choose. Nothing for a row that has no sibling to be told apart from.
pub(super) fn row_hover(
    group: Option<&str>,
    entry_bundles: &[&str],
    words: &[BundleWords],
) -> String {
    match group {
        Some(id) => {
            let help = words
                .iter()
                .find(|b| b.id == id)
                .map(|b| b.help.as_str())
                .unwrap_or_default();
            if help.is_empty() {
                String::new()
            } else {
                format!("{help} You can fetch the model without it: it is your choice.")
            }
        }
        None if !entry_bundles.is_empty() => {
            let mut parts: Vec<&str> = Vec::new();
            for b in words
                .iter()
                .filter(|b| entry_bundles.contains(&b.id.as_str()))
            {
                for p in &b.parts {
                    if !parts.contains(&p.as_str()) {
                        parts.push(p);
                    }
                }
            }
            if parts.is_empty() {
                return String::new();
            }
            let named = parts.join(" and the ");
            format!(
                "Only the model, without the {named}: smaller, but whatever needs the {named} (the other row says what it is for) will not work. It is your choice."
            )
        }
        None => String::new(),
    }
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

    fn words() -> Vec<BundleWords> {
        vec![BundleWords {
            id: "vision".into(),
            label: "with projector".into(),
            help: "A vision model reads pictures through a second file, its projector.".into(),
            parts: vec!["projector".into()],
        }]
    }

    #[test]
    fn p3_j19b_the_rows_of_a_bundle_say_what_the_extra_file_is_for_and_that_the_choice_is_the_persons(
    ) {
        let words = words();
        // the row of the bundle says what its file is, and that it can be left out
        let with = row_hover(Some("vision"), &["vision"], &words);
        assert!(
            with.starts_with("A vision model reads pictures through a second file, its projector.")
                && with.contains("You can fetch the model without it: it is your choice."),
            "{with}"
        );
        // the row of the weights alone says what it leaves out
        let alone = row_hover(None, &["vision"], &words);
        assert!(
            alone.starts_with(
                "Only the model, without the projector: smaller, but whatever needs the projector"
            ) && alone.ends_with("It is your choice."),
            "{alone}"
        );
        // a model with no bundle has one row and nothing to explain; a bundle nobody knows says nothing
        assert_eq!(row_hover(None, &[], &words), "");
        assert_eq!(row_hover(Some("draft"), &["draft"], &words), "");
        assert_eq!(row_hover(None, &["draft"], &words), "");
        // two bundles of one model: the parts of both are named once each
        let mut two = words.clone();
        two.push(BundleWords {
            id: "speculative".into(),
            label: "with draft model".into(),
            help: String::new(),
            parts: vec!["draft model".into(), "projector".into()],
        });
        let both = row_hover(None, &["vision", "speculative"], &two);
        assert!(
            both.contains("without the projector and the draft model:"),
            "{both}"
        );
        // and the words of the rows are the ones they always were
        let offers: Vec<&Offer> = Vec::new();
        assert_eq!(row_label(None, true, &offers, &words), "Get model only:");
        assert_eq!(
            row_label(Some("vision"), true, &offers, &words),
            "Get with projector:"
        );
        assert_eq!(row_label(None, false, &offers, &words), "Get:");
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
