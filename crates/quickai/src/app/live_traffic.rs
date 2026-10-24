//! The Live traffic tab: the clients that talk to the server down the left, and down the right what the picked one
//! asked and was answered, as it happens.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, CornerRadius, Margin, Stroke};

use super::theme::{self, Role};
use super::{App, Tab, TrafficTurn};
use crate::events::ServerState;

/// The widest the conversation is drawn: a line of an answer wider than this is one nobody follows to its end.
const CONVERSATION_WIDTH: f32 = 820.0;
/// How much of that width a bubble may take.
const BUBBLE_SHARE: f32 = 0.86;
/// The height of a row of the list of clients.
const ROW_HEIGHT: f32 = 46.0;
/// How much of what a client last asked its row keeps (the row cuts it to its width).
const PREVIEW_CHARS: usize = 120;

impl App {
    /// Every distinct client (see `TrafficTurn::session`) that has shown up
    /// in the Live traffic Traffic feed so far, in first-seen order.
    fn session_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for t in &self.traffic {
            if !keys.iter().any(|k| k == &t.session) {
                keys.push(t.session.clone());
            }
        }
        keys
    }

    /// Content of the Live traffic tab (see the tab strip in `ui()`): a list of the clients down the left (one row each,
    /// click to switch, or "All" to see everything mixed together) with the picked client's conversation filling the
    /// rest of the tab on the right: what it asked in a bubble on the right, the answer in one on the left.
    pub(super) fn show_live_traffic_tab(&mut self, ui: &mut egui::Ui) {
        // Owned, not borrowed from `self.traffic` — so nothing here keeps
        // `self` borrowed once we get to the mutable `selected_session`
        // writes below.
        let session_keys = self.session_keys();
        if let Some(sel) = &self.selected_session {
            if !session_keys.contains(sel) {
                self.selected_session = None;
            }
        }
        let now = Instant::now();
        // the times on the tab say "12 s ago": a second later they say another number, and a reply that is on its
        // way has dots that move
        if self.traffic.iter().any(|t| !t.done) {
            ui.ctx().request_repaint_after(Duration::from_millis(60));
        } else if !self.traffic.is_empty() {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        let p = theme::palette();

        egui::Panel::left("live_traffic_sessions_panel")
            .resizable(true)
            .default_size(236.0)
            .size_range(180.0..=360.0)
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .inner_margin(Margin::same(10)),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::bottom_up(Align::Min), |ui| {
                    // the button that empties the list is at the foot of the panel, where it is found, and the
                    // list is what is left above it
                    let width = ui.available_width();
                    if theme::button_sized(
                        ui,
                        Role::Normal,
                        !self.traffic.is_empty(),
                        "Clear all",
                        egui::vec2(width, 26.0),
                    )
                    .on_hover_text(
                        "Forget every conversation shown here (the clients are not told).",
                    )
                    .clicked()
                    {
                        self.traffic.clear();
                        self.selected_session = None;
                    }
                    ui.add_space(6.0);
                    ui.with_layout(egui::Layout::top_down(Align::Min), |ui| {
                        self.show_client_list(ui, &session_keys, now);
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.page)
                    .inner_margin(Margin::symmetric(14, 8)),
            )
            .show(ui, |ui| {
                self.show_conversation(ui, &session_keys, now);
            });
    }

    /// "Conversations" and the rows under it: All, then one for each client.
    fn show_client_list(&mut self, ui: &mut egui::Ui, session_keys: &[String], now: Instant) {
        ui.label(egui::RichText::new("Conversations").strong())
            .on_hover_text(
                "Click a client below to view just its conversation, or \"All\" for \
                 everything mixed together.",
            );
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("live_clients_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let total = self.traffic.len();
                let all = Row {
                    avatar: None,
                    title: "All".to_string(),
                    detail: format!("{total} message{}", plural(total)),
                    when: None,
                    live: self.traffic.iter().any(|t| !t.done),
                    selected: self.selected_session.is_none(),
                };
                if client_row(ui, &all).clicked() {
                    self.selected_session = None;
                }
                for (i, key) in session_keys.iter().enumerate() {
                    let mine = || self.traffic.iter().filter(|t| &t.session == key);
                    let count = mine().count();
                    let preview = mine()
                        .next_back()
                        .map(|t| {
                            t.prompt.clone().unwrap_or_else(|| {
                                if t.attachments.is_empty() {
                                    t.path.clone()
                                } else {
                                    t.attachments.join(", ")
                                }
                            })
                        })
                        .unwrap_or_default();
                    let last = mine().next_back().map(|t| t.ended.unwrap_or(t.started));
                    let row = Row {
                        avatar: Some((i + 1, client_color(i, theme::is_dark()))),
                        title: format!("Session {}", i + 1),
                        detail: format!(
                            "{count} message{} \u{b7} {}",
                            plural(count),
                            truncate_preview(&preview, PREVIEW_CHARS)
                        ),
                        when: last.map(|at| short_ago(now.saturating_duration_since(at))),
                        live: mine().any(|t| !t.done),
                        selected: self.selected_session.as_deref() == Some(key.as_str()),
                    };
                    if client_row(ui, &row)
                        .on_hover_text(format!("Client at {key}"))
                        .clicked()
                    {
                        self.selected_session = Some(key.clone());
                    }
                }
            });
    }

    /// The right-hand side: what the tab is, which conversation it shows and the way to copy it, then the exchanges
    /// (or, while there are none, what to do to get one).
    fn show_conversation(&mut self, ui: &mut egui::Ui, session_keys: &[String], now: Instant) {
        let selected = self.selected_session.clone();
        let shown = |t: &&TrafficTurn| selected.as_deref().is_none_or(|s| t.session == s);
        let number_of = |key: &str| session_keys.iter().position(|k| k == key);
        let which = match selected.as_deref().and_then(number_of) {
            Some(i) => format!("Session {}", i + 1),
            None => "All conversations".to_string(),
        };
        let nothing_to_copy = !self.traffic.iter().any(|t| shown(&t));
        // the heading, which conversation it is and the Copy button at the end of the row when it fits: in a narrow window
        // the button goes under them, and not out of the window (a row that is wider than the pane widens the pane's room
        // and everything under it with it)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().interact_size.y = 26.0;
            ui.label(egui::RichText::new("Live chat/completion traffic").strong())
                .on_hover_text(
                    "Every chat/completion request hitting this model, from any client, shown as \
                     it happens. Only chat/completion endpoints are shown here — other traffic \
                     (the chat UI's own page assets, health checks, etc.) is relayed to \
                     llama-server as normal but not logged.",
                );
            theme::chip(ui, &which, None);
            let copy = 60.0;
            let room = row_room(ui) - copy - ui.spacing().item_spacing.x;
            if room > 0.0 {
                ui.add_space(room);
            }
            if theme::button_sized(
                ui,
                Role::Normal,
                !nothing_to_copy,
                "Copy",
                egui::vec2(copy, 24.0),
            )
            .on_hover_text("Copy what is shown here, as plain text.")
            .clicked()
            {
                let text = transcript(
                    self.traffic
                        .iter()
                        .filter(|t| shown(t))
                        .map(|t| (t, number_of(&t.session).map(|i| i + 1))),
                );
                ui.ctx().copy_text(text);
            }
        });
        ui.add_space(6.0);
        ui.separator();

        if self.traffic.is_empty() {
            self.show_nothing_yet(ui);
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("live_turns_scroll")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::column(ui, CONVERSATION_WIDTH, |ui| {
                    ui.add_space(8.0);
                    let width = ui.available_width() * BUBBLE_SHARE;
                    for turn in self.traffic.iter().filter(|t| shown(t)) {
                        // with every client mixed together each exchange says whose it is
                        let tag = if selected.is_none() {
                            number_of(&turn.session)
                                .map(|i| (i + 1, client_color(i, theme::is_dark())))
                        } else {
                            None
                        };
                        show_turn(ui, turn, tag, now, width);
                        ui.add_space(16.0);
                    }
                });
            });
    }

    /// What the right-hand side says while nothing has been asked of the server yet: that it is waiting, and for what to
    /// ask it with, or what to do first when it is not there to be asked.
    fn show_nothing_yet(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let state = self.session.state();
        let address = (state == ServerState::Running)
            .then(|| self.connection_panel())
            .flatten()
            .map(|panel| panel.base_url);
        let words = match state {
            ServerState::Running => {
                "Waiting for the first request. Whatever talks to this server, the chat page \
                 or an app of yours, shows up here as it happens."
            }
            ServerState::Starting => "The server is starting. Requests show up here once it is up.",
            ServerState::Stopping => "The server is stopping.",
            _ => "The server is not running. Start it on the Main tab: requests show up here while it runs.",
        };
        let stopped = matches!(state, ServerState::Idle | ServerState::Crashed);
        let room = ui.available_height();
        ui.vertical_centered(|ui| {
            ui.add_space(((room - 250.0) / 2.0).max(20.0));
            signal_icon(ui, p.faint);
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("No traffic yet")
                    .size(18.0)
                    .color(p.text),
            );
            ui.add_space(2.0);
            ui.scope(|ui| {
                ui.set_max_width(420.0);
                ui.label(theme::muted(words));
            });
            ui.add_space(12.0);
            if let Some(address) = address {
                ui.horizontal(|ui| {
                    let text = egui::RichText::new(&address).monospace().color(p.text);
                    // centred: the address and its button are one thing
                    let font = egui::TextStyle::Monospace.resolve(ui.style());
                    let words = ui
                        .painter()
                        .layout_no_wrap(address.clone(), font, p.text)
                        .size()
                        .x;
                    // the box (10 px of margin each side and its outline), the space and the button, about 60 wide
                    let wide = words + 22.0 + ui.spacing().item_spacing.x + 60.0;
                    ui.add_space(((ui.available_width() - wide) / 2.0).max(0.0));
                    egui::Frame::new()
                        .fill(p.well)
                        .stroke(Stroke::new(1.0, p.line))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 4))
                        .show(ui, |ui| {
                            ui.label(text);
                        });
                    if theme::button_sized(ui, Role::Normal, true, "Copy", egui::vec2(0.0, 26.0))
                        .on_hover_text("Copy this address, for the app that is to use it.")
                        .clicked()
                    {
                        ui.ctx().copy_text(address.clone());
                    }
                });
            } else if stopped
                && theme::button_sized(
                    ui,
                    Role::Normal,
                    true,
                    "Open the Main tab",
                    egui::vec2(0.0, 28.0),
                )
                .clicked()
            {
                self.active_tab = Tab::Main;
            }
        });
    }
}

