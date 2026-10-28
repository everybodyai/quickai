//! The recommended models: the list the application ships, in a window of its own with a way to
//! find one (docs/catalog.md). What the list says is a document (`kind = "catalog"`); what an engine
//! offers of it is made by `quickai_core::models::recommended` into the same entries the look-up
//! draws, and the same code draws them and fetches what a button says. This adds what only a list
//! that is known can add: the choice of what to look at (Start here, a type, words, a size), which
//! buttons fetch what is already in the models folder, and how big each is for this machine.
//!
//! The window opens on a page that asks what the person wants to do (the vocabulary's tasks, one or
//! several) and shows the few models that do all of it best for this computer, one button each
//! (`quickai_core::lookup::picks`); the whole list is a click away.
//!
//! Nothing is asked of any site to open it or to browse it: the list is in the application.

use std::collections::BTreeMap;
use std::path::Path;

use eframe::egui;
use quickai_core::lookup::picks;
use quickai_core::lookup::present::{Entry, Fetch, Offer};
use quickai_core::lookup::types::TaskDef;
use quickai_core::lookup::view::{browse, Browse, Scope};
use quickai_core::models::{find_listed, recommended};

use super::icons::{self, Icon};
use super::lookup_view::{bundle_words, show_entry, show_section, BundleWords, Decor};
use super::{theme, widgets, App};
use crate::hardware::{self, Fit};

/// The page of the window that is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// What the person wants to do, and the models that do it best here.
    #[default]
    Tasks,
    /// The whole list, by type, with words and sizes to narrow it.
    List,
}

/// What the window holds between frames.
#[derive(Default)]
pub struct CatalogView {
    /// The window is on the screen.
    pub open: bool,
    pub page: Page,
    /// The ids of the tasks chosen on the first page, in the order of their buttons.
    pub chosen: Vec<String>,
    /// The key of the model of the first page whose other sizes are shown.
    pub sizes_open: Option<String>,
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

