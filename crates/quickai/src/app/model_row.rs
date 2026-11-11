//! One model as a row (option A of the mockups the person chose on 2026-11-11, `mockups/models-pages`): the same in the
//! recommended list, in Add a model's suggestions and in what a look-up found, so that a model looks the same wherever
//! it is offered. Two lines (its name with who made it, then what it is); what it can do, as pictures each in a place
//! of its own; the size its Get button fetches and whether that fits this computer; and the Get button, whose ▾ lists
//! the other sizes in plain words, with the file's own name small beside them for whoever wants it.

use eframe::egui;
use egui::{Color32, FontId, Pos2, Rect, Sense};
use quickai_core::lookup::present::{Entry, Fetch, Offer};

use super::icons::{self, Icon};
use super::lookup_view::BundleWords;
use super::theme;
use crate::hardware::{self, Fit};

/// The height of a row whose second line is one line.
const ROW_HEIGHT: f32 = 52.0;
/// Narrower than this, a row leaves out the column of what a model can do (its details still say it).
const ROOM_FOR_ABILITIES: f32 = 620.0;
/// The widths of the columns at the right, and the gap between columns.
const GET_WIDTH: f32 = 96.0;
const FIT_WIDTH: f32 = 64.0;
const SIZE_WIDTH: f32 = 70.0;
const ABILITIES_WIDTH: f32 = 6.0 * 18.0 + 5.0 * 4.0;
const GAP: f32 = 12.0;

/// What a row is drawn with besides its model: which offers fetch what is already there, this computer's memory and
/// whether a download is under way.
pub(super) struct RowLook<'a> {
    pub installed: &'a dyn Fn(&Offer) -> bool,
    /// The memory of this computer, to say whether a size fits; 0 says nothing.
    pub ram: u64,
    /// A download or a pull is under way: one at a time.
    pub busy: bool,
    /// The words of the engine's bundles (`with projector`).
    pub bundles: &'a [BundleWords],
}

/// One row: the model, the offer its Get button fetches and the ones its ▾ lists, and what it says.
pub(super) struct Row<'a> {
    pub entry: &'a Entry,
    pub main: &'a Offer,
    /// Every offer the ▾ lists, the main one among them, in the order they are listed.
    pub choices: Vec<&'a Offer>,
    /// The name it is shown under (a look-up's repository without its owner).
    pub title: String,
    /// What follows the name, small: who made it and how big it is.
    pub by: String,
    /// The second line: what it is, or for a look-up who put it there and how often it was downloaded.
    pub line: String,
    /// It is one the list says to start with: a star before its name.
    pub star: bool,
    /// The column of what it can do (a model of the recommended list says it; a look-up does not know).
    pub abilities: bool,
    /// A click on the row opens what more there is to say of it.
    pub expandable: bool,
}

/// What a row asks for.
pub(super) enum RowAction {
    Fetch(Fetch),
    /// Its details were opened or closed.
    Toggle,
}

