//! The page of an engine that runs on its own (ComfyUI), once it is here: it has no model to choose before Start,
//! because the workflow a person builds in its page names the files it uses. The page says how it is used in three
//! steps, offers the three ways to get models (its own Manager first, then QuickAI's list, then Hugging Face, with the box
//! that takes a name, a link or a file of this computer and the folder it goes in), and lists the files of its models
//! folder.
//!
//! The models of the other engines are drawn by `model_bar.rs` and `acquire.rs`; nothing here changes
//! what they show.

use std::path::{Path, PathBuf};

use eframe::egui;
use quickai_core::lookup::present::Download;

use super::icons::{self, Icon};
use super::theme::{self, Role, Tone};
use super::App;
use crate::downloader;
use crate::events::ServerState;
use crate::hardware;
use quickai_core::acquire;

/// The endings of the files a link or a file given by hand may be: the formats that hold numbers and
/// cannot hold a program. A pickled `.ckpt`, `.pt` or `.pth` is the person's to put in the folder
/// (**Open folder**), as it is for the page's Download button.
const MODEL_ENDINGS: &[&str] = &[".gguf", ".safetensors", ".sft"];

/// What the area holds besides the models.
#[derive(Default)]
pub struct Bench {
    /// The folder the person chose for a file they give by a link or a path; `None` is the engine's
    /// guess from the file's name.
    pub into: Option<String>,
    /// What the button that was pressed fetches, in words, while it does.
    fetching: Option<String>,
    /// What the last fetch did, or why the box would not take what was given.
    pub note: Option<Note>,
    /// The panel of Hugging Face was asked for (Search or paste…).
    pub box_open: bool,
    /// It was asked for in this frame, or a look-up has just brought results into it: it is scrolled to, once it is
    /// drawn.
    scroll_to_box: bool,
    /// A look-up was running at the last frame.
    lookup_was_running: bool,
    /// The Manager was asked for before ComfyUI ran: its page is opened with it once it does.
    pub open_manager: Option<std::time::Instant>,
    /// The files of the models folder, as the list shows them, and when they were looked at (the frame's time).
    pub files: Vec<ModelFile>,
    pub files_at: Option<f64>,
    /// The launcher of the build in use and what it says, for what the build was made with.
    pub launcher: Option<(PathBuf, Option<String>)>,
}

/// A sentence about the last thing that was tried, and whether it went wrong.
pub struct Note {
    pub text: String,
    pub failed: bool,
}

/// What the box makes of what was typed or chosen.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Given {
    /// Nothing yet.
    Nothing,
    /// The address of one file to download.
    Link(String),
    /// A file of this computer to link into the models folder.
    File(PathBuf),
    /// Words to search the hub for.
    Words(String),
    /// Something the box will not take, and why.
    Refused(String),
}

/// Reads what was given to the box: the address of a file, a file of this computer, a page of the hub
/// that is a repository (whose name is searched for), or words.
pub(super) fn read_box(text: &str) -> Given {
    read_box_for(text, MODEL_ENDINGS)
}

/// `read_box` for an engine that takes files with these endings (llama.cpp: `.gguf`). A path pasted in quotes (as
/// "Copy as path" of Windows' Explorer gives it) is the path without them.
pub(super) fn read_box_for(text: &str, endings: &[&str]) -> Given {
    let text = unquoted(text.trim());
    if text.is_empty() {
        return Given::Nothing;
    }
    if downloader::looks_like_url(text) {
        return read_link(text, endings);
    }
    let path = Path::new(text);
    if path.is_file() {
        return if has_model_ending(text, endings) {
            Given::File(path.to_path_buf())
        } else {
            Given::Refused(format!(
                "{text} is not a model file this window takes in ({}).",
                endings.join(", ")
            ))
        };
    }
    Given::Words(text.to_string())
}

/// `text` without the quotes round it, when it has a pair.
fn unquoted(text: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|q| text.strip_prefix(*q).and_then(|t| t.strip_suffix(*q)))
        .map_or(text, str::trim)
}

