//! *Add a model* (UI v2, C177): one box for every way a model comes in, where there were five controls (the box, ...,
//! Search, Look up online, Recommended). Words search online, a link downloads that file, a path (or the folder
//! beside the box) adds a file of this computer, and for Ollama a name with a tag (`qwen3:4b`) or a Hugging Face
//! reference is pulled; Enter in the empty box lists the models most downloaded online. With the box empty, the models of the recommended list that suit this computer are under it,
//! each with one button. It is a window of its own opened from the head of the list (and the Models menu, and Ctrl+K), and
//! the whole page of a first run, where there is nothing else to do.

use eframe::egui;
use quickai_core::lookup::present::{Entry, Offer};
use quickai_core::lookup::{huggingface, ollama_library};
use quickai_core::registry::engine::LookupKind;

use super::icons::{self, Icon};
use super::theme;
use super::App;
use crate::hardware::{self, Fit};

/// How many models the box suggests when it is empty.
const SUGGESTED: usize = 4;

/// The kinds of models the box suggests, in the order it suggests them: the ones the chat can talk to.
const CHAT_KINDS: [&str; 4] = ["chat", "vision", "reasoning", "code"];

/// The title of the window, and the size it opens at: the box and the models it suggests, with room for a few results
/// (C194: a window of its own, made bigger with the window manager's own buttons and edges).
pub(super) const ADD_MODEL_TITLE: &str = "Add a model";
const OPENS_AT: [f32; 2] = [720.0, 480.0];
const LEAST: [f32; 2] = [440.0, 300.0];

/// Which window *Add a model* is: the id the display knows it by.
pub(super) fn add_model_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_add_model")
}

/// The id of the box, so that it can be given the keyboard.
fn box_id() -> egui::Id {
    egui::Id::new("add_model_box")
}

impl App {
    /// Opens *Add a model* with the keyboard in its box. A first run has it on the page already: the box is given the
    /// keyboard there.
    pub(super) fn open_add_model(&mut self) {
        if !self.models.is_empty() {
            // open already, somewhere behind the main window: it comes to the front
            self.raise_add_model = self.add_model_open;
            self.add_model_open = true;
        }
        self.focus_add_box = true;
        self.refresh_catalog_have();
    }

    /// What the page of a first run says under its title.
    pub(super) fn first_run_sentence(&self) -> String {
        if self.lists_models() {
            format!(
                "Pick one below or search. {} downloads it once and keeps it on {}.",
                self.server_name(),
                self.remote_in_use()
                    .map_or_else(|| "this computer".to_string(), |r| r.name.clone())
            )
        } else {
            "Pick one below or search. It downloads once and stays on this computer.".to_string()
        }
    }