/// The face of a row of the list of clients.
struct Row {
    /// The number of the client and its colour; "All" has none.
    avatar: Option<(usize, Color32)>,
    title: String,
    detail: String,
    /// How long ago the client last did something, in a word or two.
    when: Option<String>,
    /// A reply to it is on its way.
    live: bool,
    selected: bool,
}

/// One row of the list: the client's number in its colour, its name, when it was last heard and the last of what it
/// asked. Clicking it is the response's.
fn client_row(ui: &mut egui::Ui, row: &Row) -> egui::Response {
    let p = theme::palette();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let (fill, stroke) = if row.selected {
        (p.accent_soft, Stroke::new(1.0, p.accent))
    } else if response.hovered() {
        (p.raised, Stroke::NONE)
    } else {
        (Color32::TRANSPARENT, Stroke::NONE)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(8),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let base = if row.selected { p.accent_soft } else { p.card };
    let mut left = rect.left() + 10.0;
    if let Some((number, color)) = row.avatar {
        let centre = egui::pos2(left + 13.0, rect.center().y);
        painter.circle_filled(centre, 13.0, base.lerp_to_gamma(color, AVATAR_TINT));
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            number.to_string(),
            egui::FontId::proportional(13.0),
            client_ink(color, theme::is_dark()),
        );
        left += 26.0 + 10.0;
    }
    let right = rect.right() - 10.0;
    if row.live {
        let centre = egui::pos2(right - 4.0, rect.top() + 14.0);
        painter.circle_filled(centre, 7.0, p.ok.gamma_multiply(0.22));
        painter.circle_filled(centre, 4.0, p.ok);
    } else if let Some(when) = &row.when {
        let galley = painter.layout_no_wrap(
            when.clone(),
            egui::TextStyle::Small.resolve(ui.style()),
            p.muted,
        );
        let at = egui::pos2(
            right - galley.size().x,
            rect.top() + 14.0 - galley.size().y / 2.0,
        );
        painter.galley(at, galley, p.muted);
    }
    let title = painter.layout_no_wrap(
        row.title.clone(),
        egui::TextStyle::Body.resolve(ui.style()),
        p.text,
    );
    painter.galley(egui::pos2(left, rect.top() + 6.0), title, p.text);
    let detail = one_line(
        painter,
        &row.detail,
        egui::TextStyle::Small.resolve(ui.style()),
        p.muted,
        (rect.right() - 10.0 - left).max(20.0),
    );
    painter.galley(egui::pos2(left, rect.top() + 26.0), detail, p.muted);
    response
}