fn has_model_ending(name: &str, endings: &[&str]) -> bool {
    let lower = name
        .split(['?', '#'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    endings.iter().any(|ending| lower.ends_with(ending))
}

/// A link: the page of a file of the hub is turned into the address of the file, the page of a
/// repository is a name to search for, and any other address must end in a model file.
fn read_link(link: &str, endings: &[&str]) -> Given {
    let (host, path) = match link.split_once("://") {
        Some((_, rest)) => rest.split_once('/').unwrap_or((rest, "")),
        None => return Given::Nothing,
    };
    let host = host.to_ascii_lowercase();
    let on_the_hub = matches!(
        host.as_str(),
        "huggingface.co" | "www.huggingface.co" | "hf.co"
    );
    if on_the_hub {
        let before_query = path.split(['?', '#']).next().unwrap_or(path);
        let parts: Vec<&str> = before_query.split('/').filter(|p| !p.is_empty()).collect();
        match parts.as_slice() {
            // owner/repo/blob/<revision>/<file>: the page of a file; the file is at /resolve/
            [owner, repo, "blob", rest @ ..] if rest.len() >= 2 => {
                let tail = before_query
                    .trim_matches('/')
                    .splitn(4, '/')
                    .nth(3)
                    .unwrap_or_default();
                let scheme = if link.to_ascii_lowercase().starts_with("http://") {
                    "http"
                } else {
                    "https"
                };
                let address = format!("{scheme}://{host}/{owner}/{repo}/resolve/{tail}");
                return if has_model_ending(&address, endings) {
                    Given::Link(address)
                } else {
                    refused_link(link, endings)
                };
            }
            [_, _, "resolve", rest @ ..] if rest.len() >= 2 => {}
            // owner/repo, or a page of it: the name of the repository is what to search for
            [owner, repo, ..] if !matches!(parts.get(2), Some(&"resolve")) => {
                return Given::Words(format!("{owner}/{repo}"));
            }
            _ => {}
        }
    }
    if has_model_ending(link, endings) {
        Given::Link(link.to_string())
    } else {
        refused_link(link, endings)
    }
}

fn refused_link(link: &str, endings: &[&str]) -> Given {
    Given::Refused(format!(
        "{link} is not the address of a model file ({}). Give the address of one file, or the name of what you look for.",
        endings.join(", ")
    ))
}

/// The name a file given by a link or a path is kept under: the last part of it, without a query.
fn file_name_of(given: &str) -> String {
    let last = given
        .split(['?', '#'])
        .next()
        .unwrap_or(given)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(given);
    quickai_core::lookup::present::safe_file_name(last)
}

impl App {
    /// The page of an engine that runs on its own (ComfyUI), once it is here: how it is used in three steps, the three
    /// ways to get models (its Manager first, then QuickAI's list, then Hugging Face, whose box opens under them), and the
    /// files that are in its models folder.
    pub(super) fn show_workbench(&mut self, ui: &mut egui::Ui, state: ServerState) {
        let engine = self.engine_name();
        let page_label = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.as_ref())
            .map(|w| w.label.clone())
            .unwrap_or_else(|| format!("Open {engine}"));
        self.open_manager_when_ready(state);
        self.refresh_model_files(ui.ctx(), state);

        let steps = howto_steps(state, self.session.port(), &page_label);
        howto_strip(ui, &steps);
        ui.add_space(16.0);

        ui.label(
            egui::RichText::new("Get models")
                .family(theme::semibold(ui))
                .color(theme::palette().text),
        );
        ui.add_space(6.0);
        let manager = self.manager_tile(state);
        let pressed = self.ways_to_get_models(ui, manager.as_ref());
        match pressed {
            Some(Way::Manager) => self.open_manager(ui.ctx(), state),
            Some(Way::ManagerSettings) => {
                self.advanced_section_wanted = true;
                self.open_settings();
            }
            Some(Way::Recommended) => self.open_catalog(),
            Some(Way::HuggingFace) => {
                self.bench.box_open = !self.hub_panel_open();
                self.bench.scroll_to_box = self.bench.box_open;
            }
            None => {}
        }
        // a look-up that has just ended with results brings the panel up with them, once: in a short window they would
        // be under the fold, and when the panel was opened the page was not yet long enough to scroll it up
        let running = self.lookup.running();
        if std::mem::replace(&mut self.bench.lookup_was_running, running)
            && !running
            && !self.lookup.entries.is_empty()
        {
            self.bench.scroll_to_box = true;
        }
        if self.hub_panel_open() {
            ui.add_space(10.0);
            let panel = self.show_hub_panel(ui);
            // opened by its button, in a page too short to show it: it is brought up to where it can be seen
            if std::mem::take(&mut self.bench.scroll_to_box) {
                panel.scroll_to_me(Some(egui::Align::Min));
            }
        }
        ui.add_space(16.0);
        self.show_model_files(ui, &engine);
    }

    /// Whether the panel of Hugging Face is open: asked for, or with something in it (a look-up's results, words in its
    /// box, a fetch it began).
    fn hub_panel_open(&self) -> bool {
        self.bench.box_open
            || self.lookup.open
            || !self.model_input.trim().is_empty()
            || self.download.is_some()
    }

    /// The panel of Hugging Face, under the three ways: the box that takes a name or a link, the look-up of the most
    /// downloaded, what the last fetch did, and the results.
    fn show_hub_panel(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let p = theme::palette();
        let mut close = false;
        let shown = theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Hugging Face")
                        .family(theme::semibold(ui))
                        .color(p.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.download.is_none() && super::named_models::close_button(ui).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(4.0);
            self.show_get_box(ui);
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(theme::muted("Or"));
                let can_look_up = self.can_look_up() && self.download.is_none();
                if ui
                    .add_enabled(can_look_up, egui::Button::new("Look up online").small())
                    .on_hover_text(
                        "Search Hugging Face for the most downloaded GGUF models of pictures and video, with the text \
                         encoders and VAEs that go with them, each of which you can fetch with one click into the \
                         folder it belongs in. Type a name in the box first to look for that instead.",
                    )
                    .clicked()
                {
                    self.start_lookup(ui.ctx(), "");
                }
                ui.label(theme::muted(
                    "the most downloaded models of pictures and video, with their text encoders and VAEs.",
                ));
            });
            if let Some(job) = &self.download {
                ui.add_space(4.0);
                let mut cancel = false;
                ui.horizontal(|ui| {
                    ui.colored_label(p.warn, job.progress_line());
                    cancel = ui
                        .small_button("Cancel")
                        .on_hover_text("Stop the download. What was fetched so far is thrown away.")
                        .clicked();
                });
                if let Some(fraction) = job.fraction() {
                    super::widgets::progress_bar(ui, Some(fraction));
                }
                if cancel {
                    self.cancel_download();
                }
            }
            if let Some(note) = &self.bench.note {
                ui.add_space(2.0);
                let colour = if note.failed { p.bad } else { p.ok };
                ui.add(
                    egui::Label::new(egui::RichText::new(&note.text).small().color(colour)).wrap(),
                );
            }
            self.show_lookup_results(ui);
        });
        if close {
            self.bench.box_open = false;
            self.lookup.open = false;
        }
        shown.response
    }

    /// The three ways to get models, side by side (under each other in a narrow page), the Manager first and in the
    /// colour of a recommendation. What was pressed.
    fn ways_to_get_models(
        &mut self,
        ui: &mut egui::Ui,
        manager: Option<&ManagerTile>,
    ) -> Option<Way> {
        let p = theme::palette();
        let mut tiles: Vec<Tile> = Vec::new();
        if let Some(manager) = manager {
            tiles.push(Tile {
                icon: Icon::Package,
                title: "ComfyUI-Manager",
                badge: Some("recommended"),
                words: manager.words.clone(),
                button: manager.button,
                primary: manager.primary,
                enabled: manager.enabled,
                hover: manager.hover.clone(),
                best: true,
                way: manager.way,
            });
        }
        tiles.push(Tile {
            icon: Icon::Star,
            title: "QuickAI's picks",
            badge: None,
            words: "A few models tested with QuickAI, with every file each one needs, checked against its sha256."
                .to_string(),
            button: "Recommended",
            primary: false,
            enabled: self.can_recommend(),
            hover: format!(
                "Browse the models QuickAI recommends for {}, with the files that go with each, and fetch them into \
                 the right folders with one click. The list is part of the application: nothing is asked of the \
                 internet to open it.",
                self.engine_name()
            ),
            best: false,
            way: Way::Recommended,
        });
        tiles.push(Tile {
            icon: Icon::Search,
            title: "Hugging Face",
            badge: None,
            words: "Search GGUF models of pictures and video, or paste a link to one file.".to_string(),
            button: "Search or paste…",
            primary: false,
            enabled: true,
            hover: "A box for a name to search Hugging Face for, a link to a model file, or a file of this computer; \
                    and the most downloaded models of pictures and video."
                .to_string(),
            best: false,
            way: Way::HuggingFace,
        });
        let gap = 10.0;
        let room = ui.available_width();
        let narrow = room < 600.0;
        // the first is wider: it has the most to say
        let shares: Vec<f32> = tiles
            .iter()
            .map(|t| if t.best { 1.6 } else { 1.0 })
            .collect();
        let total: f32 = shares.iter().sum();
        let free = room - gap * (tiles.len() as f32 - 1.0);
        // the tiles of a row are as tall as the tallest was in the frame before
        let height_id = ui.id().with("ways_height");
        let tallest: f32 = ui.ctx().data(|d| d.get_temp(height_id)).unwrap_or(0.0);
        let mut measured: f32 = 0.0;
        let mut pressed = None;
        let mut draw = |ui: &mut egui::Ui, tile: &Tile, width: f32, min_height: f32| {
            let (fill, stroke) = if tile.best {
                (
                    p.card.lerp_to_gamma(p.accent, 0.16),
                    p.accent.gamma_multiply(0.7),
                )
            } else {
                (p.card, p.line)
            };
            let shown = egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, stroke))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(width - 26.0);
                    ui.set_min_height((min_height - 22.0).max(0.0));
                    // a tile of a row is a column of its own; in a narrow page, where they are under each other, the button
                    // is at the right of the title when the title, its chip and the button fit on one line, and the
                    // words under them
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        let role = if tile.primary {
                            Role::Primary
                        } else {
                            Role::Normal
                        };
                        let spacing = ui.spacing().item_spacing.x;
                        let line = 22.0
                            + theme::text_width(ui, tile.title, egui::TextStyle::Body)
                            + tile.badge.map_or(0.0, |b| {
                                theme::text_width(ui, b, egui::TextStyle::Small) + 12.0 + spacing
                            })
                            + theme::text_width(ui, tile.button, egui::TextStyle::Button)
                            + 2.0 * ui.spacing().button_padding.x
                            + 3.0 * spacing
                            + 8.0;
                        let beside = narrow && line <= ui.available_width();
                        ui.horizontal(|ui| {
                            icons::paint(
                                ui.painter(),
                                egui::Rect::from_min_size(
                                    ui.cursor().min + egui::vec2(0.0, 2.0),
                                    egui::vec2(16.0, 16.0),
                                ),
                                tile.icon,
                                p.accent_text,
                            );
                            ui.add_space(22.0);
                            ui.label(
                                egui::RichText::new(tile.title)
                                    .family(theme::semibold(ui))
                                    .color(p.text),
                            );
                            if beside {
                                if let Some(badge) = tile.badge {
                                    theme::chip(ui, badge, Some(Tone::Good));
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if theme::button_sized(
                                            ui,
                                            role,
                                            tile.enabled,
                                            tile.button,
                                            egui::vec2(0.0, 26.0),
                                        )
                                        .on_hover_text(&tile.hover)
                                        .clicked()
                                        {
                                            pressed = Some(tile.way);
                                        }
                                    },
                                );
                            } else if let Some(badge) = tile.badge {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        theme::chip(ui, badge, Some(Tone::Good));
                                    },
                                );
                            }
                        });
                        ui.add(egui::Label::new(theme::muted(&tile.words)).wrap());
                        if !beside
                            && theme::button_sized(
                                ui,
                                role,
                                tile.enabled,
                                tile.button,
                                egui::vec2(0.0, 28.0),
                            )
                            .on_hover_text(&tile.hover)
                            .clicked()
                        {
                            pressed = Some(tile.way);
                        }
                    });
                });
            measured = measured.max(shown.response.rect.height());
        };
        if narrow {
            for tile in &tiles {
                draw(ui, tile, room, 0.0);
                ui.add_space(gap - ui.spacing().item_spacing.y);
            }
        } else {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (tile, share) in tiles.iter().zip(&shares) {
                    draw(ui, tile, (free * share / total).floor(), tallest);
                }
            });
            if (measured - tallest).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(height_id, measured));
                ui.ctx().request_repaint();
            }
        }
        pressed
    }

    /// What the Manager's tile says and does: whether the build has it, and whether ComfyUI runs. `None` for an engine
    /// whose install has no Manager to offer.
    fn manager_tile(&self, state: ServerState) -> Option<ManagerTile> {
        let engine = self.engine_doc()?;
        let offered = acquire::install_options(engine, &self.host_facts)
            .iter()
            .any(|o| o.id == MANAGER_OPTION);
        if !offered {
            return None;
        }
        let name = self.engine_name();
        if self.build_has_option(MANAGER_OPTION) == Some(false) {
            return Some(ManagerTile {
                words: format!(
                    "This build of {name} has no ComfyUI-Manager. Install a build with Install ComfyUI-Manager ticked \
                     (Settings, Program) to get about 500 models and every custom node from inside its page."
                ),
                button: "Settings…",
                primary: false,
                enabled: true,
                hover: "Settings, Program: the builds, and the box of ComfyUI-Manager over them.".to_string(),
                way: Way::ManagerSettings,
            });
        }
        let running = state == ServerState::Running;
        let starting = state == ServerState::Starting;
        Some(ManagerTile {
            words: if running {
                format!(
                    "In {name}'s page: Manager at the top, then Model Manager. A model a workflow is missing has its \
                     own Download button there too."
                )
            } else {
                format!(
                    "About 500 models and every custom node, from inside {name}'s page: Manager, then Model Manager. \
                     Each goes in the right folder."
                )
            },
            button: if running || starting {
                "Open the Manager"
            } else {
                "Start and open the Manager"
            },
            primary: true,
            enabled: running || starting || !self.engine_missing(),
            hover: if running {
                format!("Open {name}'s page in your browser with the Manager's Model Manager open.")
            } else {
                format!(
                    "Start {name}, then open its page in your browser with the Manager's Model Manager open, as soon \
                     as it is ready."
                )
            },
            way: Way::Manager,
        })
    }

    /// Whether the build in use was made with the part `option` of the install (its words are in its launcher). `None`
    /// when that cannot be told: a program the person named, or a launcher that cannot be read.
    fn build_has_option(&self, option: &str) -> Option<bool> {
        let engine = self.engine_doc()?;
        let words = acquire::launch_args(engine, &[option]);
        if words.is_empty() {
            return None;
        }
        let (path, text) = self.bench.launcher.as_ref()?;
        if *path != self.binary {
            return None;
        }
        let text = text.as_ref()?;
        Some(words.iter().all(|w| text.contains(w.as_str())))
    }

    /// Opens ComfyUI's page with the Manager's Model Manager open: now, when it runs; once it is ready, when it does not
    /// yet (it is started).
    fn open_manager(&mut self, ctx: &egui::Context, state: ServerState) {
        match state {
            ServerState::Running => self.open_page_at(MANAGER_QUERY),
            ServerState::Starting => self.bench.open_manager = Some(std::time::Instant::now()),
            _ => {
                self.bench.open_manager = Some(std::time::Instant::now());
                self.start_pressed_at = Some(std::time::Instant::now());
                self.start_selected(ctx);
            }
        }
    }

    /// The Manager that was asked for before ComfyUI ran is opened once it does; an ask that ComfyUI did not come up for
    /// (it stopped, or failed to start) is let go.
    fn open_manager_when_ready(&mut self, state: ServerState) {
        let Some(asked) = self.bench.open_manager else {
            return;
        };
        match state {
            ServerState::Running => {
                self.bench.open_manager = None;
                self.open_page_at(MANAGER_QUERY);
            }
            ServerState::Starting => {}
            _ if asked.elapsed() > std::time::Duration::from_secs(5) => {
                self.bench.open_manager = None
            }
            _ => {}
        }
    }

    /// Opens the engine's own page in the browser, with `query` after its address (`?quickai=manager`).
    fn open_page_at(&self, query: &str) {
        if let Some(url) = self
            .engine_doc()
            .and_then(|e| e.doc.provides.web_ui.clone())
            .and_then(|w| self.web_ui_url(&w))
        {
            let _ = webbrowser::open(&page_address(&url, query));
        }
    }

    /// The files of the models folder are looked at again every few seconds while the engine runs (its page and its
    /// Manager put files there), and when the page is first drawn.
    fn refresh_model_files(&mut self, ctx: &egui::Context, state: ServerState) {
        let now = ctx.input(|i| i.time);
        let running = state == ServerState::Running;
        let stale = match self.bench.files_at {
            None => true,
            Some(at) => running && now - at > FILES_EVERY,
        };
        if stale {
            self.bench.files = scan_model_files(&self.models_dir);
            self.bench.files_at = Some(now);
        }
        // what the engine's page fetches lands in the folder without a word to the window, which draws nothing until
        // something happens in it: a frame is asked for, so that the folder is read again when nothing else draws one
        if running {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(FILES_EVERY));
        }
        // the launcher of the build in use says what it was made with: read again when it is another build
        let read = self
            .bench
            .launcher
            .as_ref()
            .is_none_or(|(path, _)| *path != self.binary);
        if read || stale {
            // a launcher is a few lines; a program the person named may be anything, and is not read whole
            let small = std::fs::metadata(&self.binary).is_ok_and(|m| m.len() < 64 * 1024);
            let text = small
                .then(|| std::fs::read_to_string(&self.binary).ok())
                .flatten();
            self.bench.launcher = Some((self.binary.clone(), text));
        }
    }

    /// The files of the models folder, by kind, each with its size, the ones still coming first.
    fn show_model_files(&mut self, ui: &mut egui::Ui, engine: &str) {
        let p = theme::palette();
        let files = &self.bench.files;
        let done: Vec<&ModelFile> = files.iter().filter(|f| !f.coming).collect();
        let coming = files.len() - done.len();
        let bytes: u64 = done.iter().map(|f| f.size).sum();
        let mut refresh = false;
        let mut open = false;
        let mut said = match done.len() {
            0 => "none yet".to_string(),
            1 => format!("1 file · {}", hardware::format_bytes(bytes)),
            n => format!("{n} files · {}", hardware::format_bytes(bytes)),
        };
        if coming > 0 {
            said.push_str(&format!(" · {coming} coming"));
        }
        theme::card(ui, |ui| {
            // the heading, how many and how big, and the two buttons: on one line when they fit, else how many under the
            // heading (a narrow page, between the sidebar and the panel)
            let padding = 2.0 * ui.spacing().button_padding.x;
            let spacing = ui.spacing().item_spacing.x;
            let wanted = theme::text_width(ui, "On this computer", egui::TextStyle::Body)
                + theme::text_width(ui, &said, egui::TextStyle::Small)
                + theme::text_width(ui, "Open folder", egui::TextStyle::Button)
                + theme::text_width(ui, "Refresh", egui::TextStyle::Button)
                + 2.0 * padding
                + 4.0 * spacing
                + 12.0;
            let one_line = ui.available_width() >= wanted;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("On this computer")
                        .family(theme::semibold(ui))
                        .color(p.text),
                )
                .on_hover_text(format!(
                    "The files {engine}'s workflows use: the weights of a model, its text encoder, its VAE, a LoRA. \
                     They are kept in the folders {engine} looks in, under the models folder, and a workflow names \
                     them by file name."
                ));
                if one_line {
                    ui.label(theme::muted(said.as_str()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    open = ui
                        .button("Open folder")
                        .on_hover_text(format!(
                            "Show the models folder ({}) in your file manager.",
                            self.models_dir.display()
                        ))
                        .clicked();
                    refresh = ui
                        .button("Refresh")
                        .on_hover_text(format!(
                            "Look in the models folder again, for a file you put there by hand. A file {engine}'s page \
                             or its Manager fetched shows by itself."
                        ))
                        .clicked();
                });
            });
            if !one_line {
                ui.label(theme::muted(said.as_str()));
            }
            if files.is_empty() {
                ui.add_space(4.0);
                ui.label(theme::muted(
                    "Nothing yet. What you get in any of the three ways above is listed here.",
                ));
                return;
            }
            ui.add_space(2.0);
            let named = |file: &ModelFile| {
                self.models
                    .iter()
                    .find(|m| m.filename == file.name.rsplit('/').next().unwrap_or(&file.name))
                    .map(|m| m.display.clone())
                    .filter(|d| *d != file.name)
            };
            for file in files.iter().take(SHOWN_FILES) {
                ui.separator();
                ui.horizontal(|ui| {
                    // the kind, at the left of a column of its own
                    ui.allocate_ui_with_layout(
                        egui::vec2(116.0, 18.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(116.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(kind_of_folder(&file.folder))
                                        .small()
                                        .color(p.faint),
                                )
                                .truncate(),
                            );
                        },
                    );
                    let size = if file.coming {
                        format!("{} so far", hardware::format_bytes(file.size))
                    } else {
                        hardware::format_bytes(file.size)
                    };
                    let size_width = theme::text_width(ui, &size, egui::TextStyle::Small) + 4.0;
                    let name_width = (ui.available_width() - size_width - 12.0).max(80.0);
                    ui.scope(|ui| {
                        ui.set_width(name_width);
                        ui.horizontal(|ui| {
                            let mut text = egui::text::LayoutJob::default();
                            text.append(
                                &file.name,
                                0.0,
                                egui::TextFormat {
                                    font_id: egui::TextStyle::Body.resolve(ui.style()),
                                    color: p.text,
                                    ..Default::default()
                                },
                            );
                            let aside = if file.coming {
                                Some("coming".to_string())
                            } else {
                                named(file)
                            };
                            if let Some(aside) = aside {
                                text.append(
                                    &format!("  {aside}"),
                                    0.0,
                                    egui::TextFormat {
                                        font_id: egui::TextStyle::Small.resolve(ui.style()),
                                        color: p.faint,
                                        ..Default::default()
                                    },
                                );
                            }
                            ui.add(egui::Label::new(text).truncate())
                                .on_hover_text(format!("{}/{}", file.folder, file.name));
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(theme::muted(size));
                    });
                });
            }
            if files.len() > SHOWN_FILES {
                ui.separator();
                ui.label(theme::muted(format!(
                    "… and {} more (Open folder shows them all)",
                    files.len() - SHOWN_FILES
                )));
            }
        });
        if refresh {
            self.rescan_models();
            self.bench.files_at = None;
        }
        if open {
            open_folder(&self.models_dir);
        }
    }

    /// The box: a link to a file, a file of this computer, or a name to search the hub for, and the
    /// folder a file goes in.
    fn show_get_box(&mut self, ui: &mut egui::Ui) {
        let busy = self.download.is_some();
        let folders = self
            .engine_doc()
            .map(quickai_core::models::folders)
            .unwrap_or_default();
        let given = read_box(&self.model_input);
        // the box takes what the row leaves it, up to 260; the folder and the button go on to a second line when they
        // do not fit beside it (a narrow page, between the sidebar and the panel, C203)
        let box_width = (ui.available_width() - 90.0).clamp(140.0, 260.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                ui.label("Get:").on_hover_text(
                    "One box, three things it can mean: the address of a model file (a link to it on \
                     Hugging Face, or any https address that ends in .gguf or .safetensors) is \
                     downloaded into the folder chosen beside it, a file of this computer is linked \
                     into it, and anything else is a name to search Hugging Face for.",
                );
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.model_input)
                        .hint_text("link to a file, or search by name...")
                        .desired_width(box_width),
                );
                if ui
                    .button("...")
                    .on_hover_text("Browse for a model file on this computer")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Model file", &["gguf", "safetensors", "sft"])
                        .pick_file()
                    {
                        self.model_input = path.display().to_string();
                    }
                }
                ui.label("Into:").on_hover_text(
                    "The folder of the models folder a file given by a link or a path is kept in. \
                     Auto chooses it from the file's name: a text encoder or a VAE where ComfyUI looks \
                     for those, anything else with the models. It does not change what the buttons of \
                     the results fetch: each of those goes where it belongs.",
                );
                egui::ComboBox::from_id_salt("into_folder")
                    .selected_text(self.bench.into.as_deref().unwrap_or("auto"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.bench.into, None, "auto");
                        for folder in &folders {
                            ui.selectable_value(&mut self.bench.into, Some(folder.clone()), folder);
                        }
                    });
                let (label, hover) = match &given {
                    Given::Link(_) => ("Download", "Download that file into the folder chosen."),
                    Given::File(_) => (
                        "Import",
                        "Link that file into the folder chosen: instant and no extra disk space, \
                         falling back to a copy only if linking is not possible.",
                    ),
                    _ => (
                        "Search",
                        "Search Hugging Face for the name typed, then for each of its words alone: \
                         all the GGUF models found are listed below.",
                    ),
                };
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let go = ui
                    .add_enabled(given != Given::Nothing, egui::Button::new(label))
                    .on_hover_text(hover)
                    .clicked();
                if go || submitted {
                    self.submit_get_box(ui.ctx());
                }
            });
        });
        if let Given::Refused(why) = &given {
            ui.label(
                egui::RichText::new(why)
                    .small()
                    .color(super::theme::palette().bad),
            );
        } else if let Given::Link(link) = &given {
            let name = file_name_of(link);
            if let Some(folder) = self.target_folder(&name) {
                ui.label(
                    egui::RichText::new(format!("{name} will be kept in the {folder} folder."))
                        .small()
                        .color(super::theme::palette().muted),
                );
            }
        }
    }

    /// The folder a file of this name goes in: the one the person chose, else the engine's guess from the
    /// name; `None` is the models folder itself.
    fn target_folder(&self, name: &str) -> Option<String> {
        self.bench.into.clone().or_else(|| {
            self.engine_doc()
                .and_then(|e| quickai_core::models::place_for(e, name))
        })
    }

    /// Does what the box was given: searches for words, downloads a link or links in a file.
    pub(super) fn submit_get_box(&mut self, ctx: &egui::Context) {
        if self.download.is_some() {
            return;
        }
        self.bench.note = None;
        let wake = {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        match read_box(&self.model_input) {
            Given::Nothing => {}
            Given::Words(words) => self.start_lookup(ctx, &words),
            Given::Refused(why) => {
                self.bench.note = Some(Note {
                    text: why,
                    failed: true,
                })
            }
            Given::Link(link) => {
                let name = file_name_of(&link);
                self.model_input.clear();
                self.fetch_by_hand(link, name, wake);
            }
            Given::File(path) => {
                let name = file_name_of(&path.display().to_string());
                self.model_input.clear();
                self.fetch_by_hand(path.display().to_string(), name, wake);
            }
        }
    }

    /// Downloads a link or links a file in, into the folder chosen or guessed.
    fn fetch_by_hand(
        &mut self,
        input: String,
        name: String,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        let folder = self.target_folder(&name);
        let into = match &folder {
            Some(folder) => self.models_dir.join(folder),
            None => self.models_dir.clone(),
        };
        self.bench.fetching = Some(match &folder {
            Some(folder) => format!("{name} (in {folder})"),
            None => name.clone(),
        });
        self.download = Some(downloader::spawn_download(input, None, into, wake));
    }

    /// Remembers what a button that fetches files is about to do, to say it when it has.
    pub(super) fn bench_fetching(&mut self, download: &Download) {
        self.bench.note = None;
        let mut named: Vec<String> = Vec::new();
        let mut folders: Vec<String> = Vec::new();
        for piece in &download.files {
            named.push(piece.store_as.clone());
            if let Some(place) = &piece.place {
                if !folders.contains(place) {
                    folders.push(place.clone());
                }
            }
        }
        let list = match named.as_slice() {
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            [] => String::new(),
        };
        self.bench.fetching = Some(if folders.is_empty() {
            list
        } else {
            format!("{list} (in {})", folders.join(", "))
        });
    }

    /// A fetch ended well: where its files are.
    pub(super) fn bench_fetched(&mut self) {
        if !self.runs_alone() {
            return;
        }
        if let Some(what) = self.bench.fetching.take() {
            self.bench.note = Some(Note {
                text: format!(
                    "Fetched {what}. {} finds it after Refresh (the r key in its page).",
                    self.engine_name()
                ),
                failed: false,
            });
        }
    }

    /// A fetch ended badly: what it said.
    /// The download the page asked for was cancelled: nothing is being fetched, and nothing failed.
    pub(super) fn bench_cancelled(&mut self) {
        self.bench.fetching = None;
    }

    pub(super) fn bench_failed(&mut self, message: &str) {
        if !self.runs_alone() {
            return;
        }
        self.bench.fetching = None;
        self.bench.note = Some(Note {
            text: format!("Download failed: {message}"),
            failed: true,
        });
    }
}