    /// *Add a model*, while it is open: a window of its own (C194), as big as the person makes it with the window manager's
    /// own buttons and edges (a list of results fills it).
    pub(super) fn show_add_model_window(&mut self, ctx: &egui::Context) {
        if !self.add_model_open {
            return;
        }
        if self.models.is_empty() || self.runs_alone() {
            // the page has the box itself
            self.add_model_open = false;
            return;
        }
        let id = add_model_viewport();
        if std::mem::take(&mut self.raise_add_model) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let mut close = false;
        let builder = theme::own_window(ADD_MODEL_TITLE, OPENS_AT, LEAST);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            // Escape closes it, and so does the window manager's close button
            if ui.input(|i| i.key_pressed(egui::Key::Escape))
                || (class != egui::ViewportClass::EmbeddedWindow
                    && ui.input(|i| i.viewport().close_requested()))
            {
                close = true;
                return;
            }
            theme::own_window_page(ui, class, |ui| self.show_add_model_body(ui));
        });
        if close {
            self.add_model_open = false;
        }
    }

    /// The box, what is being fetched, and under it the results of a search or the suggestions.
    pub(super) fn show_add_model_body(&mut self, ui: &mut egui::Ui) {
        // Escape puts the results away (in the window, it closes the window first)
        if self.lookup.open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.lookup.open = false;
        }
        let named = self.lists_models();
        let busy = self.download.is_some() || self.named.pulling();
        let mut submit = false;
        let mut browse = false;
        // the box: a picture of a magnifier, what is typed, and the folder for a file of this computer
        let p = theme::palette();
        let focused_before = ui.memory(|m| m.has_focus(box_id()));
        egui::Frame::new()
            .fill(p.well)
            .stroke(egui::Stroke::new(
                1.0,
                if focused_before {
                    p.accent_text
                } else {
                    p.outline
                },
            ))
            .corner_radius(8)
            .inner_margin(egui::Margin {
                left: 10,
                right: 4,
                top: 4,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    icons::paint(ui.painter(), rect, Icon::Search, p.faint);
                    let room = ui.available_width() - if named { 4.0 } else { 40.0 };
                    let hint = if named {
                        "Search, or a name to pull (qwen3:4b, hf.co/user/repo)"
                    } else {
                        "Search, or paste a link or file path"
                    };
                    // greyed while a download or a pull is under way (one at a time; its progress is under the box), and
                    // free while a model runs: a model is got while another is used
                    let field = ui.add_enabled(
                        !busy,
                        egui::TextEdit::singleline(&mut self.model_input)
                            .id(box_id())
                            .frame(egui::Frame::NONE)
                            .hint_text(hint)
                            .desired_width(room)
                            .margin(egui::Margin::symmetric(2, 6)),
                    );
                    if std::mem::take(&mut self.focus_add_box) {
                        field.request_focus();
                    }
                    submit = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if !named {
                        browse = icons::icon_button(
                            ui,
                            Icon::Folder,
                            egui::vec2(32.0, 28.0),
                            !busy,
                            false,
                        )
                        .on_hover_text("Add a .gguf file you already have")
                        .clicked();
                    }
                });
            });
        if browse {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("GGUF model", &["gguf"])
                .pick_file()
            {
                self.model_input = path.display().to_string();
                submit = true;
            }
        }
        if submit && !self.model_input.trim().is_empty() {
            if named {
                self.submit_named_box(ui.ctx());
            } else {
                self.submit_file_box(ui.ctx());
            }
        } else if submit && self.can_look_up() {
            // Enter in the empty box: the models most downloaded online, to browse
            self.named.notice = None;
            self.start_lookup(ui.ctx(), "");
        }
        // a box emptied of the words it looked up goes back to what suits this computer
        if self.model_input.trim().is_empty()
            && !self.lookup.words.is_empty()
            && self.lookup.open
            && !self.lookup.running()
        {
            self.lookup.open = false;
        }
        if named {
            self.show_pull_status(ui);
        } else {
            self.show_download_status(ui);
        }
        if self.lookup.open {
            self.show_lookup_results(ui);
        } else {
            self.show_suggestions(ui);
        }
        // the site the box searches, for whoever would rather look there and bring back a link (or a name)
        if let Some((site, address)) = self.browse_page() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(theme::muted("Or find one yourself on"));
                if ui.link(&site).on_hover_text(&address).clicked() {
                    let _ = webbrowser::open(&address);
                }
                ui.label(theme::muted(if named {
                    "and type its name in the box."
                } else {
                    "and paste its link in the box."
                }));
            });
        }
    }

    /// The page of the site the box searches where a person looks for a model themselves (Hugging Face; ollama.com
    /// for Ollama), for the words looked up last while their results are shown: its name and its address. `None` for an
    /// engine that looks nothing up.
    pub(super) fn browse_page(&self) -> Option<(String, String)> {
        let lookup = quickai_core::models::lookups(self.engine_doc()?).first()?;
        let words = if self.lookup.open {
            self.lookup.words.as_str()
        } else {
            ""
        };
        Some(match &lookup.kind {
            LookupKind::HuggingFace { site, .. } => (
                "Hugging Face".to_string(),
                huggingface::page_url(site, words),
            ),
            LookupKind::OllamaLibrary { site, .. } => (
                site.split_once("://")
                    .map_or(site.as_str(), |(_, host)| host)
                    .trim_end_matches('/')
                    .to_string(),
                ollama_library::search_url(site, words),
            ),
        })
    }

    /// What Enter in the box does for an engine that pulls models by name: a name with a tag, or a reference to Hugging
    /// Face, is pulled; other words are looked up in the engine's library.
    fn submit_named_box(&mut self, ctx: &egui::Context) {
        let text = self.model_input.trim().to_string();
        if names_a_model(&text) || !self.can_look_up() {
            self.submit_model_input(ctx);
            if self.model_input.is_empty() {
                self.lookup.open = false;
            }
        } else {
            self.named.notice = None;
            self.start_lookup(ctx, &text);
        }
    }

    /// The models of the recommended list that suit this computer, each with one button that gets the size that fits,
    /// and the way to the whole list.
    fn show_suggestions(&mut self, ui: &mut egui::Ui) {
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let entries = quickai_core::models::recommended(&self.registry, engine);
        if entries.is_empty() {
            return;
        }
        let ram = self.total_ram_bytes;
        let listed: Vec<String> = if self.lists_models() {
            self.models.iter().map(|m| m.filename.clone()).collect()
        } else {
            Vec::new()
        };
        let have = &self.catalog.have;
        let picks = suggestions(&entries, ram);
        let too_big = |o: &Offer| o.bytes.map(|b| hardware::fit(b, ram)) == Some(Fit::TooBig);
        let busy = self.download.is_some() || self.named.pulling();
        let p = theme::palette();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SUGGESTED FOR THIS COMPUTER")
                    .size(11.0)
                    .family(theme::semibold(ui))
                    .color(p.faint),
            );
            if ram > 0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} memory", hardware::format_bytes(ram)))
                            .size(11.5)
                            .color(p.faint),
                    );
                });
            }
        });
        ui.add_space(2.0);
        let mut chosen = None;
        for (i, (entry, offer)) in picks.iter().enumerate() {
            if i > 0 {
                let y = ui.cursor().top();
                ui.painter()
                    .hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, p.line));
            }
            let installed = super::catalog_view::installed(offer, have, &listed);
            let too_big = too_big(offer);
            ui.horizontal(|ui| {
                ui.set_min_height(46.0);
                let words = (ui.available_width() - 130.0).max(120.0);
                ui.vertical(|ui| {
                    ui.set_width(words);
                    ui.add_space(5.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(entry.name.as_str()).color(p.text))
                            .truncate(),
                    );
                    let about = if too_big {
                        egui::RichText::new("May be too big for this computer").color(p.warn)
                    } else {
                        theme::muted(
                            super::shell::first_sentence(&entry.description).trim_end_matches('.'),
                        )
                    };
                    ui.add(egui::Label::new(about.small()).truncate());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if installed {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(30.0, 28.0), egui::Sense::hover());
                        icons::paint(ui.painter(), rect.shrink(6.0), Icon::Check, p.ok);
                    } else if icons::icon_button(
                        ui,
                        Icon::Download,
                        egui::vec2(30.0, 28.0),
                        !busy,
                        false,
                    )
                    .on_hover_text(format!("Get {} ({})", entry.name, offer.label))
                    .clicked()
                    {
                        chosen = Some(offer.fetch.clone());
                    }
                    if let Some(bytes) = offer.bytes {
                        ui.label(theme::muted(super::shell::size_text(bytes)));
                    }
                });
            });
        }
        ui.add_space(8.0);
        let mut all = false;
        ui.horizontal(|ui| {
            all = ui
                .link("See all recommended models")
                .on_hover_text("The whole list QuickAI checked, by kind and size. Nothing is downloaded until you choose.")
                .clicked();
            if !self.lists_models() {
                ui.label(theme::muted(format!(
                    "· A .gguf file in {} shows up by itself.",
                    super::runtime_manager::display_root(&self.models_dir, &self.host_facts.home)
                )));
            }
        });
        if all {
            self.open_catalog();
        }
        if let Some(fetch) = chosen {
            self.fetch_offer(&fetch, ui.ctx());
        }
    }
}