/// What a model can do, in the order of their places in a row: the picture and the words its hover says.
pub(super) fn ability_slots(entry: &Entry) -> [(bool, Icon, &'static str); 6] {
    let card = entry.card.as_deref();
    let reads = |m: &str| card.is_some_and(|c| c.input.iter().any(|i| i == m));
    let has = |f: &str| card.is_some_and(|c| c.features.iter().any(|x| x == f));
    [
        (reads("image"), Icon::Eye, "Reads pictures"),
        (reads("audio"), Icon::Headphones, "Hears audio"),
        (has("thinking"), Icon::Bulb, "Thinks before it answers"),
        (has("tools"), Icon::Wrench, "Uses tools"),
        (has("multilingual"), Icon::Globe, "Many languages"),
        (has("fast"), Icon::Bolt, "Fast"),
    ]
}

/// Whether `offer` fits a computer of `ram` bytes: nothing said when either is unknown.
pub(super) fn fit_of(offer: &Offer, ram: u64) -> Option<Fit> {
    match offer.bytes {
        Some(bytes) if ram > 0 => Some(hardware::fit(bytes, ram)),
        _ => None,
    }
}

/// The words and colour of a fit.
fn fit_words(fit: Fit) -> (&'static str, Color32) {
    let p = theme::palette();
    match fit {
        Fit::Comfortable => ("Fits", p.ok),
        Fit::Tight => ("Tight", p.warn),
        Fit::TooBig => ("Too big", p.bad),
    }
}

/// What the hover of a size says of its fit.
fn fit_sentence(fit: Option<Fit>, ram: u64) -> String {
    let memory = hardware::format_bytes(ram);
    match fit {
        Some(Fit::Comfortable) => format!("It fits in this computer's memory ({memory})."),
        Some(Fit::Tight) => {
            format!("It fits in this computer's memory ({memory}), with little to spare.")
        }
        Some(Fit::TooBig) => format!("It is more than this computer's memory ({memory}) holds."),
        None => String::new(),
    }
}

/// Text in at most `rows` lines of `width`, ending in `…` only where even those would not hold it.
fn lines(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(10.0),
        max_rows: rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// The file's own name of an offer (`Q4_K_M`), without its size.
fn code_of(offer: &Offer) -> &str {
    offer.label.split(" · ").next().unwrap_or(&offer.label)
}

/// How good a quantization is for its size, in words; the label as it is when it is none known.
pub(super) fn quality(label: &str) -> String {
    let l = label.to_ascii_lowercase();
    let bits = l.trim_start_matches('i').trim_start_matches('t');
    let words = if bits.starts_with("q4") {
        "Standard"
    } else if bits.starts_with("q5") {
        "Better quality"
    } else if bits.starts_with("q6") || bits.starts_with("q8") {
        "Higher quality"
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

/// What an item of the ▾ says of an offer, in plain words, and the file's own name under it (empty when it would
/// say the same): `Recommended · reads pictures` over `Q4_K_M with projector`, `Without picture reading` over
/// `Q4_K_M`, `Smaller` over `Q4_K_S`.
pub(super) fn choice_words(
    entry: &Entry,
    offer: &Offer,
    main: &Offer,
    bundles: &[BundleWords],
) -> (String, String) {
    let code = code_of(offer);
    if let Some(piece) = offer.only() {
        return (format!("The {} alone", piece.label), code.to_string());
    }
    let bundle_label = offer.bundle.as_deref().map(|id| {
        bundles
            .iter()
            .find(|b| b.id == id)
            .map_or(id, |b| b.label.as_str())
    });
    let file = match bundle_label {
        Some(label) => format!("{code} {label}"),
        None => code.to_string(),
    };
    let is_main = std::ptr::eq(offer, main);
    let same_kind = code_of(main) == code;
    let mut grade = if is_main {
        "Recommended".to_string()
    } else {
        quality(code)
    };
    // two of a kind (Q4_K_S beside Q4_K_M): the one that is not the main one is smaller or bigger than it
    if !is_main && !same_kind && grade == quality(code_of(main)) {
        if let (Some(this), Some(that)) = (offer.bytes, main.bytes) {
            grade = if this < that { "Smaller" } else { "Bigger" }.to_string();
        }
    }
    let pictures = entry
        .offers
        .iter()
        .any(|o| o.bundle.as_deref() == Some("vision"));
    let words = match (offer.bundle.as_deref(), pictures) {
        (Some("vision"), _) => format!("{grade} · reads pictures"),
        (Some(_), _) => format!("{grade} · {}", bundle_label.unwrap_or_default()),
        (None, true) if same_kind => "Without picture reading".to_string(),
        (None, true) => format!("{grade}, without pictures"),
        (None, false) => grade,
    };
    let file = if file.eq_ignore_ascii_case(&words) {
        String::new()
    } else {
        file
    };
    (words, file)
}

/// A model's page on the site it comes from, when its offers say where: a repository of Hugging Face or a name of
/// Ollama's library.
pub(super) fn page_of(offer: &Offer) -> Option<String> {
    match &offer.fetch {
        Fetch::Files(download) => download
            .source
            .strip_prefix("huggingface:")
            .map(|repo| format!("https://huggingface.co/{repo}")),
        Fetch::Reference(name) if !name.contains('/') => Some(format!(
            "https://ollama.com/library/{}",
            name.split(':').next().unwrap_or(name)
        )),
        Fetch::Reference(_) => None,
    }
}

/// Draws a row; what it asks for, if anything. `open` is whether its details are shown under it (the caller draws
/// them, with `show_details`).
pub(super) fn show(
    ui: &mut egui::Ui,
    row: &Row<'_>,
    look: &RowLook<'_>,
    open: bool,
) -> Option<RowAction> {
    let p = theme::palette();
    let mut action = None;
    let width = ui.available_width();
    let abilities = row.abilities && width >= ROOM_FOR_ABILITIES;
    // the words have what the columns at the right leave; the second line may take two lines of it
    let columns = 8.0
        + GET_WIDTH
        + GAP
        + FIT_WIDTH
        + GAP
        + SIZE_WIDTH
        + GAP
        + if abilities {
            ABILITIES_WIDTH + GAP
        } else {
            0.0
        };
    let words_left = if row.expandable { 30.0 } else { 8.0 };
    let words_width = (width - columns - words_left).max(60.0);
    let line = lines(
        ui,
        &row.line,
        FontId::proportional(12.5),
        p.muted,
        words_width,
        2,
    );
    let height = (28.0 + line.size().y + 8.0).max(ROW_HEIGHT);
    let sense = if row.expandable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let id = ui.id().with(("model_row", &row.entry.key));
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let painter = ui.painter().clone();
    if open || (row.expandable && response.hovered()) {
        painter.rect_filled(rect, 6.0, p.card);
    }
    if !open {
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, p.line),
        );
    }

    // the columns, from the right, beside the name and the first line of the second
    let mid = rect.top() + ROW_HEIGHT / 2.0;
    let get_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 8.0 - GET_WIDTH, mid - 14.0),
        egui::vec2(GET_WIDTH, 28.0),
    );
    let fit_rect = Rect::from_min_size(
        Pos2::new(get_rect.left() - GAP - FIT_WIDTH, mid - 9.0),
        egui::vec2(FIT_WIDTH, 18.0),
    );
    let size_rect = Rect::from_min_size(
        Pos2::new(fit_rect.left() - GAP - SIZE_WIDTH, mid - 9.0),
        egui::vec2(SIZE_WIDTH, 18.0),
    );
    let abilities_left = size_rect.left() - GAP - ABILITIES_WIDTH;
    let words_right = if abilities {
        abilities_left - GAP
    } else {
        size_rect.left() - GAP
    };

    // the name and the second line
    let mut x = rect.left() + 8.0;
    if row.expandable {
        let square = Rect::from_center_size(Pos2::new(x + 7.0, mid), egui::vec2(12.0, 12.0));
        let chevron = if open {
            Icon::Chevron
        } else {
            Icon::ChevronRight
        };
        icons::paint(&painter, square, chevron, p.faint);
        x += 22.0;
    }
    let top = rect.top() + 8.0;
    if row.star {
        let square = Rect::from_min_size(Pos2::new(x, top + 2.0), egui::vec2(13.0, 13.0));
        icons::paint(&painter, square, Icon::Star, p.warn);
        x += 17.0;
    }
    let name_font = FontId::new(14.0, theme::semibold(ui));
    let name = lines(ui, &row.title, name_font, p.text, words_right - x, 1);
    let name_width = name.size().x;
    painter.galley(Pos2::new(x, top), name, p.text);
    // who made it, when it fits whole beside the name (its details say it otherwise)
    if !row.by.is_empty() {
        let by = painter.layout_no_wrap(row.by.clone(), FontId::proportional(12.0), p.faint);
        if x + name_width + 8.0 + by.size().x <= words_right {
            painter.galley(Pos2::new(x + name_width + 8.0, top + 2.5), by, p.faint);
        }
    }
    painter.galley(
        Pos2::new(rect.left() + words_left, top + 20.0),
        line,
        p.muted,
    );

    // what it can do, each in a place of its own
    if abilities {
        for (i, (on, icon, words)) in ability_slots(row.entry).into_iter().enumerate() {
            if !on {
                continue;
            }
            let slot = Rect::from_min_size(
                Pos2::new(abilities_left + i as f32 * 22.0, mid - 9.0),
                egui::vec2(18.0, 18.0),
            );
            icons::paint(&painter, slot.shrink(1.5), icon, p.muted);
            ui.interact(slot, id.with(("can", i)), Sense::hover())
                .on_hover_text(words);
        }
    }

    // the size the button fetches, and whether it fits
    let fit = fit_of(row.main, look.ram);
    if let Some(bytes) = row.main.bytes {
        painter.text(
            Pos2::new(size_rect.right(), mid),
            egui::Align2::RIGHT_CENTER,
            hardware::format_bytes(bytes),
            FontId::proportional(13.0),
            p.body,
        );
    }
    if let Some(fit) = fit {
        let (words, color) = fit_words(fit);
        painter.circle_filled(Pos2::new(fit_rect.left() + 4.0, mid), 3.5, color);
        painter.text(
            Pos2::new(fit_rect.left() + 13.0, mid),
            egui::Align2::LEFT_CENTER,
            words,
            FontId::proportional(12.0),
            if fit == Fit::Comfortable {
                p.muted
            } else {
                color
            },
        );
    }

    // the button, or that it is there
    if (look.installed)(row.main) {
        let square = Rect::from_min_size(
            Pos2::new(get_rect.left() + 8.0, mid - 7.0),
            egui::vec2(14.0, 14.0),
        );
        icons::paint(&painter, square, Icon::Check, p.ok);
        painter.text(
            Pos2::new(square.right() + 6.0, mid),
            egui::Align2::LEFT_CENTER,
            "Installed",
            FontId::proportional(12.5),
            p.ok,
        );
        ui.interact(get_rect, id.with("there"), Sense::hover())
            .on_hover_text("Already in your models folder.");
    } else {
        let more = row.choices.len() > 1;
        let (get_part, more_part) = if more {
            let split = get_rect.right() - 28.0;
            (
                Rect::from_min_max(get_rect.min, Pos2::new(split, get_rect.bottom())),
                Some(Rect::from_min_max(
                    Pos2::new(split, get_rect.top()),
                    get_rect.max,
                )),
            )
        } else {
            (get_rect, None)
        };
        let enabled = !look.busy;
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let get = ui.interact(get_part, id.with("get"), sense);
        let fill = |r: &egui::Response| {
            if !enabled {
                p.raised.gamma_multiply(0.6)
            } else if r.is_pointer_button_down_on() {
                p.raised_active
            } else if r.hovered() {
                p.raised_hover
            } else {
                p.raised
            }
        };
        let fg = if enabled { p.text } else { p.faint };
        painter.rect_filled(
            get_part,
            egui::CornerRadius {
                nw: 7,
                sw: 7,
                ne: if more { 0 } else { 7 },
                se: if more { 0 } else { 7 },
            },
            fill(&get),
        );
        let square = Rect::from_min_size(
            Pos2::new(get_part.left() + 10.0, mid - 7.0),
            egui::vec2(14.0, 14.0),
        );
        icons::paint(&painter, square, Icon::Download, fg);
        painter.text(
            Pos2::new(square.right() + 6.0, mid),
            egui::Align2::LEFT_CENTER,
            "Get",
            FontId::proportional(12.5),
            fg,
        );
        let mut hover = row.main.hover.clone();
        let said = fit_sentence(fit, look.ram);
        if !said.is_empty() {
            hover = format!("{hover} {said}");
        }
        let get = if enabled {
            get.on_hover_text(hover)
        } else {
            get.on_hover_text("One download at a time: this one waits for the one under way.")
        };
        if get.clicked() {
            action = Some(RowAction::Fetch(row.main.fetch.clone()));
        }
        if let Some(more_part) = more_part {
            let others = ui.interact(more_part, id.with("sizes"), sense);
            painter.rect_filled(
                more_part,
                egui::CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 7,
                    se: 7,
                },
                fill(&others),
            );
            painter.vline(
                more_part.left(),
                more_part.y_range().shrink(5.0),
                egui::Stroke::new(1.0, p.page),
            );
            icons::paint(
                &painter,
                Rect::from_center_size(more_part.center(), egui::vec2(11.0, 11.0)),
                Icon::Chevron,
                if enabled { p.muted } else { p.faint },
            );
            let others = others.on_hover_text("The other sizes");
            egui::Popup::menu(&others)
                .align(egui::RectAlign::BOTTOM_END)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(340.0)
                .show(|ui| {
                    ui.set_min_width(340.0);
                    if let Some(fetch) = size_menu(ui, row, look) {
                        action = Some(RowAction::Fetch(fetch));
                        ui.close();
                    }
                });
        }
    }

    if row.expandable && response.clicked() && action.is_none() {
        action = Some(RowAction::Toggle);
    }
    action
}

