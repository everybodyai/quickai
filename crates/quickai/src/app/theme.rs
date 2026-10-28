//! The look of the window: the colours of each theme, the type, the style egui draws with, and the few pieces the
//! panels share (a group box for a section, an alert, a tag, the state of the server as the status bar says it, and
//! buttons that say which one matters).
//!
//! The look is a classic desktop tool's: neutral greys, one accent (the green of the brand), crisp 1 px lines, 4 px
//! corners, IBM Plex for the words. The colours are chosen here and nowhere else. The state of the server keeps the
//! meaning it always had (grey stopped, amber starting, green running, red crashed); the greens, ambers and reds are
//! lighter on the dark theme and deeper on the light one, so that they read on both.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

use crate::events::ServerState;

/// Which theme is drawn now, for the code that has no `Ui` to ask (`state_color`, the constants of a
/// panel). Set when the theme is applied.
static DARK: AtomicBool = AtomicBool::new(true);

/// What a theme is made of.
pub(super) struct Palette {
    /// The page: the face of the window behind its group boxes.
    pub page: Color32,
    /// The bars round the page: the menu bar, the toolbar, the status bar and the list of models.
    pub bar: Color32,
    /// A group box, and the panels of a window that is not the main one.
    pub card: Color32,
    /// A button at rest, and how it looks hovered and pressed.
    pub raised: Color32,
    pub raised_hover: Color32,
    pub raised_active: Color32,
    /// What is typed into (the darkest thing of the dark theme).
    pub well: Color32,
    /// The log's own fill, never taken for a box that is typed into.
    pub log: Color32,
    pub line: Color32,
    /// The outline of a control (a button, a box, a tick box): stronger than the line between sections.
    pub outline: Color32,
    /// Text that is read (labels, sentences): a little softer than `text`, which is that of a title, a name, a
    /// button, and of whatever is strong.
    pub body: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    /// The button that matters, and the text on it.
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_active: Color32,
    pub on_accent: Color32,
    /// A tint behind what is chosen (the model in the list), and the green of text.
    pub accent_soft: Color32,
    pub accent_text: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub bad: Color32,
}

const DARK_PALETTE: Palette = Palette {
    page: Color32::from_rgb(31, 32, 35),
    bar: Color32::from_rgb(43, 45, 49),
    card: Color32::from_rgb(38, 39, 43),
    raised: Color32::from_rgb(58, 60, 65),
    raised_hover: Color32::from_rgb(69, 71, 77),
    raised_active: Color32::from_rgb(51, 53, 58),
    well: Color32::from_rgb(14, 15, 16),
    log: Color32::from_rgb(24, 25, 27),
    line: Color32::from_rgb(57, 59, 64),
    outline: Color32::from_rgb(85, 88, 95),
    body: Color32::from_rgb(211, 213, 217),
    text: Color32::from_rgb(241, 242, 244),
    muted: Color32::from_rgb(164, 168, 175),
    faint: Color32::from_rgb(142, 146, 153),
    // a step brighter than the website's green on dark (#2E8545): the button stands 3 to 1 from the toolbar and 3.6 from
    // the page, where (36, 110, 55) stood 2.6 and a control wants 3; white on it is 4.55. The GUI suites find the green
    // within a band that holds both (WP4.26)
    accent: Color32::from_rgb(46, 134, 69),
    accent_hover: Color32::from_rgb(41, 122, 63),
    accent_active: Color32::from_rgb(34, 104, 53),
    on_accent: Color32::from_rgb(255, 255, 255),
    accent_soft: Color32::from_rgb(31, 58, 41),
    accent_text: Color32::from_rgb(124, 210, 142),
    ok: Color32::from_rgb(111, 208, 138),
    warn: Color32::from_rgb(233, 179, 90),
    bad: Color32::from_rgb(242, 135, 122),
};