// ---- the page of an engine that runs on its own, once it is here ------------------------------------------------------

/// The part of the install that is ComfyUI-Manager (`[[acquire.install.options]] id`).
const MANAGER_OPTION: &str = "manager";

/// What the address of ComfyUI's page is given for QuickAI's script in it (`quickai_manager.js`) to open the Manager's
/// Model Manager as the page opens.
pub(super) const MANAGER_QUERY: &str = "?quickai=manager";

/// A page narrower than this has the three steps under each other, and not side by side.
const STACKED_BELOW: f32 = 560.0;

/// How many files the list shows before it says how many more there are.
const SHOWN_FILES: usize = 12;

/// How often, in seconds, the files of the models folder are read again while the engine runs.
const FILES_EVERY: f64 = 3.0;

/// The endings of the files the list shows: the formats models come in.
const LISTED_ENDINGS: &[&str] = &[
    ".gguf",
    ".safetensors",
    ".sft",
    ".ckpt",
    ".pt",
    ".pth",
    ".bin",
    ".onnx",
];

/// The folders of the models folder, in the order the list puts their files: the model first, then what goes with it.
const FOLDER_ORDER: &[&str] = &[
    "diffusion_models",
    "unet",
    "checkpoints",
    "text_encoders",
    "clip",
    "vae",
    "loras",
    "upscale_models",
    "controlnet",
    "clip_vision",
    "embeddings",
];

/// `http://127.0.0.1:8188` and `?quickai=manager` as `http://127.0.0.1:8188/?quickai=manager`.
pub(super) fn page_address(url: &str, query: &str) -> String {
    let url = url.trim_end_matches('/');
    if url.contains('?') {
        format!("{url}&{}", query.trim_start_matches('?'))
    } else {
        format!("{url}/{query}")
    }
}

/// One file of the models folder, as the list of the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ModelFile {
    /// The folder of the models folder it is in (`diffusion_models`), empty for the models folder itself.
    pub folder: String,
    /// Its name below that folder (`wan/wan2.2.safetensors`), without the `.part` of one that is still coming.
    pub name: String,
    pub size: u64,
    /// It is still being fetched: what is there is a `.part`.
    pub coming: bool,
}