/// Whether what is typed in the box of an engine that pulls by name is a model to pull rather than words to look up:
/// one word with a tag (`qwen3:4b`) or a path (`hf.co/user/repo`, `user/model`).
pub(super) fn names_a_model(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && !text.contains(char::is_whitespace)
        && (text.contains(':') || text.contains('/'))
}

/// The models the box suggests, with the size of each: the ones the list says to start with, of the kinds the chat
/// can talk to (chat, then vision, reasoning and code), in the list's own order within a kind; what would not fit this
/// computer only when nothing else would. At most `SUGGESTED`.
pub(super) fn suggestions(entries: &[Entry], ram: u64) -> Vec<(&Entry, &Offer)> {
    let rank = |e: &Entry| {
        CHAT_KINDS
            .iter()
            .position(|k| e.model_type.as_deref() == Some(*k))
    };
    let starters: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.card.as_ref().is_some_and(|c| c.start) && rank(e).is_some())
        .collect();
    let pool: Vec<&Entry> = if starters.is_empty() {
        entries.iter().filter(|e| rank(e).is_some()).collect()
    } else {
        starters
    };
    let mut picks: Vec<(&Entry, &Offer)> = pool
        .into_iter()
        .filter_map(|e| suggested_offer(e, ram).map(|o| (e, o)))
        .collect();
    let too_big = |o: &Offer| o.bytes.map(|b| hardware::fit(b, ram)) == Some(Fit::TooBig);
    if picks.iter().any(|(_, o)| !too_big(o)) {
        picks.retain(|(_, o)| !too_big(o));
    }
    // (a stable sort: the list's own order within a kind)
    picks.sort_by_key(|(e, _)| rank(e));
    picks.truncate(SUGGESTED);
    picks
}