    /// Opens the window on the list of this engine: on what the person wants to do, with the first
    /// task chosen, when the list says what its models do; else on the list, at what the list says to
    /// start with, or everything when it says none.
    pub(super) fn open_catalog(&mut self) {
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let entries = recommended(&self.registry, engine);
        let start = entries
            .iter()
            .any(|e| e.card.as_ref().is_some_and(|c| c.start));
        let first_task = picks::offered(self.registry.vocabulary().tasks(), &entries)
            .first()
            .map(|t| t.id.clone());
        self.catalog = CatalogView {
            open: true,
            page: if first_task.is_some() {
                Page::Tasks
            } else {
                Page::List
            },
            chosen: first_task.into_iter().collect(),
            sizes_open: None,
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
        // what was said under the box of the page is not about what this window will pull
        if self.named.pull.is_none() && self.named.queued.is_none() {
            self.named.notice = None;
        }
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
        let busy = self.download.is_some() || self.named.pulling();
        let have = self.catalog.have.clone();
        let note = self.catalog.note.clone();
        let entries = std::mem::take(&mut self.catalog.entries);
        let mut what = std::mem::take(&mut self.catalog.browse);
        let drawn = self.catalog.drawn.take();
        let mut drawn_now: Option<Browse> = None;
        let mut focus_search = std::mem::take(&mut self.catalog.focus_search);
        let mut chosen: Option<Fetch> = None;
        let mut open = true;
        let all_tasks = self.registry.vocabulary().tasks().to_vec();
        let mut page = self.catalog.page;
        let mut tasks_chosen = std::mem::take(&mut self.catalog.chosen);
        let mut sizes_open = self.catalog.sizes_open.take();

        egui::Window::new("Recommended models")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            // the eight tasks in one row and the three picks under them, on a screen that has the room
            .default_size([
                (ctx.content_rect().width() - 16.0).clamp(744.0, 920.0),
                (ctx.content_rect().height() - 16.0).clamp(530.0, 640.0),
            ])
            .default_pos(egui::pos2(8.0, 8.0))
            .show(ctx, |ui| {
                super::theme::compact(ui);
                if page == Page::Tasks {
                    let installed_fn = |o: &Offer| installed(o, &have, &listed);
                    let view = TaskView {
                        entries: &entries,
                        offered: picks::offered(&all_tasks, &entries),
                        all_tasks: &all_tasks,
                        ram,
                        busy,
                        installed: &installed_fn,
                        verified: verified.as_deref(),
                        note: note.as_deref(),
                    };
                    let mut state = TaskState {
                        page: &mut page,
                        chosen: &mut tasks_chosen,
                        sizes_open: &mut sizes_open,
                        browse: &mut what,
                        focus_search: &mut focus_search,
                        fetch: &mut chosen,
                    };
                    show_task_page(ui, &view, &mut state);
                    return;
                }
                if !all_tasks.is_empty()
                    && !picks::offered(&all_tasks, &entries).is_empty()
                    && back_button(ui).clicked()
                {
                    page = Page::Tasks;
                    what.tasks.clear();
                }
                if !what.tasks.is_empty() {
                    let said: Vec<String> = what
                        .tasks
                        .iter()
                        .map(|id| {
                            all_tasks
                                .iter()
                                .find(|t| &t.id == id)
                                .map_or_else(|| id.clone(), |t| t.label.clone())
                        })
                        .collect();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Only the models that do: {}",
                                said.join(", ")
                            ))
                            .small(),
                        );
                        if ui
                            .add(egui::Button::new("Show all").small())
                            .on_hover_text("Show the models whatever they do.")
                            .clicked()
                        {
                            what.tasks.clear();
                        }
                    });
                }
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
                            .id(search_id())
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
        self.catalog.page = page;
        self.catalog.chosen = tasks_chosen;
        self.catalog.sizes_open = sizes_open;
        self.catalog.focus_search |= focus_search;
        if !open {
            self.catalog.open = false;
        }
        // the progress of what is being fetched, where the person is looking
        let mut cancel = false;
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
                        ui.horizontal(|ui| {
                            ui.colored_label(super::theme::palette().warn, line);
                            if ui
                                .small_button("Cancel")
                                .on_hover_text(
                                    "Stop the download. What was fetched so far is thrown away.",
                                )
                                .clicked()
                            {
                                cancel = true;
                            }
                        });
                        if let Some(fraction) = fraction {
                            widgets::progress_bar(ui, Some(fraction));
                        }
                    });
            }
            // a model pulled by the engine: the engine coming up for it, the pull, or why it was not pulled
            let server = self.server_name();
            let pulled = match (&self.named.queued, &self.named.pull) {
                (Some(queued), _) => Some((queued.waiting_line(&server), None, false)),
                (None, Some((job, _))) => Some((job.progress_line(), job.fraction(), false)),
                (None, None) => self.named.notice.clone().map(|n| (n, None, true)),
            };
            if let Some((line, fraction, failed)) = pulled {
                egui::Window::new("Pulling")
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -12.0])
                    .show(ctx, |ui| {
                        let p = super::theme::palette();
                        ui.colored_label(if failed { p.bad } else { p.warn }, line);
                        if fraction.is_some() {
                            widgets::progress_bar(ui, fraction);
                        }
                    });
            }
        }
        if cancel {
            self.cancel_download();
        }
        if let Some(fetch) = chosen {
            self.catalog.note = None;
            self.fetch_offer(&fetch, ctx);
        }
    }
}

/// What the first page is drawn from.
struct TaskView<'a> {
    entries: &'a [Entry],
    /// The tasks a model of the engine does, in the order of their buttons.
    offered: Vec<&'a TaskDef>,
    all_tasks: &'a [TaskDef],
    ram: u64,
    busy: bool,
    installed: &'a dyn Fn(&Offer) -> bool,
    verified: Option<&'a str>,
    note: Option<&'a str>,
}