const LIGHT_PALETTE: Palette = Palette {
    page: Color32::from_rgb(247, 247, 247),
    bar: Color32::from_rgb(238, 238, 238),
    card: Color32::from_rgb(255, 255, 255),
    raised: Color32::from_rgb(240, 240, 240),
    raised_hover: Color32::from_rgb(229, 229, 229),
    raised_active: Color32::from_rgb(217, 217, 217),
    well: Color32::from_rgb(255, 255, 255),
    log: Color32::from_rgb(251, 251, 251),
    line: Color32::from_rgb(218, 218, 218),
    outline: Color32::from_rgb(184, 184, 184),
    body: Color32::from_rgb(46, 47, 50),
    text: Color32::from_rgb(20, 20, 22),
    muted: Color32::from_rgb(92, 96, 102),
    faint: Color32::from_rgb(103, 107, 113),
    accent: Color32::from_rgb(36, 110, 55),
    accent_hover: Color32::from_rgb(30, 94, 47),
    accent_active: Color32::from_rgb(24, 80, 40),
    on_accent: Color32::from_rgb(255, 255, 255),
    accent_soft: Color32::from_rgb(225, 240, 229),
    accent_text: Color32::from_rgb(30, 107, 50),
    ok: Color32::from_rgb(31, 122, 60),
    warn: Color32::from_rgb(143, 82, 0),
    bad: Color32::from_rgb(179, 38, 30),
};

/// The colours of the theme that is drawn now.
pub(super) fn palette() -> &'static Palette {
    if DARK.load(Ordering::Relaxed) {
        &DARK_PALETTE
    } else {
        &LIGHT_PALETTE
    }
}

/// Whether the theme that is drawn now is the dark one.
pub(super) fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// The colours of one theme.
pub(super) fn palette_of(dark: bool) -> &'static Palette {
    if dark {
        &DARK_PALETTE
    } else {
        &LIGHT_PALETTE
    }
}

/// The colour that says what the server is doing.
pub(super) fn state_color(state: ServerState) -> Color32 {
    state_color_in(palette(), state)
}

/// The colour of a state in the colours of one theme.
fn state_color_in(p: &Palette, state: ServerState) -> Color32 {
    match state {
        ServerState::Idle => p.faint,
        ServerState::Starting | ServerState::Stopping => p.warn,
        ServerState::Running => p.ok,
        ServerState::Crashed => p.bad,
    }
}

/// The family of headings: Plex Sans SemiBold, with the words' own fonts behind it for what it lacks.
pub(super) const SEMIBOLD: &str = "semibold";

/// The family of headings where it is bound (the theme binds it), and the words' own family where it is not (a context
/// that was never given the theme: a test's).
pub(super) fn semibold(ui: &egui::Ui) -> egui::FontFamily {
    let family = egui::FontFamily::Name(SEMIBOLD.into());
    if ui
        .ctx()
        .fonts(|f| f.definitions().families.contains_key(&family))
    {
        family
    } else {
        egui::FontFamily::Proportional
    }
}

/// IBM Plex Sans for the words and Plex Mono for the log, embedded; egui's own fonts stay behind them, for the signs
/// and symbols Plex does not have (★, the emoji of a model's name).
fn fonts() -> egui::FontDefinitions {
    use egui::FontFamily::{Monospace, Name, Proportional};
    let mut fonts = egui::FontDefinitions::default();
    for (key, bytes) in [
        (
            "plex-sans",
            &include_bytes!("../../../../assets/fonts/IBMPlexSans-Regular.ttf")[..],
        ),
        (
            "plex-sans-semibold",
            &include_bytes!("../../../../assets/fonts/IBMPlexSans-SemiBold.ttf")[..],
        ),
        (
            "plex-mono",
            &include_bytes!("../../../../assets/fonts/IBMPlexMono-Regular.ttf")[..],
        ),
    ] {
        fonts.font_data.insert(
            key.to_string(),
            Arc::new(egui::FontData::from_static(bytes)),
        );
    }
    let proportional = fonts
        .families
        .get(&Proportional)
        .cloned()
        .unwrap_or_default();
    let mut semibold = vec!["plex-sans-semibold".to_string()];
    semibold.extend(proportional.iter().cloned());
    fonts.families.insert(Name(SEMIBOLD.into()), semibold);
    fonts
        .families
        .entry(Proportional)
        .or_default()
        .insert(0, "plex-sans".to_string());
    fonts
        .families
        .entry(Monospace)
        .or_default()
        .insert(0, "plex-mono".to_string());
    fonts
}

