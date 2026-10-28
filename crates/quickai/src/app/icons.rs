//! The small pictures of the window's buttons (the theme, the guides, Settings, a model's menu, the box that adds a
//! model, the chat's), drawn as lines in the colour of the text so that they are sharp at any scale and in both
//! themes, and need no font of pictures. Each is drawn in a square of 24 units, as the usual icon sets are.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

use super::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Icon {
    Sun,
    Moon,
    Help,
    Gear,
    Chevron,
    Plus,
    Folder,
    Search,
    Download,
    More,
    Send,
    Clip,
    Close,
    Check,
    Computer,
    Sliders,
    Chat,
    Refresh,
    Plug,
    Globe,
    Stop,
    Play,
    Copy,
    Pencil,
}

/// Draws `icon` in `rect` (it is drawn in the largest square that fits, in the middle) in `color`.
pub(super) fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let unit = side / 24.0;
    let origin = rect.center() - egui::vec2(side, side) / 2.0;
    let at = |x: f32, y: f32| Pos2::new(origin.x + x * unit, origin.y + y * unit);
    let stroke = Stroke::new((1.7 * unit).max(1.2), color);
    let line = |points: &[(f32, f32)]| {
        painter.add(Shape::line(
            points.iter().map(|(x, y)| at(*x, *y)).collect(),
            stroke,
        ));
    };
    let ring = |x: f32, y: f32, r: f32| {
        painter.circle_stroke(at(x, y), r * unit, stroke);
    };
    let dot = |x: f32, y: f32, r: f32| {
        painter.circle_filled(at(x, y), r * unit, color);
    };
    match icon {
        Icon::Sun => {
            ring(12.0, 12.0, 4.0);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (s, c) = a.sin_cos();
                line(&[
                    (12.0 + 7.0 * c, 12.0 + 7.0 * s),
                    (12.0 + 9.5 * c, 12.0 + 9.5 * s),
                ]);
            }
        }
        Icon::Moon => {
            // a crescent: most of a circle, and the arc of the circle that bites it
            let mut points = Vec::new();
            for i in 0..=26 {
                let a = (5.0 + i as f32 * 10.0).to_radians();
                points.push((12.0 + 9.0 * a.cos(), 12.0 + 9.0 * a.sin()));
            }
            for i in 0..=16 {
                let a = (216.3 - i as f32 * (162.6 / 16.0)).to_radians();
                points.push((16.85 + 7.0 * a.cos(), 7.15 + 7.0 * a.sin()));
            }
            line(&points);
        }
        Icon::Help => {
            ring(12.0, 12.0, 9.0);
            let mut points = Vec::new();
            for i in 0..=10 {
                let a = (200.0 + i as f32 * 21.0).to_radians();
                points.push((12.0 + 2.6 * a.cos(), 9.6 + 2.6 * a.sin()));
            }
            points.push((12.0, 13.6));
            points.push((12.0, 14.2));
            line(&points);
            dot(12.0, 17.0, 1.1);
        }
        Icon::Gear => {
            ring(12.0, 12.0, 3.0);
            // the body of the wheel, and its eight teeth
            ring(12.0, 12.0, 6.6);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (s, c) = a.sin_cos();
                painter.add(Shape::line_segment(
                    [
                        at(12.0 + 6.6 * c, 12.0 + 6.6 * s),
                        at(12.0 + 9.4 * c, 12.0 + 9.4 * s),
                    ],
                    Stroke::new(stroke.width * 1.9, color),
                ));
            }
        }
        Icon::Chevron => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        Icon::Plus => {
            line(&[(12.0, 5.0), (12.0, 19.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::Folder => line(&[
            (3.0, 18.0),
            (3.0, 6.0),
            (9.5, 6.0),
            (11.5, 8.0),
            (21.0, 8.0),
            (21.0, 18.0),
            (3.0, 18.0),
        ]),
        Icon::Search => {
            ring(10.5, 10.5, 6.5);
            line(&[(15.5, 15.5), (20.0, 20.0)]);
        }
        Icon::Download => {
            line(&[(12.0, 4.0), (12.0, 15.0)]);
            line(&[(7.0, 10.5), (12.0, 15.5), (17.0, 10.5)]);
            line(&[(5.0, 20.0), (19.0, 20.0)]);
        }
        Icon::More => {
            for x in [5.0, 12.0, 19.0] {
                dot(x, 12.0, 1.7);
            }
        }
        Icon::Send => {
            line(&[(5.0, 12.0), (18.0, 12.0)]);
            line(&[(12.5, 6.0), (18.5, 12.0), (12.5, 18.0)]);
        }
        Icon::Clip => {
            // a paper clip: two long sides joined by turns
            let mut points = vec![(15.5, 7.0), (15.5, 16.0)];
            for i in 0..=8 {
                let a = (i as f32 * 22.5).to_radians();
                points.push((12.0 + 3.5 * a.cos(), 16.0 + 3.5 * a.sin()));
            }
            points.push((8.5, 6.0));
            for i in 0..=8 {
                let a = (180.0 + i as f32 * 22.5).to_radians();
                points.push((11.0 + 2.5 * a.cos(), 6.0 + 2.5 * a.sin()));
            }
            points.push((13.5, 15.0));
            line(&points);
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Check => line(&[(5.0, 12.5), (9.5, 17.0), (19.0, 7.5)]),
        Icon::Computer => {
            line(&[
                (3.0, 5.0),
                (21.0, 5.0),
                (21.0, 16.0),
                (3.0, 16.0),
                (3.0, 5.0),
            ]);
            line(&[(8.5, 20.0), (15.5, 20.0)]);
            line(&[(12.0, 16.0), (12.0, 20.0)]);
        }
        Icon::Sliders => {
            line(&[(4.0, 8.0), (13.0, 8.0)]);
            line(&[(19.0, 8.0), (20.0, 8.0)]);
            ring(16.0, 8.0, 2.4);
            line(&[(4.0, 16.0), (6.5, 16.0)]);
            line(&[(12.5, 16.0), (20.0, 16.0)]);
            ring(9.5, 16.0, 2.4);
        }
        Icon::Chat => line(&[
            (4.0, 5.0),
            (20.0, 5.0),
            (20.0, 16.0),
            (10.0, 16.0),
            (5.5, 20.0),
            (5.5, 16.0),
            (4.0, 16.0),
            (4.0, 5.0),
        ]),
        Icon::Refresh => {
            let mut points = Vec::new();
            for i in 0..=20 {
                let a = (-60.0 + i as f32 * 15.0).to_radians();
                points.push((12.0 + 7.5 * a.cos(), 12.0 + 7.5 * a.sin()));
            }
            line(&points);
            let end = (-60.0f32).to_radians();
            let (ex, ey) = (12.0 + 7.5 * end.cos(), 12.0 + 7.5 * end.sin());
            line(&[(ex - 4.5, ey - 0.6), (ex, ey), (ex - 0.4, ey + 4.6)]);
        }
        Icon::Plug => {
            // two prongs, the body of the plug, and its cord
            line(&[(9.0, 2.5), (9.0, 7.5)]);
            line(&[(15.0, 2.5), (15.0, 7.5)]);
            let mut body = vec![(6.0, 7.5), (18.0, 7.5), (18.0, 12.0)];
            for i in 0..=8 {
                let a = (i as f32 * 22.5).to_radians();
                body.push((12.0 + 6.0 * a.cos(), 12.0 + 5.0 * a.sin()));
            }
            body.push((6.0, 7.5));
            line(&body);
            line(&[(12.0, 17.0), (12.0, 21.5)]);
        }
        Icon::Globe => {
            ring(12.0, 12.0, 9.0);
            line(&[(3.0, 12.0), (21.0, 12.0)]);
            // a meridian: an ellipse as tall as the globe
            let points: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let a = (i as f32 * 15.0).to_radians();
                    (12.0 + 4.0 * a.cos(), 12.0 + 9.0 * a.sin())
                })
                .collect();
            line(&points);
        }
        Icon::Pencil => {
            // a pencil slanting up to the right, its point at the bottom left, and the line where its tip begins
            line(&[
                (16.5, 4.0),
                (20.0, 7.5),
                (8.0, 19.5),
                (4.0, 20.0),
                (4.5, 16.0),
                (16.5, 4.0),
            ]);
            line(&[(14.0, 6.5), (17.5, 10.0)]);
        }
        Icon::Copy => {
            // two sheets, the front one whole
            line(&[
                (8.5, 8.5),
                (19.5, 8.5),
                (19.5, 19.5),
                (8.5, 19.5),
                (8.5, 8.5),
            ]);
            line(&[
                (15.5, 8.5),
                (15.5, 4.5),
                (4.5, 4.5),
                (4.5, 15.5),
                (8.5, 15.5),
            ]);
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![at(8.0, 5.0), at(19.5, 12.0), at(8.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_min_max(at(7.0, 7.0), at(17.0, 17.0)),
                1.8 * unit,
                color,
            );
        }
    }
}

/// A button that is a picture and nothing else: no frame until the pointer is on it, `hover` said when it rests
/// there. `on` draws it pressed (its popup is open).
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: egui::Vec2,
    enabled: bool,
    on: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let p = theme::palette();
        let fill = if on || response.is_pointer_button_down_on() {
            p.raised_active
        } else if response.hovered() && enabled {
            p.raised_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 5.0, fill);
        let color = if !enabled {
            p.faint
        } else if response.hovered() || on {
            p.text
        } else {
            p.body
        };
        let side = (size.y - 10.0).clamp(12.0, 18.0);
        let square = Rect::from_center_size(rect.center(), egui::vec2(side, side));
        paint(ui.painter(), square, icon, color);
    }
    response
}