/// Every model file of the models folder and of the folders in it, down to two folders below each: the ones still
/// coming first, then by kind in the order of `FOLDER_ORDER` (a folder it does not name after those), then by name.
/// A file whose ending is not a model's (ComfyUI's `put_models_here` notes, a `.yaml`) is not listed.
pub(super) fn scan_model_files(models_dir: &Path) -> Vec<ModelFile> {
    fn listed(name: &str) -> Option<(String, bool)> {
        let lower = name.to_ascii_lowercase();
        let (stem, coming) = match lower.strip_suffix(".part") {
            Some(_) => (&name[..name.len() - ".part".len()], true),
            None => (name, false),
        };
        let stem_lower = stem.to_ascii_lowercase();
        LISTED_ENDINGS
            .iter()
            .any(|e| stem_lower.ends_with(e))
            .then(|| (stem.to_string(), coming))
    }
    fn walk(dir: &Path, folder: &str, below: &str, depth: u32, into: &mut Vec<ModelFile>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            // a file linked in from elsewhere on the computer (Import) is the file it points to
            let Ok(meta) = std::fs::metadata(entry.path()).or_else(|_| entry.metadata()) else {
                continue;
            };
            let path = if below.is_empty() {
                name.clone()
            } else {
                format!("{below}/{name}")
            };
            if meta.is_dir() {
                if depth > 0 {
                    walk(&entry.path(), folder, &path, depth - 1, into);
                }
            } else if let Some((stem, coming)) = listed(&name) {
                let shown = match path.rsplit_once('/') {
                    Some((dir, _)) => format!("{dir}/{stem}"),
                    None => stem,
                };
                into.push(ModelFile {
                    folder: folder.to_string(),
                    name: shown,
                    size: meta.len(),
                    coming,
                });
            }
        }
    }
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(models_dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        match std::fs::metadata(entry.path()).or_else(|_| entry.metadata()) {
            Ok(meta) if meta.is_dir() => walk(&entry.path(), &name, "", 2, &mut files),
            Ok(meta) => {
                if let Some((stem, coming)) = listed(&name) {
                    files.push(ModelFile {
                        folder: String::new(),
                        name: stem,
                        size: meta.len(),
                        coming,
                    });
                }
            }
            Err(_) => {}
        }
    }
    let rank = |folder: &str| {
        FOLDER_ORDER
            .iter()
            .position(|f| *f == folder)
            .unwrap_or(FOLDER_ORDER.len())
    };
    files.sort_by(|a, b| {
        b.coming
            .cmp(&a.coming)
            .then(rank(&a.folder).cmp(&rank(&b.folder)))
            .then(a.folder.cmp(&b.folder))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    // a file that is there whole is not also coming (a `.part` left over beside it)
    let whole: Vec<(String, String)> = files
        .iter()
        .filter(|f| !f.coming)
        .map(|f| (f.folder.clone(), f.name.clone()))
        .collect();
    files.retain(|f| !f.coming || !whole.contains(&(f.folder.clone(), f.name.clone())));
    files
}

/// What a folder of the models folder holds, in a person's words: `vae` is `VAE`, `diffusion_models` is
/// `Diffusion model`; a folder this does not know is named as it is, with spaces.
pub(super) fn kind_of_folder(folder: &str) -> String {
    let known = match folder {
        "diffusion_models" | "unet" => "Diffusion model",
        "checkpoints" => "Checkpoint",
        "text_encoders" | "clip" => "Text encoder",
        "vae" => "VAE",
        "loras" => "LoRA",
        "upscale_models" => "Upscaler",
        "controlnet" => "ControlNet",
        "clip_vision" => "CLIP vision",
        "embeddings" => "Embedding",
        "" => "Model",
        _ => "",
    };
    if known.is_empty() {
        super::install_page::sentence_case(&folder.replace('_', " "))
    } else {
        known.to_string()
    }
}

/// Where one of the three steps of using the engine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StepLook {
    Done,
    Now,
    Next,
}

/// The three steps of using the engine, as its state has them: start it, open its page, load a workflow.
pub(super) fn howto_steps(
    state: ServerState,
    port: Option<u16>,
    page_label: &str,
) -> [(String, String, StepLook); 3] {
    let open = (
        page_label.to_string(),
        "Its page opens in your browser".to_string(),
    );
    let load = (
        "Load a workflow".to_string(),
        "Templates, or one you have".to_string(),
    );
    match state {
        ServerState::Running => [
            (
                "Started".to_string(),
                port.map_or("It runs".to_string(), |p| format!("On port {p}")),
                StepLook::Done,
            ),
            (open.0, "The button at the top".to_string(), StepLook::Now),
            (load.0, load.1, StepLook::Next),
        ],
        ServerState::Starting => [
            (
                "Starting…".to_string(),
                "The first start can take a minute".to_string(),
                StepLook::Now,
            ),
            (open.0, open.1, StepLook::Next),
            (load.0, load.1, StepLook::Next),
        ],
        _ => [
            (
                "Start".to_string(),
                "It runs on this computer".to_string(),
                StepLook::Now,
            ),
            (open.0, open.1, StepLook::Next),
            (load.0, load.1, StepLook::Next),
        ],
    }
}

/// The three steps in a box, each with its number (a tick once done), the one to do now in green: side by side, or, in a
/// page too narrow for three columns of words, under each other with the words after the name.
fn howto_strip(ui: &mut egui::Ui, steps: &[(String, String, StepLook); 3]) {
    let p = theme::palette();
    let frame = egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(0, 10));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let stacked = ui.available_width() < STACKED_BELOW;
        let width = if stacked {
            ui.available_width()
        } else {
            ui.available_width() / 3.0
        };
        let room = (width - 54.0).max(20.0);
        let title_font = egui::FontId::new(13.5, theme::semibold(ui));
        let small = egui::TextStyle::Small.resolve(ui.style());
        // each step's words, laid out in the room it has: side by side the strip is as tall as the tallest, under each other
        // a step is a line, the name and then what it is
        type Laid = (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>);
        let laid: Vec<Laid> = steps
            .iter()
            .map(|(title, words, _)| {
                let title = ui
                    .painter()
                    .layout(title.clone(), title_font.clone(), p.text, room);
                let words_room = if stacked {
                    (room - title.size().x - 10.0).max(20.0)
                } else {
                    room
                };
                let words = ui
                    .painter()
                    .layout(words.clone(), small.clone(), p.muted, words_room);
                (title, words)
            })
            .collect();
        let height_of = |(t, w): &Laid| {
            if stacked {
                t.size().y.max(w.size().y).max(22.0)
            } else {
                t.size().y + w.size().y + 1.0
            }
        };
        let tallest = laid.iter().map(height_of).fold(24.0_f32, f32::max);
        let draw = |ui: &mut egui::Ui, i: usize, look: StepLook, (title, words): Laid| {
            let height = if stacked {
                height_of(&(title.clone(), words.clone()))
            } else {
                tallest
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            if i > 0 && !stacked {
                ui.painter().vline(
                    rect.left(),
                    (rect.top() - 2.0)..=(rect.bottom() + 2.0),
                    egui::Stroke::new(1.0, p.line),
                );
            }
            let centre = egui::pos2(rect.left() + 26.0, rect.top() + 11.0);
            let (fill, number) = match look {
                StepLook::Done => (p.accent_soft, p.accent_text),
                StepLook::Now => (p.accent, p.on_accent),
                StepLook::Next => (p.raised, p.text),
            };
            ui.painter().circle_filled(centre, 11.0, fill);
            if look == StepLook::Done {
                icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(centre, egui::vec2(12.0, 12.0)),
                    Icon::Check,
                    number,
                );
            } else {
                ui.painter().text(
                    centre,
                    egui::Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::new(12.0, theme::semibold(ui)),
                    number,
                );
            }
            let left = rect.left() + 46.0;
            let title_top = rect.top() + (22.0 - title.size().y).max(0.0) / 2.0;
            let words_at = if stacked {
                egui::pos2(left + title.size().x + 10.0, title_top + 1.0)
            } else {
                egui::pos2(left, rect.top() + title.size().y + 1.0)
            };
            ui.painter().galley(
                egui::pos2(left, if stacked { title_top } else { rect.top() }),
                title,
                p.text,
            );
            ui.painter().galley(words_at, words, p.muted);
        };
        if stacked {
            ui.spacing_mut().item_spacing.y = 6.0;
            for (i, ((_, _, look), laid)) in steps.iter().zip(laid).enumerate() {
                draw(ui, i, *look, laid);
            }
        } else {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, ((_, _, look), laid)) in steps.iter().zip(laid).enumerate() {
                    draw(ui, i, *look, laid);
                }
            });
        }
    });
}

/// Which of the ways to get models was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    /// Open ComfyUI's page with the Manager's Model Manager open (started first when it is not running).
    Manager,
    /// The build has no Manager: Settings, Program, where a build with it is installed.
    ManagerSettings,
    /// QuickAI's list.
    Recommended,
    /// The panel of Hugging Face.
    HuggingFace,
}

/// One of the ways to get models, as its tile says it.
struct Tile {
    icon: Icon,
    title: &'static str,
    badge: Option<&'static str>,
    words: String,
    button: &'static str,
    primary: bool,
    enabled: bool,
    hover: String,
    /// The recommended way: wider, and in the colour of a recommendation.
    best: bool,
    way: Way,
}

/// What the Manager's tile says and does.
struct ManagerTile {
    words: String,
    button: &'static str,
    primary: bool,
    enabled: bool,
    hover: String,
    way: Way,
}