/// What the first page changes.
struct TaskState<'a> {
    page: &'a mut Page,
    chosen: &'a mut Vec<String>,
    sizes_open: &'a mut Option<String>,
    browse: &'a mut Browse,
    focus_search: &'a mut bool,
    fetch: &'a mut Option<Fetch>,
}

/// The first page: a button for each task, the models that do every task chosen (the best for this
/// computer, a smaller one, the smallest, and the others to start with), and the way to the whole list.
fn show_task_page(ui: &mut egui::Ui, view: &TaskView<'_>, state: &mut TaskState<'_>) {
    let p = theme::palette();
    egui::ScrollArea::vertical()
        .id_salt("catalog_tasks")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - 58.0).max(200.0))
        .show(ui, |ui| {
            // room for the scroll bar beside the cards, not over their edge
            ui.set_max_width(ui.available_width() - 12.0);
            ui.label(
                egui::RichText::new("What do you want to do?")
                    .size(17.0)
                    .color(p.text)
                    .family(theme::semibold(ui)),
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for task in &view.offered {
                    let on = state.chosen.contains(&task.id);
                    let mut tile = task_tile(ui, task, on);
                    if !task.help.is_empty() {
                        tile = tile.on_hover_text(&task.help);
                    }
                    if tile.clicked() {
                        if on {
                            state.chosen.retain(|t| t != &task.id);
                        } else {
                            state.chosen.push(task.id.clone());
                            state
                                .chosen
                                .sort_by_key(|id| view.offered.iter().position(|t| &t.id == id));
                        }
                        *state.sizes_open = None;
                    }
                }
            });
            ui.add_space(12.0);

            let found = picks::pick(view.entries, state.chosen, view.all_tasks, view.ram);
            let Some(best) = found.best else {
                let words = if state.chosen.is_empty() {
                    "Choose one or more, and the models that do them are shown here."
                } else {
                    "No model in the list does all of these. Choose fewer."
                };
                ui.label(egui::RichText::new(words).color(p.muted));
                return;
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    egui::RichText::new("Best for this computer")
                        .color(p.text)
                        .family(theme::semibold(ui)),
                );
                if view.ram > 0 {
                    ui.label(
                        egui::RichText::new(format!(
                            "({} of memory)",
                            hardware::format_bytes(view.ram)
                        ))
                        .color(p.muted),
                    );
                }
            });
            ui.add_space(4.0);
            let cards = [
                Some((best, "Recommended", true)),
                found.smaller.map(|i| (i, "Smaller, faster", false)),
                found.smallest.map(|i| (i, "Smallest", false)),
            ];
            ui.columns(3, |columns| {
                for (column, card) in columns.iter_mut().zip(cards) {
                    if let Some((i, label, is_best)) = card {
                        let fits = !is_best || found.best_fits;
                        pick_card(column, &view.entries[i], label, is_best, fits, view, state);
                    }
                }
            });
            if !found.also.is_empty() {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Also good:").color(p.muted));
                for &i in &found.also {
                    also_row(ui, &view.entries[i], view, state);
                }
            }
            let shown = 1
                + usize::from(found.smaller.is_some())
                + usize::from(found.smallest.is_some())
                + found.also.len();
            if found.able > shown {
                ui.add_space(4.0);
                let all = ui.add(
                    egui::Button::new(
                        egui::RichText::new(format!("All {} models that do this", found.able))
                            .color(p.accent_text),
                    )
                    .frame(false),
                );
                if all
                    .on_hover_text(
                        "The whole list, with only the models that do everything chosen.",
                    )
                    .clicked()
                {
                    state.browse.tasks = state.chosen.clone();
                    state.browse.scope = Scope::All;
                    state.browse.words.clear();
                    state.browse.size = None;
                    *state.page = Page::List;
                    *state.focus_search = true;
                }
            }
        });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let browse_text = format!("Browse all {}", view.entries.len());
        let room = (ui.available_width() - browse_text.len() as f32 * 7.5 - 40.0).max(160.0);
        // the list's own box: what is typed goes on in it on the list, with the keyboard still there
        state.browse.words.clear();
        let field = ui.add(
            egui::TextEdit::singleline(&mut state.browse.words)
                .id(search_id())
                .hint_text(format!("Or search all {} models", view.entries.len()))
                .desired_width(room),
        );
        if std::mem::take(state.focus_search) {
            field.request_focus();
        }
        if !state.browse.words.is_empty() {
            state.browse.tasks.clear();
            state.browse.scope = Scope::All;
            *state.page = Page::List;
        }
        if ui
            .button(browse_text)
            .on_hover_text("Every model of the list, by type, with its sizes.")
            .clicked()
        {
            state.browse.tasks.clear();
            state.browse.words.clear();
            *state.page = Page::List;
            *state.focus_search = true;
        }
    });
    if let Some(note) = view.note {
        ui.label(egui::RichText::new(note).small().color(p.bad));
    }
    let mut footer = "Nothing is downloaded until you click.".to_string();
    if let Some(day) = view.verified {
        footer.push_str(&format!(" The list was checked on {day}."));
    }
    ui.label(egui::RichText::new(footer).small().color(p.faint));
}