/// Makes `dark` or the light theme the one the window is drawn in: the colours, the type, the sizes of the
/// text, the spacing and the shape of the widgets.
pub(super) fn apply(ctx: &egui::Context, dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(egui::Theme::Dark, visuals(true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(false));
    ctx.all_styles_mut(shape);
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

fn visuals(dark: bool) -> egui::Visuals {
    let p = palette_of(dark);
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let radius = CornerRadius::same(4);
    v.override_text_color = None;
    v.weak_text_color = Some(p.muted);
    v.panel_fill = p.page;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.outline);
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 38 }),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 3],
        blur: 10,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 100 } else { 32 }),
    };
    v.extreme_bg_color = p.well;
    v.faint_bg_color = p.raised;
    v.code_bg_color = p.well;
    v.hyperlink_color = p.accent_text;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.bad;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.5 };

    let w = &mut v.widgets;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.body);
    w.noninteractive.corner_radius = radius;

    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0, p.outline);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = radius;

    w.hovered.weak_bg_fill = p.raised_hover;
    w.hovered.bg_fill = p.raised_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.faint);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.weak_bg_fill = p.raised_active;
    w.active.bg_fill = p.raised_active;
    w.active.bg_stroke = Stroke::new(1.0, p.accent_text);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.weak_bg_fill = p.raised;
    w.open.bg_fill = p.raised;
    w.open.bg_stroke = Stroke::new(1.0, p.outline);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = radius;
    v
}

/// The sizes: Plex at a size that reads, room enough, controls of a desktop tool's height.
fn shape(style: &mut egui::Style) {
    use egui::FontFamily::{Monospace, Name, Proportional};
    use egui::TextStyle::{Body, Button, Heading, Small};
    style
        .text_styles
        .insert(Small, egui::FontId::new(12.0, Proportional));
    style
        .text_styles
        .insert(Body, egui::FontId::new(14.0, Proportional));
    style
        .text_styles
        .insert(Button, egui::FontId::new(14.0, Proportional));
    style
        .text_styles
        .insert(Heading, egui::FontId::new(18.0, Name(SEMIBOLD.into())));
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(13.0, Monospace),
    );
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.interact_size = egui::vec2(40.0, 26.0);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.menu_margin = Margin::same(4);
    style.spacing.combo_width = 120.0;
    // a list that drops down is as tall as what is in it, up to a window's height: with rows of 32 the list of
    // configurations and the three commands under it did not fit in 200, and the last of them was behind a scroll
    style.spacing.combo_height = 400.0;
}

// ---- frames -------------------------------------------------------------------------------------

/// Makes the window of its own that is being drawn as tall as what it holds: `wanted` is its content with its margins
/// and footer, kept between `least` and most of the screen (past that it scrolls). No empty room is left at its foot.
/// The width stays the person's; a height they set by hand stays too, until what the window holds changes (the height
/// last asked for is remembered, and asked for again only when it is another).
pub(super) fn fit_window_height(ctx: &egui::Context, wanted: f32, least: f32) {
    let (inner, monitor) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().monitor_size));
    let id = ctx.viewport_id();
    let Some(inner) = inner else {
        return;
    };
    let most = monitor.map_or(900.0, |m| (m.y * 0.85).max(least));
    let want = wanted.ceil().clamp(least, most);
    let key = egui::Id::new(("fit_window_height", id));
    let asked: Option<f32> = ctx.data(|d| d.get_temp(key));
    if asked.is_some_and(|a| (a - want).abs() < 1.5) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(key, want));
    if (inner.height() - want).abs() > 1.5 {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            inner.width(),
            want,
        )));
    }
}

/// The page of a window of its own, in `frame`, under a footer `footer` tall: scrolled when it must be, and the window as
/// tall as what `add` draws in it, the least `least` (`fit_window_height`).
pub(super) fn fitted_page(
    ui: &mut egui::Ui,
    salt: &str,
    frame: egui::Frame,
    footer: f32,
    least: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let margin = frame.total_margin().sum().y;
    let content = egui::CentralPanel::default()
        .frame(frame)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(salt)
                .auto_shrink([false, false])
                .show(ui, add)
                .content_size
                .y
        })
        .inner;
    fit_window_height(ui.ctx(), content + margin + footer, least);
}

/// The frame of the page: the face of the window, and room round its group boxes.
pub(super) fn page_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(palette().page)
        .inner_margin(Margin::symmetric(12, 10))
}

