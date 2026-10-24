//! The title row of the main window (panel `header`), and what is under it before the page of a tab.

use eframe::egui;

use super::theme::{self, Role};
use super::{App, Tab};

/// The height of the title row's content; the band behind it is a little more.
const ROW_HEIGHT: f32 = 28.0;
/// The size the logo is drawn at, and the one it is made at (three times as many pixels, for screens
/// that have more than one to a point).
const LOGO_POINTS: f32 = 24.0;
const LOGO_PIXELS: usize = 72;

impl App {
    /// What the ⚙ icon does: opens the settings, or closes them. Closing them by the icon is closing them, as
    /// the window's own close button and their Close button do, so what was changed in them is written then (it
    /// was not: a Python or an architecture typed in Settings was lost to anyone who closed the window with the
    /// icon and then quit).
    pub(super) fn toggle_settings(&mut self) {
        if self.show_settings {
            self.close_settings();
        } else {
            self.show_settings = true;
        }
    }

    /// The top of the window: the title row, and under it the Live tab when that is the one open, or on the
    /// Main tab the section of the configuration. Returns whether the rest of the Main page follows.
    pub(super) fn show_page_top(&mut self, ui: &mut egui::Ui) -> bool {
        self.show_header(ui);
        if self.active_tab == Tab::Live {
            self.show_live_traffic_tab(ui);
            return false;
        }
        theme::column(ui, theme::PAGE_WIDTH, |ui| {
            self.show_configuration_section(ui);
        });
        ui.add_space(8.0);
        true
    }

    /// The logo, drawn from the application's icon: made once, in the size it is shown at.
    fn show_logo(&mut self, ui: &mut egui::Ui) {
        let texture = self.logo.get_or_insert_with(|| {
            let icon = crate::app_icon();
            let pixels = shrink(
                &icon.rgba,
                icon.width as usize,
                icon.height as usize,
                LOGO_PIXELS,
            );
            ui.ctx().load_texture(
                "quickai-logo",
                egui::ColorImage::from_rgba_unmultiplied([LOGO_PIXELS, LOGO_PIXELS], &pixels),
                egui::TextureOptions::LINEAR,
            )
        });
        ui.add(egui::Image::new(&*texture).fit_to_exact_size(egui::Vec2::splat(LOGO_POINTS)));
    }

    /// The title row: the logo and the heading, the Main/Live tab strip (only with live traffic on), and on the
    /// right the state of the server and the About, settings and theme icons, on a band across the window.
    pub(super) fn show_header(&mut self, ui: &mut egui::Ui) {
        // llama.cpp's settings are described as they always were; another engine's by its own name
        let settings_hover = if self.engine == crate::settings::DEFAULT_ENGINE {
            "App and llama.cpp settings — downloaded-models folder, \
             custom llama-server binary, extra command-line arguments"
                .to_string()
        } else {
            format!(
                "App and {} settings — the engine, where {} is found and its options",
                self.engine_name(),
                self.server_name()
            )
        };
        let state = self.session.state();
        // the band is drawn behind the row, when the row's height is known
        let band = ui.painter().add(egui::Shape::Noop);
        let row = ui.horizontal(|ui| {
            ui.set_min_height(ROW_HEIGHT);
            self.show_logo(ui);
            ui.add_space(2.0);
            ui.heading("QuickAI");

            if self.traffic_proxy_on() {
                ui.add_space(12.0);
                ui.selectable_value(&mut self.active_tab, Tab::Main, "Main");
                ui.selectable_value(&mut self.active_tab, Tab::Live, "📡 Live")
                    .on_hover_text(
                        "Watch chat/completion requests and replies in real time as any \
                         client talks to this model.",
                    );
            } else if self.active_tab != Tab::Main {
                self.active_tab = Tab::Main;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (theme_icon, theme_hover) = if self.dark_mode {
                    ("☀", "Switch to light theme")
                } else {
                    ("🌙", "Switch to dark theme")
                };
                if icon(ui, theme_icon).on_hover_text(theme_hover).clicked() {
                    self.dark_mode = !self.dark_mode;
                    theme::apply(ui.ctx(), self.dark_mode);
                    self.save_settings();
                }
                if icon(ui, "⚙").on_hover_text(settings_hover).clicked() {
                    self.toggle_settings();
                }
                if icon(ui, "ℹ")
                    .on_hover_text("About QuickAI: usage, disclaimer, and license")
                    .clicked()
                {
                    self.show_about = !self.show_about;
                }
                ui.add_space(8.0);
                theme::status_pill(ui, state, state.label()).on_hover_text(
                    "What the server is doing now. The buttons under the settings start and stop it.",
                );
            });
        });