/// How much of a client's colour is in the disc behind its number.
const AVATAR_TINT: f32 = 0.24;

/// `text` on one line, cut with an ellipsis where `width` ends.
fn one_line(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    painter.layout_job(job)
}

/// The colour of the client with this index: the same each time it is drawn, whatever else is on the tab, and a
/// different one for each of the first six.
fn client_color(index: usize, dark: bool) -> Color32 {
    const DARK: [Color32; 6] = [
        Color32::from_rgb(110, 175, 255),
        Color32::from_rgb(255, 170, 100),
        Color32::from_rgb(195, 150, 255),
        Color32::from_rgb(90, 215, 200),
        Color32::from_rgb(255, 130, 160),
        Color32::from_rgb(215, 205, 100),
    ];
    const LIGHT: [Color32; 6] = [
        Color32::from_rgb(25, 100, 190),
        Color32::from_rgb(160, 80, 10),
        Color32::from_rgb(110, 60, 185),
        Color32::from_rgb(10, 115, 105),
        Color32::from_rgb(175, 45, 90),
        Color32::from_rgb(110, 100, 0),
    ];
    let set = if dark { &DARK } else { &LIGHT };
    set[index % set.len()]
}

/// The colour a client's number and name are written in: its colour, moved toward the colour of text until it reads on
/// the disc that is tinted with it (a blue that is bright enough to be the colour of a client is not bright enough to
/// be read on the dark green of a row that is picked).
fn client_ink(color: Color32, dark: bool) -> Color32 {
    if dark {
        color.lerp_to_gamma(Color32::WHITE, 0.45)
    } else {
        color.lerp_to_gamma(Color32::BLACK, 0.3)
    }
}