/// A button of a task: its picture over its name, tinted when it is chosen. A click chooses it or takes it
/// back; several can be chosen.
fn task_tile(ui: &mut egui::Ui, task: &TaskDef, on: bool) -> egui::Response {
    let p = theme::palette();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(task.label.clone(), font, egui::Color32::PLACEHOLDER);
    let size = egui::vec2((galley.size().x + 20.0).max(84.0), 60.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, &task.label)
    });
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let (fill, stroke, fg, picture) = if on {
            (p.accent_soft, p.accent_text, p.text, p.accent_text)
        } else if hovered {
            (p.raised_hover, p.outline, p.text, p.text)
        } else {
            (p.card, p.line, p.body, p.muted)
        };
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(6),
            fill,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        let square = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 20.0),
            egui::vec2(20.0, 20.0),
        );
        icons::paint(ui.painter(), square, task_icon(&task.id), picture);
        ui.painter().galley(
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.bottom() - 10.0 - galley.size().y,
            ),
            galley,
            fg,
        );
    }
    response
}

/// The picture of a task, by its id; a task the application has no picture for has a plain one.
fn task_icon(id: &str) -> Icon {
    match id {
        "chat" => Icon::Chat,
        "code" => Icon::Code,
        "reason" => Icon::Bulb,
        "pictures" => Icon::Image,
        "scans" => Icon::Document,
        "translate" => Icon::Translate,
        "transcribe" => Icon::Mic,
        "search" => Icon::FileSearch,
        "make-pictures" => Icon::Brush,
        "make-videos" => Icon::Film,
        _ => Icon::Plus,
    }
}

/// What a model can do, as small pictures with their words when the pointer rests on them.
fn abilities(ui: &mut egui::Ui, entry: &Entry) {
    let Some(card) = &entry.card else {
        return;
    };
    let reads = |m: &str| card.input.iter().any(|i| i == m);
    let has = |f: &str| card.features.iter().any(|x| x == f);
    let shown = [
        (reads("image"), Icon::Eye, "Reads pictures"),
        (reads("audio"), Icon::Headphones, "Hears audio"),
        (has("thinking"), Icon::Bulb, "Thinks before it answers"),
        (has("tools"), Icon::Wrench, "Calls tools"),
        (has("multilingual"), Icon::Globe, "Many languages"),
        (has("fast"), Icon::Bolt, "Fast"),
    ];
    let color = theme::palette().muted;
    let shown: Vec<_> = shown.iter().filter(|(on, _, _)| *on).collect();
    // in their order whichever way the row they are in is laid out
    let width = shown.len() as f32 * 22.0;
    ui.allocate_ui_with_layout(
        egui::vec2(width, 16.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (_, icon, words) in shown {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                icons::paint(ui.painter(), rect, *icon, color);
                response.on_hover_text(*words);
            }
        },
    );
}

