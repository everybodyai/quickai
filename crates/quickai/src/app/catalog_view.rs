//! The recommended models: the list the application ships, in a window of its own with a way to
//! find one (docs/catalog.md). What the list says is a document (`kind = "catalog"`); what an engine
//! offers of it is made by `quickai_core::models::recommended` into the same entries the look-up
//! draws, and the same code draws them and fetches what a button says. This adds what only a list
//! that is known can add: the choice of what to look at (Start here, a type, words, a size), which
//! buttons fetch what is already in the models folder, and how big each is for this machine.
//!
//! Nothing is asked of any site to open it or to browse it: the list is in the application.

use std::collections::BTreeMap;
use std::path::Path;

use eframe::egui;
use quickai_core::lookup::present::{Entry, Fetch, Offer};
use quickai_core::lookup::view::{browse, Browse, Scope};
use quickai_core::models::{find_listed, recommended};

use super::lookup_view::{bundle_words, show_entry, show_section, BundleWords, Decor};
use super::{widgets, App};
use crate::hardware::{self, Fit};

/// What the window holds between frames.
#[derive(Default)]
pub struct CatalogView {
    /// The window is on the screen.
    pub open: bool,
    /// What is looked at: where, which words, which size.
    pub browse: Browse,
    /// What the engine offers of the list, made when the window opens.
    pub entries: Vec<Entry>,
    /// What is in the models folders, by file name, with its size: made when the window opens and
    /// when a download ends.
    pub have: BTreeMap<String, u64>,
    /// The search box takes the keyboard on the first frame of an opened window, so that a person can
    /// type at once.
    pub focus_search: bool,
    /// What the last download said when it failed. The log that holds it is under this window, so the
    /// window says it too, until the next click on a size.
    pub note: Option<String>,
    /// What the list was last drawn for. A list that is not what was drawn (another page, other words,
    /// another size, or a window just opened) is drawn from its top: the toolkit keeps the scroll
    /// position of the list before it, and a page that opens at its end looks as if it had no more
    /// than its last few models.
    pub drawn: Option<Browse>,
}

/// Whether what a button fetches is already there: every file in the models folders at the size the
/// list says (one that says none is there by its name), or the name among those the engine lists.
pub fn installed(offer: &Offer, have: &BTreeMap<String, u64>, listed: &[String]) -> bool {
    match &offer.fetch {
        Fetch::Files(download) => {
            !download.files.is_empty()
                && download.files.iter().all(|piece| {
                    have.get(&have_key(piece))
                        .is_some_and(|size| piece.size.is_none_or(|wanted| wanted == *size))
                })
        }
        Fetch::Reference(reference) => {
            find_listed(listed.iter().map(String::as_str), reference).is_some()
        }
    }
}

/// What `have` calls a file a piece fetches: its name, or its name in the folder it is kept in.
pub fn have_key(piece: &quickai_core::lookup::present::Piece) -> String {
    match &piece.place {
        Some(place) => format!("{place}/{}", piece.store_as),
        None => piece.store_as.clone(),
    }
}

/// The model files of a folder with their sizes (a folder that is not there has none), under the name
/// they have in the folder, or `<in_folder>/<name>` when it is one of the folders of the models folder.
fn files_in(folder: &Path, in_folder: Option<&str>, into: &mut BTreeMap<String, u64>) {
    let Ok(read) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        if lower.ends_with(".gguf") || lower.ends_with(".safetensors") {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    let key = match in_folder {
                        Some(folder) => format!("{folder}/{name}"),
                        None => name,
                    };
                    into.insert(key, meta.len());
                }
            }
        }
    }
}

impl App {
    /// Whether the list has a model this engine offers: only then is there a button for it.
    pub(super) fn can_recommend(&self) -> bool {
        self.registry
            .catalog()
            .for_engine(&self.engine)
            .next()
            .is_some()
    }

