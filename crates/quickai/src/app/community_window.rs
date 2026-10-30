//! The configurations people shared on quickai.everybodyai.dev (docs/reports-and-community.md, section 11.9): a
//! window of its own, opened from the window of the configurations, and the dialog that shares one.
//!
//! The list is fetched when the window is opened, never before, and filtered on this computer: *Fits this computer*
//! compares what each file says it was made on with this computer, and nothing about it is sent. **Get...** fetches a
//! file, holds it to the SHA-256 the list names, and gives it to the import dialog, which reviews it as it reviews any
//! file from outside before anything is taken. **Share with the community...** uploads the text the export makes, as it
//! is shown, and the person finishes in the browser, signed in with GitHub. No request carries the install id.
//!
//! What the window learns of the list also marks, in the window of the configurations, the ones that are shared and the
//! ones that came from a version that was withdrawn. The application changes nothing by itself.

use crossbeam_channel::Receiver;
use eframe::egui;

use quickai_core::profile::canonical::identity;
use quickai_core::profile::check::open_text;
use quickai_core::profile::CommunityRef;

use super::configurations_window::{computer_line, tag, ImportDialog};
use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;
use crate::community::{self, Draft, Index, Shared};
use crate::configurations::{export_text, Entry};

/// What the window is called to the window manager.
pub(super) const COMMUNITY_TITLE: &str = "QuickAI Community configurations";

/// The size the window opens at, and the least it can be made.
const OPENS_AT: [f32; 2] = [760.0, 640.0];
const LEAST: [f32; 2] = [440.0, 300.0];

/// How many ratings the site's mean counts for in the order by rating: one 5-star rating does not top the list
/// (section 11.3).
const PRIOR: f64 = 5.0;

/// Which window the community is: the id the display knows it by.
pub(super) fn community_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_community")
}

/// The order of the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Sort {
    #[default]
    Rating,
    Downloads,
    Newest,
}

impl Sort {
    fn label(self) -> &'static str {
        match self {
            Sort::Rating => "Best rated",
            Sort::Downloads => "Most downloaded",
            Sort::Newest => "Newest",
        }
    }
}

/// What the list is narrowed to, on this computer.
#[derive(Clone, Debug, Default)]
pub(super) struct Filter {
    pub search: String,
    /// The id of an engine.
    pub engine: Option<String>,
    /// The id of a category.
    pub category: Option<String>,
    /// Only what was made on a computer like this one: the same system, and no more memory than it has.
    pub fits: bool,
    pub sort: Sort,
}

/// The computer, as *Fits this computer* compares it.
#[derive(Clone, Debug, Default)]
pub(super) struct Here {
    pub os: String,
    pub memory_gb: Option<u64>,
}

/// An engine or a category of the filters: its id and its name.
type Named = (String, String);

/// What the window keeps.
pub(super) struct CommunityUi {
    pub open: bool,
    /// It was asked for while open: it comes to the front once.
    pub raise: bool,
    /// Where the community is (`QUICKAI_COMMUNITY_URL`, or api.everybodyai.dev).
    pub endpoint: String,
    /// The list, once it was fetched while the application runs.
    pub index: Option<Index>,
    fetching: Option<Receiver<Result<Index, String>>>,
    /// Why the list or a file could not be had.
    pub error: Option<String>,
    pub filter: Filter,
    /// **Get...**: the configuration being fetched.
    getting: Option<(Shared, Receiver<Result<String, String>>)>,
    pub share: Option<ShareDialog>,
    /// The last thing that was done in the window, in a sentence.
    pub status: Option<String>,
}

impl CommunityUi {
    pub fn new() -> CommunityUi {
        CommunityUi {
            open: false,
            raise: false,
            endpoint: community::endpoint_from_environment(),
            index: None,
            fetching: None,
            error: None,
            filter: Filter::default(),
            getting: None,
            share: None,
            status: None,
        }
    }

    pub fn fetching(&self) -> bool {
        self.fetching.is_some()
    }

    /// The id of the configuration being fetched, while one is.
    pub fn getting(&self) -> Option<&str> {
        self.getting.as_ref().map(|(s, _)| s.id.as_str())
    }
}