/// The frame of a bar round the page (the menu bar, the toolbar, the status bar).
pub(super) fn bar_frame(margin: Margin) -> egui::Frame {
    egui::Frame::new().fill(palette().bar).inner_margin(margin)
}

/// The widest the page of the Main tab is drawn. In a window wider than this the group boxes keep this width, in the
/// middle, and the page's own colour is on both sides: a row of controls a screen wide has its labels and its buttons
/// far apart and is read with the head turning.
pub(super) const PAGE_WIDTH: f32 = 960.0;

/// `add`, in a column of at most `width` in the middle of the room there is. In a window that is not wider than the column
/// this is the page as it is, and the same call is made for each part of a page so that all of it has the same sides.
pub(super) fn column<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let room = ui.available_rect_before_wrap();
    let side = ((room.width() - width) / 2.0).floor();
    if side <= 0.0 {
        return add(ui);
    }
    let column = egui::Rect::from_min_max(
        egui::pos2(room.left() + side, room.top()),
        egui::pos2(room.right() - side, room.bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(column), add)
        .inner
}

/// A section of the page, as wide as the page: what belongs together, in a box. `add` draws in it.
pub(super) fn card<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    })
}

/// The frame of a box, for a section that adds to it (the configuration's, when it has changes).
pub(super) fn card_frame() -> egui::Frame {
    let p = palette();
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(12, 8))
}

/// The frame of what is to be copied as it is (an address, a command, code): sunk into the page, in a thin outline.
pub(super) fn well_frame(margin: Margin) -> egui::Frame {
    let p = palette();
    egui::Frame::new()
        .fill(p.well)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(margin)
}

/// What an alert is about.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Bad,
    Warn,
    Good,
}

impl Tone {
    pub fn color(self) -> Color32 {
        let p = palette();
        match self {
            Tone::Bad => p.bad,
            Tone::Warn => p.warn,
            Tone::Good => p.ok,
        }
    }
}

/// A message that is to be seen: a box tinted in the colour of its tone, with a bar of that colour along its
/// left edge.
pub(super) fn alert<R>(
    ui: &mut egui::Ui,
    tone: Tone,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let color = tone.color();
    let p = palette();
    let fill = p.card.lerp_to_gamma(color, 0.10);
    let shown = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin {
            left: 14,
            right: 10,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let rect = shown.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 3.0, rect.bottom()),
        ),
        CornerRadius {
            nw: 4,
            sw: 4,
            ne: 0,
            se: 0,
        },
        color,
    );
    shown
}

/// Rows closer together for a list of things (the results of a look-up, the models that are recommended): buttons 20
/// high and 3 between the rows, as a list that is meant to be looked down wants, and not a form's room.
pub(super) fn compact(ui: &mut egui::Ui) {
    let spacing = ui.spacing_mut();
    spacing.interact_size.y = 20.0;
    spacing.button_padding.y = 1.0;
    spacing.item_spacing.y = 3.0;
}

// ---- small things -------------------------------------------------------------------------------

/// Small print in the colour of secondary text.
pub(super) fn muted(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).small().color(palette().muted)
}

/// A short tag for a fact about the thing next to it (how fast, what it can do): small words in a box with a thin
/// line round it, tinted in the colour of its tone when it has one.
pub(super) fn chip(ui: &mut egui::Ui, text: &str, tone: Option<Tone>) -> egui::Response {
    let p = palette();
    let (fill, stroke, fg) = match tone {
        Some(tone) => (
            p.card.lerp_to_gamma(tone.color(), 0.10),
            tone.color().gamma_multiply(0.45),
            tone.color(),
        ),
        None => (p.raised, p.line, p.muted),
    };
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = galley.size() + egui::vec2(12.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect(
            rect,
            CornerRadius::same(3),
            fill,
            Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0),
            galley,
            fg,
        );
    }
    response
}

/// What a button is for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// An ordinary button.
    Normal,
    /// The one that matters now: Start while stopped, Open Chat UI while running.
    Primary,
    /// One that ends something.
    Danger,
    /// One with no frame until the pointer is on it.
    Quiet,
}