/// The small tag in front of an exchange that says whose it is: the client's number in its colour, and its name.
fn client_tag(ui: &mut egui::Ui, number: usize, color: Color32) {
    let p = theme::palette();
    let text = format!("Session {number}");
    let font = egui::TextStyle::Small.resolve(ui.style());
    let ink = client_ink(color, theme::is_dark());
    let galley = ui.painter().layout_no_wrap(text, font, ink);
    let size = galley.size() + egui::vec2(20.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::same((size.y / 2.0) as u8),
            p.page.lerp_to_gamma(color, AVATAR_TINT),
        );
        painter.circle_filled(egui::pos2(rect.left() + 9.0, rect.center().y), 3.0, color);
        painter.galley(
            egui::pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
            galley,
            ink,
        );
    }
}

/// One exchange: the line that says whose it is, what was asked of the server and when; what the client asked, in a
/// bubble on the right; and the answer, in one on the left that fills in while it is written.
fn show_turn(
    ui: &mut egui::Ui,
    turn: &TrafficTurn,
    tag: Option<(usize, Color32)>,
    now: Instant,
    width: f32,
) {
    let p = theme::palette();
    // the line that says whose it is, what was asked and when, with the time the answer took (or that it is on its way)
    // at the end of the row when it fits, and on the next row when it does not
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if let Some((number, color)) = tag {
            client_tag(ui, number, color);
        }
        ui.label(
            egui::RichText::new(format!("{} {}", turn.method, turn.path))
                .monospace()
                .size(12.0)
                .color(p.body),
        )
        .on_hover_text("What the client asked of the server.");
        ui.label(theme::muted(ago(
            now.saturating_duration_since(turn.started)
        )));
        let end = if !turn.done {
            Some((String::new(), live_chip_width(ui)))
        } else {
            turn.ended.map(|ended| {
                let text = took(ended.saturating_duration_since(turn.started));
                let width = wrapped_width(
                    ui,
                    &text,
                    egui::TextStyle::Small.resolve(ui.style()),
                    f32::INFINITY,
                );
                (text, width)
            })
        };
        if let Some((text, width)) = end {
            let room = row_room(ui) - width - ui.spacing().item_spacing.x;
            if room > 0.0 {
                ui.add_space(room);
            }
            if !turn.done {
                live_chip(ui);
            } else {
                ui.label(theme::muted(text))
                    .on_hover_text("How long the answer took.");
            }
        }
    });
    // what a bubble holds is measured first, so that it is as wide as its words (up to `width`) and is put where it goes
    let inner = (width - 2.0 * BUBBLE_PADDING).max(60.0);
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    if turn.prompt.is_some() || !turn.attachments.is_empty() {
        let mut wide = 0.0_f32;
        for attachment in &turn.attachments {
            wide = wide.max(wrapped_width(
                ui,
                &format!("📎 {attachment}"),
                small.clone(),
                inner,
            ));
        }
        if let Some(prompt) = &turn.prompt {
            wide = wide.max(wrapped_width(ui, prompt, body.clone(), inner));
        }
        ui.add_space(4.0);
        bubble(
            ui,
            true,
            p.accent_soft,
            Stroke::new(1.0, p.accent.gamma_multiply(0.7)),
            wide,
            |ui| {
                for attachment in &turn.attachments {
                    ui.label(theme::muted(format!("📎 {attachment}")));
                }
                if let Some(prompt) = &turn.prompt {
                    ui.label(egui::RichText::new(prompt).color(p.text));
                }
            },
        );
    }
    let waiting = !turn.done && turn.error.is_none();
    if !turn.response.is_empty() || turn.error.is_some() || waiting {
        let mut wide = if waiting { 34.0 } else { 0.0_f32 };
        if !turn.response.is_empty() {
            wide = wide.max(wrapped_width(ui, &turn.response, body.clone(), inner));
        }
        if let Some(err) = &turn.error {
            wide = wide.max(wrapped_width(
                ui,
                &format!("error: {err}"),
                body.clone(),
                inner,
            ));
        }
        ui.add_space(6.0);
        let failed = turn.error.is_some() && turn.response.is_empty();
        let (fill, stroke) = if failed {
            (
                p.card.lerp_to_gamma(p.bad, 0.12),
                Stroke::new(1.0, p.bad.gamma_multiply(0.45)),
            )
        } else {
            (p.card, Stroke::new(1.0, p.line))
        };
        bubble(ui, false, fill, stroke, wide, |ui| {
            if !turn.response.is_empty() {
                ui.label(egui::RichText::new(&turn.response).color(p.text));
            }
            if let Some(err) = &turn.error {
                ui.label(egui::RichText::new(format!("error: {err}")).color(p.bad));
            }
            if waiting {
                typing_dots(ui);
            }
        });
    }
}

