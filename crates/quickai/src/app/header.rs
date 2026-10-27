//! What the old title row of the main window had and the new layout keeps: the Settings toggle, and the logo, which
//! the About window shows. The layout itself is `shell.rs`.

use eframe::egui;

use super::App;

/// The pixels the logo is made at once: three times what it is drawn at in the About window, for screens that have
/// more than one pixel to a point.
const LOGO_PIXELS: usize = 144;

impl App {
    /// What the Settings button and menu item do: open the settings, or close them. Closing them so is closing them, as
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

    /// The logo, drawn from the application's icon, `points` wide: made once.
    pub(super) fn show_logo(&mut self, ui: &mut egui::Ui, points: f32) {
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
        ui.add(egui::Image::new(&*texture).fit_to_exact_size(egui::Vec2::splat(points)));
    }
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

    use super::super::chat::Turn;
    use super::super::{theme, Tab};
    use super::*;
    use crate::config::{build_registry, ConfigDirs};
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

    /// One frame of the whole window (the menu bar, the list of models, the page with its tabs, the status bar and the
    /// windows over them), in a window of the size the GUI suites use. `_whole` is what the frame of the old title row was asked:
    /// the window is drawn whole either way.
    fn frame(app: &mut App, ctx: &egui::Context, _whole: bool) -> egui::FullOutput {
        frame_with(app, ctx, true, Vec::new())
    }