/// A button of the role, enabled or not, with the least size it has. The response is the button's own, so that
/// hover text and clicks are asked of it as of any other.
pub(super) fn button_sized(
    ui: &mut egui::Ui,
    role: Role,
    enabled: bool,
    text: impl Into<egui::WidgetText>,
    least: egui::Vec2,
) -> egui::Response {
    let p = palette();
    let text = text.into();
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        match role {
            Role::Normal => {}
            Role::Primary => {
                for (state, fill) in [
                    (&mut w.inactive, p.accent),
                    (&mut w.hovered, p.accent_hover),
                    (&mut w.active, p.accent_active),
                ] {
                    state.weak_bg_fill = fill;
                    state.bg_fill = fill;
                    state.bg_stroke = Stroke::NONE;
                    state.fg_stroke = Stroke::new(state.fg_stroke.width, p.on_accent);
                }
            }
            Role::Danger => {
                let outline = p.bad.gamma_multiply(0.55);
                w.inactive.bg_stroke = Stroke::new(1.0, outline);
                w.inactive.fg_stroke = Stroke::new(1.0, p.bad);
                w.hovered.weak_bg_fill = p.card.lerp_to_gamma(p.bad, 0.12);
                w.hovered.bg_fill = w.hovered.weak_bg_fill;
                w.hovered.bg_stroke = Stroke::new(1.0, p.bad);
                w.hovered.fg_stroke = Stroke::new(1.0, p.bad);
                w.active.bg_stroke = Stroke::new(1.0, p.bad);
                w.active.fg_stroke = Stroke::new(1.0, p.bad);
            }
            Role::Quiet => {
                w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                w.inactive.bg_fill = Color32::TRANSPARENT;
                w.inactive.bg_stroke = Stroke::NONE;
                w.inactive.fg_stroke = Stroke::new(1.0, p.muted);
                w.hovered.bg_stroke = Stroke::NONE;
            }
        }
        ui.add_enabled(enabled, egui::Button::new(text).min_size(least))
    })
    .inner
}

/// A dot `radius` round in the middle of a square `side` wide, in `color`: a state at a glance.
pub(super) fn dot(ui: &mut egui::Ui, side: f32, radius: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
}

