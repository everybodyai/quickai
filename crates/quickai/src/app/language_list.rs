//! A list of languages in a button (Settings › Voice: the language spoken, and the language answers are read in).
//! Opened, it has a search box, one choice above the languages (letting it tell, or none), then two columns: on the
//! left the groups (the most widely spoken, then the parts of the world, then, for reading, the person's own voices),
//! on the right the languages of the group chosen, which scroll on their own. The list keeps one height however many
//! languages there are, and any language is two clicks away, or a few letters of its name, in English or in itself.

use eframe::egui;

use super::theme;
use crate::voice::languages::{folded, Group, Language, WIDELY_SPOKEN};

/// The width of the opened list, of its left column, the height of its columns (ten groups of rows), and of a row.
const WIDTH: f32 = 440.0;
const LEFT: f32 = 160.0;
const COLUMN: f32 = 262.0;
const ROW: f32 = 24.0;

/// A group of the left column: one of languages, or the person's own voices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Column {
    Languages(Group),
    Own,
}

/// What the list remembers while it is open: what is typed, and the group shown.
#[derive(Default)]
pub(super) struct ListState {
    query: String,
    column: Option<Column>,
    open: bool,
}

/// What is chosen in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    /// The choice above the languages.
    Top,
    Language(&'static str),
    /// One of the person's own voices, by its place in `own`.
    Own(usize),
}

/// What a list offers besides its languages.
pub(super) struct Offer<'a> {
    /// The words of the choice above the languages.
    pub(super) top: &'a str,
    /// The languages at the very top, above the groups (English): before the choice above the languages, or after it
    /// when `top_first`.
    pub(super) pinned: &'a [&'static str],
    pub(super) top_first: bool,
    /// The person's own voices, with what each is: a group of their own when there are any.
    pub(super) own: &'a [(&'a str, &'a str)],
    /// The languages there is no voice for yet: shown quieter, and said so when the pointer rests on one.
    pub(super) unoffered: &'a [&'static str],
}

/// The list. `shown` is on the button (`placeholder` greys it, for nothing chosen yet); what was chosen comes back.
#[allow(clippy::too_many_arguments)]
pub(super) fn language_list(
    ui: &mut egui::Ui,
    id: &str,
    shown: &str,
    placeholder: bool,
    languages: &[&'static Language],
    current: Choice,
    offer: &Offer<'_>,
    state: &mut ListState,
) -> Option<Choice> {
    let p = theme::palette();
    let mut picked = None;
    let mut open_now = false;
    let text = if placeholder {
        egui::RichText::new(shown).color(p.muted)
    } else {
        egui::RichText::new(shown)
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(220.0)
        .height(
            COLUMN
                + 80.0
                + (ROW + 2.0) * offer.pinned.len() as f32
                + if offer.own.is_empty() {
                    0.0
                } else {
                    ROW + 14.0
                },
        )
        .truncate()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            open_now = true;
            if !state.open {
                // just opened: on the group of what is chosen, with nothing typed
                state.query.clear();
                state.column = Some(match current {
                    Choice::Language(code) => Column::Languages(Group::of(Some(code))),
                    Choice::Own(_) => Column::Own,
                    Choice::Top => Column::Languages(Group::WidelySpoken),
                });
            }
            ui.set_width(WIDTH);
            let query = super::talk::search_box(ui, &mut state.query, "Search languages");
            let query = folded(&query);
            if query.is_empty() {
                // the choice above the languages and the languages pinned at the top, in the order the list asks
                ui.spacing_mut().item_spacing.y = 2.0;
                if offer.top_first
                    && row(ui, current == Choice::Top, offer.top, None, false).clicked()
                {
                    picked = Some(Choice::Top);
                }
                for l in offer
                    .pinned
                    .iter()
                    .filter_map(|code| languages.iter().find(|l| l.code == *code))
                {
                    if language_row(ui, l, current == Choice::Language(l.code), offer).clicked() {
                        picked = Some(Choice::Language(l.code));
                    }
                }
                if !offer.top_first
                    && row(ui, current == Choice::Top, offer.top, None, false).clicked()
                {
                    picked = Some(Choice::Top);
                }
                ui.add_space(3.0);
                let line = ui.available_rect_before_wrap();
                ui.painter()
                    .hline(line.x_range(), line.top(), egui::Stroke::new(1.0, p.line));
                ui.add_space(4.0);
                let column = state
                    .column
                    .unwrap_or(Column::Languages(Group::WidelySpoken));
                ui.horizontal_top(|ui| {
                    // the groups
                    ui.vertical(|ui| {
                        ui.set_width(LEFT);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for group in Group::all() {
                            let count = languages.iter().filter(|l| group.has(l)).count();
                            if count == 0 {
                                continue;
                            }
                            let here = matches!(current, Choice::Language(code)
                                if languages.iter().any(|l| l.code == code && group.has(l)));
                            let row = group_row(
                                ui,
                                column == Column::Languages(group),
                                here,
                                group.name(),
                                count,
                            );
                            let row = match group {
                                Group::Region(r) => row.on_hover_text(r.long_name()),
                                Group::WidelySpoken => row.on_hover_text(
                                    "The languages most people speak, as a first language or not",
                                ),
                            };
                            if row.clicked() {
                                state.column = Some(Column::Languages(group));
                            }
                        }
                        if !offer.own.is_empty() {
                            ui.add_space(4.0);
                            let line = ui.available_rect_before_wrap();
                            ui.painter().hline(
                                line.x_range(),
                                line.top(),
                                egui::Stroke::new(1.0, p.line),
                            );
                            ui.add_space(4.0);
                            let here = matches!(current, Choice::Own(_));
                            if group_row(
                                ui,
                                column == Column::Own,
                                here,
                                "Your own voice",
                                offer.own.len(),
                            )
                            .on_hover_text("A voice you have, a speech server, or a program")
                            .clicked()
                            {
                                state.column = Some(Column::Own);
                            }
                        }
                    });
                    ui.separator();
                    // what is in the group chosen
                    egui::ScrollArea::vertical()
                        .id_salt((id, "column"))
                        .max_height(COLUMN)
                        .max_width(WIDTH - LEFT - 16.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(WIDTH - LEFT - 30.0);
                                ui.set_min_height(COLUMN);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                match column {
                                    Column::Languages(group) => {
                                        // the most widely spoken in the order of how many speak them, English first;
                                        // a region's in the order of their names
                                        let mut shown: Vec<&&'static Language> =
                                            languages.iter().filter(|l| group.has(l)).collect();
                                        if group == Group::WidelySpoken {
                                            shown.sort_by_key(|l| {
                                                WIDELY_SPOKEN.iter().position(|c| *c == l.code)
                                            });
                                        }
                                        for l in shown {
                                            let row = language_row(
                                                ui,
                                                l,
                                                current == Choice::Language(l.code),
                                                offer,
                                            );
                                            if row.clicked() {
                                                picked = Some(Choice::Language(l.code));
                                            }
                                        }
                                    }
                                    Column::Own => {
                                        for (k, (label, what)) in offer.own.iter().enumerate() {
                                            if row(
                                                ui,
                                                current == Choice::Own(k),
                                                label,
                                                None,
                                                false,
                                            )
                                            .on_hover_text(*what)
                                            .clicked()
                                            {
                                                picked = Some(Choice::Own(k));
                                            }
                                        }
                                    }
                                }
                            });
                        });
                });
            } else {
                // what was typed: every language it matches, from every group
                egui::ScrollArea::vertical()
                    .id_salt((id, "found"))
                    .max_height(COLUMN + 30.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let mut found = 0;
                        for l in languages.iter().filter(|l| l.matches(&query)) {
                            found += 1;
                            let row =
                                language_row(ui, l, current == Choice::Language(l.code), offer);
                            if row.clicked() {
                                picked = Some(Choice::Language(l.code));
                            }
                        }
                        if found == 0 {
                            ui.label(theme::muted("No language by that name.").small());
                        }
                    });
            }
            if picked.is_some() {
                ui.close();
            }
        });
    state.open = open_now;
    picked
}