/// The dialog that shares a configuration: the text that will be uploaded, then what the community said.
#[derive(Clone)]
pub(super) struct ShareDialog {
    pub name: String,
    /// The text the export makes of it, or why there is none.
    pub text: Result<String, String>,
    /// What the export left out, in sentences.
    pub left_out: Vec<String>,
    /// It is in the list already: its title, its page and the version that has this identity.
    pub already: Option<(String, String, u32)>,
    sending: Option<Receiver<Result<Draft, String>>>,
    pub answer: Option<Result<Draft, String>>,
}

/// The configurations of `index` that `filter` lets through, in its order.
pub(super) fn listed<'a>(index: &'a Index, filter: &Filter, here: &Here) -> Vec<&'a Shared> {
    let words: Vec<String> = filter
        .search
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut out: Vec<&Shared> = index
        .configurations
        .iter()
        .filter(|c| filter.engine.as_ref().is_none_or(|e| &c.engine == e))
        .filter(|c| {
            filter
                .category
                .as_ref()
                .is_none_or(|k| c.categories.contains(k))
        })
        .filter(|c| !filter.fits || fits(c, here))
        .filter(|c| {
            let text = format!(
                "{} {} {} {} {} {}",
                c.title,
                c.summary,
                c.author.as_deref().unwrap_or_default(),
                c.engine_name,
                c.models.join(" "),
                c.categories
                    .iter()
                    .map(|k| index.category_name(k))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .collect();
    match filter.sort {
        Sort::Rating => {
            let mean = site_mean(index);
            out.sort_by(|a, b| {
                score(b, mean)
                    .total_cmp(&score(a, mean))
                    .then(b.downloads.cmp(&a.downloads))
            });
        }
        Sort::Downloads => out.sort_by(|a, b| {
            b.downloads
                .cmp(&a.downloads)
                .then(b.published.cmp(&a.published))
        }),
        Sort::Newest => {
            out.sort_by(|a, b| b.published.cmp(&a.published).then(a.title.cmp(&b.title)))
        }
    }
    out
}

/// Whether `shared` was made on a computer like this one: the same system, and no more memory than this one has. A
/// file that does not say fits.
pub(super) fn fits(shared: &Shared, here: &Here) -> bool {
    let Some(on) = &shared.made_on else {
        return true;
    };
    let same_os = on
        .os
        .as_deref()
        .is_none_or(|os| here.os.is_empty() || os.eq_ignore_ascii_case(&here.os));
    let enough = match (on.memory_gb, here.memory_gb) {
        (Some(needed), Some(have)) => needed <= have,
        _ => true,
    };
    same_os && enough
}

/// The mean of every rating of the list.
fn site_mean(index: &Index) -> f64 {
    let (sum, n) = index
        .configurations
        .iter()
        .filter_map(|c| c.stars.map(|s| (s * c.ratings as f64, c.ratings as f64)))
        .fold((0.0, 0.0), |(s, n), (a, b)| (s + a, n + b));
    if n > 0.0 {
        sum / n
    } else {
        0.0
    }
}

/// A rating that counts the site's mean as `PRIOR` ratings of its own (a Bayesian mean).
fn score(shared: &Shared, mean: f64) -> f64 {
    let n = shared.ratings as f64;
    let stars = shared.stars.unwrap_or(mean);
    (PRIOR * mean + stars * n) / (PRIOR + n)
}

/// `4.5 (12)`, or that it has no rating yet.
pub(super) fn stars_words(shared: &Shared) -> String {
    match shared.stars {
        Some(stars) if shared.ratings > 0 => format!("{stars:.1} ({})", shared.ratings),
        _ => "No ratings yet".to_string(),
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Opens `url` in the browser, when it is an address of the web.
fn open_page(url: &str) {
    if url.starts_with("https://") || url.starts_with("http://127.0.0.1") {
        let _ = webbrowser::open(url);
    }
}

impl App {
    /// Opens the window, or brings it to the front, and fetches the list again.
    pub(super) fn open_community(&mut self, ctx: &egui::Context) {
        self.community.raise = self.community.open;
        self.community.open = true;
        // who this computer's configurations are, for the cards to say which of them it has
        if !self.show_configurations {
            self.refresh_configurations();
        }
        self.fetch_community_list(ctx);
    }

    fn fetch_community_list(&mut self, ctx: &egui::Context) {
        if self.community.fetching.is_some() {
            return;
        }
        self.community.error = None;
        let (tx, rx) = crossbeam_channel::bounded(1);
        let endpoint = self.community.endpoint.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(community::fetch_index(&endpoint, community::TIMEOUT));
            ctx.request_repaint();
        });
        self.community.fetching = Some(rx);
    }

    /// What the community's requests answered since the last frame.
    pub(super) fn poll_community(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.community.fetching {
            if let Ok(answer) = rx.try_recv() {
                self.community.fetching = None;
                match answer {
                    Ok(index) => {
                        self.community.index = Some(index);
                        self.community.error = None;
                    }
                    Err(why) => self.community.error = Some(why),
                }
            }
        }
        if let Some((shared, rx)) = &self.community.getting {
            if let Ok(answer) = rx.try_recv() {
                let shared = shared.clone();
                self.community.getting = None;
                match answer {
                    Ok(text) => self.review_from_community(&shared, text),
                    Err(why) => self.community.error = Some(why),
                }
            }
        }
        if let Some(dialog) = &mut self.community.share {
            if let Some(rx) = &dialog.sending {
                if let Ok(answer) = rx.try_recv() {
                    dialog.sending = None;
                    if let Ok(Draft::Claim { url, .. }) = &answer {
                        open_page(url);
                        self.configs.status =
                            Some("Finish sharing it in your browser.".to_string());
                    }
                    dialog.answer = Some(answer);
                }
            }
        }
        let waiting = self.community.fetching.is_some()
            || self.community.getting.is_some()
            || self
                .community
                .share
                .as_ref()
                .is_some_and(|d| d.sending.is_some());
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// **Get...**: fetches the file of `shared`, to be reviewed.
    fn get_from_community(&mut self, shared: &Shared, ctx: &egui::Context) {
        if self.community.getting.is_some() {
            return;
        }
        self.community.error = None;
        self.community.status = Some(format!("Fetching \"{}\"...", shared.title));
        let (tx, rx) = crossbeam_channel::bounded(1);
        let endpoint = self.community.endpoint.clone();
        let (wanted, ctx) = (shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(community::fetch_configuration(
                &endpoint,
                &wanted,
                community::TIMEOUT,
            ));
            ctx.request_repaint();
        });
        self.community.getting = Some((shared.clone(), rx));
    }

    /// A file of the community, checked against the list: the import dialog reviews it as any file from outside.
    fn review_from_community(&mut self, shared: &Shared, text: String) {
        let mut dialog = ImportDialog {
            community: Some((
                CommunityRef {
                    id: shared.id.clone(),
                    version: shared.version,
                },
                shared.title.clone(),
            )),
            ..ImportDialog::default()
        };
        self.load_for_import(&mut dialog, text);
        self.community.status = None;
        self.configs.import = Some(dialog);
    }

    /// The window, while it is open: a window of its own.
    pub(super) fn show_community_window(&mut self, ctx: &egui::Context) {
        if !self.community.open {
            return;
        }
        let id = community_viewport();
        if std::mem::take(&mut self.community.raise) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let mut close = false;
        let builder = theme::own_window(COMMUNITY_TITLE, OPENS_AT, LEAST);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                let room = (ui.ctx().content_rect().height() - 120.0).max(200.0);
                egui::ScrollArea::vertical()
                    .id_salt("community_scroll")
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.show_community_body(ui));
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            } else {
                if ui.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
                let footer = egui::Panel::bottom("community_footer")
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("Close").clicked() {
                                close = true;
                            }
                            let fetching = self.community.fetching();
                            if ui
                                .add_enabled(!fetching, egui::Button::new("Refresh"))
                                .on_hover_text("Fetch the list again.")
                                .clicked()
                            {
                                let ctx = ui.ctx().clone();
                                self.fetch_community_list(&ctx);
                            }
                            if let Some(status) = &self.community.status {
                                ui.label(egui::RichText::new(status).small().weak());
                            }
                        });
                        ui.add_space(6.0);
                    })
                    .response
                    .rect
                    .height();
                let frame = egui::Frame::central_panel(&ui.ctx().global_style());
                theme::fitted_page(ui, "community_scroll", frame, footer, LEAST[1], |ui| {
                    self.show_community_body(ui)
                });
            }
            // a file of the community is reviewed in this window
            self.show_import_dialog(ui.ctx(), true);
        });
        if close {
            self.close_community();
        }
    }

    pub(super) fn close_community(&mut self) {
        self.community.open = false;
        self.community.raise = false;
        if self
            .configs
            .import
            .as_ref()
            .is_some_and(|d| d.community.is_some())
        {
            self.configs.import = None;
        }
    }

    /// What the window holds: what it is, the filters, and a card for each configuration.
    fn show_community_body(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        ui.label(
            egui::RichText::new(
                "Configurations people shared on quickai.everybodyai.dev. Each is reviewed before it is listed, and \
                 QuickAI reviews it again, as any file from outside, before anything is taken from it.",
            )
            .small()
            .color(p.body),
        );
        ui.add_space(6.0);
        self.show_community_filters(ui);
        ui.add_space(6.0);
        if let Some(why) = self.community.error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(why).small().color(p.bad));
                if self.community.index.is_none()
                    && !self.community.fetching()
                    && ui.button("Try again").clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.fetch_community_list(&ctx);
                }
            });
            ui.add_space(4.0);
        }
        let Some(index) = &self.community.index else {
            if self.community.fetching() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(theme::muted("Fetching the list..."));
                });
            }
            return;
        };
        let here = self.community_here();
        let shown: Vec<Shared> = listed(index, &self.community.filter, &here)
            .into_iter()
            .cloned()
            .collect();
        let total = index.configurations.len();
        if total == 0 {
            ui.label(theme::muted(
                "Nobody has shared a configuration yet. Share one of yours from its menu in Configurations.",
            ));
            return;
        }
        if shown.is_empty() {
            ui.label(theme::muted("None of them fits what you chose."));
            return;
        }
        ui.label(theme::muted(if shown.len() == total {
            plural(total as u64, "configuration", "configurations")
        } else {
            format!("{} of {total}", shown.len())
        }));
        ui.add_space(2.0);
        let mut get = None;
        for shared in &shown {
            if self.show_shared_card(ui, shared) {
                get = Some(shared.clone());
            }
        }
        if let Some(shared) = get {
            let ctx = ui.ctx().clone();
            self.get_from_community(&shared, &ctx);
        }
    }

    /// This computer, as *Fits this computer* compares it.
    fn community_here(&self) -> Here {
        let made = self.host().made_on();
        Here {
            os: made.os.unwrap_or_default(),
            memory_gb: made.memory_gb,
        }
    }

    fn show_community_filters(&mut self, ui: &mut egui::Ui) {
        // each as its id and its name
        let (engines, categories): (Vec<Named>, Vec<Named>) = match &self.community.index {
            Some(index) => {
                let mut engines: Vec<(String, String)> = index
                    .configurations
                    .iter()
                    .map(|c| {
                        let name = if c.engine_name.is_empty() {
                            c.engine.clone()
                        } else {
                            c.engine_name.clone()
                        };
                        (c.engine.clone(), name)
                    })
                    .collect();
                engines.sort();
                engines.dedup();
                let categories = index
                    .categories
                    .iter()
                    .map(|c| (c.id.clone(), c.name.clone()))
                    .collect();
                (engines, categories)
            }
            None => (Vec::new(), Vec::new()),
        };
        let filter = &mut self.community.filter;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut filter.search)
                    .id(egui::Id::new("community_search"))
                    .hint_text("Search")
                    .desired_width(180.0),
            );
            let named = |list: &[Named], id: &Option<String>, all: &str| {
                id.as_ref()
                    .and_then(|id| list.iter().find(|(k, _)| k == id))
                    .map_or(all.to_string(), |(_, name)| name.clone())
            };
            egui::ComboBox::from_id_salt("community_engine")
                .selected_text(named(&engines, &filter.engine, "Every engine"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut filter.engine, None, "Every engine");
                    for (id, name) in &engines {
                        ui.selectable_value(&mut filter.engine, Some(id.clone()), name);
                    }
                });
            egui::ComboBox::from_id_salt("community_category")
                .selected_text(named(&categories, &filter.category, "Every use"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut filter.category, None, "Every use");
                    for (id, name) in &categories {
                        ui.selectable_value(&mut filter.category, Some(id.clone()), name);
                    }
                });
            egui::ComboBox::from_id_salt("community_sort")
                .selected_text(filter.sort.label())
                .show_ui(ui, |ui| {
                    for sort in [Sort::Rating, Sort::Downloads, Sort::Newest] {
                        ui.selectable_value(&mut filter.sort, sort, sort.label());
                    }
                });
            ui.checkbox(&mut filter.fits, "Fits this computer")
                .on_hover_text(
                    "Only what was made on the same system with no more memory than this computer has. \
                     It is worked out here: nothing about this computer is sent.",
                );
        });
    }

    /// One configuration of the list, as a card: the badge of its engine, its title and who shared it, what it runs,
    /// its summary, what is worth knowing before taking it, and its stars and downloads; at the right, **Get...** and
    /// its page. Returns whether **Get...** was pressed.
    fn show_shared_card(&mut self, ui: &mut egui::Ui, shared: &Shared) -> bool {
        let p = theme::palette();
        let mut get = false;
        let have = self
            .configs
            .identities
            .values()
            .any(|i| *i == shared.identity);
        let busy = self.community.getting();
        egui::Frame::new()
            .fill(p.card)
            .stroke(egui::Stroke::new(1.0, p.line))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let layout = egui::Layout::right_to_left(egui::Align::Min);
                ui.with_layout(layout, |ui| {
                    ui.vertical(|ui| {
                        let fetching = busy == Some(shared.id.as_str());
                        let label = if fetching { "Fetching..." } else { "Get..." };
                        if theme::button_sized(
                            ui,
                            Role::Primary,
                            busy.is_none(),
                            label,
                            egui::vec2(96.0, 26.0),
                        )
                        .on_hover_text(
                            "Fetch it and see what it would change on this computer. Nothing is taken until you import it.",
                        )
                        .clicked()
                        {
                            get = true;
                        }
                        if !shared.url.is_empty()
                            && theme::button_sized(
                                ui,
                                Role::Normal,
                                true,
                                "Open page",
                                egui::vec2(96.0, 24.0),
                            )
                            .on_hover_text(&shared.url)
                            .clicked()
                        {
                            open_page(&shared.url);
                        }
                    });
                    let layout = egui::Layout::left_to_right(egui::Align::Min);
                    ui.with_layout(layout, |ui| {
                        self.engine_badge(ui, &shared.engine);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            self.show_shared_lines(ui, shared, have);
                        });
                    });
                });
            });
        ui.add_space(4.0);
        get
    }

    fn show_shared_lines(&self, ui: &mut egui::Ui, shared: &Shared, have: bool) {
        let p = theme::palette();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&shared.title).strong().color(p.text));
            let by = match &shared.author {
                Some(login) => format!("by {login}"),
                None => "by a former member".to_string(),
            };
            ui.label(theme::muted(by));
        });
        let index = self.community.index.as_ref();
        let mut what = vec![if shared.engine_name.is_empty() {
            shared.engine.clone()
        } else {
            shared.engine_name.clone()
        }];
        what.extend(shared.models.iter().cloned());
        if let Some(build) = &shared.build {
            what.push(build.clone());
        }
        ui.add(
            egui::Label::new(egui::RichText::new(what.join(" · ")).small().color(p.body)).wrap(),
        );
        if !shared.summary.is_empty() {
            ui.add(
                egui::Label::new(egui::RichText::new(&shared.summary).small().color(p.body)).wrap(),
            );
        }
        // what a person wants to know before taking it: the arguments first, as they can do the most
        let warn = p.warn;
        let warn_fill = p.card.lerp_to_gamma(warn, 0.16);
        let mut tags: Vec<(Option<Icon>, String, egui::Color32, egui::Color32, &str)> = Vec::new();
        if shared.custom_args {
            tags.push((
                Some(Icon::Warn),
                "Sets its own command-line arguments".into(),
                warn,
                warn_fill,
                "It adds words of its own to the engine's command line. The import shows them in full and takes them only if you tick them.",
            ));
        }
        if shared.custom_env {
            tags.push((
                None,
                "Sets environment variables".into(),
                warn,
                warn_fill,
                "It sets lines of the engine's environment. The import shows them in full and takes them only if you tick them.",
            ));
        }
        if shared.needs_review && !shared.custom_args && !shared.custom_env {
            tags.push((
                None,
                "Asks before some settings".into(),
                warn,
                warn_fill,
                "It has a setting the import asks about before taking it.",
            ));
        }
        if have {
            tags.push((
                Some(Icon::Check),
                "You have it".into(),
                p.accent_text,
                p.accent_soft,
                "One of your configurations runs the same thing.",
            ));
        }
        for category in &shared.categories {
            let name = index.map_or(category.as_str(), |i| i.category_name(category));
            tags.push((
                None,
                name.to_string(),
                p.muted,
                egui::Color32::TRANSPARENT,
                "",
            ));
        }
        if !tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(5.0, 4.0);
                for (icon, text, color, fill, hover) in &tags {
                    let shown = tag(ui, *icon, text, *color, *fill);
                    if !hover.is_empty() {
                        shown.on_hover_text(*hover);
                    }
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (square, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
            icons::paint(ui.painter(), square, Icon::Star, p.muted);
            let mut line = stars_words(shared);
            line.push_str(&format!(
                " · {}",
                plural(shared.downloads, "download", "downloads")
            ));
            if shared.sharers > 1 {
                line.push_str(&format!(" · shared by {} people", shared.sharers));
            }
            if let Some(on) = &shared.made_on {
                let computer = computer_line(
                    on.os.as_deref().unwrap_or(""),
                    on.arch.as_deref().unwrap_or(""),
                    &on.gpu,
                    on.memory_gb,
                );
                if !computer.is_empty() {
                    line.push_str(&format!(" · made on {computer}"));
                }
            }
            ui.label(theme::muted(line));
        });
    }

    // ---- sharing --------------------------------------------------------------------------------------

    /// **Share with the community...** of a configuration: the text the export makes, and whether the list has it.
    pub(super) fn open_share(&mut self, entry: &Entry) {
        let host = self.host();
        let exported = export_text(&self.registry, &entry.id, None, None, &host);
        let (text, left_out) = match exported {
            Ok((text, notes)) => (
                Ok(text),
                notes
                    .iter()
                    .map(|n| n.text(self.registry.vocabulary()))
                    .collect(),
            ),
            Err(e) => (Err(e.to_string()), Vec::new()),
        };
        // who the uploaded text is: what the server will compute, not the stored file
        let already = text.as_ref().ok().and_then(|text| {
            let opened = open_text(&self.registry, text, env!("CARGO_PKG_VERSION")).ok()?;
            let id = identity(&self.registry, &opened.configuration).identity;
            let index = self.community.index.as_ref()?;
            index
                .shared_as(&id)
                .map(|(c, version)| (c.title.clone(), c.url.clone(), version))
        });
        self.community.share = Some(ShareDialog {
            name: entry.name.clone(),
            text,
            left_out,
            already,
            sending: None,
            answer: None,
        });
    }

    fn upload_share(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.community.share else {
            return;
        };
        let Ok(text) = dialog.text.clone() else {
            return;
        };
        let (tx, rx) = crossbeam_channel::bounded(1);
        let endpoint = self.community.endpoint.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(community::upload_draft(
                &endpoint,
                &text,
                community::TIMEOUT,
            ));
            ctx.request_repaint();
        });
        dialog.answer = None;
        dialog.sending = Some(rx);
    }

    /// The dialog that shares a configuration, in the window of the configurations.
    pub(super) fn show_share_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.community.share.clone() else {
            return;
        };
        let p = theme::palette();
        let (mut upload, mut close, mut open) = (false, false, None::<String>);
        let modal = egui::Modal::new(egui::Id::new("share_configuration_modal")).show(ctx, |ui| {
            ui.set_width(super::configurations_ui::modal_width(ctx, 500.0));
            ui.label(egui::RichText::new(format!("Share \"{}\" with the community", dialog.name)).strong());
            ui.add_space(6.0);
            if let Some((title, url, version)) = &dialog.already {
                ui.label(format!(
                    "It is shared already, as \"{title}\" (version {version}). Nothing is uploaded."
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    if !url.is_empty() && ui.button("Open its page").clicked() {
                        open = Some(url.clone());
                    }
                });
                return;
            }
            match &dialog.answer {
                Some(Ok(Draft::Claim { url, .. })) => {
                    ui.label(
                        "Your browser has opened quickai.everybodyai.dev. Sign in with GitHub there, give it a \
                         title, a summary and its uses, and send it. It appears once it is reviewed.",
                    );
                    ui.label(theme::muted("The address works for 24 hours."));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                        if ui.button("Open it again").clicked() {
                            open = Some(url.clone());
                        }
                    });
                    return;
                }
                Some(Ok(Draft::Already {
                    title,
                    url,
                    pending,
                    version,
                    message,
                    ..
                })) => {
                    let said = if !message.is_empty() {
                        message.clone()
                    } else if *pending {
                        format!("It is shared already, as \"{title}\", and is waiting for review.")
                    } else {
                        format!("It is shared already, as \"{title}\" (version {version}).")
                    };
                    ui.label(said);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                        if !*pending && !url.is_empty() && ui.button("Open its page").clicked() {
                            open = Some(url.clone());
                        }
                    });
                    return;
                }
                Some(Ok(Draft::Refused { message, problems })) => {
                    ui.label(egui::RichText::new(message).color(p.bad));
                    for problem in problems {
                        ui.label(egui::RichText::new(format!("• {problem}")).small());
                    }
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    return;
                }
                Some(Err(why)) => {
                    ui.label(egui::RichText::new(why).color(p.bad));
                    ui.add_space(4.0);
                }
                None => {}
            }
            match &dialog.text {
                Ok(text) => {
                    ui.label(
                        "This file is uploaded as it is shown, and nothing else: no install id, nothing about you.",
                    );
                    for note in &dialog.left_out {
                        ui.label(egui::RichText::new(note).small());
                    }
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("share_preview")
                        .max_height((ctx.content_rect().height() - 300.0).clamp(60.0, 220.0))
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut text.as_str())
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(ui.available_width()),
                            );
                        });
                    ui.add_space(4.0);
                    ui.label(theme::muted(
                        "You'll finish on quickai.everybodyai.dev, signed in with GitHub. Shared configurations \
                         are CC0 1.0 and are reviewed before they appear.",
                    ));
                }
                Err(why) => {
                    ui.label(egui::RichText::new(why).color(p.bad));
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                let sending = dialog.sending.is_some();
                if ui
                    .add_enabled(
                        dialog.text.is_ok() && !sending,
                        egui::Button::new(if sending { "Uploading..." } else { "Upload" }),
                    )
                    .clicked()
                {
                    upload = true;
                }
                if sending {
                    ui.spinner();
                }
            });
        });
        if modal.should_close() {
            close = true;
        }
        if let Some(url) = open {
            open_page(&url);
        }
        if upload {
            self.upload_share(ctx);
        }
        if close {
            self.community.share = None;
        }
    }

    /// What the list says of a configuration of this computer: the one it is shared as (and the version), and why the
    /// version it came from was withdrawn.
    pub(super) fn community_of(&self, entry: &Entry) -> (Option<(&Shared, u32)>, Option<String>) {
        let Some(index) = &self.community.index else {
            return (None, None);
        };
        let shared = self
            .configs
            .identities
            .get(&entry.id)
            .and_then(|id| index.shared_as(id));
        let withdrawn = entry
            .configuration
            .meta
            .community
            .as_ref()
            .and_then(|c| index.withdrawn(&c.id, c.version))
            .map(|w| w.reason.clone());
        (shared, withdrawn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::community::MadeOn;

    fn shared(
        id: &str,
        stars: Option<f64>,
        ratings: u64,
        downloads: u64,
        published: &str,
    ) -> Shared {
        Shared {
            id: id.into(),
            version: 1,
            title: id.into(),
            engine: "llamacpp".into(),
            engine_name: "llama.cpp".into(),
            stars,
            ratings,
            downloads,
            published: published.into(),
            ..Shared::default()
        }
    }

    fn index(list: Vec<Shared>) -> Index {
        Index {
            configurations: list,
            ..Index::default()
        }
    }

    #[test]
    fn one_rating_of_five_does_not_top_many_good_ones() {
        let list = index(vec![
            shared("one-five", Some(5.0), 1, 0, "2026-10-01"),
            shared("many-good", Some(4.6), 40, 0, "2026-10-01"),
        ]);
        let order: Vec<&str> = listed(&list, &Filter::default(), &Here::default())
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(order, ["many-good", "one-five"]);
    }

    #[test]
    fn the_filters_are_worked_out_here() {
        let mut big = shared("big", None, 0, 5, "2026-10-02");
        big.made_on = Some(MadeOn {
            os: Some("linux".into()),
            memory_gb: Some(64),
            ..MadeOn::default()
        });
        big.summary = "Long documents".into();
        let small = shared("small", None, 0, 9, "2026-10-01");
        let list = index(vec![big, small]);
        let here = Here {
            os: "linux".into(),
            memory_gb: Some(16),
        };
        let ids = |filter: &Filter| -> Vec<String> {
            listed(&list, filter, &here)
                .iter()
                .map(|c| c.id.clone())
                .collect()
        };
        let fits = Filter {
            fits: true,
            ..Filter::default()
        };
        assert_eq!(ids(&fits), ["small"]);
        let newest = Filter {
            sort: Sort::Newest,
            ..Filter::default()
        };
        assert_eq!(ids(&newest), ["big", "small"]);
        let downloads = Filter {
            sort: Sort::Downloads,
            ..Filter::default()
        };
        assert_eq!(ids(&downloads), ["small", "big"]);
        let search = Filter {
            search: "long DOC".into(),
            ..Filter::default()
        };
        assert_eq!(ids(&search), ["big"]);
    }

    /// A configuration of the community is held to the list's SHA-256, reviewed by the import dialog, and stored with
    /// where it came from; no request carries the install id.
    #[test]
    fn a_configuration_got_from_the_community_is_reviewed_and_says_where_it_came_from() {
        use crate::config::{build_registry, ConfigDirs};
        use crate::settings::Settings;
        use crate::testkit::fake_telemetry::{FakeApi, Reply};
        use crate::testkit::{scratch, wait_until};
        use std::time::Duration;

        const FILE: &str = "schema = 1\nkind = \"profile\"\nid = \"fast-chat\"\nname = \"Fast chat\"\nengine = \"llamacpp\"\n\n[[model]]\ncatalog = \"bonsai-2-4b\"\n\n[values.llamacpp]\nctx_size = 8192\n";
        let reply = |body: String| Reply {
            status: 200,
            retry_after: None,
            body,
        };
        let api = FakeApi::start();
        let list = serde_json::json!({
            "configurations": [{
                "id": "fast-chat", "version": 3, "title": "Fast chat", "engine": "llamacpp",
                "sha256": crate::community::sha256_hex(FILE.as_bytes()), "identity": "aa",
            }],
        });
        api.reply(reply(list.to_string()));
        api.reply(reply(FILE.replace("8192", "4096")));
        api.reply(reply(FILE.to_string()));

        let dir = scratch("community-get");
        let mut settings = Settings::default();
        settings.set_chosen_engine("llamacpp");
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * 1024 * 1024 * 1024,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        app.config_dirs = ConfigDirs {
            user: Some(dir.join("config")),
            data: Some(dir.join("data")),
            ..ConfigDirs::default()
        };
        app.reload_configurations();
        app.community.endpoint = api.url.clone();
        let ctx = egui::Context::default();

        app.open_community(&ctx);
        wait_until(Duration::from_secs(10), "the list", || {
            app.poll_community(&ctx);
            app.community.index.is_some()
        });
        let shared = app.community.index.as_ref().unwrap().configurations[0].clone();

        // a file that is not the one the list names is refused, and nothing is opened
        app.get_from_community(&shared, &ctx);
        wait_until(Duration::from_secs(10), "the first file", || {
            app.poll_community(&ctx);
            app.community.getting().is_none()
        });
        assert!(app.configs.import.is_none());
        assert!(app.community.error.is_some());

        // the right one is reviewed before anything is written
        app.get_from_community(&shared, &ctx);
        wait_until(Duration::from_secs(10), "the second file", || {
            app.poll_community(&ctx);
            app.configs.import.is_some()
        });
        let mut dialog = app.configs.import.clone().unwrap();
        assert!(dialog.loaded.is_some(), "{:?}", dialog.error);
        assert!(!dir.join("config/profiles/fast-chat.toml").exists());
        app.finish_import(&mut dialog, false);
        let stored = std::fs::read_to_string(dir.join("config/profiles/fast-chat.toml")).unwrap();
        assert!(
            stored.contains("community = { id = \"fast-chat\", version = 3 }"),
            "{stored}"
        );
        assert!(api.received().iter().all(|r| r.install.is_none()));
    }
}