        let screen = ui.ctx().content_rect();
        let bottom = row.response.rect.bottom() + 6.0;
        let p = theme::palette();
        ui.ctx().layer_painter(ui.layer_id()).set(
            band,
            egui::Shape::Vec(vec![
                egui::epaint::RectShape::filled(
                    egui::Rect::from_min_max(screen.min, egui::pos2(screen.right(), bottom)),
                    0,
                    p.bar,
                )
                .into(),
                egui::Shape::hline(screen.x_range(), bottom, egui::Stroke::new(1.0, p.line)),
            ]),
        );
        ui.add_space(10.0);
    }
}

/// An icon of the title row: no frame until the pointer is on it.
fn icon(ui: &mut egui::Ui, glyph: &str) -> egui::Response {
    theme::button_sized(
        ui,
        Role::Quiet,
        true,
        egui::RichText::new(glyph).size(16.0),
        egui::vec2(30.0, 26.0),
    )
}

/// `rgba`, a `width` by `height` picture, made `size` by `size` pixels by taking the mean of the pixels
/// that fall in each of the new ones (the colours weighed by how opaque they are, so that the edge of the
/// picture does not get a dark rim).
fn shrink(rgba: &[u8], width: usize, height: usize, size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        let y0 = y * height / size;
        let y1 = ((y + 1) * height / size).max(y0 + 1).min(height);
        for x in 0..size {
            let x0 = x * width / size;
            let x1 = ((x + 1) * width / size).max(x0 + 1).min(width);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * width + sx) * 4;
                    let alpha = u64::from(rgba[i + 3]);
                    r += u64::from(rgba[i]) * alpha;
                    g += u64::from(rgba[i + 1]) * alpha;
                    b += u64::from(rgba[i + 2]) * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            let o = (y * size + x) * 4;
            if let (Some(red), Some(green), Some(blue)) =
                (r.checked_div(a), g.checked_div(a), b.checked_div(a))
            {
                out[o] = red as u8;
                out[o + 1] = green as u8;
                out[o + 2] = blue as u8;
            }
            out[o + 3] = (a / n.max(1)) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::events::ServerState;
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// An application of an engine, with models in `dir/models` and nothing bundled.
    fn app_in(dir: &Path, engine: &str) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
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
        app.find_active_configuration();
        app.bundled_models_dir = dir.join("bundled");
        app
    }

    /// A model file of this many bytes (a hole: it costs the disk nothing) as the only one.
    fn only_model(app: &mut App, dir: &Path, name: &str, bytes: u64) {
        let folder = dir.join("models");
        std::fs::create_dir_all(&folder).unwrap();
        for old in std::fs::read_dir(&folder).unwrap() {
            std::fs::remove_file(old.unwrap().path()).unwrap();
        }
        std::fs::File::create(folder.join(name))
            .unwrap()
            .set_len(bytes)
            .unwrap();
        app.models = app.scan_models();
        app.selected = 0;
    }

    /// One frame of the page: the title row and the section of the configuration, and with `whole` the card of the
    /// model, the run options and the buttons under them, in a window of the width of a first start.
    fn frame(app: &mut App, ctx: &egui::Context, whole: bool) -> egui::FullOutput {
        frame_with(app, ctx, whole, Vec::new())
    }

    /// `frame`, with what the pointer and the keyboard did since the one before.
    fn frame_with(
        app: &mut App,
        ctx: &egui::Context,
        whole: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 560.0),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if app.show_page_top(ui) && whole {
                    app.show_models_area(ui, false);
                    app.show_controls(ui);
                    app.show_run_bar(ui, ServerState::Idle, false);
                }
            });
        });
        output.textures_delta.clear();
        output
    }

    fn settled(app: &mut App, ctx: &egui::Context, whole: bool) -> egui::FullOutput {
        let mut output = frame(app, ctx, whole);
        for _ in 0..3 {
            output = frame(app, ctx, whole);
        }
        output
    }

    /// The person clicks `at`: the pointer goes there, presses and lets go, a frame each.
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
            frame_with(app, ctx, true, events);
        }
    }

    /// The rectangle each text of a frame is drawn in, by text.
    fn rects(output: &egui::FullOutput) -> BTreeMap<String, egui::Rect> {
        fn walk(shape: &egui::epaint::Shape, into: &mut BTreeMap<String, egui::Rect>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => {
                    into.insert(
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    );
                }
                _ => {}
            }
        }
        let mut into = BTreeMap::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    /// The rectangles of a frame that are filled with this colour.
    fn filled_with(output: &egui::FullOutput, color: egui::Color32) -> Vec<egui::Rect> {
        fn walk(shape: &egui::epaint::Shape, color: egui::Color32, into: &mut Vec<egui::Rect>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, color, into)),
                egui::epaint::Shape::Rect(r) if r.fill == color => into.push(r.rect),
                _ => {}
            }
        }
        let mut into = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, color, &mut into);
        }
        into
    }

    #[test]
    fn p3_i10_the_title_row_says_what_the_server_is_doing_before_its_icons_on_both_tabs() {
        let dir = scratch("z10");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        for live in [false, true] {
            app.enable_traffic_proxy = live;
            app.active_tab = if live { Tab::Live } else { Tab::Main };
            let found = rects(&settled(&mut app, &ctx, false));
            let at = |text: &str| {
                *found
                    .get(text)
                    .unwrap_or_else(|| panic!("{text} on the {live} tab: {:?}", found.keys()))
            };
            let (state, about, gear) = (at("Stopped"), at("ℹ"), at("⚙"));
            assert!(
                state.center().y < 40.0,
                "{live}: {state:?} is not in the title row"
            );
            assert!(state.right() < about.left(), "{live}: {state:?} {about:?}");
            assert!(about.right() <= gear.left(), "{live}: {about:?} {gear:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i11_the_speed_of_the_model_is_a_chip_in_its_card_and_not_an_icon_in_the_title_row() {
        let dir = scratch("z11");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        // a model a sixteenth of the memory is fast; one that is most of it is slower
        for (name, bytes, said) in [
            ("Small-Q4_K_M.gguf", GB, "Fast on this computer"),
            ("Large-Q4_K_M.gguf", 12 * GB, "Slower on this computer"),
        ] {
            only_model(&mut app, &dir, name, bytes);
            let found = rects(&settled(&mut app, &ctx, true));
            let chip = found
                .iter()
                .find(|(text, _)| text.ends_with(said))
                .unwrap_or_else(|| panic!("{said}: {:?}", found.keys()))
                .1;
            let label = found.get("Model:").expect("the picker's label");
            // in the card of the model, on the row of the picker and after the picker, not in the title row
            assert!(
                chip.center().y > 100.0,
                "{name}: {chip:?} is not under the title row"
            );
            assert!(
                (chip.center().y - label.center().y).abs() < 8.0,
                "{name}: {chip:?} is not on the row of {label:?}"
            );
            assert!(chip.left() > label.right() + 320.0, "{name}: {chip:?}");
        }
        // an engine with no picker has no model to say it of
        let mut comfy = app_in(&dir, "comfyui");
        let found = rects(&settled(&mut comfy, &ctx, true));
        assert!(
            !found.keys().any(|text| text.contains("on this computer")),
            "{:?}",
            found.keys()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i13_the_logo_is_made_smaller_without_a_dark_rim_at_the_edge_of_the_picture() {
        // a red pixel with a transparent one beside it is red and half as opaque, not a darker red
        assert_eq!(
            shrink(&[255, 0, 0, 255, 0, 0, 0, 0], 2, 1, 1),
            vec![255, 0, 0, 127]
        );
        // a picture of one colour stays that colour, and the size is the one asked for
        let blue: Vec<u8> = std::iter::repeat_n([10u8, 20, 200, 255], 16 * 16)
            .flatten()
            .collect();
        let small = shrink(&blue, 16, 16, 4);
        assert_eq!(small.len(), 4 * 4 * 4);
        assert!(small.chunks(4).all(|pixel| pixel == [10, 20, 200, 255]));
        // nothing at all stays nothing
        assert!(shrink(&[0u8; 8 * 8 * 4], 8, 8, 2)
            .iter()
            .all(|byte| *byte == 0));
    }

    #[test]
    fn p3_i12_while_the_server_is_stopped_the_one_green_button_is_start() {
        let dir = scratch("z12");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        only_model(&mut app, &dir, "Small-Q4_K_M.gguf", GB);
        let output = settled(&mut app, &ctx, true);
        let found = rects(&output);
        let green = filled_with(&output, theme::palette_of(true).accent);
        let start = found.get("Start").expect("a Start button");
        // dark is what a context starts in; the colour of the button is the accent of the theme drawn now
        let green: Vec<egui::Rect> = if green.is_empty() {
            filled_with(&output, theme::palette_of(false).accent)
        } else {
            green
        };
        assert_eq!(green.len(), 1, "{green:?}");
        assert!(
            green[0].contains(start.center()),
            "{:?} {start:?}",
            green[0]
        );
        // and the buttons are in their order, Start before Stop before the chat before the API
        let order: Vec<f32> = ["Start", "Stop", "Open Chat UI", "🔌 API"]
            .iter()
            .map(|text| found.get(*text).unwrap_or_else(|| panic!("{text}")).left())
            .collect();
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i14_a_download_that_failed_says_so_under_the_box_it_was_asked_for_in_until_it_is_dismissed(
    ) {
        let dir = scratch("z14");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let said = |found: &BTreeMap<String, egui::Rect>| {
            found.keys().any(|text| text.starts_with("Download failed"))
        };
        assert!(!said(&rects(&settled(&mut app, &ctx, true))));

        // the log has it too, and the results of a look-up, which have the page, hide the log: it is in the card
        app.catalog.note =
            Some("Download failed: tango-Q4_K_M.gguf does not match the checksum".into());
        let found = rects(&settled(&mut app, &ctx, true));
        let (_, note) = found
            .iter()
            .find(|(text, _)| text.starts_with("Download failed"))
            .unwrap_or_else(|| panic!("the failure in the window: {:?}", found.keys()));
        let get = found.get("Get:").expect("the box it was asked for in");
        assert!(note.top() > get.bottom(), "{note:?} is not under {get:?}");

        // Dismiss takes it away
        let dismiss = found.get("Dismiss").expect("a button to dismiss it");
        click(&mut app, &ctx, dismiss.center());
        assert_eq!(app.catalog.note, None);
        assert!(!said(&rects(&settled(&mut app, &ctx, true))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A model of the picker, as an engine that lists its models, or the folder of the bundled ones, gave it.
    fn listed(name: &str, note: &str) -> crate::models::ModelEntry {
        crate::models::ModelEntry {
            filename: name.to_string(),
            path: std::path::PathBuf::new(),
            display: name.to_string(),
            note: note.to_string(),
            bundled: false,
            size_bytes: 6 * GB,
            companions: Vec::new(),
            mode: None,
        }
    }

    #[test]
    fn p3_i24_what_is_said_of_a_listed_model_is_chips_on_one_row_and_not_lines_of_small_print() {
        let dir = scratch("z24");
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        app.models = vec![listed("qwen3-vl:8b", "completion, vision, tools, thinking")];
        app.selected = 0;
        app.recommended_filename = Some("qwen3-vl:8b".to_string());
        app.named.stale = true;
        let found = rects(&settled(&mut app, &ctx, true));
        let picker = *found.get("Model:").expect("the picker's label");
        let get = *found.get("Get:").expect("the box that gets another model");
        let chips: Vec<egui::Rect> = [
            "vision",
            "tools",
            "thinking",
            "★ Recommended",
            "from the last run",
        ]
        .iter()
        .map(|text| {
            *found
                .get(*text)
                .unwrap_or_else(|| panic!("{text}: {:?}", found.keys()))
        })
        .collect();
        // all on one row, under the picker's and over the box that gets another model
        for chip in &chips {
            assert!(
                chip.top() > picker.bottom() && chip.bottom() < get.top(),
                "{chip:?} is not between {picker:?} and {get:?}"
            );
            assert!(
                (chip.center().y - chips[0].center().y).abs() < 3.0,
                "{chip:?} is not on the row of {:?}",
                chips[0]
            );
        }
        // the three lines they replace are gone, and so is `completion`, which every model that chats has
        for text in found.keys() {
            assert!(
                !text.contains("RAM detected")
                    && !text.starts_with("(as ")
                    && !text.contains("completion"),
                "{text:?} is still there"
            );
        }
        // the row is the height of its chips: the box under it is not further down than it was under one line of print
        assert!(
            get.top() - picker.bottom() < 60.0,
            "{get:?} is far below {picker:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i25_a_note_is_cut_at_its_first_sentence_and_the_rest_is_there_when_the_pointer_is() {
        use super::super::model_bar::first_sentence;
        assert_eq!(first_sentence("A, b. C d."), "A, b.");
        assert_eq!(first_sentence("One sentence."), "One sentence.");
        assert_eq!(first_sentence("No stop at all"), "No stop at all");
        assert_eq!(first_sentence(""), "");
        let (note, mode) =
            super::super::model_bar::split_mode("A note. Starts as an embedding server.");
        assert_eq!(
            (note, mode.map(|m| m.text)),
            ("A note.", Some("embedding server".to_string()))
        );
        let (note, mode) = super::super::model_bar::split_mode("Only a note.");
        assert_eq!((note, mode.is_none()), ("Only a note.", true));

        let dir = scratch("z25");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        app.models = vec![listed(
            "Bonsai-1.7B-Q1_0.gguf",
            "1-bit quantized, ~0.25 GB, Apache-2.0. Bundled with this package \u{2014} the lighter pick if the 8B runs slowly on this machine.",
        )];
        app.selected = 0;
        let found = rects(&settled(&mut app, &ctx, true));
        assert!(
            found.contains_key("1-bit quantized, ~0.25 GB, Apache-2.0."),
            "{:?}",
            found.keys()
        );
        assert!(
            !found
                .keys()
                .any(|text| text.contains("Bundled with this package")),
            "{:?}",
            found.keys()
        );
        // a note that is one sentence is all there
        app.models[0].note = "Instruction-tuned chat model, small and fast.".to_string();
        let found = rects(&settled(&mut app, &ctx, true));
        assert!(found.contains_key("Instruction-tuned chat model, small and fast."));
        // how a model is started is a chip, whatever else its note says: it is not left for the hover
        app.models[0].note =
            "Turns text into numbers for the tests. Starts as an embedding server.".to_string();
        let found = rects(&settled(&mut app, &ctx, true));
        assert!(
            found.contains_key("embedding server")
                && found.contains_key("Turns text into numbers for the tests."),
            "{:?}",
            found.keys()
        );
        assert!(
            !found.keys().any(|text| text.contains("Starts as")),
            "{:?}",
            found.keys()
        );
        app.models[0].note =
            "It is meant to start in the mode `rerank`, which this engine does not have."
                .to_string();
        let found = rects(&settled(&mut app, &ctx, true));
        assert!(
            found.contains_key("mode this engine lacks"),
            "{:?}",
            found.keys()
        );
        // and a model with none has no row for it: the box is where it was under a picker alone
        app.models[0].note = String::new();
        let found = rects(&settled(&mut app, &ctx, true));
        let (picker, get) = (found["Model:"], found["Get:"]);
        assert!(
            get.top() - picker.bottom() < 40.0,
            "{get:?} is far below {picker:?} with nothing said of the model"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A look-up that is over and found `entries` of a card with the one row of buttons each: the first has
    /// `latest`, the second two sizes in the one row and a bundle in another.
    fn looked_up(app: &mut App, status: &str) {
        use quickai_core::lookup::present::{Entry, Fetch, Offer};
        let offer = |label: &str, bundle: Option<&str>| Offer {
            bundle: bundle.map(str::to_string),
            label: label.to_string(),
            hover: format!("Pull {label}"),
            bytes: Some(1),
            estimated: true,
            class: None,
            recommended: false,
            fetch: Fetch::Reference(label.to_string()),
        };
        let entry = |name: &str, offers: Vec<Offer>| Entry {
            key: format!("k:{name}"),
            name: name.to_string(),
            description: format!("What {name} is."),
            downloads: None,
            likes: None,
            tags: Vec::new(),
            model_type: Some("chat".to_string()),
            offers,
            card: None,
        };
        app.lookup.open = true;
        app.lookup.status = status.to_string();
        app.lookup.entries = vec![
            entry("plain", vec![offer("latest", None)]),
            entry(
                "bundled",
                vec![offer("Q4", None), offer("Q4 with eyes", Some("vision"))],
            ),
        ];
    }

    #[test]
    fn p3_i27_a_look_up_that_found_models_says_its_count_once_and_a_model_has_its_sizes_beside_its_name(
    ) {
        let dir = scratch("z27");
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        looked_up(
            &mut app,
            "Found 2 models in ollama.com's library. There are more: type some words to narrow the search.",
        );
        let found = rects(&settled(&mut app, &ctx, true));
        // the heading has the count; the sentence that said it again is not drawn, and what it added is, in the row of
        // the sizes
        let heading = found
            .get("Results (2)")
            .expect("the heading of the results");
        assert!(
            !found.keys().any(|text| text.starts_with("Found ")),
            "{:?}",
            found.keys()
        );
        let more = found
            .get("There are more: type some words to narrow the search.")
            .expect("what the sentence added");
        let size = found.get("Size:").expect("the row of sizes");
        assert!(more.top() > heading.bottom(), "{more:?} {heading:?}");
        assert!(
            more.center().y - size.center().y < 30.0,
            "{more:?} is not by the sizes {size:?}"
        );
        // a model with one row of buttons has them on the line of its name, and its words under that
        let (name, button, said) = (found["plain"], found["latest"], found["What plain is."]);
        assert!(
            (button.center().y - name.center().y).abs() < 6.0 && button.left() > name.right(),
            "{button:?} is not beside {name:?}"
        );
        assert!(said.top() > name.bottom(), "{said:?} {name:?}");
        // a model with a row for the model and one for a bundle keeps them under its name, a row each
        let name = found["bundled"];
        let (plain, with_eyes) = (found["Q4"], found["Q4 with eyes"]);
        assert!(
            plain.top() > name.bottom() && with_eyes.top() > plain.bottom(),
            "{name:?} {plain:?} {with_eyes:?}"
        );

        // when it could not look, or found nothing, its sentence is what is said
        app.lookup.status = "ollama.com did not answer.".to_string();
        app.lookup.failed = true;
        let found = rects(&settled(&mut app, &ctx, true));
        assert!(
            found.contains_key("ollama.com did not answer."),
            "{:?}",
            found.keys()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