    /// `frame`, with what the pointer and the keyboard did since the one before.
    fn frame_with(
        app: &mut App,
        ctx: &egui::Context,
        _whole: bool,
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
            app.show_window(ui);
            // the windows of the main window (UI v2): Add a model, Connect an app, the guides
            app.show_add_model_window(ui.ctx());
            app.show_connect_window(ui.ctx());
            app.show_guide_window(ui.ctx());
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

    /// Where the page draws `name` as its heading: the name is in the list on the left too, and the heading is the one
    /// further right.
    fn heading_of(found: &BTreeMap<String, egui::Rect>, name: &str) -> egui::Rect {
        let rect = *found
            .get(name)
            .unwrap_or_else(|| panic!("{name}: {:?}", found.keys()));
        assert!(
            rect.left() > 170.0,
            "{name} is drawn only in the list: {rect:?}"
        );
        rect
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
    fn p3_i10_the_status_bar_says_what_the_server_is_doing_at_the_foot_of_the_window_on_both_tabs()
    {
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
            let (state, place) = (at("Stopped"), at("This computer"));
            // in the status bar: the last row of the window, the state at its left and where the models run at its
            // right (the engine is in the menu bar now)
            assert!(
                state.center().y > 530.0,
                "{live}: {state:?} is not in the status bar"
            );
            assert!(state.left() < 40.0, "{live}: {state:?}");
            assert!(
                (place.center().y - state.center().y).abs() < 4.0 && place.left() > 600.0,
                "{live}: {place:?} {state:?}"
            );
            // (the menu bar, the first row, is held to its order by A70 of the configurations)
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i11_the_speed_of_the_model_is_a_tag_under_its_name_in_the_page_and_not_in_a_bar() {
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
            let display = app.models[0].display.clone();
            // the name is in the list on the left and is the heading of the page: the heading is the one on the right
            let heading = found
                .iter()
                .filter(|(text, _)| **text == display)
                .map(|(_, rect)| *rect)
                .max_by(|a, b| a.left().total_cmp(&b.left()))
                .unwrap_or_else(|| panic!("{display}: {:?}", found.keys()));
            assert!(
                heading.left() > 170.0,
                "{name}: {heading:?} is not in the page"
            );
            assert!(
                chip.top() >= heading.bottom() - 1.0 && chip.top() - heading.bottom() < 30.0,
                "{name}: {chip:?} is not under {heading:?}"
            );
            assert!(
                chip.left() > 170.0 && chip.bottom() < 500.0,
                "{name}: {chip:?}"
            );
        }
        // an engine with no list of models to start has no model to say it of
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
        // and Start is beside the model's name, at the right end of its row; Stop and Connect an app are not there while
        // nothing runs (UI v2: there is no toolbar)
        let name = heading_of(&found, &app.models[0].display);
        assert!(
            (start.center().y - name.center().y).abs() < 12.0 && start.left() > name.right(),
            "{start:?} is not beside {name:?}"
        );
        for text in ["Stop", "Connect an app"] {
            assert!(!found.contains_key(text), "{text} while stopped");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_i12b_an_ollama_with_no_models_listed_yet_has_start_over_the_box_that_adds_one() {
        // Ollama lists its models once it runs: a first run has none, and Start must not be out of reach (UI v2 had only the
        // box that adds a model there, and no way to start Ollama to see the ones it has)
        let dir = scratch("z12b");
        let mut app = app_in(&dir, "ollama");
        std::fs::write(dir.join("ollama"), "").unwrap();
        app.binary = dir.join("ollama");
        app.models.clear();
        let ctx = egui::Context::default();
        let found = rects(&settled(&mut app, &ctx, true));
        let start = *found
            .get("Start")
            .unwrap_or_else(|| panic!("a Start button: {:?}", found.keys()));
        let name = heading_of(&found, "Ollama");
        assert!(
            (start.center().y - name.center().y).abs() < 12.0 && start.left() > name.right(),
            "{start:?} is not beside {name:?}"
        );
        let title = found
            .get("Add your first model")
            .unwrap_or_else(|| panic!("the box that adds a model: {:?}", found.keys()));
        assert!(start.bottom() < title.top(), "{start:?} {title:?}");
        // llama.cpp's first run has no Start: there is nothing to start until a model is added
        let mut llama = app_in(&dir, "llamacpp");
        llama.models.clear();
        let found = rects(&settled(&mut llama, &ctx, true));
        assert!(
            found.contains_key("Add your first model"),
            "{:?}",
            found.keys()
        );
        assert!(!found.contains_key("Start"), "{:?}", found.keys());
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

        // the log has it too: it is in a card under the box of Add a model (on the page of a first run, as here)
        app.catalog.note =
            Some("Download failed: tango-Q4_K_M.gguf does not match the checksum".into());
        let found = rects(&settled(&mut app, &ctx, true));
        let (_, note) = found
            .iter()
            .find(|(text, _)| text.starts_with("Download failed"))
            .unwrap_or_else(|| panic!("the failure in the window: {:?}", found.keys()));
        let get = found
            .get("Search, or paste a link or file path")
            .expect("the box it was asked for in");
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
        // an Ollama that is installed (a page of its own installs one that is not)
        std::fs::write(dir.join("ollama"), "").unwrap();
        app.binary = dir.join("ollama");
        let ctx = egui::Context::default();
        app.models = vec![listed("qwen3-vl:8b", "completion, vision, tools, thinking")];
        app.selected = 0;
        app.recommended_filename = Some("qwen3-vl:8b".to_string());
        app.named.stale = true;
        let found = rects(&settled(&mut app, &ctx, true));
        let picker = heading_of(&found, "qwen3-vl:8b");
        let get = *found
            .iter()
            .find(|(text, _)| text.contains(" context · "))
            .unwrap_or_else(|| panic!("the line of how it runs: {:?}", found.keys()))
            .1;
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
        // all on one row, under the name and over the line that says how it runs
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
        // the row is the height of its chips: the line under it is that row further down
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
        // and a model with nothing but its speed to say has a row of one tag: the line of how it runs is only that row
        // further down
        app.models[0].note = String::new();
        let found = rects(&settled(&mut app, &ctx, true));
        let picker = heading_of(&found, "Bonsai-1.7B-Q1_0.gguf");
        let get = *found
            .iter()
            .find(|(text, _)| text.contains(" context · "))
            .unwrap_or_else(|| panic!("the line of how it runs: {:?}", found.keys()))
            .1;
        assert!(
            get.top() - picker.bottom() < 60.0,
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
        // (llama.cpp, which is always there: the results are under the box of Add a model, on the page of a first run)
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        app.model_input = "qwen".to_string();
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

    // ---- UI v2 (C175 to C180) ---------------------------------------------------------------------------------

    /// A key pressed with Ctrl (Cmd on a Mac), as one frame of events.
    fn command_key(key: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]
    }

    #[test]
    fn the_menu_bar_has_four_menus_and_the_view_menu_is_gone() {
        let dir = scratch("v2-menus");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let found = rects(&settled(&mut app, &ctx, true));
        let menus: Vec<egui::Rect> = ["File", "Models", "Server", "Help"]
            .iter()
            .map(|m| {
                *found
                    .get(*m)
                    .unwrap_or_else(|| panic!("{m}: {:?}", found.keys()))
            })
            .collect();
        assert!(menus.iter().all(|r| r.center().y < 30.0), "{menus:?}");
        assert!(
            menus.windows(2).all(|p| p[0].right() < p[1].left()),
            "{menus:?}"
        );
        assert!(!found.contains_key("View"));
        // nor what the View menu and the toolbar had
        for gone in ["Dark theme", "Settings", "Set up chat UI...", "Refresh"] {
            assert!(!found.contains_key(gone), "{gone} is still drawn");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_theme_is_changed_at_once_and_kept() {
        let dir = scratch("v2-theme");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let dark = app.dark_mode;
        app.set_dark_mode(&ctx, !dark);
        assert_eq!(app.dark_mode, !dark);
        assert_eq!(theme::is_dark(), !dark);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["dark_mode"], !dark, "{saved}");
        app.set_dark_mode(&ctx, dark);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ctrl_k_adds_a_model_and_ctrl_l_goes_to_the_chat() {
        let dir = scratch("v2-keys");
        let mut app = app_in(&dir, "llamacpp");
        only_model(&mut app, &dir, "Small-Q4_K_M.gguf", GB);
        let ctx = egui::Context::default();
        app.active_tab = Tab::Main;
        settled(&mut app, &ctx, true);
        frame_with(&mut app, &ctx, true, command_key(egui::Key::K));
        assert!(app.add_model_open, "Ctrl+K opens Add a model");
        let found = rects(&settled(&mut app, &ctx, true));
        assert!(
            found.contains_key("Add a model") && found.contains_key("SUGGESTED FOR THIS COMPUTER"),
            "{:?}",
            found.keys()
        );
        app.add_model_open = false;
        frame_with(&mut app, &ctx, true, command_key(egui::Key::L));
        assert!(app.active_tab == Tab::Chat, "Ctrl+L opens the chat");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn getting_started_ticks_its_steps_as_they_are_done() {
        let dir = scratch("v2-steps");
        let mut app = app_in(&dir, "llamacpp");
        let done = |app: &App| app.getting_started().map(|(_, d)| d);
        assert_eq!(
            done(&app),
            [true, false, false],
            "llama.cpp is ready; no model yet"
        );
        only_model(&mut app, &dir, "Small-Q4_K_M.gguf", GB);
        assert_eq!(done(&app), [true, true, false]);
        app.chat
            .turns
            .extend([Turn::said(true, "Hello"), Turn::said(false, "Hi there")]);
        assert_eq!(done(&app), [true, true, true], "a reply is the last step");
        // and an engine that is not installed is the first step not done
        let ollama = app_in(&dir, "ollama");
        assert!(!done(&ollama)[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_the_person_wrote_and_the_reply_are_both_drawn_each_on_its_side() {
        let dir = scratch("v2-chat");
        let mut app = app_in(&dir, "llamacpp");
        only_model(&mut app, &dir, "Small-Q4_K_M.gguf", GB);
        app.active_tab = Tab::Chat;
        app.chat.turns.extend([
            Turn::said(true, "Write a haiku about local AI"),
            Turn::said(false, "Quiet fans hum on"),
        ]);
        let ctx = egui::Context::default();
        let found = rects(&settled(&mut app, &ctx, true));
        // the person's message was drawn nowhere (a row laid from right to left, with no height): it is at the right
        let mine = *found
            .get("Write a haiku about local AI")
            .unwrap_or_else(|| panic!("the person's message: {:?}", found.keys()));
        let reply = *found
            .get("Quiet fans hum on")
            .unwrap_or_else(|| panic!("the reply: {:?}", found.keys()));
        assert!(mine.right() > 700.0, "{mine:?} is not at the right");
        assert!(reply.left() < 300.0, "{reply:?} is not at the left");
        assert!(mine.bottom() <= reply.top(), "{mine:?} {reply:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_port_in_use_is_said_as_such_for_llamacpp_and_its_button_opens_the_numbers() {
        let dir = scratch("v2-port");
        let mut app = app_in(&dir, "llamacpp");
        only_model(&mut app, &dir, "Small-Q4_K_M.gguf", GB);
        app.log_lines.push_back(
            "couldn't bind HTTP server socket, hostname: 127.0.0.1, port: 8080".to_string(),
        );
        let ctx = egui::Context::default();
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 560.0),
            )),
            ..Default::default()
        };
        let mut said = String::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(raw(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    app.show_diagnostics(ui, crate::events::ServerState::Crashed)
                });
            });
            output.textures_delta.clear();
            said = format!("{:?}", output.shapes);
        }
        assert!(
            said.contains("Another program is using port 8080")
                && said.contains("Go to the setting"),
            "{said}"
        );
        assert!(!said.contains("Choose another build"), "{said}");
        // the button goes to the numbers: Change, Custom
        app.invoke("focus_field:port");
        assert!(app.change_open && app.change_custom);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_start_chooses_the_model_recommended_for_this_computer_and_then_the_one_chosen() {
        let dir = scratch("v2-first-pick");
        // two models before the window is made: a small one first by name, and a bigger one that suits 16 GB better
        let folder = dir.join("models");
        std::fs::create_dir_all(&folder).unwrap();
        for (name, bytes) in [
            ("A-tiny-Q4_K_M.gguf", GB / 4),
            ("Z-chat-Q4_K_M.gguf", 2 * GB),
        ] {
            std::fs::File::create(folder.join(name))
                .unwrap()
                .set_len(bytes)
                .unwrap();
        }
        let app = app_in(&dir, "llamacpp");
        assert_eq!(
            app.recommended_filename.as_deref(),
            Some("Z-chat-Q4_K_M.gguf")
        );
        assert_eq!(app.models[app.selected].filename, "Z-chat-Q4_K_M.gguf");
        // the one chosen last time wins over the recommendation
        let settings = Settings {
            last_model_filename: Some("A-tiny-Q4_K_M.gguf".to_string()),
            models_dir_override: Some(folder.display().to_string()),
            ..Settings::default()
        };
        let app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        assert_eq!(app.models[app.selected].filename, "A-tiny-Q4_K_M.gguf");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_item_of_a_list_in_a_reply_wraps_inside_the_window() {
        let dir = scratch("v2-wrap");
        let mut app = app_in(&dir, "llamacpp");
        only_model(&mut app, &dir, "Small-Q4_K_M.gguf", GB);
        app.active_tab = Tab::Chat;
        let long = "word ".repeat(60);
        app.chat.turns.extend([
            Turn::said(true, "Three tips, please"),
            Turn::said(false, format!("1. **First**: {long}\n2. Second\n> {long}")),
        ]);
        let ctx = egui::Context::default();
        let output = settled(&mut app, &ctx, true);
        // every text the reply painted is inside the window (760 wide), and the long ones are on several lines
        let mut texts = Vec::new();
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<(String, egui::Rect, usize)>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => into.push((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    t.galley.rows.len(),
                )),
                _ => {}
            }
        }
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut texts);
        }
        let wordy: Vec<&(String, egui::Rect, usize)> = texts
            .iter()
            .filter(|(t, _, _)| t.contains("word word"))
            .collect();
        assert_eq!(wordy.len(), 2, "{texts:?}");
        for (_, rect, rows) in wordy {
            assert!(rect.right() <= 760.0, "{rect:?} runs out of the window");
            assert!(*rows > 1, "{rect:?} is one line");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tab_shown_under_the_model_is_kept_for_the_next_start() {
        let dir = scratch("v2-tab");
        let mut app = app_in(&dir, "llamacpp");
        assert!(app.active_tab == Tab::Chat, "the chat, on a first start");
        app.active_tab = Tab::Main;
        app.save_settings();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["values"]["app.tab"], "log", "{saved}");
        let stored: Settings = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(stored.tab().as_deref(), Some("log"));
        // and the chat again takes the key away
        app.active_tab = Tab::Chat;
        app.save_settings();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert!(saved["values"].get("app.tab").is_none(), "{saved}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
