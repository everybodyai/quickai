//! The model area of an engine that runs on its own (panel `workbench`): ComfyUI has no model to
//! choose before Start, because the workflow a person builds in its page names the files it uses. What
//! the window shows is what is installed, the ways to get more (the recommended list, a look-up of the
//! hub, a box that takes a link or a file of this computer and the folder it goes in) and where the
//! files are.
//!
//! The models of the other engines are drawn by `model_bar.rs` and `acquire.rs`; nothing here changes
//! what they show.

use std::path::{Path, PathBuf};

use eframe::egui;
use quickai_core::lookup::present::Download;

use super::App;
use crate::downloader;
use crate::hardware;

/// How many models the panel lists; Settings lists them all.
const SHOWN: usize = 5;

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
    let text = text.trim();
    if text.is_empty() {
        return Given::Nothing;
    }
    if downloader::looks_like_url(text) {
        return read_link(text);
    }
    let path = Path::new(text);
    if path.is_file() {
        return if has_model_ending(text) {
            Given::File(path.to_path_buf())
        } else {
            Given::Refused(format!(
                "{text} is not a model file this window takes in ({}).",
                MODEL_ENDINGS.join(", ")
            ))
        };
    }
    Given::Words(text.to_string())
}

fn has_model_ending(name: &str) -> bool {
    let lower = name
        .split(['?', '#'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    MODEL_ENDINGS.iter().any(|ending| lower.ends_with(ending))
}

/// A link: the page of a file of the hub is turned into the address of the file, the page of a
/// repository is a name to search for, and any other address must end in a model file.
fn read_link(link: &str) -> Given {
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
                return if has_model_ending(&address) {
                    Given::Link(address)
                } else {
                    refused_link(link)
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
    if has_model_ending(link) {
        Given::Link(link.to_string())
    } else {
        refused_link(link)
    }
}

fn refused_link(link: &str) -> Given {
    Given::Refused(format!(
        "{link} is not the address of a model file ({}). Give the address of one file, or the name of what you look for.",
        MODEL_ENDINGS.join(", ")
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
    /// The models installed, the ways to get more, the progress of a download and a sentence about what
    /// to do next.
    pub(super) fn show_workbench(&mut self, ui: &mut egui::Ui, _active: bool) {
        let engine = self.engine_name();
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("Models:").on_hover_text(format!(
                    "The files {engine}'s workflows use: the weights of a model, its text encoder, its \
                     VAE. They are kept in the folders {engine} looks in, under the models folder, and a \
                     workflow names them by file name."
                ));
                let count = self.models.len();
                ui.label(match count {
                    0 => "none yet".to_string(),
                    1 => "1 installed".to_string(),
                    n => format!("{n} installed"),
                });
                ui.add_space(8.0);
                if ui
                    .add_enabled(self.can_recommend(), egui::Button::new("Recommended"))
                    .on_hover_text(format!(
                        "Browse the models QuickAI recommends for {engine}, with the files that go \
                         with each, and fetch them into the right folders with one click. The list is \
                         part of the application: nothing is asked of the internet to open it."
                    ))
                    .clicked()
                {
                    self.open_catalog();
                }
                let can_look_up = self.can_look_up() && self.download.is_none();
                if ui
                    .add_enabled(can_look_up, egui::Button::new("Look up online"))
                    .on_hover_text(
                        "Search Hugging Face for the most downloaded GGUF models of pictures and video, \
                         with the text encoders and VAEs that go with them, each of which you can fetch \
                         with one click into the folder it belongs in. Type a name in the box first to \
                         look for that instead.",
                    )
                    .clicked()
                {
                    self.start_lookup(ui.ctx(), "");
                }
                if ui
                    .button("Refresh")
                    .on_hover_text(format!(
                        "Look in the models folder again, for a file you put there by hand or one that \
                         {engine}'s page fetched with its Download button."
                    ))
                    .clicked()
                {
                    self.rescan_models();
                }
                if ui
                    .button("Open folder")
                    .on_hover_text(format!(
                        "Show the models folder ({}) in your file manager.",
                        self.models_dir.display()
                    ))
                    .clicked()
                {
                    open_folder(&self.models_dir);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.lookup.has_results()
                        && ui
                            .add(egui::Button::new("X").small())
                            .on_hover_text("Close lookup results")
                            .clicked()
                    {
                        self.lookup.open = false;
                    }
                });
            });

            self.show_get_box(ui);

            for model in self.models.iter().take(SHOWN) {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!(
                        "• {} ({})",
                        model.display,
                        hardware::format_bytes(model.size_bytes)
                    )));
                });
                if !model.note.is_empty() {
                    ui.label(
                        egui::RichText::new(&model.note)
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                }
            }
            if self.models.len() > SHOWN {
                ui.label(
                    egui::RichText::new(format!(
                        "... and {} more (⚙ Settings lists them all)",
                        self.models.len() - SHOWN
                    ))
                    .small()
                    .color(egui::Color32::GRAY),
                );
            }

            if let Some(job) = &self.download {
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(212, 160, 23), job.progress_line());
                if let Some(fraction) = job.fraction() {
                    super::widgets::progress_bar(ui, Some(fraction));
                }
            }
            if let Some(note) = &self.bench.note {
                ui.add_space(2.0);
                let colour = if note.failed {
                    egui::Color32::from_rgb(192, 57, 43)
                } else {
                    egui::Color32::from_rgb(46, 160, 67)
                };
                ui.add(egui::Label::new(egui::RichText::new(&note.text).small().color(colour)).wrap());
            }

            self.show_lookup_results(ui);

            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(format!(
                    "{engine} makes pictures and video from workflows built in its own page: press Start, \
                     then Open {engine}."
                ))
                .small()
                .color(egui::Color32::GRAY),
            );
        });
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
        ui.horizontal(|ui| {
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
                        .desired_width(260.0),
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
                    .color(egui::Color32::from_rgb(192, 57, 43)),
            );
        } else if let Given::Link(link) = &given {
            let name = file_name_of(link);
            if let Some(folder) = self.target_folder(&name) {
                ui.label(
                    egui::RichText::new(format!("{name} will be kept in the {folder} folder."))
                        .small()
                        .color(egui::Color32::GRAY),
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

/// Shows a folder in the file manager of the system. Best effort: nothing is said when there is none.
fn open_folder(path: &Path) {
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
    use crate::events::ServerState;
    use crate::settings::Settings;
    use crate::testkit::procs::write_script;
    use crate::testkit::{scratch, wait_until};
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

    /// The texts a frame of `show` paints.
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
                egui::vec2(760.0, 560.0),
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
    fn p3_f61_the_models_area_says_what_is_installed_and_how_to_get_more() {
        let dir = scratch("f61");
        let mut app = app_for(&dir, "comfyui");
        let ctx = egui::Context::default();
        let texts = |app: &mut App, ctx: &egui::Context| {
            painted(ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.show_models_area(ui, false));
            })
            .join("\n")
        };
        let shown = texts(&mut app, &ctx);
        assert!(
            shown.contains("Models:")
                && shown.contains("none yet")
                && shown.contains("Recommended"),
            "{shown}"
        );
        assert!(
            shown.contains("press Start, then Open ComfyUI"),
            "the next step is said: {shown}"
        );
        // it is not llama.cpp's picker (no `.gguf` to find in a folder, no "Model:" box), but it has the ways
        // to get a model that llama.cpp's has: a look-up of the hub, and a box for a link and the folder
        assert!(
            !shown.contains("(no models found)") && !shown.contains("Model:"),
            "{shown}"
        );
        assert!(
            shown.contains("Look up online") && shown.contains("Get:") && shown.contains("Into:"),
            "{shown}"
        );

        let models = app.models_dir.clone();
        for i in 0..7 {
            put(&models, "diffusion_models", &format!("m{i}.gguf"), 100 + i);
        }
        app.rescan_models();
        let shown = texts(&mut app, &ctx);
        assert!(shown.contains("7 installed"), "{shown}");
        assert!(
            shown.contains("m0.gguf") && shown.contains("m4.gguf") && !shown.contains("m5.gguf"),
            "{shown}"
        );
        assert!(shown.contains("... and 2 more"), "{shown}");

        // llama.cpp's window is still the picker and the box
        let mut llama = app_for(&dir, "llamacpp");
        let shown = texts(&mut llama, &ctx);
        assert!(
            shown.contains("Model:") && shown.contains("Look up online"),
            "{shown}"
        );
    }

    #[test]
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
        // the same two choices, with the memory left to the program, are fine
        app.values.set("memory", "auto");
        let shown = controls(&mut app, &ctx);
        assert!(!shown.contains("graphics card: choose auto"), "{shown}");
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
            texts = painted(&ctx, |ui| app.show_catalog_window(ui.ctx()));
        }
        assert!(
            texts.iter().any(|t| t.contains("Recommended models")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("LTX-Video 2B distilled")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("with text encoder and VAE")),
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
                egui::CentralPanel::default().show(ui, |ui| app.show_models_area(ui, false));
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

        // only the model whose VAE was there before is marked
        let marks = |app: &mut App| -> usize {
            let mut texts = Vec::new();
            for _ in 0..3 {
                texts = painted(&ctx, |ui| {
                    egui::CentralPanel::default().show(ui, |ui| app.show_models_area(ui, false));
                });
            }
            texts.iter().filter(|t| t.as_str() == "Installed").count()
        };
        assert_eq!(marks(&mut app), 1);

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
        // what is there is said in the results, by the model that has it: the video model (its weights and
        // its VAE) and the text encoder, and not the picture model, which is not
        assert_eq!(marks(&mut app), 2);
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
        assert_eq!(marks(&mut app), 3);
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
            egui::CentralPanel::default().show(ui, |ui| app.show_models_area(ui, false));
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
                egui::CentralPanel::default().show(ui, |ui| app.show_models_area(ui, false));
            });
        }
        let all = texts.join("\n");
        for wanted in [
            "Video (1)",
            "Text encoders (1)",
            "QuantStack/Wan2.2-TI2V-5B-GGUF",
            "city96/umt5-xxl-encoder-gguf",
            "Get model:",
            "Get VAE only:",
            "Get text encoder only:",
            "Wan2.2_VAE",
            "Q8_0",
        ] {
            assert!(all.contains(wanted), "`{wanted}` is not drawn:\n{all}");
        }
        // the weights row does not say "model only" when no bundle gives it a sibling to be told from
        assert!(!all.contains("Get model only:"), "{all}");
    }

    #[test]
    fn p3_q11b_comfyui_marks_the_results_it_has_and_llamacpps_results_are_never_marked() {
        use std::fs::File;
        let dir = scratch("q11b");
        let draw = |app: &mut App| -> usize {
            let ctx = egui::Context::default();
            let mut texts = Vec::new();
            for _ in 0..3 {
                texts = painted(&ctx, |ui| {
                    egui::CentralPanel::default().show(ui, |ui| app.show_models_area(ui, false));
                });
            }
            texts.iter().filter(|t| t.as_str() == "Installed").count()
        };
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