/// The ▾ of a row: each size in plain words with the file's name under it, whether it fits, and how big it is; the
/// sizes with what goes with them (a projector) first, a line, then the model alone.
fn size_menu(ui: &mut egui::Ui, row: &Row<'_>, look: &RowLook<'_>) -> Option<Fetch> {
    let p = theme::palette();
    let mut chosen = None;
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(format!("GET {}", row.title.to_uppercase()))
            .size(11.0)
            .color(p.faint),
    );
    ui.add_space(2.0);
    let mut last_bundle: Option<Option<&str>> = None;
    for offer in &row.choices {
        let bundle = offer.bundle.as_deref();
        if last_bundle.is_some_and(|b| b != bundle) {
            ui.separator();
        }
        last_bundle = Some(bundle);
        let (words, file) = choice_words(row.entry, offer, row.main, look.bundles);
        let there = (look.installed)(offer);
        let enabled = !look.busy && !there;
        let height = if file.is_empty() { 30.0 } else { 40.0 };
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, height),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let main = std::ptr::eq(*offer, row.main);
            if response.hovered() && enabled {
                painter.rect_filled(rect, 5.0, p.raised_hover);
            } else if main {
                painter.rect_filled(rect, 5.0, p.raised);
            }
            let left = rect.left() + 8.0;
            let words_y = if file.is_empty() {
                rect.center().y
            } else {
                rect.top() + 12.0
            };
            painter.text(
                Pos2::new(left, words_y),
                egui::Align2::LEFT_CENTER,
                &words,
                FontId::proportional(13.0),
                if enabled { p.text } else { p.faint },
            );
            if !file.is_empty() {
                painter.text(
                    Pos2::new(left, rect.top() + 28.0),
                    egui::Align2::LEFT_CENTER,
                    &file,
                    FontId::monospace(11.0),
                    p.faint,
                );
            }
            let right = rect.right() - 8.0;
            if let Some(bytes) = offer.bytes {
                painter.text(
                    Pos2::new(right, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    hardware::format_bytes(bytes),
                    FontId::proportional(12.5),
                    p.body,
                );
            }
            let status_x = right - 64.0;
            if there {
                painter.text(
                    Pos2::new(status_x - 8.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "Installed",
                    FontId::proportional(12.0),
                    p.ok,
                );
            } else if let Some(fit) = fit_of(offer, look.ram) {
                let (fit_said, color) = fit_words(fit);
                painter.circle_filled(Pos2::new(status_x - 52.0, rect.center().y), 3.5, color);
                painter.text(
                    Pos2::new(status_x - 44.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    fit_said,
                    FontId::proportional(12.0),
                    if fit == Fit::Comfortable {
                        p.muted
                    } else {
                        color
                    },
                );
            }
        }
        let mut hover = offer.hover.clone();
        // what the extra file of a bundle is for, and what the model alone goes without
        match offer.bundle.as_deref() {
            Some(id) => {
                if let Some(bundle) = look.bundles.iter().find(|b| b.id == id) {
                    if !bundle.help.is_empty() {
                        hover = format!("{hover} {}", bundle.help);
                    }
                }
            }
            None => {
                let parts: Vec<&str> = look
                    .bundles
                    .iter()
                    .filter(|b| {
                        row.entry
                            .offers
                            .iter()
                            .any(|o| o.bundle.as_deref() == Some(b.id.as_str()))
                    })
                    .flat_map(|b| b.parts.iter().map(String::as_str))
                    .collect();
                if !parts.is_empty() {
                    let named = parts.join(" and the ");
                    hover = format!(
                        "{hover} Only the model, without the {named}: smaller, but what the {named} is for will not work."
                    );
                }
            }
        }
        let said = fit_sentence(fit_of(offer, look.ram), look.ram);
        if !said.is_empty() {
            hover = format!("{hover} {said}");
        }
        if response.on_hover_text(hover).clicked() && enabled {
            chosen = Some(offer.fetch.clone());
        }
    }
    chosen
}