/// How wide `text` is when it is wrapped at `wrap`: the widest of its lines.
fn wrapped_width(ui: &egui::Ui, text: &str, font: egui::FontId, wrap: f32) -> f32 {
    ui.painter()
        .layout(text.to_string(), font, Color32::WHITE, wrap)
        .size()
        .x
}

/// What is left of the row of a wrapped layout from where the next widget goes to its end (`available_width` of a wrapped
/// layout is the width of a whole row, since what does not fit goes to the next).
fn row_room(ui: &egui::Ui) -> f32 {
    (ui.max_rect().right() - ui.cursor().left()).max(0.0)
}

/// How wide `live_chip` is: the word, the dot and the room round them.
fn live_chip_width(ui: &egui::Ui) -> f32 {
    wrapped_width(
        ui,
        "live",
        egui::TextStyle::Small.resolve(ui.style()),
        f32::INFINITY,
    ) + 30.0
}

/// The marker of an exchange whose answer is still being written: a green dot with a ring that breathes round it, and
/// the word. The dot is drawn, not typed: the fonts the window has no `●` at this size, and a missing one is a box.
fn live_chip(ui: &mut egui::Ui) {
    let p = theme::palette();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap("live".to_string(), font, p.ok);
    let size = egui::vec2(live_chip_width(ui), galley.size().y + 6.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    response.on_hover_text("The answer is still being written.");
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.ctx().request_repaint_after(Duration::from_millis(60));
    let breath = ((ui.input(|i| i.time) as f32 * 4.0).sin() + 1.0) / 2.0;
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same((size.y / 2.0) as u8),
        p.card.lerp_to_gamma(p.ok, 0.16),
    );
    let centre = egui::pos2(rect.left() + 11.0, rect.center().y);
    painter.circle_filled(centre, 4.0 + 2.5 * breath, p.ok.gamma_multiply(0.22));
    painter.circle_filled(centre, 3.5, p.ok);
    painter.galley(
        egui::pos2(rect.left() + 21.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.ok,
    );
}

/// The room a bubble leaves between its edge and its words, on each side.
const BUBBLE_PADDING: f32 = 12.0;

/// A rounded box for what one side said, as wide as its words (`content`, which the caller measured) and put at the right
/// of the row for the client's side and at the left for the model's.
fn bubble(
    ui: &mut egui::Ui,
    right: bool,
    fill: Color32,
    stroke: Stroke,
    content: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    // a little more than the words: a width that is exactly theirs wraps the last word on some machines
    let inside = content.ceil() + 2.0;
    let indent = if right {
        (ui.available_width() - inside - 2.0 * BUBBLE_PADDING - 2.0).max(0.0)
    } else {
        0.0
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(indent);
        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::symmetric(BUBBLE_PADDING as i8, 8))
            .show(ui, |ui| {
                ui.set_width(inside);
                ui.with_layout(egui::Layout::top_down(Align::Min), add);
            });
    });
}

/// Three dots that rise and fall one after the other: the answer is being written.
fn typing_dots(ui: &mut egui::Ui) {
    let p = theme::palette();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 16.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let t = ui.input(|i| i.time) as f32;
    for k in 0..3 {
        let phase = ((t * 4.0 - k as f32 * 0.9).sin() + 1.0) / 2.0;
        ui.painter().circle_filled(
            egui::pos2(rect.left() + 5.0 + 12.0 * k as f32, rect.center().y),
            3.0,
            p.muted.gamma_multiply(0.35 + 0.65 * phase),
        );
    }
}