/// A row of the list, of one height whatever is in it: `name` at its left, `right` (grey) at its right; `chosen` is
/// filled as egui fills what is chosen, `quiet` greys the name.
fn row(
    ui: &mut egui::Ui,
    chosen: bool,
    name: &str,
    right: Option<&str>,
    quiet: bool,
) -> egui::Response {
    let p = theme::palette();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let selection = ui.visuals().selection;
        let fill = if chosen {
            selection.bg_fill
        } else if response.hovered() {
            p.raised_hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4.0, fill);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let colour = if chosen {
            selection.stroke.color
        } else if quiet {
            p.muted
        } else {
            p.text
        };
        let painter = ui.painter().with_clip_rect(rect);
        painter.text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            colour,
        );
        if let Some(right) = right {
            painter.text(
                rect.right_center() - egui::vec2(8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                right,
                font,
                if chosen {
                    selection.stroke.color
                } else {
                    p.muted
                },
            );
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A group of the left column: its name, and how many languages it has; `chosen` is the group shown, `here` holds
/// what is chosen (a dot says so).
fn group_row(
    ui: &mut egui::Ui,
    chosen: bool,
    here: bool,
    name: &str,
    count: usize,
) -> egui::Response {
    let p = theme::palette();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if chosen {
            p.selected
        } else if response.hovered() {
            p.raised_hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4.0, fill);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let colour = if chosen { p.text } else { p.body };
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            colour,
        );
        let small = egui::TextStyle::Small.resolve(ui.style());
        ui.painter().text(
            rect.right_center() - egui::vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            small,
            p.faint,
        );
        if here {
            let width = ui
                .painter()
                .layout_no_wrap(name.to_string(), font, colour)
                .size()
                .x;
            ui.painter().circle_filled(
                rect.left_center() + egui::vec2(8.0 + width + 7.0, 0.0),
                2.5,
                p.accent_text,
            );
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A language of the right column: its name, and its own name in grey where this computer's fonts can draw it; a
/// language there is no voice for yet is quieter, and says so when the pointer rests on it.
fn language_row(
    ui: &mut egui::Ui,
    l: &Language,
    chosen: bool,
    offer: &Offer<'_>,
) -> egui::Response {
    let unoffered = offer.unoffered.contains(&l.code);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let native = (l.native != l.name && ui.ctx().fonts_mut(|f| f.has_glyphs(&font, l.native)))
        .then_some(l.native);
    let row = row(ui, chosen, l.name, native, unoffered);
    if unoffered {
        row.on_hover_text(
            "No voice for it yet: read with the default voice, or use a voice of your own.",
        )
    } else {
        row
    }
}