    /// Opens the window on the list of this engine. What it shows first is what the list says to
    /// start with, or everything when it says none.
    pub(super) fn open_catalog(&mut self) {
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let entries = recommended(&self.registry, engine);
        let start = entries
            .iter()
            .any(|e| e.card.as_ref().is_some_and(|c| c.start));
        self.catalog = CatalogView {
            open: true,
            browse: Browse {
                scope: if start { Scope::Start } else { Scope::All },
                ..Browse::default()
            },
            entries,
            have: BTreeMap::new(),
            focus_search: true,
            note: None,
            drawn: None,
        };
        self.refresh_catalog_have();
    }

    /// Looks again at what the models folders hold.
    pub(super) fn refresh_catalog_have(&mut self) {
        let mut have = BTreeMap::new();
        files_in(&self.bundled_models_dir, None, &mut have);
        files_in(&self.models_dir, None, &mut have);
        // the folders of the models folder, for an engine that keeps its files by the folder they are in
        if let Ok(read) = std::fs::read_dir(&self.models_dir) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if quickai_core::lookup::present::is_place(&name) && entry.path().is_dir() {
                    files_in(&entry.path(), Some(&name), &mut have);
                }
            }
        }
        self.catalog.have = have;
    }

    /// The names of the models the engine lists (an engine that keeps its own).
    fn listed_names(&self) -> Vec<String> {
        if self.lists_models() {
            self.models.iter().map(|m| m.filename.clone()).collect()
        } else {
            Vec::new()
        }
    }

    /// The window, each frame it is open.
    pub(super) fn show_catalog_window(&mut self, ctx: &egui::Context) {
        if !self.catalog.open {
            return;
        }
        // Escape closes the window, as it closes anything else that was put over the one beneath
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.catalog.open = false;
            return;
        }
        let engine_name = self.engine_name();
        let types = self.registry.vocabulary().model_types().to_vec();
        let sizes = self.registry.vocabulary().size_classes().to_vec();
        let bundle_labels: Vec<BundleWords> =
            self.engine_doc().map(bundle_words).unwrap_or_default();
        let verified = self.registry.catalog().verified().map(str::to_string);
        let ram = self.total_ram_bytes;
        let listed = self.listed_names();
        let busy = self.download.is_some() || self.named.pull.is_some();
        let have = self.catalog.have.clone();
        let note = self.catalog.note.clone();
        let entries = std::mem::take(&mut self.catalog.entries);
        let mut what = std::mem::take(&mut self.catalog.browse);
        let drawn = self.catalog.drawn.take();
        let mut drawn_now: Option<Browse> = None;
        let mut focus_search = std::mem::take(&mut self.catalog.focus_search);
        let mut chosen: Option<Fetch> = None;
        let mut open = true;

        egui::Window::new("Recommended models")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([744.0, 530.0])
            .default_pos(egui::pos2(8.0, 8.0))
            .show(ctx, |ui| {
                super::theme::compact(ui);
                let mut line = format!("{} models that run with {engine_name}.", entries.len());
                if let Some(day) = &verified {
                    line.push_str(&format!(" The list was checked on {day}."));
                }
                line.push_str(" Nothing is downloaded until you click a size.");
                ui.label(
                    egui::RichText::new(line)
                        .small()
                        .color(super::theme::palette().muted),
                );
                if ram > 0 {
                    ui.label(
                        egui::RichText::new(format!(
                            "This computer has {} of memory: sizes that would not fit are greyed.",
                            hardware::format_bytes(ram)
                        ))
                        .small()
                        .color(super::theme::palette().muted),
                    );
                }
                if let Some(note) = &note {
                    ui.label(
                        egui::RichText::new(note)
                            .small()
                            .color(super::theme::palette().bad),
                    );
                }
                ui.add_space(4.0);
                // two rows that stay as they are (a button that comes and goes would move the chips)
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    // the rest of the row, less the Clear button: the hint is the longest thing in it
                    let room = (ui.available_width() - 64.0).max(160.0);
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut what.words)
                            .hint_text(
                                "a name, a maker, or what you need: image, code, thinking...",
                            )
                            .desired_width(room),
                    );
                    if std::mem::take(&mut focus_search) {
                        field.request_focus();
                    }
                    let clear =
                        ui.add_enabled(!what.words.is_empty(), egui::Button::new("Clear").small());
                    if clear.clicked() {
                        what.words.clear();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Size:").small());
                    if ui.selectable_label(what.size.is_none(), "Any").clicked() {
                        what.size = None;
                    }
                    for class in &sizes {
                        let on = what.size.as_deref() == Some(class.id.as_str());
                        let mut chip = ui.selectable_label(on, &class.label);
                        if !class.help.is_empty() {
                            chip = chip.on_hover_text(&class.help);
                        }
                        if chip.clicked() {
                            what.size = Some(class.id.clone());
                        }
                    }
                });
                ui.separator();

                let shown = browse(&entries, &types, &what);
                let new_list = drawn.as_ref() != Some(&what);
                drawn_now = Some(what.clone());
                let installed_fn = |o: &Offer| installed(o, &have, &listed);
                let fit_fn = |o: &Offer| match o.bytes {
                    Some(b) => hardware::fit(b, ram),
                    None => Fit::Comfortable,
                };
                let decor = Decor {
                    installed: &installed_fn,
                    fit: &fit_fn,
                    ram,
                };
                let height = (ui.available_height() - 26.0).max(160.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        ui.set_min_height(height);
                        let mut choose = |ui: &mut egui::Ui, scope: Scope, label: String| {
                            if ui.selectable_label(what.scope == scope, label).clicked() {
                                what.scope = scope;
                            }
                        };
                        if shown.counts.start > 0 {
                            choose(
                                ui,
                                Scope::Start,
                                format!("★ Start here ({})", shown.counts.start),
                            );
                        }
                        choose(ui, Scope::All, format!("All ({})", shown.counts.all));
                        ui.add_space(4.0);
                        for (id, count) in &shown.counts.types {
                            let label = types
                                .iter()
                                .find(|t| &t.id == id)
                                .map_or(id.clone(), |t| t.label.clone());
                            choose(ui, Scope::Type(id.clone()), format!("{label} ({count})"));
                        }
                    });
                    ui.separator();
                    // A scroll area lays its content out the way the row it stands in does, and this row
                    // goes from left to right: in a column of its own, each section and each card is under
                    // the one before it (beside it, everything but the first was off to the right, where
                    // a list that scrolls only down cannot be reached).
                    ui.vertical(|ui| {
                        // always there, and beside the text and not on it: a list of eighty has to say
                        // that it goes on
                        ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
                        let mut area = egui::ScrollArea::vertical()
                            .id_salt("catalog_entries")
                            .auto_shrink([false, false])
                            .max_height(height);
                        if new_list {
                            area = area.vertical_scroll_offset(0.0);
                        }
                        area.show(ui, |ui| {
                            if shown.sections.is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "Nothing matches. Try other words, or another size.",
                                    )
                                    .small()
                                    .color(super::theme::palette().muted),
                                );
                            }
                            // a single type is a list; several are sections
                            let single = matches!(what.scope, Scope::Type(_));
                            for section in &shown.sections {
                                if single {
                                    for item in &section.entries {
                                        show_entry(
                                            ui,
                                            &entries[item.entry],
                                            &item.offers,
                                            &bundle_labels,
                                            busy,
                                            &mut chosen,
                                            Some(&decor),
                                        );
                                    }
                                } else {
                                    show_section(
                                        ui,
                                        section,
                                        &entries,
                                        &bundle_labels,
                                        busy,
                                        &mut chosen,
                                        Some(&decor),
                                    );
                                }
                            }
                        });
                    });
                });
            });

        self.catalog.entries = entries;
        self.catalog.browse = what;
        self.catalog.drawn = drawn_now;
        if !open {
            self.catalog.open = false;
        }
        // the progress of what is being fetched, where the person is looking
        if self.catalog.open {
            if let Some(job) = &self.download {
                let line = job.progress_line();
                let fraction = job.fraction();
                egui::Window::new("Downloading")
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -12.0])
                    .show(ctx, |ui| {
                        ui.colored_label(super::theme::palette().warn, line);
                        if let Some(fraction) = fraction {
                            widgets::progress_bar(ui, Some(fraction));
                        }
                    });
            }
        }
        if let Some(fetch) = chosen {
            self.catalog.note = None;
            self.fetch_offer(&fetch, ctx);
        }
    }
}