/// Shows a folder in the file manager of the system. Best effort: nothing is said when there is none.
pub(super) fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    if let Ok(mut child) = std::process::Command::new(program).arg(path).spawn() {
        // the file manager is not waited for here; its end is, so that it does not stay a zombie
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    #[cfg(unix)]
    use crate::events::ServerState;
    use crate::settings::Settings;
    #[cfg(unix)]
    use crate::testkit::procs::write_script;
    use crate::testkit::scratch;
    #[cfg(unix)]
    use crate::testkit::wait_until;
    use std::path::PathBuf;
    use std::time::Duration;

    const GB: u64 = 1024 * 1024 * 1024;

    /// An application on settings of its own in `dir`, with `engine` chosen, and no real home in sight.
    fn app_for(dir: &Path, engine: &str) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        )
    }

    /// The texts a frame of `show` paints, in a window tall enough for the whole page of ComfyUI.
    fn painted(ctx: &egui::Context, show: impl FnMut(&mut egui::Ui)) -> Vec<String> {
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => into.push(t.galley.text().to_string()),
                _ => {}
            }
        }
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 1800.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, show);
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut texts);
        }
        texts
    }

    /// How many offers of the results are marked installed, in their row or in its ▾: the rule the rows are drawn by
    /// (`lookup_view`), what is there at its size in its folder, for an engine that keeps its files in folders of their
    /// own; llama.cpp's are never marked. The page is drawn too, so that what it draws is drawn from the same results.
    fn marked(app: &mut App) -> usize {
        let ctx = egui::Context::default();
        for _ in 0..2 {
            painted(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| app.show_workbench(ui, ServerState::Idle));
            });
        }
        if !app.runs_alone() {
            return 0;
        }
        app.lookup
            .entries
            .iter()
            .flat_map(|e| &e.offers)
            .filter(|o| super::super::catalog_view::installed(o, &app.catalog.have, &[]))
            .count()
    }

    fn put(dir: &Path, folder: &str, name: &str, bytes: usize) {
        let folder = dir.join(folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(name), vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn p3_f60_an_engine_that_runs_on_its_own_keeps_its_models_in_its_own_folders() {
        let dir = scratch("f60");
        let app = app_for(&dir, "comfyui");
        assert!(app.runs_alone(), "ComfyUI is given no model to start");
        assert!(!app_for(&dir, "llamacpp").runs_alone());
        assert!(!app_for(&dir, "ollama").runs_alone());

        // its models folder is its own, below the application's data, and not the one llama.cpp keeps
        let models = dir.join("data/comfyui/models");
        assert_eq!(app.models_dir, models);
        assert_eq!(app.default_models_dir(), models);
        assert_ne!(app.models_dir, crate::paths::user_models_dir());

        // the places its models are found are the folders of that folder
        let sources = app.model_sources();
        let paths: Vec<PathBuf> = sources.iter().map(|s| PathBuf::from(&s.path)).collect();
        assert_eq!(
            paths,
            [models.join("diffusion_models"), models.join("checkpoints")]
        );
        assert_eq!(sources[0].ext, [".gguf", ".safetensors"]);

        // what is in them is the list: a diffusion model and a checkpoint, and not a text encoder
        put(&models, "diffusion_models", "ltx.gguf", 10);
        put(&models, "checkpoints", "sdxl.safetensors", 20);
        put(&models, "text_encoders", "t5.gguf", 30);
        put(&models, "vae", "vae.safetensors", 40);
        let listed: Vec<String> = app.scan_models().into_iter().map(|m| m.filename).collect();
        assert_eq!(listed, ["ltx.gguf", "sdxl.safetensors"]);
        // llama.cpp's window still lists the .gguf files of its one folder
        let llama = app_for(&dir, "llamacpp");
        assert!(llama.model_sources().is_empty());
    }

    #[test]
    fn the_page_of_comfyui_is_opened_with_the_manager_and_the_folders_are_named_in_words() {
        assert_eq!(
            page_address("http://127.0.0.1:8188", MANAGER_QUERY),
            "http://127.0.0.1:8188/?quickai=manager"
        );
        assert_eq!(
            page_address("http://127.0.0.1:8188/", MANAGER_QUERY),
            "http://127.0.0.1:8188/?quickai=manager"
        );
        assert_eq!(
            page_address("http://127.0.0.1:8188/?token=a", MANAGER_QUERY),
            "http://127.0.0.1:8188/?token=a&quickai=manager"
        );
        assert_eq!(kind_of_folder("diffusion_models"), "Diffusion model");
        assert_eq!(kind_of_folder("vae"), "VAE");
        assert_eq!(kind_of_folder("loras"), "LoRA");
        assert_eq!(kind_of_folder("style_models"), "Style models");
        assert_eq!(kind_of_folder(""), "Model");
    }

    #[test]
    fn the_three_steps_follow_the_state_of_comfyui() {
        let [start, open, load] = howto_steps(ServerState::Idle, None, "Open ComfyUI");
        assert_eq!((start.0.as_str(), start.2), ("Start", StepLook::Now));
        assert_eq!((open.0.as_str(), open.2), ("Open ComfyUI", StepLook::Next));
        assert_eq!(load.2, StepLook::Next);
        let [start, open, _] = howto_steps(ServerState::Running, Some(8188), "Open ComfyUI");
        assert_eq!(
            (start.0.as_str(), start.1.as_str(), start.2),
            ("Started", "On port 8188", StepLook::Done)
        );
        assert_eq!(open.2, StepLook::Now);
        let [start, _, _] = howto_steps(ServerState::Starting, None, "Open ComfyUI");
        assert_eq!((start.0.as_str(), start.2), ("Starting…", StepLook::Now));
    }

    #[test]
    fn the_files_of_the_models_folder_are_listed_two_folders_down_and_a_whole_file_is_not_also_coming(
    ) {
        let dir = scratch("model-files");
        put(&dir, "checkpoints", "sdxl.safetensors", 20);
        put(&dir, "diffusion_models/wan", "wan.gguf", 10);
        put(&dir, "diffusion_models/a/b/c", "too-deep.gguf", 10);
        put(&dir, "loras", "style.safetensors", 5);
        put(&dir, "loras", "style.safetensors.part", 2);
        put(&dir, "vae", "notes.txt", 5);
        put(&dir, "vae", ".hidden.safetensors", 5);
        put(&dir, "", "loose.gguf", 7);
        let files = scan_model_files(&dir);
        let seen: Vec<(String, String, bool)> = files
            .iter()
            .map(|f| (f.folder.clone(), f.name.clone(), f.coming))
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "diffusion_models".to_string(),
                    "wan/wan.gguf".to_string(),
                    false
                ),
                (
                    "checkpoints".to_string(),
                    "sdxl.safetensors".to_string(),
                    false
                ),
                ("loras".to_string(), "style.safetensors".to_string(), false),
                (String::new(), "loose.gguf".to_string(), false),
            ]
        );
        assert_eq!(files[1].size, 20);
        assert!(scan_model_files(&dir.join("nowhere")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn while_the_engine_runs_a_file_its_page_fetched_is_listed_without_anything_happening_in_the_window(
    ) {
        let dir = scratch("files-while-running");
        let mut app = app_for(&dir, "comfyui");
        let models = app.models_dir.clone();
        let ctx = egui::Context::default();
        // a frame at `time` seconds, and how long the window may then wait before it draws the next one
        let frame = |app: &mut App, time: f64, state: ServerState| {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 1800.0),
                )),
                time: Some(time),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.show_workbench(ui, state));
            });
            output.textures_delta.clear();
            output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
        };
        for t in 0..3 {
            frame(&mut app, f64::from(t) * 0.1, ServerState::Idle);
        }
        // stopped, nothing comes into the folder on its own: no frame is asked for
        assert!(
            frame(&mut app, 0.4, ServerState::Idle) > std::time::Duration::from_secs(60),
            "a stopped engine's page draws nothing by itself"
        );
        // running, ComfyUI's page (its Manager, a workflow's Download) puts files there without a word to the window
        let wait = frame(&mut app, 0.5, ServerState::Running);
        assert!(
            wait <= std::time::Duration::from_secs_f64(FILES_EVERY),
            "{wait:?}"
        );
        assert!(app.bench.files.is_empty());
        put(&models, "diffusion_models", "ltxv-tiny.gguf", 40);
        frame(&mut app, 1.0, ServerState::Running);
        assert!(
            app.bench.files.is_empty(),
            "read again only every few seconds"
        );
        frame(&mut app, 0.5 + FILES_EVERY + 0.1, ServerState::Running);
        let names: Vec<&str> = app.bench.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["ltxv-tiny.gguf"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_file_linked_into_the_models_folder_is_listed_at_the_size_of_the_file_it_is() {
        let dir = scratch("model-files-link");
        let elsewhere = scratch("model-files-elsewhere");
        let real = elsewhere.join("mine.safetensors");
        std::fs::write(&real, vec![b'x'; 33]).unwrap();
        std::fs::create_dir_all(dir.join("diffusion_models")).unwrap();
        std::os::unix::fs::symlink(&real, dir.join("diffusion_models/mine.safetensors")).unwrap();
        let files = scan_model_files(&dir);
        assert_eq!(files.len(), 1);
        assert_eq!(
            (files[0].name.as_str(), files[0].size),
            ("mine.safetensors", 33)
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    #[test]
    fn p3_f61_the_models_area_says_what_is_installed_and_how_to_get_more() {
        let dir = scratch("f61");
        let mut app = app_for(&dir, "comfyui");
        let ctx = egui::Context::default();
        // a page as tall as it is: what is under the fold is drawn too
        let texts = |app: &mut App, ctx: &egui::Context| {
            let raw = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 1400.0),
                )),
                ..Default::default()
            };
            let mut shown = Vec::new();
            for _ in 0..2 {
                let mut output = ctx.run_ui(raw(), |ui| {
                    egui::CentralPanel::default()
                        .show(ui, |ui| app.show_workbench(ui, ServerState::Idle));
                });
                output.textures_delta.clear();
                fn walk(shape: &egui::epaint::Shape, into: &mut Vec<String>) {
                    match shape {
                        egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                        egui::epaint::Shape::Text(t) => into.push(t.galley.text().to_string()),
                        _ => {}
                    }
                }
                shown.clear();
                for clipped in &output.shapes {
                    walk(&clipped.shape, &mut shown);
                }
            }
            shown.join("\n")
        };
        let shown = texts(&mut app, &ctx);
        // how it is used, in three steps, the first to do now
        for step in ["Start", "Open ComfyUI", "Load a workflow"] {
            assert!(shown.contains(step), "`{step}`: {shown}");
        }
        // the three ways to get models, the Manager first and recommended
        for way in [
            "Get models",
            "ComfyUI-Manager",
            "recommended",
            "Start and open the Manager",
            "QuickAI's picks",
            "Recommended",
            "Hugging Face",
            "Search or paste…",
        ] {
            assert!(shown.contains(way), "`{way}`: {shown}");
        }
        let manager = shown.find("ComfyUI-Manager").unwrap();
        assert!(
            manager < shown.find("QuickAI's picks").unwrap()
                && manager < shown.find("Hugging Face").unwrap()
        );
        // the list of the files, empty
        assert!(
            shown.contains("On this computer") && shown.contains("none yet"),
            "{shown}"
        );
        assert!(
            shown.contains("Refresh") && shown.contains("Open folder"),
            "{shown}"
        );
        // the box of Hugging Face is under its button, not on the page until it is asked for; then it has the box, the
        // folder, and the look-up of the most downloaded
        assert!(
            !shown.contains("Get:") && !shown.contains("Into:"),
            "{shown}"
        );
        app.bench.box_open = true;
        let shown = texts(&mut app, &ctx);
        assert!(
            shown.contains("Look up online") && shown.contains("Get:") && shown.contains("Into:"),
            "{shown}"
        );
        app.bench.box_open = false;

        // the files of every folder, each with its kind and size, the model first; one still coming is said so
        let models = app.models_dir.clone();
        for i in 0..7 {
            put(&models, "diffusion_models", &format!("m{i}.gguf"), 100 + i);
        }
        put(&models, "vae", "ae.safetensors", 50);
        put(&models, "text_encoders", "t5.gguf.part", 30);
        put(&models, "vae", "put_vae_here", 0);
        app.rescan_models();
        app.bench.files_at = None;
        let shown = texts(&mut app, &ctx);
        assert!(
            shown.contains("8 files") && shown.contains("1 coming"),
            "{shown}"
        );
        assert!(
            shown.contains("m0.gguf")
                && shown.contains("m6.gguf")
                && shown.contains("ae.safetensors"),
            "{shown}"
        );
        assert!(
            shown.contains("Diffusion model")
                && shown.contains("VAE")
                && shown.contains("Text encoder"),
            "{shown}"
        );
        assert!(
            shown.contains("t5.gguf") && shown.contains("coming"),
            "{shown}"
        );
        assert!(!shown.contains("put_vae_here"), "{shown}");
        assert!(
            shown.find("t5.gguf").unwrap() < shown.find("m0.gguf").unwrap(),
            "what is still coming is first: {shown}"
        );
        for i in 7..14 {
            put(&models, "loras", &format!("l{i}.safetensors"), 10);
        }
        app.bench.files_at = None;
        let shown = texts(&mut app, &ctx);
        assert!(shown.contains("… and 4 more"), "{shown}");

        // llama.cpp's window is the list of models on the left, under Add a model, and with none yet, the box of Add a
        // model on the page (its program is there, a file being all the window looks for: the page of an engine that is not
        // installed is the one that installs it, WP4.31)
        let mut llama = app_for(&dir, "llamacpp");
        llama.binary = dir.join("llama-server");
        std::fs::write(&llama.binary, "").unwrap();
        // and none yet is none: its folders are the test's, not the models folder of the home and the one of the models
        // that came with the application
        llama.models_dir = dir.join("models");
        llama.bundled_models_dir = dir.join("bundled");
        llama.rescan_models();
        assert!(llama.models.is_empty(), "{:?}", llama.models_dir);
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 560.0),
            )),
            ..Default::default()
        };
        let mut shown = String::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(raw(), |ui| llama.show_window(ui));
            output.textures_delta.clear();
            shown = format!("{:?}", output.shapes);
        }
        assert!(
            shown.contains("MODELS")
                && shown.contains("Add your first model")
                && shown.contains("Search, or paste a link or file path"),
            "{shown}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn p3_f62_start_runs_the_engine_with_no_model_and_the_traffic_proxy_stays_out_of_its_way() {
        let dir = scratch("f62");
        let mut app = app_for(&dir, "comfyui");
        // a launcher that writes down what it is started with
        let log = dir.join("args");
        let script = write_script(
            &dir,
            "comfyui",
            &format!("echo \"$@\" > '{}'\nsleep 30 &\nwait", log.display()),
        );
        app.set_bin_override(Some(script.clone()));
        app.binary = script;
        assert!(!app.engine_missing());
        assert!(
            app.models.is_empty(),
            "no model, and Start is not waiting for one"
        );

        // a port of its own: a ComfyUI that already answers on the default one (the person's, while they
        // work) would be attached to and nothing would be started
        let port = crate::testkit::free_port();
        app.values.set("port", port);

        // the person asked for live traffic; there is no conversation, so there is no proxy
        app.enable_traffic_proxy = true;
        assert!(
            !app.traffic_proxy_on(),
            "a proxy would only be a hop in front of the page's connections"
        );
        assert!(
            !app_for(&dir, "llamacpp").traffic_proxy_on(),
            "off by default for the others"
        );
        let mut llama = app_for(&dir, "llamacpp");
        llama.enable_traffic_proxy = true;
        assert!(
            llama.traffic_proxy_on(),
            "it is still there for the engines that have a conversation"
        );

        let ctx = egui::Context::default();
        app.start_selected(&ctx);
        wait_until(Duration::from_secs(10), "the launcher to run", || {
            log.exists()
        });
        let args = std::fs::read_to_string(&log).unwrap();
        assert!(
            args.contains(&format!("--models-directory {}", app.models_dir.display()))
                && args.contains(&format!("--port {port}"))
                && args.contains("--offline"),
            "{args}"
        );
        assert!(!args.contains("--model "), "no model was passed: {args}");
        assert_eq!(app.session.model_name(), None);
        // stopped, nothing is left
        app.session.stop();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline && app.session.is_active() {
            app.session.poll(&mut |_| {}, &mut |_| {});
            std::thread::sleep(Duration::from_millis(15));
        }
        assert!(!app.session.is_active());
        assert!(matches!(
            app.session.state(),
            ServerState::Idle | ServerState::Crashed
        ));
    }

    #[test]
    fn p3_f83_a_choice_that_is_wrong_for_the_others_says_so_under_the_row_and_in_settings() {
        // ComfyUI refuses `--cpu` with a memory option (the document says so in a rule of the Memory field): a person who
        // turned the GPU off and chose low memory is told, where the choice is, instead of finding ComfyUI crashed with a
        // sentence about its extra arguments
        let dir = scratch("f83");
        let mut app = app_for(&dir, "comfyui");
        let ctx = egui::Context::default();
        let controls = |app: &mut App, ctx: &egui::Context| {
            painted(ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.show_controls(ui));
            })
            .join("\n")
        };
        let shown = controls(&mut app, &ctx);
        assert!(
            shown.contains("Memory:") && !shown.contains("graphics card: choose auto"),
            "nothing is wrong with the defaults: {shown}"
        );
        app.values.set("gpu", "off");
        app.values.set("memory", "low");
        let shown = controls(&mut app, &ctx);
        assert!(
            shown.contains("Memory is about the graphics card: choose auto, or turn the GPU on."),
            "the rule of the Memory field is said: {shown}"
        );
        // and the panel at the right of the page (C203) says it under the fields: nothing else on the page would
        app.change_open = false;
        let line = painted(&ctx, |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.show_inspector(ui));
        })
        .join("\n");
        assert!(
            line.contains("Memory is about the graphics card: choose auto, or turn the GPU on."),
            "{line}"
        );
        // the same two choices, with the memory left to the program, are fine
        app.values.set("memory", "auto");
        let shown = controls(&mut app, &ctx);
        assert!(!shown.contains("graphics card: choose auto"), "{shown}");
        let line = painted(&ctx, |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.show_inspector(ui));
        })
        .join("\n");
        assert!(!line.contains("graphics card: choose auto"), "{line}");
        // llama.cpp's row has no choice and says nothing of the kind
        let mut llama = app_for(&dir, "llamacpp");
        let shown = controls(&mut llama, &ctx);
        assert!(
            shown.contains("Port:") && !shown.contains("graphics card"),
            "{shown}"
        );
    }

    #[test]
    fn p3_f84_closing_the_settings_with_the_icon_keeps_what_was_changed_in_them() {
        let dir = scratch("f84");
        let mut app = app_for(&dir, "comfyui");
        let file = dir.join("settings.json");
        let saved = |file: &std::path::Path| -> serde_json::Value {
            std::fs::read_to_string(file)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or(serde_json::Value::Null)
        };
        // open, change a setting the way the window does, and close with the same icon
        app.toggle_settings();
        assert!(app.show_settings);
        app.values.set("python", "/usr/bin/python3.12");
        assert_eq!(
            saved(&file)["values"]["comfyui.python"],
            serde_json::Value::Null,
            "nothing is written by opening them or by typing"
        );
        app.toggle_settings();
        assert!(!app.show_settings);
        assert_eq!(
            saved(&file)["values"]["comfyui.python"],
            "/usr/bin/python3.12",
            "closed by the icon, they are kept"
        );
    }

    #[test]
    fn p3_f63_the_models_folder_of_comfyui_is_kept_apart_from_llamacpps_in_the_settings_file() {
        let dir = scratch("f63");
        let mut app = app_for(&dir, "comfyui");
        let elsewhere = dir.join("elsewhere");
        app.models_dir = elsewhere.clone();
        app.models_dir_override = Some(elsewhere.clone());
        app.save_settings();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        // under the engine's own values, and not in the key llama.cpp's folder has always had
        assert_eq!(
            saved["values"]["comfyui.models_dir"],
            elsewhere.display().to_string(),
            "{saved}"
        );
        assert!(
            saved["models_dir_override"].is_null(),
            "llama.cpp's key was not touched: {saved}"
        );
        assert_eq!(saved["values"]["app.engine"], "comfyui", "{saved}");

        // and it is read back, and what the planner gives the engine is that folder
        let mut again = App::build_in(
            serde_json::from_value(saved).unwrap_or_default(),
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        assert_eq!(again.models_dir, elsewhere);
        let args = again
            .run_selection(None)
            .values
            .get("comfyui.models_dir")
            .cloned();
        assert_eq!(
            args.as_ref().and_then(|v| v.as_str()).map(str::to_string),
            Some(elsewhere.display().to_string())
        );
    }

    #[test]
    fn p3_f64_the_recommended_list_of_comfyui_fetches_each_file_into_its_folder_and_marks_what_is_there(
    ) {
        use quickai_core::lookup::present::Fetch;
        let dir = scratch("f64");
        let mut app = app_for(&dir, "comfyui");
        assert!(app.can_recommend(), "ComfyUI has models in the list");
        app.open_catalog();
        let entries = app.catalog.entries.clone();
        assert_eq!(
            entries.len(),
            1,
            "{:?}",
            entries.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        let entry = &entries[0];
        assert_eq!(entry.model_type.as_deref(), Some("video"));
        assert!(entry
            .card
            .as_ref()
            .is_some_and(|c| c.start && !c.license.is_empty()));

        // the first row is the bundled download: the weights, the text encoder and the VAE, each in its folder
        let bundled = &entry.offers[0];
        assert_eq!(bundled.bundle.as_deref(), Some("video"));
        let Fetch::Files(download) = &bundled.fetch else {
            panic!("files");
        };
        let places: Vec<(&str, Option<&str>)> = download
            .files
            .iter()
            .map(|p| (p.kind.as_deref().unwrap_or("weights"), p.place.as_deref()))
            .collect();
        assert_eq!(
            places,
            [
                ("weights", Some("diffusion_models")),
                ("text_encoder", Some("text_encoders")),
                ("vae", Some("vae")),
            ]
        );
        assert!(
            download.files.iter().all(|p| p.sha256.is_some()),
            "every file is checked"
        );

        // the example workflow of the docs names the files this button fetches, as they are called on disk
        let workflow: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../docs/examples/workflows/ltx-video-2b-t2v.json"
        ))
        .unwrap();
        let stored: Vec<&str> = download.files.iter().map(|p| p.store_as.as_str()).collect();
        assert_eq!(workflow["1"]["inputs"]["unet_name"], stored[0]);
        assert_eq!(workflow["2"]["inputs"]["clip_name"], stored[1]);
        assert_eq!(workflow["3"]["inputs"]["vae_name"], stored[2]);
        // and decodes in tiles that fit a card of 12 GiB with either quantization: at 256 and 32 the Q8_0
        // weights ran out of memory in the decode (engine-facts F13)
        assert!(
            workflow["12"]["inputs"]["tile_size"].as_u64().unwrap() <= 192
                && workflow["12"]["inputs"]["temporal_size"].as_u64().unwrap() <= 16,
            "{}",
            workflow["12"]
        );
        // the second row is the weights alone, and says that is all it fetches
        assert!(entry.offers.iter().any(|o| o.bundle.is_none()));
        let first_alone = entry.offers.iter().find(|o| o.bundle.is_none()).unwrap();
        let Fetch::Files(alone) = &first_alone.fetch else {
            panic!("files");
        };
        assert_eq!(alone.files.len(), 1);
        assert_eq!(alone.files[0].place.as_deref(), Some("diffusion_models"));

        // nothing is marked until every file of a button is there at the size the list says
        let installed = |app: &App, offer: &quickai_core::lookup::present::Offer| {
            super::super::catalog_view::installed(offer, &app.catalog.have, &[])
        };
        assert!(!installed(&app, bundled));
        let models = app.models_dir.clone();
        let sparse = |folder: &str, name: &str, size: u64| {
            std::fs::create_dir_all(models.join(folder)).unwrap();
            let file = std::fs::File::create(models.join(folder).join(name)).unwrap();
            file.set_len(size).unwrap();
        };
        for piece in &download.files {
            sparse(
                piece.place.as_deref().unwrap(),
                &piece.store_as,
                piece.size.unwrap(),
            );
        }
        app.refresh_catalog_have();
        assert!(
            installed(&app, bundled),
            "{:?}",
            app.catalog.have.keys().collect::<Vec<_>>()
        );
        assert!(
            installed(&app, first_alone),
            "the weights alone are there too"
        );
        // a file of the right name in the wrong folder is not the file
        std::fs::remove_file(models.join("vae").join(&download.files[2].store_as)).unwrap();
        sparse(
            "diffusion_models",
            &download.files[2].store_as,
            download.files[2].size.unwrap(),
        );
        app.refresh_catalog_have();
        assert!(!installed(&app, bundled));

        // and the window draws it (the first frames of a window only measure it)
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        for _ in 0..3 {
            texts = painted(&ctx, |ui| app.show_catalog_page(ui));
        }
        // (the page's title is in the bar over it, C203)
        assert!(
            texts.iter().any(|t| t.contains("What do you want to do")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("LTX-Video 2B distilled")),
            "{texts:?}"
        );
        // the one thing it does is the one button of the first page, chosen
        assert!(texts.iter().any(|t| t == "Make videos"), "{texts:?}");
        assert_eq!(app.catalog.chosen, ["make-videos"]);
        // and in the list, the row of the download with what goes with it
        app.catalog.page = super::super::catalog_view::Page::List;
        for _ in 0..3 {
            texts = painted(&ctx, |ui| app.show_catalog_page(ui));
        }
        // the row of the model, whose Get fetches it with its text encoder and VAE (what its ▾ says), at the size of all three
        assert!(
            texts.iter().any(|t| t == "LTX-Video 2B distilled")
                && texts.iter().any(|t| t == "Get")
                && texts.iter().any(|t| t == "6.6 GB"),
            "the row of the bundled download is there: {texts:?}"
        );
    }

    // ---- getting models from the window (work package 4.13, group Q) -----------------------------------

    /// ComfyUI's window with its look-up pointed at `site` by a document of the person's, which patches the
    /// look-up of the application by its `id` (the rest of what that says stays), and no real home in sight.
    fn comfy_with_hub(dir: &Path, site: &str) -> App {
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(cfg.join("engines")).unwrap();
        std::fs::write(
            cfg.join("engines/comfyui.toml"),
            format!(
                "schema = 1\nkind = \"engine\"\nid = \"comfyui\"\n\n[[models.lookup]]\nid = \"hub\"\nsite = \"{site}\"\n"
            ),
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.set_chosen_engine("comfyui");
        let registry = build_registry(&ConfigDirs {
            user: Some(cfg),
            ..ConfigDirs::default()
        });
        assert!(
            registry.diagnostics.iter().all(|d| !d.is_error()),
            "{:#?}",
            registry.diagnostics
        );
        App::build_in(
            settings,
            registry,
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        )
    }

    fn until_looked_up(app: &mut App) {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while app.lookup.running() {
            assert!(
                std::time::Instant::now() < deadline,
                "the look-up did not end"
            );
            app.poll_lookup();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn until_downloaded(app: &mut App) {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while app.download.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "the download did not end"
            );
            app.poll_download();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The fake hub with a small video model that holds its VAE, a text encoder, and a picture model.
    fn comfy_world(
        name: &str,
    ) -> (
        crate::testkit::Scratch,
        crate::testkit::fake_hub::FakeHub,
        App,
    ) {
        let dir = scratch(name);
        let hub = crate::testkit::fake_hub::FakeHub::start();
        hub.add_checked_repo(
            "me/tiny-video-GGUF",
            Some("text-to-video"),
            &["gguf"],
            &[
                ("tiny-video-Q4_K_M.gguf", 3000),
                ("tiny-video-Q8_0.gguf", 5000),
                ("VAE/tiny_vae.safetensors", 2000),
            ],
        );
        hub.add_checked_repo(
            "me/tiny-picture-GGUF",
            Some("text-to-image"),
            &["gguf"],
            &[("tiny-picture-Q8_0.gguf", 1500)],
        );
        hub.add_checked_repo(
            "me/tiny-encoder-GGUF",
            None,
            &["gguf"],
            &[
                ("tiny-encoder-Q5_K_M.gguf", 4000),
                ("tiny-encoder-Q8_0.gguf", 6000),
            ],
        );
        hub.only(&[
            "me/tiny-video-GGUF",
            "me/tiny-picture-GGUF",
            "me/tiny-encoder-GGUF",
        ]);
        let app = comfy_with_hub(&dir, &hub.base());
        (dir, hub, app)
    }

    /// The button of the first result called `name` whose label starts with `label` and which is in the
    /// row of `only` (a kind of file by itself) or, none, the weights.
    fn button(
        app: &App,
        name: &str,
        label: &str,
        only: Option<&str>,
    ) -> quickai_core::lookup::present::Fetch {
        let entry = app
            .lookup
            .entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("no result {name}"));
        entry
            .offers
            .iter()
            .find(|o| {
                o.label.starts_with(label)
                    && o.bundle.is_none()
                    && o.only().and_then(|p| p.kind.as_deref()) == only
            })
            .unwrap_or_else(|| {
                panic!(
                    "no button `{label}` of {only:?} in {name}: {:?}",
                    entry.offers.iter().map(|o| &o.label).collect::<Vec<_>>()
                )
            })
            .fetch
            .clone()
    }

    /// The entry ComfyUI's look-up makes of a repository the hub answered for on 2026-10-08 (the recorded
    /// answer in `tests/fixtures/huggingface/comfyui/repos`), read with the rules of the application's ComfyUI.
    fn recorded_entry(app: &App, id: &str) -> quickai_core::lookup::present::Entry {
        let engine = app.registry.engine("comfyui").unwrap();
        let rules = quickai_core::models::lookup_rules(
            &app.registry,
            engine,
            &quickai_core::models::lookups(engine)[0],
        );
        let text = std::fs::read_to_string(
            crate::testkit::fixtures_dir()
                .join("huggingface/comfyui/repos")
                .join(format!("{}.json", id.replace('/', "__"))),
        )
        .unwrap();
        let repo = quickai_core::lookup::huggingface::parse_repo(&text).unwrap();
        let hit = quickai_core::lookup::huggingface::Hit {
            id: id.to_string(),
            downloads: 1,
            likes: 0,
            pipeline: serde_json::from_str::<serde_json::Value>(&text).unwrap()["pipeline_tag"]
                .as_str()
                .map(str::to_string),
            tags: Vec::new(),
        };
        rules.present(&quickai_core::lookup::huggingface::candidate(
            "https://huggingface.co",
            &hit,
            &repo,
        ))
    }

    /// Whether the first button of the result called `name` whose label starts with `label` is one that the
    /// window marks as there (all its files are in their folders at the size the hub says).
    fn button_is_marked(app: &App, name: &str, label: &str) -> bool {
        let entry = app.lookup.entries.iter().find(|e| e.name == name).unwrap();
        let offer = entry
            .offers
            .iter()
            .find(|o| o.label.starts_with(label))
            .unwrap();
        super::super::catalog_view::installed(offer, &app.catalog.have, &[])
    }

    #[test]
    fn p3_q08_the_box_takes_the_address_of_a_file_a_file_of_this_computer_a_name_or_nothing_it_will_fetch(
    ) {
        let dir = scratch("q08");
        let file = dir.join("mine.safetensors");
        std::fs::write(&file, b"x").unwrap();
        let odd = dir.join("notes.txt");
        std::fs::write(&odd, b"x").unwrap();
        let hub = "https://huggingface.co";
        let words = |w: &str| Given::Words(w.to_string());
        let link = |l: &str| Given::Link(l.to_string());
        let refused = |g: Given| matches!(g, Given::Refused(_));

        assert_eq!(read_box(""), Given::Nothing);
        assert_eq!(read_box("   "), Given::Nothing);
        // words, with the spaces and the tag-like colon left to the look-up to read
        assert_eq!(read_box("  wan2.2 ti2v 5b  "), words("wan2.2 ti2v 5b"));
        assert_eq!(read_box("umt5"), words("umt5"));
        // the page of a file of the hub is turned into the address of the file; the address of the file is kept
        assert_eq!(
            read_box(&format!(
                "{hub}/QuantStack/Wan2.2-TI2V-5B-GGUF/blob/main/VAE/Wan2.2_VAE.safetensors"
            )),
            link(&format!(
                "{hub}/QuantStack/Wan2.2-TI2V-5B-GGUF/resolve/main/VAE/Wan2.2_VAE.safetensors"
            ))
        );
        assert_eq!(
            read_box(&format!("{hub}/QuantStack/Wan2.2-TI2V-5B-GGUF/resolve/main/VAE/Wan2.2_VAE.safetensors?download=true")),
            link(&format!("{hub}/QuantStack/Wan2.2-TI2V-5B-GGUF/resolve/main/VAE/Wan2.2_VAE.safetensors?download=true"))
        );
        assert_eq!(
            read_box("https://hf.co/hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF/blob/9873ba/Wan2_2-TI2V-5B-Turbo-Q8_0.gguf"),
            link("https://hf.co/hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF/resolve/9873ba/Wan2_2-TI2V-5B-Turbo-Q8_0.gguf")
        );
        // the page of a repository is a name to search for: the files are what its results offer
        for page in [
            format!("{hub}/city96/umt5-xxl-encoder-gguf"),
            format!("{hub}/city96/umt5-xxl-encoder-gguf/"),
            format!("{hub}/city96/umt5-xxl-encoder-gguf/tree/main"),
            format!("{hub}/city96/umt5-xxl-encoder-gguf?show_file_info=x.gguf"),
        ] {
            assert_eq!(
                read_box(&page),
                words("city96/umt5-xxl-encoder-gguf"),
                "{page}"
            );
        }
        // any other address must end in a file of numbers
        assert_eq!(
            read_box("https://example.com/files/model-Q8_0.gguf"),
            link("https://example.com/files/model-Q8_0.gguf")
        );
        assert_eq!(
            read_box("http://127.0.0.1:9/a/resolve/main/x.SAFETENSORS?x=1#y"),
            link("http://127.0.0.1:9/a/resolve/main/x.SAFETENSORS?x=1#y")
        );
        for bad in [
            "https://example.com/page.html",
            "https://example.com/model.ckpt",
            "https://example.com/model.pt",
            "https://example.com/",
            &format!("{hub}/city96/umt5-xxl-encoder-gguf/blob/main/README.md"),
            &format!("{hub}/city96/umt5-xxl-encoder-gguf/resolve/main/model.bin"),
            "https://example.com/a.gguf.exe",
        ] {
            assert!(refused(read_box(bad)), "{bad}: {:?}", read_box(bad));
        }
        // a file of this computer: one of numbers is linked in, any other is refused with its name
        assert_eq!(
            read_box(&file.display().to_string()),
            Given::File(file.clone())
        );
        match read_box(&odd.display().to_string()) {
            Given::Refused(why) => assert!(
                why.contains("notes.txt") && why.contains(".safetensors"),
                "{why}"
            ),
            other => panic!("{other:?}"),
        }
        // a path that is not a file is words, as in llama.cpp's box
        assert!(matches!(read_box("/no/such/model.gguf"), Given::Words(_)));
        // the name a file is kept under
        assert_eq!(
            file_name_of("https://h.co/a/b/Wan2.2_VAE.safetensors?download=true"),
            "Wan2.2_VAE.safetensors"
        );
        assert_eq!(file_name_of(r"C:\models\x.gguf"), "x.gguf");
        assert_eq!(file_name_of("/a/b/c d.gguf"), "cd.gguf");
    }

    #[test]
    fn p3_q09_look_up_online_searches_each_pipeline_and_the_encoders_and_every_button_fetches_into_its_folder(
    ) {
        let (dir, hub, mut app) = comfy_world("q09");
        let ctx = egui::Context::default();
        assert!(app.can_look_up() && app.runs_alone());
        // nothing is asked of the hub until the button is pressed, however often the window is drawn
        for _ in 0..3 {
            painted(&ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    super::super::theme::card(ui, |ui| app.show_workbench(ui, ServerState::Idle))
                });
            });
        }
        assert!(hub.requests().is_empty());

        // a file that is there before the look-up begins is found by it
        std::fs::create_dir_all(dir.join("data/comfyui/models/vae")).unwrap();
        std::fs::write(
            dir.join("data/comfyui/models/vae/tiny_vae.safetensors"),
            vec![0u8; 2000],
        )
        .unwrap();
        app.start_lookup(&ctx, "");
        until_looked_up(&mut app);
        assert!(!app.lookup.failed, "{}", app.lookup.status);
        // one search for each pipeline, then the words, in the order the document gives them
        let searches: Vec<String> = hub
            .paths()
            .into_iter()
            .filter(|p| p.starts_with("/api/models?"))
            .collect();
        let tails: Vec<&str> = searches
            .iter()
            .map(|p| p.split("limit=50").nth(1).unwrap())
            .collect();
        assert_eq!(
            tails,
            [
                "&pipeline_tag=text-to-video",
                "&pipeline_tag=image-to-video",
                "&pipeline_tag=text-to-image",
                "&pipeline_tag=image-to-image",
                "&search=encoder"
            ]
        );
        let names: Vec<&str> = app.lookup.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "me/tiny-video-GGUF",
                "me/tiny-picture-GGUF",
                "me/tiny-encoder-GGUF"
            ]
        );
        let sections = quickai_core::lookup::view::arrange(
            &app.lookup.entries,
            app.registry.vocabulary().model_types(),
            None,
        );
        assert_eq!(
            sections
                .iter()
                .map(|s| s.type_id.as_str())
                .collect::<Vec<_>>(),
            ["video", "image", "text_encoder"],
            "in the order the vocabulary shows them"
        );
        assert!(
            app.lookup.status.starts_with("Found 3 models on"),
            "{}",
            app.lookup.status
        );

        // only the VAE of the model whose VAE was there before is marked (it is an item of that model's ▾)
        assert_eq!(marked(&mut app), 1);

        // the weights go with the models, the VAE of the model's repository into `vae`, the text encoder
        // into `text_encoders`, each under its own name, and the hash the hub states is checked as it comes
        let models = dir.join("data/comfyui/models");
        assert_eq!(app.models_dir, models);
        let video = button(&app, "me/tiny-video-GGUF", "Q8_0", None);
        app.fetch_offer(&video, &ctx);
        assert!(app.download.is_some());
        until_downloaded(&mut app);
        assert_eq!(
            std::fs::metadata(models.join("diffusion_models/tiny-video-Q8_0.gguf"))
                .unwrap()
                .len(),
            5000
        );
        assert!(!models
            .join("diffusion_models/tiny-video-Q8_0.gguf.part")
            .exists());
        assert_eq!(
            app.models
                .iter()
                .map(|m| m.filename.as_str())
                .collect::<Vec<_>>(),
            ["tiny-video-Q8_0.gguf"]
        );
        let note = app.bench.note.as_ref().expect("a note");
        assert!(
            !note.failed
                && note
                    .text
                    .contains("tiny-video-Q8_0.gguf (in diffusion_models)"),
            "{}",
            note.text
        );

        let vae = button(&app, "me/tiny-video-GGUF", "tiny_vae", Some("vae"));
        app.fetch_offer(&vae, &ctx);
        until_downloaded(&mut app);
        assert_eq!(
            std::fs::metadata(models.join("vae/tiny_vae.safetensors"))
                .unwrap()
                .len(),
            2000
        );
        assert!(app
            .bench
            .note
            .as_ref()
            .unwrap()
            .text
            .contains("tiny_vae.safetensors (in vae)"));

        let encoder = button(&app, "me/tiny-encoder-GGUF", "Q8_0", Some("text_encoder"));
        app.fetch_offer(&encoder, &ctx);
        until_downloaded(&mut app);
        assert_eq!(
            std::fs::metadata(models.join("text_encoders/tiny-encoder-Q8_0.gguf"))
                .unwrap()
                .len(),
            6000
        );
        assert!(
            app.log_lines
                .iter()
                .any(|l| l == "tiny-encoder-Q8_0.gguf is in text_encoders."),
            "{:?}",
            app.log_lines
        );
        // what is there is said in the results, offer by offer: the video model's weights and its VAE (in its ▾), and
        // the text encoder, and not the picture model, which is not
        assert_eq!(marked(&mut app), 3);
        // none of them is a pair of anything: no record beside any, and the picker lists the model only
        assert!(std::fs::read_dir(models.join("text_encoders"))
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".bundle.json")));
        assert_eq!(app.models.len(), 1);

        // a picture model is a model like the other: weights, in the same folder
        let picture = button(&app, "me/tiny-picture-GGUF", "Q8_0", None);
        app.fetch_offer(&picture, &ctx);
        until_downloaded(&mut app);
        assert!(models
            .join("diffusion_models/tiny-picture-Q8_0.gguf")
            .is_file());
        assert_eq!(app.models.len(), 2);
        assert_eq!(marked(&mut app), 4);
        // a file put there by hand, or fetched by the page, is found by Refresh
        std::fs::write(
            models.join("text_encoders/tiny-encoder-Q5_K_M.gguf"),
            vec![0u8; 4000],
        )
        .unwrap();
        assert!(!button_is_marked(&app, "me/tiny-encoder-GGUF", "Q5_K_M"));
        app.rescan_models();
        assert!(button_is_marked(&app, "me/tiny-encoder-GGUF", "Q5_K_M"));
        std::fs::remove_file(models.join("text_encoders/tiny-encoder-Q5_K_M.gguf")).unwrap();
        app.rescan_models();
        assert!(!button_is_marked(&app, "me/tiny-encoder-GGUF", "Q5_K_M"));

        // a button is not pressed twice at once
        let again = button(&app, "me/tiny-encoder-GGUF", "Q5_K_M", Some("text_encoder"));
        app.fetch_offer(&again, &ctx);
        let first = app.download.is_some();
        app.fetch_offer(
            &button(&app, "me/tiny-encoder-GGUF", "Q8_0", Some("text_encoder")),
            &ctx,
        );
        until_downloaded(&mut app);
        assert!(first);
        assert!(models
            .join("text_encoders/tiny-encoder-Q5_K_M.gguf")
            .is_file());
    }

    #[test]
    fn p3_q09b_a_file_that_is_not_what_the_hub_states_it_is_is_not_kept_and_the_window_says_so() {
        let (dir, hub, mut app) = comfy_world("q09b");
        let ctx = egui::Context::default();
        hub.corrupt("me/tiny-encoder-GGUF", "tiny-encoder-Q8_0.gguf");
        app.start_lookup(&ctx, "encoder");
        until_looked_up(&mut app);
        // words are one search: no pipeline, and not the plain list's words as well
        let searches: Vec<String> = hub
            .paths()
            .into_iter()
            .filter(|p| p.starts_with("/api/models?"))
            .collect();
        assert_eq!(searches.len(), 1, "{searches:?}");
        assert!(
            searches[0].ends_with("&search=encoder") && !searches[0].contains("pipeline_tag"),
            "{searches:?}"
        );
        let encoder = button(&app, "me/tiny-encoder-GGUF", "Q8_0", Some("text_encoder"));
        app.fetch_offer(&encoder, &ctx);
        until_downloaded(&mut app);
        let folder = dir.join("data/comfyui/models/text_encoders");
        assert!(!folder.join("tiny-encoder-Q8_0.gguf").exists(), "not kept");
        assert!(
            !folder.join("tiny-encoder-Q8_0.gguf.part").exists(),
            "not even as a part"
        );
        let note = app.bench.note.as_ref().expect("a note");
        assert!(
            note.failed && note.text.contains("does not match the checksum"),
            "{}",
            note.text
        );
        // the other quantization of it is fine
        let other = button(&app, "me/tiny-encoder-GGUF", "Q5_K_M", Some("text_encoder"));
        app.fetch_offer(&other, &ctx);
        until_downloaded(&mut app);
        assert!(folder.join("tiny-encoder-Q5_K_M.gguf").is_file());
        assert!(!app.bench.note.as_ref().unwrap().failed);
    }

    #[test]
    fn p3_q10_the_box_downloads_a_link_into_the_folder_it_guesses_or_the_one_chosen_links_a_file_in_and_searches_words(
    ) {
        let (dir, hub, mut app) = comfy_world("q10");
        let ctx = egui::Context::default();
        let models = dir.join("data/comfyui/models");
        let link = |file: &str| format!("{}/me/tiny-encoder-GGUF/resolve/main/{file}", hub.base());

        // a text encoder is guessed from its name, and the window says where it will go before it does
        app.model_input = link("tiny-encoder-Q8_0.gguf");
        assert_eq!(
            app.target_folder("tiny-encoder-Q8_0.gguf").as_deref(),
            Some("text_encoders")
        );
        let shown = painted(&ctx, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                super::super::theme::card(ui, |ui| app.show_workbench(ui, ServerState::Idle))
            });
        })
        .join("\n");
        assert!(
            shown.contains("tiny-encoder-Q8_0.gguf will be kept in the text_encoders folder."),
            "{shown}"
        );
        assert!(shown.contains("Download"), "{shown}");
        app.submit_get_box(&ctx);
        assert!(app.model_input.is_empty());
        assert!(app.download.is_some());
        // not while one is going
        app.model_input = link("tiny-encoder-Q5_K_M.gguf");
        app.submit_get_box(&ctx);
        assert_eq!(
            app.model_input,
            link("tiny-encoder-Q5_K_M.gguf"),
            "the box keeps what was typed"
        );
        until_downloaded(&mut app);
        assert!(models
            .join("text_encoders/tiny-encoder-Q8_0.gguf")
            .is_file());
        assert!(app
            .bench
            .note
            .as_ref()
            .unwrap()
            .text
            .contains("tiny-encoder-Q8_0.gguf (in text_encoders)"));

        // the folder the person chose is believed over the guess
        app.bench.into = Some("loras".to_string());
        app.submit_get_box(&ctx);
        until_downloaded(&mut app);
        assert!(models.join("loras/tiny-encoder-Q5_K_M.gguf").is_file());
        assert!(!models
            .join("text_encoders/tiny-encoder-Q5_K_M.gguf")
            .exists());
        app.bench.into = None;

        // a file of this computer is linked in, into the folder its name says
        let mine = dir.join("my_vae.safetensors");
        std::fs::write(&mine, vec![7u8; 100]).unwrap();
        app.model_input = mine.display().to_string();
        app.submit_get_box(&ctx);
        until_downloaded(&mut app);
        let linked = models.join("vae/my_vae.safetensors");
        assert_eq!(std::fs::read(&linked).unwrap(), vec![7u8; 100]);
        assert!(mine.is_file(), "the original stays where it was");

        // what is not a model file is said, in red, and nothing is asked of anyone
        let asked = hub.requests().len();
        app.model_input = "https://example.com/page.html".to_string();
        app.submit_get_box(&ctx);
        let note = app.bench.note.as_ref().expect("a note");
        assert!(
            note.failed && note.text.contains("is not the address of a model file"),
            "{}",
            note.text
        );
        assert!(app.download.is_none() && !app.lookup.running());
        assert_eq!(hub.requests().len(), asked);

        // words are a search of the hub, the one the Search button makes
        app.model_input = "tiny encoder".to_string();
        app.submit_get_box(&ctx);
        assert!(app.lookup.running() || !app.lookup.entries.is_empty());
        until_looked_up(&mut app);
        assert!(
            hub.paths()
                .iter()
                .any(|p| p.ends_with("&search=tiny%20encoder")),
            "{:?}",
            hub.paths()
        );
        // the phrase, and then each of its words: `tiny` finds all three repositories, `encoder` one of them
        assert!(
            app.lookup
                .entries
                .iter()
                .any(|e| e.name == "me/tiny-encoder-GGUF"),
            "{:?}",
            app.lookup
                .entries
                .iter()
                .map(|e| &e.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn p3_q11_the_window_draws_a_row_for_the_model_and_a_row_for_each_file_that_goes_by_itself() {
        let dir = scratch("q11");
        let mut app = app_for(&dir, "comfyui");
        let recorded = |id: &str| recorded_entry(&app, id);
        app.lookup.entries = vec![
            recorded("QuantStack/Wan2.2-TI2V-5B-GGUF"),
            recorded("city96/umt5-xxl-encoder-gguf"),
        ];
        app.lookup.open = true;
        app.lookup.status = "Found 2 models.".into();
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        for _ in 0..3 {
            texts = painted(&ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    super::super::theme::card(ui, |ui| app.show_workbench(ui, ServerState::Idle))
                });
            });
        }
        let all = texts.join("\n");
        // a row for each repository, under the chip of its kind, with its Get
        for wanted in [
            "Video",
            "Text encoders",
            "Wan2.2-TI2V-5B-GGUF",
            "by QuantStack",
            "umt5-xxl-encoder-gguf",
            "by city96",
            "Get",
        ] {
            assert!(all.contains(wanted), "`{wanted}` is not drawn:\n{all}");
        }
        // and in the ▾ of the model, its VAE by itself, a file of its own, as the menu says it
        let wan = app
            .lookup
            .entries
            .iter()
            .find(|e| e.name.ends_with("Wan2.2-TI2V-5B-GGUF"))
            .expect("the model's entry");
        let alone: Vec<&str> = wan
            .offers
            .iter()
            .filter_map(|o| o.only())
            .map(|piece| piece.label.as_str())
            .collect();
        assert!(alone.contains(&"VAE"), "{alone:?}");
        assert!(
            wan.offers.iter().any(|o| o.label.contains("Q8_0")),
            "{:?}",
            wan.offers.iter().map(|o| &o.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn p3_q11b_comfyui_marks_the_results_it_has_and_llamacpps_results_are_never_marked() {
        use std::fs::File;
        let dir = scratch("q11b");
        let draw = marked;
        // the text encoder of a person's list, Q4_K_M, as a file of the size the hub states
        let name = "umt5-xxl-encoder-Q4_K_M.gguf";
        let size = 3_655_145_312;

        // ComfyUI: the file in the folder of its kind makes the result marked, and only at that size
        let mut comfy = app_for(&dir, "comfyui");
        comfy.lookup.entries = vec![recorded_entry(&comfy, "city96/umt5-xxl-encoder-gguf")];
        comfy.lookup.open = true;
        comfy.lookup.status = "Found 1 model.".into();
        comfy.refresh_catalog_have();
        assert_eq!(draw(&mut comfy), 0);
        let folder = comfy.models_dir.join("text_encoders");
        std::fs::create_dir_all(&folder).unwrap();
        File::create(folder.join(name))
            .unwrap()
            .set_len(size - 1)
            .unwrap();
        comfy.refresh_catalog_have();
        assert_eq!(
            draw(&mut comfy),
            0,
            "a file of another size is not the file"
        );
        File::create(folder.join(name))
            .unwrap()
            .set_len(size)
            .unwrap();
        comfy.refresh_catalog_have();
        assert_eq!(draw(&mut comfy), 1);
        // in the models folder itself, or in another folder of it, it is not that button's file
        std::fs::rename(folder.join(name), comfy.models_dir.join(name)).unwrap();
        comfy.refresh_catalog_have();
        assert_eq!(draw(&mut comfy), 0);

        // llama.cpp, with its folder of its own: the same results, with the same file in the same place
        let settings = Settings {
            models_dir_override: Some(dir.join("llama-models").display().to_string()),
            ..Settings::default()
        };
        let mut llama = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("llama-settings.json")),
            Some(dir.join("llama-data").display().to_string()),
        );
        llama.bundled_models_dir = dir.join("bundled");
        llama.load_engine_state();
        assert_eq!(llama.engine, "llamacpp");
        assert_eq!(
            llama.models_dir,
            dir.join("llama-models"),
            "no real folder is written"
        );
        llama.lookup.entries = vec![recorded_entry(&comfy, "city96/umt5-xxl-encoder-gguf")];
        llama.lookup.open = true;
        llama.lookup.status = "Found 1 model.".into();
        let folder = llama.models_dir.join("text_encoders");
        std::fs::create_dir_all(&folder).unwrap();
        File::create(folder.join(name))
            .unwrap()
            .set_len(size)
            .unwrap();
        llama.refresh_catalog_have();
        assert!(llama
            .catalog
            .have
            .contains_key(&format!("text_encoders/{name}")));
        assert_eq!(draw(&mut llama), 0, "its results are what they were");
    }
}