/// What more there is to say of a model of the list, under its row: what it says of itself, its facts, and what it
/// can do in words.
pub(super) fn show_details(ui: &mut egui::Ui, entry: &Entry, main: &Offer) {
    let p = theme::palette();
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 30,
            right: 8,
            top: 2,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !entry.description.is_empty() {
                ui.add(
                    egui::Label::new(egui::RichText::new(&entry.description).color(p.body)).wrap(),
                );
                ui.add_space(6.0);
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let fact = |ui: &mut egui::Ui, name: &str, value: &str| {
                    if value.is_empty() {
                        return;
                    }
                    ui.label(egui::RichText::new(name).small().color(p.muted));
                    ui.label(egui::RichText::new(value).small().color(p.body));
                    ui.add_space(14.0);
                };
                if let Some(card) = entry.card.as_deref() {
                    fact(ui, "Maker", &card.by);
                    fact(ui, "Parameters", &card.params);
                    if let Some(context) = card.context {
                        fact(
                            ui,
                            "Context",
                            &format!("{} tokens", super::lookup_view::compact_count(context)),
                        );
                    }
                    fact(ui, "Licence", &card.license);
                }
                if let Some(page) = page_of(main) {
                    if ui
                        .link(egui::RichText::new("Model page").small())
                        .on_hover_text(&page)
                        .clicked()
                    {
                        let _ = webbrowser::open(&page);
                    }
                }
            });
            let can: Vec<(Icon, &str)> = ability_slots(entry)
                .into_iter()
                .filter(|(on, _, _)| *on)
                .map(|(_, icon, words)| (icon, words))
                .collect();
            if !can.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (icon, words) in can {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                        icons::paint(ui.painter(), r, icon, p.accent_text);
                        ui.label(egui::RichText::new(words).small().color(p.body));
                        ui.add_space(10.0);
                    }
                });
            }
        });
    let y = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, p.line));
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::lookup::present::{Download, Piece};

    fn offer(label: &str, bytes: u64, bundle: Option<&str>, recommended: bool) -> Offer {
        Offer {
            bundle: bundle.map(str::to_string),
            label: format!("{label} · {bytes} B"),
            hover: String::new(),
            bytes: Some(bytes),
            estimated: false,
            class: None,
            recommended,
            fetch: Fetch::Files(Download {
                files: vec![Piece {
                    kind: None,
                    label: "model".into(),
                    url: "https://huggingface.co/a/b/resolve/main/m.gguf".into(),
                    store_as: "m.gguf".into(),
                    name: "m.gguf".into(),
                    size: Some(bytes),
                    sha256: None,
                    place: None,
                }],
                bundle: bundle.map(str::to_string),
                source: "huggingface:a/b".into(),
                mode: None,
            }),
        }
    }

    fn entry(offers: Vec<Offer>) -> Entry {
        Entry {
            key: "k".into(),
            name: "a/b".into(),
            description: String::new(),
            downloads: None,
            likes: None,
            tags: Vec::new(),
            model_type: None,
            offers,
            card: None,
        }
    }

    fn words() -> Vec<BundleWords> {
        vec![BundleWords {
            id: "vision".into(),
            label: "with projector".into(),
            help: String::new(),
            parts: vec!["projector".into()],
        }]
    }

    #[test]
    fn the_sizes_of_a_model_that_reads_pictures_are_said_in_plain_words_with_the_file_under_them() {
        let e = entry(vec![
            offer("Q4_K_M", 700, Some("vision"), true),
            offer("Q8_0", 970, Some("vision"), false),
            offer("Q4_K_M", 500, None, false),
            offer("Q8_0", 770, None, false),
        ]);
        let main = &e.offers[0];
        let said: Vec<(String, String)> = e
            .offers
            .iter()
            .map(|o| choice_words(&e, o, main, &words()))
            .collect();
        assert_eq!(
            said,
            [
                (
                    "Recommended · reads pictures".into(),
                    "Q4_K_M with projector".into()
                ),
                (
                    "Higher quality · reads pictures".into(),
                    "Q8_0 with projector".into()
                ),
                ("Without picture reading".into(), "Q4_K_M".into()),
                ("Higher quality, without pictures".into(), "Q8_0".into()),
            ]
        );
    }

    #[test]
    fn a_size_of_the_same_kind_as_the_main_one_says_whether_it_is_smaller_or_bigger() {
        let e = entry(vec![
            offer("Q4_K_S", 17, None, false),
            offer("Q4_K_M", 18, None, true),
            offer("Q5_K_M", 21, None, false),
            offer("Q8_0", 32, None, false),
        ]);
        let main = &e.offers[1];
        let said: Vec<String> = e
            .offers
            .iter()
            .map(|o| choice_words(&e, o, main, &[]).0)
            .collect();
        assert_eq!(
            said,
            ["Smaller", "Recommended", "Better quality", "Higher quality"]
        );
    }

    #[test]
    fn a_page_is_found_for_a_repository_and_for_a_name_of_the_library() {
        let e = entry(vec![offer("Q4_K_M", 1, None, true)]);
        assert_eq!(
            page_of(&e.offers[0]).as_deref(),
            Some("https://huggingface.co/a/b")
        );
        let mut pulled = e.offers[0].clone();
        pulled.fetch = Fetch::Reference("qwen3:4b".into());
        assert_eq!(
            page_of(&pulled).as_deref(),
            Some("https://ollama.com/library/qwen3")
        );
    }
}