/// What the tab shows when nothing has been asked yet: a dot and three arcs round it, as a radio is drawn.
fn signal_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 48.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let origin = egui::pos2(rect.center().x, rect.bottom() - 6.0);
    let painter = ui.painter();
    painter.circle_filled(origin, 4.5, color);
    for (i, radius) in [14.0_f32, 25.0, 36.0].into_iter().enumerate() {
        let points: Vec<egui::Pos2> = (-5..=5)
            .map(|k| {
                let angle = k as f32 * 0.1745;
                egui::pos2(
                    origin.x + radius * angle.sin(),
                    origin.y - radius * angle.cos(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(3.0, color.gamma_multiply(1.0 - 0.28 * i as f32)),
        ));
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

/// How long ago, in words for a line of small print.
fn ago(since: Duration) -> String {
    match since.as_secs() {
        0..=4 => "just now".to_string(),
        s @ 5..=59 => format!("{s} s ago"),
        s @ 60..=3599 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// `ago` for a row that is a few letters wide.
fn short_ago(since: Duration) -> String {
    match since.as_secs() {
        0..=4 => "now".to_string(),
        s @ 5..=59 => format!("{s} s"),
        s @ 60..=3599 => format!("{} min", s / 60),
        s => format!("{} h", s / 3600),
    }
}

/// How long an answer took.
fn took(span: Duration) -> String {
    if span < Duration::from_millis(1) {
        "under 1 ms".to_string()
    } else if span < Duration::from_secs(1) {
        format!("{} ms", span.as_millis())
    } else if span < Duration::from_secs(60) {
        format!("{:.1} s", span.as_secs_f32())
    } else {
        format!("{} min {} s", span.as_secs() / 60, span.as_secs() % 60)
    }
}

/// What the Copy button puts on the clipboard: for each exchange the client it came from and what it asked of the
/// server, what the client asked, and the answer.
fn transcript<'a>(turns: impl Iterator<Item = (&'a TrafficTurn, Option<usize>)>) -> String {
    let mut text = String::new();
    for (turn, number) in turns {
        if !text.is_empty() {
            text.push('\n');
        }
        match number {
            Some(n) => text.push_str(&format!("[Session {n}] {} {}\n", turn.method, turn.path)),
            None => text.push_str(&format!("{} {}\n", turn.method, turn.path)),
        }
        for attachment in &turn.attachments {
            text.push_str(&format!("(attached: {attachment})\n"));
        }
        if let Some(prompt) = &turn.prompt {
            text.push_str(&format!("> {}\n", prompt.replace('\n', "\n> ")));
        }
        if !turn.response.is_empty() {
            text.push_str(&turn.response);
            text.push('\n');
        }
        if let Some(err) = &turn.error {
            text.push_str(&format!("error: {err}\n"));
        }
    }
    text
}

/// Shortens `s` to at most `max_chars` characters (UTF-8-safe), on one line, replacing
/// the empty case with a placeholder so a session's chat-list row never
/// shows a blank preview line.
fn truncate_preview(s: &str, max_chars: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return "(no prompt)".to_string();
    }
    let mut truncated: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        truncated.push('\u{2026}');
    }
    truncated
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::events::TrafficEvent;
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;
    const WINDOW: egui::Vec2 = egui::vec2(760.0, 560.0);

    /// An application with the live traffic on and the Live tab open.
    fn app_in(dir: &Path) -> App {
        let settings = Settings {
            models_dir_override: Some(dir.join("models").display().to_string()),
            enable_traffic_proxy: true,
            ..Settings::default()
        };
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        app.active_tab = Tab::Live;
        app
    }

    fn frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW)),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.show_live_traffic_tab(ui));
        });
        output.textures_delta.clear();
        output
    }

    /// Every text a frame paints, with where.
    fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => into.push((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                _ => {}
            }
        }
        let mut into = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    /// A few frames, so that what a frame decides for the next (the size of a scroll area) has happened.
    fn settled(app: &mut App, ctx: &egui::Context) -> Vec<(String, egui::Rect)> {
        let mut output = frame(app, ctx, Vec::new());
        for _ in 0..4 {
            output = frame(app, ctx, Vec::new());
        }
        texts(&output)
    }

    fn click(app: &mut App, ctx: &egui::Context, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
            Vec::new(),
        ] {
            frame(app, ctx, events);
        }
    }

    fn at(found: &[(String, egui::Rect)], text: &str) -> egui::Rect {
        found
            .iter()
            .find(|(painted, _)| painted == text)
            .unwrap_or_else(|| {
                panic!(
                    "{text:?} is not painted: {:?}",
                    found.iter().map(|f| &f.0).collect::<Vec<_>>()
                )
            })
            .1
    }

    fn painted(found: &[(String, egui::Rect)], text: &str) -> usize {
        found.iter().filter(|(painted, _)| painted == text).count()
    }

    fn ask(app: &mut App, id: u64, session: &str, prompt: &str) {
        app.apply_traffic_event(TrafficEvent::Request {
            id,
            session: session.to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            prompt: Some(prompt.to_string()),
            attachments: Vec::new(),
        });
    }

    fn answer(app: &mut App, id: u64, text: &str) {
        app.apply_traffic_event(TrafficEvent::ResponseDone {
            id,
            full_text: Some(text.to_string()),
        });
    }

    #[test]
    fn p3_i16_a_tab_with_no_traffic_says_so_and_what_to_do_first() {
        let dir = scratch("i16");
        let mut app = app_in(&dir);
        let ctx = egui::Context::default();
        let found = settled(&mut app, &ctx);
        for text in [
            "Conversations",
            "All",
            "0 messages",
            "Live chat/completion traffic",
            "No traffic yet",
            "Clear all",
        ] {
            at(&found, text);
        }
        // the server is not running, so what to do first is a button, and it goes to the Main tab
        let go = at(&found, "Open the Main tab");
        click(&mut app, &ctx, go.center());
        assert!(
            app.active_tab == Tab::Main,
            "the button did not open the Main tab"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i17_what_a_client_asked_is_a_bubble_on_the_right_over_the_answer_in_one_on_the_left() {
        let dir = scratch("i17");
        let mut app = app_in(&dir);
        let ctx = egui::Context::default();
        ask(&mut app, 1, "10.0.0.1:5000", "Hi");
        answer(&mut app, 1, "Hello! How can I help you today?");
        let found = settled(&mut app, &ctx);
        let (line, asked, said) = (
            at(&found, "POST /v1/chat/completions"),
            at(&found, "Hi"),
            at(&found, "Hello! How can I help you today?"),
        );
        // in the order it happened, and the short question is at the right-hand side where the answer is at the left
        assert!(
            line.bottom() <= asked.top() && asked.bottom() <= said.top(),
            "{line:?} {asked:?} {said:?}"
        );
        assert!(
            asked.right() > WINDOW.x - 60.0 && said.left() < 300.0,
            "the question {asked:?} is not at the right edge and the answer {said:?} at the left"
        );
        // and how long ago, and how long it took, are said with it
        assert!(painted(&found, "just now") == 1, "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i18_the_clients_are_rows_with_a_number_each_and_a_row_picked_shows_only_its_conversation()
    {
        let dir = scratch("i18");
        let mut app = app_in(&dir);
        let ctx = egui::Context::default();
        for (id, session, prompt) in [
            (1, "10.0.0.1:1", "first question"),
            (2, "10.0.0.2:2", "second question"),
            (3, "10.0.0.3:3", "third question"),
        ] {
            ask(&mut app, id, session, prompt);
            answer(&mut app, id, "an answer");
        }
        let found = settled(&mut app, &ctx);
        at(&found, "3 messages");
        for number in 1..=3 {
            // a row and, with everyone's talk together, a tag in front of each exchange
            assert_eq!(
                painted(&found, &format!("Session {number}")),
                2,
                "{found:?}"
            );
        }
        for prompt in ["first question", "second question", "third question"] {
            at(&found, prompt);
        }

        // the row of the second client is at the left; picking it leaves only what it asked
        let row = found
            .iter()
            .filter(|(text, rect)| text == "Session 2" && rect.left() < 240.0)
            .map(|(_, rect)| *rect)
            .next()
            .expect("the row of the second client");
        click(&mut app, &ctx, row.center());
        assert_eq!(app.selected_session.as_deref(), Some("10.0.0.2:2"));
        let found = settled(&mut app, &ctx);
        at(&found, "second question");
        assert_eq!(painted(&found, "first question"), 0, "{found:?}");
        assert_eq!(painted(&found, "third question"), 0, "{found:?}");
        // the heading says whose it is (and the exchange needs no tag of its own: a row and that)
        assert_eq!(painted(&found, "Session 2"), 2, "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i19_clear_all_stays_at_the_foot_of_the_list_however_many_clients_there_are() {
        let dir = scratch("i19");
        let mut app = app_in(&dir);
        let ctx = egui::Context::default();
        for id in 0..30u64 {
            ask(&mut app, id, &format!("10.0.0.{id}:1"), "a question");
            answer(&mut app, id, "an answer");
        }
        let found = settled(&mut app, &ctx);
        // the list of clients is longer than the window, and the button is in the window (it was under the list, out of it)
        let clear = at(&found, "Clear all");
        assert!(
            clear.bottom() > WINDOW.y - 50.0 && clear.bottom() < WINDOW.y,
            "{clear:?} is not at the foot of the window"
        );
        click(&mut app, &ctx, clear.center());
        assert!(app.traffic.is_empty() && app.selected_session.is_none());
        let found = settled(&mut app, &ctx);
        at(&found, "No traffic yet");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i20_an_answer_on_its_way_is_marked_live_and_one_that_is_over_says_how_long_it_took() {
        let dir = scratch("i20");
        let mut app = app_in(&dir);
        let ctx = egui::Context::default();
        ask(&mut app, 1, "10.0.0.1:1", "a slow question");
        app.apply_traffic_event(TrafficEvent::ResponseDelta {
            id: 1,
            text: "Once upon".to_string(),
        });
        let found = settled(&mut app, &ctx);
        at(&found, "live");
        at(&found, "Once upon");
        assert!(
            !found
                .iter()
                .any(|(text, _)| text.ends_with("ms") || text.ends_with(" s")),
            "a time before the answer is over: {found:?}"
        );

        answer(&mut app, 1, "Once upon a time.");
        let found = settled(&mut app, &ctx);
        assert_eq!(painted(&found, "live"), 0, "{found:?}");
        at(&found, "Once upon a time.");
        assert!(
            found
                .iter()
                .any(|(text, _)| text.ends_with(" ms") || text.ends_with(" s")),
            "no time for the answer: {found:?}"
        );

        // an answer that failed says so in red, in the place of the answer
        ask(&mut app, 2, "10.0.0.1:1", "a question that fails");
        app.apply_traffic_event(TrafficEvent::ResponseError {
            id: 2,
            message: "connection reset".to_string(),
        });
        let found = settled(&mut app, &ctx);
        at(&found, "error: connection reset");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One channel of a colour in linear light, as WCAG says it, and how well one colour reads on another.
    fn contrast(a: Color32, b: Color32) -> f64 {
        let luminance = |c: Color32| {
            let linear = |channel: u8| {
                let c = f64::from(channel) / 255.0;
                if c <= 0.03928 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn p3_i21_the_number_of_a_client_reads_on_the_disc_it_is_drawn_on_in_both_themes() {
        for dark in [true, false] {
            let p = theme::palette_of(dark);
            let colours: Vec<Color32> = (0..6).map(|i| client_color(i, dark)).collect();
            for (i, one) in colours.iter().enumerate() {
                // the disc is on the card, on the row that is picked and, as a tag, on the page
                for (base, what) in [
                    (p.card, "a row"),
                    (p.accent_soft, "the row that is picked"),
                    (p.page, "the tag of an exchange"),
                ] {
                    let ratio = contrast(
                        client_ink(*one, dark),
                        base.lerp_to_gamma(*one, AVATAR_TINT),
                    );
                    assert!(
                        ratio >= 4.5,
                        "client {i} in the theme with dark={dark}, on {what}: {ratio:.2} to 1"
                    );
                }
                for other in &colours[i + 1..] {
                    assert_ne!(one, other, "two clients have the same colour");
                }
            }
            // the seventh client is the first one's colour again, not a panic
            assert_eq!(client_color(6, dark), colours[0]);
        }
    }

    #[test]
    fn p3_i22_the_times_of_the_tab_are_said_in_words_a_row_has_room_for() {
        let seconds = Duration::from_secs;
        assert_eq!(ago(seconds(0)), "just now");
        assert_eq!(ago(seconds(4)), "just now");
        assert_eq!(ago(seconds(5)), "5 s ago");
        assert_eq!(ago(seconds(59)), "59 s ago");
        assert_eq!(ago(seconds(60)), "1 min ago");
        assert_eq!(ago(seconds(3599)), "59 min ago");
        assert_eq!(ago(seconds(7300)), "2 h ago");
        assert_eq!(short_ago(seconds(2)), "now");
        assert_eq!(short_ago(seconds(30)), "30 s");
        assert_eq!(short_ago(seconds(125)), "2 min");
        assert_eq!(short_ago(seconds(3 * 3600)), "3 h");
        assert_eq!(took(Duration::from_micros(300)), "under 1 ms");
        assert_eq!(took(Duration::from_millis(303)), "303 ms");
        assert_eq!(took(Duration::from_millis(4500)), "4.5 s");
        assert_eq!(took(seconds(135)), "2 min 15 s");
        // a row's last words are the prompt on one line, cut where it is long, and never blank
        assert_eq!(truncate_preview("", 10), "(no prompt)");
        assert_eq!(truncate_preview(" \n ", 10), "(no prompt)");
        assert_eq!(truncate_preview("a\nb   c", 10), "a b c");
        assert_eq!(truncate_preview("ünïcödé text here", 7), "ünïcödé\u{2026}");
    }

    #[test]
    fn p3_i23_what_copy_puts_on_the_clipboard_is_the_conversation_as_plain_text() {
        let turn = |prompt: Option<&str>, response: &str, error: Option<&str>| TrafficTurn {
            id: 1,
            session: "10.0.0.1:1".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            prompt: prompt.map(str::to_string),
            attachments: vec!["image: cat.png".to_string()],
            response: response.to_string(),
            done: true,
            error: error.map(str::to_string),
            started: Instant::now(),
            ended: None,
        };
        let one = turn(Some("line one\nline two"), "the answer", None);
        let two = turn(None, "", Some("it broke"));
        let text = transcript([(&one, Some(2)), (&two, None)].into_iter());
        assert_eq!(
            text,
            "[Session 2] POST /v1/chat/completions\n\
             (attached: image: cat.png)\n\
             > line one\n> line two\n\
             the answer\n\
             \n\
             POST /v1/chat/completions\n\
             (attached: image: cat.png)\n\
             error: it broke\n"
        );
        assert_eq!(transcript(std::iter::empty()), "");
    }
}