/// The state of the server as the status bar says it: a dot in the colour of the state (one that pulses while the
/// state is on its way) and the state in words.
pub(super) fn state_indicator(ui: &mut egui::Ui, state: ServerState, text: &str) -> egui::Response {
    let color = state_color(state);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let size = egui::vec2(galley.size().x + 16.0, galley.size().y.max(16.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let busy = matches!(state, ServerState::Starting | ServerState::Stopping);
        let pulse = if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
            let t = ui.input(|i| i.time) as f32;
            0.55 + 0.45 * (t * 5.0).sin()
        } else {
            1.0
        };
        let centre = egui::pos2(rect.left() + 4.0, rect.center().y);
        ui.painter()
            .circle_filled(centre, 4.0, color.gamma_multiply(pulse));
        ui.painter().galley(
            egui::pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One channel of a colour in linear light, as WCAG says it.
    fn linear(channel: u8) -> f64 {
        let c = f64::from(channel) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: Color32) -> f64 {
        0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
    }

    /// How well one colour reads on another, from 1 (the same) to 21 (black on white).
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn p3_i01_every_colour_of_text_reads_where_the_window_draws_it_in_both_themes() {
        for dark in [true, false] {
            let p = palette_of(dark);
            let name = if dark { "dark" } else { "light" };
            for (what, fg, bg) in [
                ("text on the card", p.text, p.card),
                ("text that is read, on the card", p.body, p.card),
                ("text that is read, on the page", p.body, p.page),
                ("text that is read, in the log", p.body, p.log),
                ("text on the page", p.text, p.page),
                ("text on the band of the title row", p.text, p.bar),
                ("text in a box that is typed in", p.text, p.well),
                ("text on a button", p.text, p.raised),
                ("text in the log", p.text, p.log),
                ("secondary text on the card", p.muted, p.card),
                ("secondary text on the page", p.muted, p.page),
                ("a state that is stopped, on the page", p.faint, p.page),
                ("a state that is stopped, in the log", p.faint, p.log),
                ("running on the card", p.ok, p.card),
                ("running on the page", p.ok, p.page),
                ("starting on the card", p.warn, p.card),
                ("starting on the page", p.warn, p.page),
                ("crashed on the card", p.bad, p.card),
                ("crashed on the page", p.bad, p.page),
                ("green text on the card", p.accent_text, p.card),
                (
                    "the words of the button that matters",
                    p.on_accent,
                    p.accent,
                ),
            ] {
                let ratio = contrast(fg, bg);
                assert!(
                    ratio >= 4.5,
                    "{what} in the {name} theme is {ratio:.2} to 1, and 4.5 is the least that reads"
                );
            }
        }
    }

    #[test]
    fn the_button_that_matters_stands_out_from_what_is_round_it_as_a_control_should() {
        // WCAG asks 3 to 1 of a control against what is next to it: the page and the bars, in both themes
        for (name, dark) in [("dark", true), ("light", false)] {
            let p = palette_of(dark);
            for (what, bg) in [("the page", p.page), ("the toolbar", p.bar)] {
                let ratio = contrast(p.accent, bg);
                assert!(
                    ratio >= 3.0,
                    "the button that matters on {what} in the {name} theme is {ratio:.2} to 1"
                );
            }
        }
    }

    #[test]
    fn p3_i02_the_states_of_the_server_have_colours_of_their_own_and_stopping_is_starting() {
        for dark in [true, false] {
            let p = palette_of(dark);
            let colours: Vec<Color32> = [
                ServerState::Idle,
                ServerState::Starting,
                ServerState::Running,
                ServerState::Crashed,
            ]
            .iter()
            .map(|state| state_color_in(p, *state))
            .collect();
            for (i, one) in colours.iter().enumerate() {
                for other in &colours[i + 1..] {
                    assert_ne!(one, other, "two states look the same");
                }
            }
            assert_eq!(
                state_color_in(p, ServerState::Stopping),
                state_color_in(p, ServerState::Starting)
            );
        }
    }

    #[test]
    fn p3_i03_what_is_typed_in_is_set_into_the_page_and_the_log_is_not_taken_for_it() {
        // the dark well of a box that is typed in is what the suites find a box by (no channel above 16); the
        // title row's band and the log are lighter than that, so that neither is taken for a box
        let dark = palette_of(true);
        assert!(dark.well.r().max(dark.well.g()).max(dark.well.b()) <= 16);
        for other in [dark.log, dark.bar, dark.page, dark.card, dark.raised] {
            assert!(other.r().max(other.g()).max(other.b()) > 16);
        }
        // and the box of a number differs from the card it is on, which is how its edges are found
        for dark in [true, false] {
            let p = palette_of(dark);
            let spread = |a: Color32, b: Color32| {
                a.r()
                    .abs_diff(b.r())
                    .max(a.g().abs_diff(b.g()))
                    .max(a.b().abs_diff(b.b()))
            };
            assert!(spread(p.raised, p.card) > 12, "{dark}");
        }
    }

    #[test]
    fn p3_i26_a_column_is_in_the_middle_of_a_window_wider_than_it_and_is_the_window_in_one_that_is_not(
    ) {
        let ctx = egui::Context::default();
        let column_in = |window: f32| -> (f32, f32) {
            let mut seen = (f32::NAN, f32::NAN);
            // a few frames, as a window is drawn: the first has no room worked out
            for _ in 0..3 {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(window, 300.0),
                    )),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(raw, |ui| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| {
                            column(ui, PAGE_WIDTH, |ui| {
                                let room = ui.available_rect_before_wrap();
                                seen = (room.left(), room.width());
                            });
                        });
                });
                output.textures_delta.clear();
            }
            seen
        };
        // not wider than the column: the window is the page, as it was
        assert_eq!(column_in(700.0), (0.0, 700.0));
        assert_eq!(column_in(PAGE_WIDTH), (0.0, PAGE_WIDTH));
        // wider: the column keeps its width and has the same room on both sides (an odd room gives one pixel to the column)
        for window in [1000.0, 1400.0, 1861.0] {
            let (left, width) = column_in(window);
            assert!(
                (PAGE_WIDTH..=PAGE_WIDTH + 1.0).contains(&width),
                "{window}: the column is {width} wide"
            );
            assert!(
                (window - (left + width) - left).abs() <= 1.0,
                "{window}: {left} on the left and {} on the right",
                window - (left + width)
            );
        }
    }
}