/// What a model is in a few words: the list's tagline, else the first sentence of what it says of it.
fn tagline(entry: &Entry) -> String {
    match entry.card.as_ref().map(|c| c.tagline.as_str()) {
        Some(line) if !line.is_empty() => line.to_string(),
        _ => entry
            .description
            .split_inclusive(". ")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

/// One of the three picks: what it is called and is, and its one button.
fn pick_card(
    ui: &mut egui::Ui,
    entry: &Entry,
    label: &str,
    best: bool,
    fits: bool,
    view: &TaskView<'_>,
    state: &mut TaskState<'_>,
) {
    let p = theme::palette();
    egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, if best { p.accent } else { p.line }))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(150.0);
            // a column lays out justified, and would stretch a line that wraps from edge to edge
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(
                    egui::RichText::new(label.to_uppercase())
                        .small()
                        .color(if best { p.accent_text } else { p.faint }),
                );
                ui.label(
                    egui::RichText::new(&entry.name)
                        .size(16.0)
                        .color(p.text)
                        .family(theme::semibold(ui)),
                );
                if let Some(by) = entry
                    .card
                    .as_ref()
                    .map(|c| c.by.as_str())
                    .filter(|b| !b.is_empty())
                {
                    ui.label(egui::RichText::new(by).small().color(p.faint));
                }
                abilities(ui, entry);
                ui.label(egui::RichText::new(tagline(entry)).color(p.muted));
                ui.add_space(4.0);
                offer_buttons(ui, entry, best, view, state);
                if !fits {
                    ui.label(
                        egui::RichText::new("More than this computer's memory holds well.")
                            .small()
                            .color(p.warn),
                    );
                }
            });
        });
}

/// Another model to start with that does it: one line, with its button.
fn also_row(ui: &mut egui::Ui, entry: &Entry, view: &TaskView<'_>, state: &mut TaskState<'_>) {
    let p = theme::palette();
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&entry.name)
                    .color(p.text)
                    .family(theme::semibold(ui)),
            );
            ui.label(egui::RichText::new(tagline(entry)).color(p.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                offer_buttons(ui, entry, false, view, state);
                abilities(ui, entry);
            });
        });
        if state.sizes_open.as_deref() == Some(entry.key.as_str()) {
            other_sizes(ui, entry, view, state);
        }
    });
}

/// The button of a model (what the list recommends, with what goes with it), or that it is there, and the
/// way to its other sizes: in a card the button comes first, in a row that is laid out from the right the
/// way to the other sizes is the rightmost.
fn offer_buttons(
    ui: &mut egui::Ui,
    entry: &Entry,
    best: bool,
    view: &TaskView<'_>,
    state: &mut TaskState<'_>,
) {
    let Some(main) = picks::main_offer(entry) else {
        return;
    };
    let p = theme::palette();
    let open = state.sizes_open.as_deref() == Some(entry.key.as_str());
    let sizes_link = |ui: &mut egui::Ui, state: &mut TaskState<'_>| {
        if entry.offers.len() < 2 {
            return;
        }
        let link = ui.add(
            egui::Button::new(
                egui::RichText::new(if open { "Fewer sizes" } else { "Other sizes" })
                    .small()
                    .color(p.accent_text),
            )
            .frame(false),
        );
        if link.clicked() {
            *state.sizes_open = if open { None } else { Some(entry.key.clone()) };
        }
    };
    let button = |ui: &mut egui::Ui, state: &mut TaskState<'_>| {
        if (view.installed)(main) {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Check, p.ok);
            ui.label(egui::RichText::new("Installed").color(p.ok));
            return;
        }
        let size = main
            .bytes
            .map_or_else(|| main.label.clone(), hardware::format_bytes);
        let text = if best {
            format!("Download · {size}")
        } else {
            size
        };
        if icons::labelled_button(
            ui,
            theme::Role::Normal,
            !view.busy,
            Icon::Download,
            &text,
            28.0,
        )
        .on_hover_text(&main.hover)
        .clicked()
        {
            *state.fetch = Some(main.fetch.clone());
        }
    };
    if ui.layout().prefer_right_to_left() {
        sizes_link(ui, state);
        button(ui, state);
    } else {
        // a narrow card puts the way to the other sizes under the button
        ui.horizontal_wrapped(|ui| {
            button(ui, state);
            sizes_link(ui, state);
        });
        if open {
            other_sizes(ui, entry, view, state);
        }
    }
}