/// The size of `entry` that the box suggests: the one the list recommends when it fits this computer with room to
/// spare, else the largest that does, else the smallest. A bundle is not suggested (the model alone is).
fn suggested_offer(entry: &Entry, ram: u64) -> Option<&Offer> {
    let plain: Vec<&Offer> = entry.offers.iter().filter(|o| o.bundle.is_none()).collect();
    let fits = |o: &&Offer| {
        o.bytes
            .is_none_or(|b| hardware::fit(b, ram) == Fit::Comfortable)
    };
    plain
        .iter()
        .find(|o| o.recommended && fits(o))
        .or_else(|| {
            plain
                .iter()
                .filter(|o| fits(o))
                .max_by_key(|o| o.bytes.unwrap_or(0))
        })
        .or_else(|| plain.iter().min_by_key(|o| o.bytes.unwrap_or(u64::MAX)))
        .copied()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use quickai_core::lookup::present::{Card, Fetch};

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    fn offer(label: &str, bytes: u64, recommended: bool) -> Offer {
        Offer {
            bundle: None,
            label: label.to_string(),
            hover: String::new(),
            bytes: Some(bytes),
            estimated: false,
            class: None,
            recommended,
            fetch: Fetch::Reference(label.to_string()),
        }
    }

    fn entry(name: &str, kind: &str, start: bool, offers: Vec<Offer>) -> Entry {
        Entry {
            key: format!("k:{name}"),
            name: name.to_string(),
            description: format!("What {name} is."),
            downloads: None,
            likes: None,
            tags: Vec::new(),
            model_type: Some(kind.to_string()),
            offers,
            card: Some(Box::new(Card {
                start,
                ..Card::default()
            })),
        }
    }

    #[test]
    fn a_name_with_a_tag_or_a_path_is_pulled_and_other_words_are_looked_up() {
        for pulled in [
            "qwen3:4b",
            "hf.co/user/repo:Q4_K_M",
            "user/model",
            " qwen3:4b ",
        ] {
            assert!(names_a_model(pulled), "{pulled}");
        }
        for looked_up in ["qwen", "qwen 3", "small coder", "", "qwen3: 4b"] {
            assert!(!names_a_model(looked_up), "{looked_up}");
        }
    }

    #[test]
    fn the_suggestions_are_chat_models_first_in_the_lists_order_and_what_fits_this_computer() {
        let entries = vec![
            entry("speech", "audio", true, vec![offer("a", GB, true)]),
            entry("eyes", "vision", true, vec![offer("v", 2 * GB, true)]),
            entry("small", "chat", true, vec![offer("s", GB, true)]),
            entry("huge", "chat", true, vec![offer("h", 40 * GB, true)]),
            entry("medium", "chat", true, vec![offer("m", 3 * GB, true)]),
            entry("aside", "chat", false, vec![offer("x", GB, true)]),
            entry("numbers", "embedding", true, vec![offer("e", GB / 4, true)]),
        ];
        let names: Vec<&str> = suggestions(&entries, 16 * GB)
            .iter()
            .map(|(e, _)| e.name.as_str())
            .collect();
        // chat first in the list's order, then the vision model; nothing the chat cannot talk to, nothing that does
        // not fit, nothing the list does not say to start with
        assert_eq!(names, ["small", "medium", "eyes"]);
        // on a computer nothing fits, what there is is still said (with a word that it may be too big)
        let tiny = suggestions(&entries[3..4], 4 * GB);
        assert_eq!(tiny.len(), 1);
    }

    #[test]
    fn the_size_suggested_is_the_recommended_one_that_fits_else_the_largest_that_does_else_the_smallest(
    ) {
        let sizes = entry(
            "sizes",
            "chat",
            true,
            vec![
                offer("Q2", GB, false),
                offer("Q4", 3 * GB, true),
                offer("Q8", 6 * GB, false),
            ],
        );
        assert_eq!(suggested_offer(&sizes, 32 * GB).unwrap().label, "Q4");
        // the recommended one is too big for 6 GB, with room to spare: the largest that is not
        assert_eq!(suggested_offer(&sizes, 6 * GB).unwrap().label, "Q2");
        // nothing fits 1 GB: the smallest
        assert_eq!(suggested_offer(&sizes, GB).unwrap().label, "Q2");
    }

    fn app_in(dir: &Path) -> App {
        let settings = Settings {
            models_dir_override: Some(dir.join("models").display().to_string()),
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
        app.bundled_models_dir = dir.join("bundled");
        app.models = app.scan_models();
        app
    }

    fn until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done(app) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            app.poll_download();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_box_downloads_a_link_adds_a_file_of_this_computer_and_refuses_what_is_not_a_model() {
        let dir = scratch("add-box");
        let mut app = app_in(&dir);
        let ctx = egui::Context::default();
        assert!(app.models.is_empty());

        // a file of this computer is linked into the models folder, and the box is emptied
        let own = dir.join("elsewhere/own-Q4_K_M.gguf");
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::fs::write(&own, b"GGUF and some weights").unwrap();
        app.model_input = own.display().to_string();
        app.submit_file_box(&ctx);
        assert!(app.download.is_some() && app.model_input.is_empty());
        until(&mut app, "the file to be added", |a| a.download.is_none());
        assert!(dir.join("models/own-Q4_K_M.gguf").is_file());
        assert!(app.models.iter().any(|m| m.filename == "own-Q4_K_M.gguf"));

        // a link to a file that is not a model is said, under the box, and nothing is fetched
        app.model_input = "https://example.invalid/notes/readme.txt".to_string();
        app.submit_file_box(&ctx);
        assert!(app.download.is_none());
        assert!(app.catalog.note.is_some(), "why it was refused");
        assert_eq!(app.model_input, "https://example.invalid/notes/readme.txt");

        // a link to a model downloads it into the folder
        let files = crate::testkit::files::FileServer::start();
        files.add("/m/linked-Q8_0.gguf", b"GGUF linked".to_vec());
        app.catalog.note = None;
        app.model_input = files.url("/m/linked-Q8_0.gguf");
        app.submit_file_box(&ctx);
        until(&mut app, "the download", |a| a.download.is_none());
        assert!(dir.join("models/linked-Q8_0.gguf").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_a_model_is_a_window_once_there_are_models_and_the_page_itself_before() {
        let dir = scratch("add-window");
        let mut app = app_in(&dir);
        // a first run: the page has the box, so there is no window, and the box gets the keyboard
        app.open_add_model();
        assert!(!app.add_model_open && app.focus_add_box);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("models/one-Q4_K_M.gguf"), b"GGUF").unwrap();
        app.models = app.scan_models();
        app.open_add_model();
        assert!(app.add_model_open);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