/// A button with a picture before its words, drawn as the window's buttons are (`role`), the least `height` high.
pub(super) fn labelled_button(
    ui: &mut egui::Ui,
    role: theme::Role,
    enabled: bool,
    icon: Icon,
    text: &str,
    height: f32,
) -> egui::Response {
    let icon_side = 14.0;
    let gap = 6.0;
    let pad = 11.0;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, Color32::PLACEHOLDER);
    let size = egui::vec2(pad * 2.0 + icon_side + gap + galley.size().x, height);
    // the button is drawn by egui (so that its colours are the role's, hovered and pressed), with room left for the
    // picture, which is drawn over it
    let response = theme::button_sized(ui, role, enabled, "", size);
    if ui.is_rect_visible(response.rect) {
        let rect = response.rect;
        let visuals = ui.style().interact(&response);
        let color = text_color(role, enabled, visuals.fg_stroke.color);
        let left = rect.left() + (rect.width() - (icon_side + gap + galley.size().x)) / 2.0;
        let square = Rect::from_min_size(
            Pos2::new(left, rect.center().y - icon_side / 2.0),
            egui::vec2(icon_side, icon_side),
        );
        paint(ui.painter(), square, icon, color);
        ui.painter().galley(
            Pos2::new(
                square.right() + gap,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response
}

/// A button that is a picture in a frame, drawn as the window's buttons are (`role`), `size` big: for a header whose
/// actions are known by their pictures, each named by the text that is shown when the pointer rests on it.
pub(super) fn framed_icon_button(
    ui: &mut egui::Ui,
    role: theme::Role,
    enabled: bool,
    icon: Icon,
    size: egui::Vec2,
) -> egui::Response {
    let response = theme::button_sized(ui, role, enabled, "", size);
    if ui.is_rect_visible(response.rect) {
        let visuals = ui.style().interact(&response);
        let color = text_color(role, enabled, visuals.fg_stroke.color);
        let side = (size.y - 13.0).clamp(12.0, 18.0);
        let square = Rect::from_center_size(response.rect.center(), egui::vec2(side, side));
        paint(ui.painter(), square, icon, color);
    }
    response
}

/// A copy button: a picture of two sheets, a tick for a moment once it has copied `text`. `salt` tells it from the
/// other copy buttons of the window.
pub(super) fn copy_button(ui: &mut egui::Ui, salt: &str, text: &str) {
    let id = egui::Id::new(("copied", salt));
    let now = ui.input(|i| i.time);
    let copied_at: Option<f64> = ui.data(|d| d.get_temp(id));
    let just = copied_at.is_some_and(|at| now - at < 1.5);
    let icon = if just { Icon::Check } else { Icon::Copy };
    let response = framed_icon_button(ui, theme::Role::Normal, true, icon, egui::vec2(28.0, 24.0))
        .on_hover_text(if just { "Copied" } else { "Copy" });
    if response.clicked() {
        ui.ctx().copy_text(text.to_string());
        ui.data_mut(|d| d.insert_temp(id, now));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1600));
    } else if just {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
    }
}

/// The colour of the words of a button of `role`: what egui would draw them in, but the role's own for the roles that
/// have one.
fn text_color(role: theme::Role, enabled: bool, drawn: Color32) -> Color32 {
    let p = theme::palette();
    if !enabled {
        return p.faint;
    }
    match role {
        theme::Role::Primary => p.on_accent,
        theme::Role::Danger => p.bad,
        theme::Role::Quiet => p.muted,
        theme::Role::Normal => drawn,
    }
}