/// The other ways to fetch a model, in plain words, each a button.
fn other_sizes(ui: &mut egui::Ui, entry: &Entry, view: &TaskView<'_>, state: &mut TaskState<'_>) {
    let main = picks::main_offer(entry);
    ui.horizontal_wrapped(|ui| {
        for offer in &entry.offers {
            if main.is_some_and(|m| std::ptr::eq(m, offer)) {
                continue;
            }
            let there = (view.installed)(offer);
            let hover = if there {
                "Already in your models folder.".to_string()
            } else {
                offer.hover.clone()
            };
            if ui
                .add_enabled(
                    !view.busy && !there,
                    egui::Button::new(offer_words(entry, offer)).small(),
                )
                .on_hover_text(hover)
                .clicked()
            {
                *state.fetch = Some(offer.fetch.clone());
            }
        }
    });
}

/// What a button of another size says, in plain words: how good it is for its size (`Standard` for a
/// 4-bit file, `High quality` for an 8-bit one; the file's own label when it is neither), `text only` for
/// the weights of a model that reads more with what goes with them, and the size. Two buttons that would
/// say the same say their labels too.
fn offer_words(entry: &Entry, offer: &Offer) -> String {
    let words = |o: &Offer| -> String {
        let label = o.label.split(" · ").next().unwrap_or(&o.label);
        let mut words = quality(label);
        let bundles = entry.offers.iter().any(|x| x.bundle.is_some());
        if bundles && o.bundle.is_none() {
            let reads_more = entry
                .card
                .as_ref()
                .is_some_and(|c| c.input.iter().any(|i| i != "text"));
            words.push_str(if reads_more {
                ", text only"
            } else {
                ", model only"
            });
        }
        words
    };
    let mut said = words(offer);
    let twin = entry
        .offers
        .iter()
        .filter(|o| !std::ptr::eq(*o, offer) && words(o) == said)
        .count()
        > 0;
    if twin {
        let label = offer.label.split(" · ").next().unwrap_or(&offer.label);
        said.push_str(&format!(" ({label})"));
    }
    match offer.bytes {
        Some(bytes) => format!("{said} · {}", hardware::format_bytes(bytes)),
        None => said,
    }
}

/// How good a quantization is for its size, in words: the label as it is when it is none known.
fn quality(label: &str) -> String {
    let l = label.to_ascii_lowercase();
    let bits = l.trim_start_matches('i').trim_start_matches('t');
    let words = if bits.starts_with("q4") {
        "Standard"
    } else if bits.starts_with("q5") {
        "Better quality"
    } else if bits.starts_with("q6") || bits.starts_with("q8") {
        "High quality"
    } else if ["f16", "bf16", "fp16", "f32"]
        .iter()
        .any(|f| bits.starts_with(f))
    {
        "Full quality"
    } else if ["q1", "q2", "q3"].iter().any(|q| bits.starts_with(q)) {
        "Smaller"
    } else {
        return label.to_string();
    };
    words.to_string()
}

/// The search box, the same on both pages, so that what is typed on the first goes on in the list's.
fn search_id() -> egui::Id {
    egui::Id::new("catalog_search")
}

/// The way back from the list to what the person wants to do.
fn back_button(ui: &mut egui::Ui) -> egui::Response {
    icons::labelled_button(
        ui,
        theme::Role::Quiet,
        true,
        Icon::Back,
        "What do you want to do?",
        24.0,
    )
    .on_hover_text("Back to the models for what you want to do.")
}
