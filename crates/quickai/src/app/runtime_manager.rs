//! The runtime manager (docs/architecture.md, section 9.2): the part of the settings window that
//! manages the engine's executable. A box for a path or a download address, the builds installed
//! earlier, a look-up of the builds the engine's releases offer, and the progress of an install.
//!
//! It is configured by the engine document's `[acquire]` section (which feed to ask, which file of
//! a release is for this machine, where builds are kept, what the executable is called) and knows
//! nothing of any one engine; the words it shows are the engine's name, its server's name and where
//! its builds live, put into the sentences below.

use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use eframe::egui;
use quickai_core::acquire;
use quickai_core::form::FormField;
use quickai_core::registry::Engine;

use super::settings_window::{list_scroll_height, DELETE_LIST_MAX_ROWS};
use super::{App, PendingDelete};
use crate::downloader;
use crate::gpu;
use crate::hardware;
use crate::jobs::Job;
use crate::launch;
use crate::prims::feed::{self, Build, BuildLookupEvent, LookupSpec};
use crate::prims::install::{self, InstallSpec, InstalledBuild};

/// What the manager remembers between frames.
#[derive(Default)]
pub struct RuntimeManager {
    /// The binary box's text, which takes effect only when Apply or Download is clicked.
    pub input: String,
    /// The list of builds is open.
    pub show_lookup: bool,
    pub lookup_rx: Option<Receiver<BuildLookupEvent>>,
    pub lookup_results: Vec<LookupRow>,
    pub lookup_status: String,
    /// A live narrowing of `lookup_results`, cleared whenever a fresh look-up starts.
    pub filter: String,
    pub install: Option<Job<PathBuf>>,
    /// The overlays (extra archives, such as a GPU vendor's libraries) the person ticked in the
    /// list; cleared whenever a fresh look-up starts.
    pub overlays: Vec<String>,
}

/// A build the feed offered, with what a row needs worked out once.
pub struct LookupRow {
    pub build: Build,
    /// What the file name says about the GPU backend (`Vulkan`, `CUDA`, `default`).
    pub backend: String,
    /// A warning when the backend needs a GPU this machine does not seem to have.
    pub warning: Option<String>,
}

/// The sentences of the manager, with the engine's names put in. The wording is fixed here; the
/// engine document supplies who it is about.
pub struct RuntimeTexts {
    server: String,
    pub browse_help: String,
    pub apply_url_help: String,
    pub apply_path_help: String,
    pub installed_label: String,
    pub installed_help: String,
    pub delete_build_help: String,
    pub lookup_label: String,
    pub lookup_help: String,
    pub close_list_help: String,
    pub checking_status: String,
    pub filter_label: String,
    pub filter_hint: String,
    pub filter_help: String,
    pub default_choice: String,
    pub confirm_title: String,
}

impl RuntimeTexts {
    /// `root` is where builds are kept, as the user should read it (`~/.quickai/llama-bin`).
    pub fn for_engine(engine: &Engine, root: &str) -> RuntimeTexts {
        let name = engine.doc.engine.name.clone();
        let server = engine
            .doc
            .engine
            .server_name
            .clone()
            .unwrap_or_else(|| name.clone());
        let lookup = format!("Look up {name} builds");
        let repo = acquire::feed_repo_label(engine).unwrap_or_default();
        let filter_hint = engine
            .doc
            .acquire
            .as_ref()
            .and_then(|a| a.filter_hint.clone())
            .unwrap_or_default();
        RuntimeTexts {
            browse_help: format!("Browse for a {server} executable"),
            apply_url_help: format!(
                "Download and install a {server} build from this direct \
                 URL (e.g. a release asset link copied from GitHub), then \
                 switch to it automatically — no need to go through \"{lookup}\" first."
            ),
            apply_path_help: format!(
                "Point at an already-downloaded {server} executable on \
                 disk, or leave blank to reset to the bundled/auto-detected one."
            ),
            installed_label: "Installed builds".to_string(),
            installed_help: format!(
                "{server} builds downloaded via \"{lookup}\" in an earlier run, most recently \
                 installed first — still listed here even once that list (which only \
                 ever shows a handful of the most recent GitHub releases) \
                 no longer includes them. \"Use\" switches to one \
                 instantly, with no re-download."
            ),
            delete_build_help: "Delete this installed build".to_string(),
            lookup_help: format!(
                "List recent {server} builds published on {name}'s \
                 own GitHub releases, filtered to ones that match this \
                 machine's OS/CPU. Picking one downloads and installs it \
                 under {root} and applies it automatically — \
                 useful when the current binary can't load a model (e.g. a \
                 newer architecture) or doesn't match your GPU."
            ),
            lookup_label: lookup,
            close_list_help: "Close build list".to_string(),
            checking_status: format!("Checking {repo} releases..."),
            filter_label: "Filter:".to_string(),
            filter_hint,
            filter_help: format!(
                "Narrow the list below by tag, filename, or backend \
                 ({}) — matched against what's \
                 already listed, no extra lookup.",
                acquire::backend_words(engine)
            ),
            default_choice: "Bundled/default".to_string(),
            confirm_title: format!("Delete {server} build?"),
            server,
        }
    }

    pub fn installed_log(&self, path: &str) -> String {
        format!("{} build installed: {path}", self.server)
    }

    pub fn install_failed_log(&self, msg: &str) -> String {
        format!("{} install failed: {msg}", self.server)
    }

    pub fn deleted_log(&self, label: &str) -> String {
        format!("Deleted {} build: {label}", self.server)
    }

    pub fn delete_failed_log(&self, label: &str, e: &str) -> String {
        format!("Could not delete build {label}: {e}")
    }

    pub fn confirm_body(&self, label: &str) -> String {
        format!("Delete the installed build \"{label}\" from disk? This can't be undone.")
    }
}

/// `/home/u/.quickai/llama-bin` as `~/.quickai/llama-bin`, with forward slashes on every platform.
pub fn display_root(root: &Path, home: &str) -> String {
    match root.strip_prefix(home) {
        Ok(rest) if !home.is_empty() => {
            format!("~/{}", rest.to_string_lossy().replace('\\', "/"))
        }
        _ => root.display().to_string(),
    }
}

impl App {
    /// The engine this version of the interface runs.
    fn runtime_engine(&self) -> Option<&Engine> {
        self.registry.engine(&self.engine)
    }

    pub(super) fn install_spec(&self) -> Option<InstallSpec> {
        InstallSpec::from_engine(self.runtime_engine()?, &self.host_facts)
    }

    /// The sentences of the manager for the engine in use.
    pub(super) fn runtime_texts(&self) -> Option<RuntimeTexts> {
        let engine = self.runtime_engine()?;
        let root = acquire::root_dir(engine, &self.host_facts).unwrap_or_default();
        Some(RuntimeTexts::for_engine(
            engine,
            &display_root(Path::new(&root), &self.host_facts.home),
        ))
    }

    fn lookup_row(&self, build: Build) -> LookupRow {
        let backend = self
            .runtime_engine()
            .and_then(|e| acquire::backend(e, &build.asset_name));
        let label = backend.map_or("default", |b| b.label.as_str()).to_string();
        let warning = backend.and_then(|b| gpu::mismatch_warning(&self.gpu_vendors, b, &label));
        LookupRow {
            build,
            backend: label,
            warning,
        }
    }

    /// Drains the look-up and the install each frame: a finished look-up is summarised, a finished
    /// install points the binary box straight at the new executable and saves it, as a finished
    /// model download selects the new model.
    pub(super) fn poll_runtime_manager(&mut self) {
        if let Some(rx) = self.runtime.lookup_rx.take() {
            let (mut done, mut failed) = (false, false);
            while let Ok(event) = rx.try_recv() {
                match event {
                    BuildLookupEvent::Found(b) => {
                        let row = self.lookup_row(b);
                        self.runtime.lookup_results.push(row);
                    }
                    BuildLookupEvent::Status(s) => self.runtime.lookup_status = s,
                    BuildLookupEvent::Failed => failed = true,
                    BuildLookupEvent::Done => done = true,
                }
            }
            if done {
                // a look-up that failed keeps the sentence it gave: nothing was *found* is another thing
                if !failed {
                    self.runtime.lookup_status = if self.runtime.lookup_results.is_empty() {
                        "No compatible builds found.".to_string()
                    } else {
                        format!("Found {} build(s).", self.runtime.lookup_results.len())
                    };
                }
            } else {
                self.runtime.lookup_rx = Some(rx);
            }
        }

        let Some(mut job) = self.runtime.install.take() else {
            return;
        };
        let outcome = job.poll();
        // what the install said on the way (an engine that asks for verification says what it
        // checked, what it is unpacking and what it ran), before how it ended
        for line in job.take_statuses() {
            self.push_log(line);
        }
        match outcome {
            Some(Ok(path)) => {
                if let Some(texts) = self.runtime_texts() {
                    self.push_log(texts.installed_log(&path.display().to_string()));
                }
                self.runtime.input = path.display().to_string();
                self.binary = path.clone();
                self.set_bin_override(Some(path));
                self.missing_binary_warned = false;
                self.settings_error = None;
                self.save_settings();
            }
            Some(Err(msg)) => {
                if let Some(texts) = self.runtime_texts() {
                    self.push_log(texts.install_failed_log(&msg));
                }
            }
            None => self.runtime.install = Some(job),
        }
    }

    /// What the look-up button does: clears the list and asks the engine's feed.
    pub(super) fn start_build_lookup(
        &mut self,
        spec: LookupSpec,
        texts: &RuntimeTexts,
        ctx: &egui::Context,
    ) {
        self.runtime.show_lookup = true;
        self.runtime.lookup_results.clear();
        self.runtime.overlays.clear();
        self.runtime.filter.clear();
        self.runtime.lookup_status = texts.checking_status.clone();
        let ctx = ctx.clone();
        self.runtime.lookup_rx = Some(feed::spawn_lookup(spec, move || ctx.request_repaint()));
    }

    /// Points the engine back at the executable it finds by itself.
    fn use_default_binary(&mut self) {
        self.runtime.input.clear();
        self.binary = launch::default_binary(&self.registry, &self.engine);
        self.set_bin_override(None);
        self.missing_binary_warned = false;
        self.settings_error = None;
        self.save_settings();
    }

    /// Points the engine at one executable.
    fn use_binary(&mut self, path: PathBuf) {
        self.runtime.input = path.display().to_string();
        self.binary = path.clone();
        self.set_bin_override(Some(path));
        self.missing_binary_warned = false;
        self.settings_error = None;
        self.save_settings();
    }

    /// The widget `runtime_manager` for the field `field` (the binary override).
    pub(super) fn show_runtime_manager(&mut self, ui: &mut egui::Ui, field: &FormField) {
        let Some(texts) = self.runtime_texts() else {
            return;
        };
        let label = ui.label(format!("{}:", field.label));
        if let Some(help) = &field.help {
            label.on_hover_text(help);
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.runtime.input)
                    .desired_width(ui.available_width() - 30.0)
                    .hint_text(field.hint.clone().unwrap_or_default()),
            );
            if ui.button("...").on_hover_text(&texts.browse_help).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.runtime.input = path.display().to_string();
                }
            }
        });
        let install_spec = self.install_spec();
        ui.horizontal(|ui| {
            let trimmed = self.runtime.input.trim().to_string();
            let is_url = downloader::looks_like_url(&trimmed);
            let apply_label = if is_url { "Download" } else { "Apply" };
            let apply_hover = if is_url {
                &texts.apply_url_help
            } else {
                &texts.apply_path_help
            };
            if ui
                .add_enabled(
                    self.runtime.install.is_none(),
                    egui::Button::new(apply_label),
                )
                .on_hover_text(apply_hover)
                .clicked()
            {
                if trimmed.is_empty() {
                    self.use_default_binary();
                } else if is_url {
                    self.settings_error = None;
                    if let Some(spec) = install_spec.clone() {
                        let label = acquire::label_from_url(&trimmed, &spec.fallback_label);
                        let ctx_clone = ui.ctx().clone();
                        self.runtime.install =
                            Some(install::spawn_install(spec, trimmed, label, move || {
                                ctx_clone.request_repaint()
                            }));
                    }
                } else {
                    let candidate = PathBuf::from(&trimmed);
                    if candidate.is_file() {
                        self.binary = candidate.clone();
                        self.set_bin_override(Some(candidate));
                        self.missing_binary_warned = false;
                        self.settings_error = None;
                    } else {
                        self.settings_error = Some(format!("No file at {trimmed}"));
                    }
                    self.save_settings();
                }
            }
            if ui.button("Reset to default").clicked() {
                self.use_default_binary();
            }
        });

        let installed: Vec<(InstalledBuild, String)> = match &install_spec {
            Some(spec) => install::list_installed(spec)
                .into_iter()
                .map(|build| {
                    let backend = self
                        .runtime_engine()
                        .map_or("default", |e| acquire::backend_label(e, &build.label))
                        .to_string();
                    (build, backend)
                })
                .collect(),
            None => Vec::new(),
        };
        if !installed.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(&texts.installed_label).strong())
                .on_hover_text(&texts.installed_help);
            let mut to_select: Option<PathBuf> = None;
            let mut to_delete: Option<PendingDelete> = None;
            egui::ScrollArea::vertical()
                .id_salt("installed_builds_scroll")
                .auto_shrink([false, true])
                .max_height(list_scroll_height(ui, DELETE_LIST_MAX_ROWS))
                .show(ui, |ui| {
                    for (build, backend) in &installed {
                        let is_current =
                            self.bin_override().as_deref() == Some(build.binary_path.as_path());
                        ui.horizontal(|ui| {
                            if ui.selectable_label(is_current, "Use").clicked() && !is_current {
                                to_select = Some(build.binary_path.clone());
                            }
                            if ui
                                .add(egui::Button::new("🗑").small())
                                .on_hover_text(&texts.delete_build_help)
                                .clicked()
                            {
                                to_delete = Some(PendingDelete::Build {
                                    label: build.label.clone(),
                                    install_dir: build.install_dir.clone(),
                                });
                            }
                            ui.label(format!(
                                "{} ({backend}, {})",
                                build.label,
                                hardware::format_bytes(build.size_bytes)
                            ));
                        });
                    }
                });
            if let Some(path) = to_select {
                self.use_binary(path);
            }
            if let Some(pending) = to_delete {
                self.pending_delete = Some(pending);
            }
        }

        let lookup_spec = self
            .runtime_engine()
            .and_then(|e| LookupSpec::from_engine(e, &self.host_facts));
        if lookup_spec.is_none() {
            return;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.runtime.install.is_none(),
                    egui::Button::new(&texts.lookup_label),
                )
                .on_hover_text(&texts.lookup_help)
                .clicked()
            {
                if let Some(spec) = lookup_spec.clone() {
                    self.start_build_lookup(spec, &texts, ui.ctx());
                }
            }
            if self.runtime.show_lookup
                && ui
                    .add(egui::Button::new("X").small())
                    .on_hover_text(&texts.close_list_help)
                    .clicked()
            {
                self.runtime.show_lookup = false;
            }
        });

        if let Some(job) = &self.runtime.install {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::from_rgb(212, 160, 23), job.progress_line());
            if let Some(fraction) = job.fraction() {
                super::widgets::progress_bar(ui, Some(fraction));
            }
        }

        if self.runtime.show_lookup {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&self.runtime.lookup_status)
                    .small()
                    .color(egui::Color32::GRAY),
            );
            if self.runtime.lookup_results.len() > 4 {
                ui.horizontal(|ui| {
                    ui.label(&texts.filter_label);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.runtime.filter)
                            .desired_width(160.0)
                            .hint_text(&texts.filter_hint),
                    )
                    .on_hover_text(&texts.filter_help);
                    if !self.runtime.filter.is_empty()
                        && ui.add(egui::Button::new("X").small()).clicked()
                    {
                        self.runtime.filter.clear();
                    }
                });
            }

            let mut to_install: Option<usize> = None;
            let mut toggled_overlay: Option<String> = None;
            let mut reset_to_default = false;
            let filter = self.runtime.filter.trim().to_ascii_lowercase();

            egui::ScrollArea::vertical()
                .id_salt("llama_lookup_results_scroll")
                .auto_shrink([false, true])
                .max_height(200.0)
                .show(ui, |ui| {
                    let is_default = self.bin_override().is_none();
                    if filter.is_empty() {
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(is_default, &texts.default_choice)
                                .clicked()
                                && !is_default
                            {
                                reset_to_default = true;
                            }
                            ui.label(
                                egui::RichText::new(
                                    launch::default_binary(&self.registry, &self.engine).display().to_string(),
                                )
                                .small()
                                .color(egui::Color32::GRAY),
                            );
                        });
                    }
                    let mut shown = 0;
                    for (i, row) in self.runtime.lookup_results.iter().enumerate() {
                        let (b, backend) = (&row.build, &row.backend);
                        if !filter.is_empty() {
                            let haystack = format!("{} {} {backend}", b.tag, b.asset_name)
                                .to_ascii_lowercase();
                            if !haystack.contains(&filter) {
                                continue;
                            }
                        }
                        shown += 1;
                        let is_current = self
                            .bin_override()
                            .is_some_and(|p| p.display().to_string().contains(&b.label));
                        ui.horizontal(|ui| {
                            // a file that changed after it was tested is not installed on a click
                            let clicked = if b.conflict {
                                ui.add_enabled_ui(false, |ui| ui.selectable_label(is_current, "Use"))
                                    .inner
                                    .clicked()
                            } else {
                                ui.selectable_label(is_current, "Use").clicked()
                            };
                            if clicked && !is_current {
                                to_install = Some(i);
                            }
                            let size = b
                                .size_bytes
                                .map(hardware::format_bytes)
                                .unwrap_or_else(|| "?".into());
                            let tested = if b.tested {
                                " — the version this was tested with"
                            } else {
                                ""
                            };
                            ui.label(format!(
                                "{} — {} ({backend}, {size}){tested}",
                                b.tag, b.asset_name
                            ));
                        });
                        if b.conflict {
                            ui.label(
                                egui::RichText::new(
                                    "This file is not the one this version was tested with (its checksum differs), so it is not installed.",
                                )
                                .small()
                                .color(egui::Color32::from_rgb(212, 160, 23)),
                            );
                        }
                        for overlay in &b.overlays {
                            let mut on = self.runtime.overlays.contains(&overlay.id);
                            let tick = ui
                                .checkbox(&mut on, &overlay.label)
                                .on_hover_text(&overlay.asset_name);
                            if tick.changed() {
                                toggled_overlay = Some(overlay.id.clone());
                            }
                        }
                        if let Some(warning) = &row.warning {
                            ui.label(
                                egui::RichText::new(warning)
                                    .small()
                                    .color(egui::Color32::from_rgb(212, 160, 23)),
                            );
                        }
                    }
                    if shown == 0 && !filter.is_empty() {
                        ui.label(
                            egui::RichText::new(format!(
                                "No builds match \"{}\".",
                                self.runtime.filter.trim()
                            ))
                            .small()
                            .color(egui::Color32::GRAY),
                        );
                    }
                });

            if let Some(id) = toggled_overlay {
                match self.runtime.overlays.iter().position(|o| *o == id) {
                    Some(at) => {
                        self.runtime.overlays.remove(at);
                    }
                    None => self.runtime.overlays.push(id),
                }
            }
            if reset_to_default {
                self.use_default_binary();
            }
            if let (Some(i), Some(spec)) = (to_install, install_spec) {
                let b = &self.runtime.lookup_results[i].build;
                let chosen: Vec<&str> = self.runtime.overlays.iter().map(String::as_str).collect();
                let request = b.install_request(&chosen);
                let ctx_clone = ui.ctx().clone();
                self.runtime.install =
                    Some(install::spawn_install_build(spec, request, move || {
                        ctx_clone.request_repaint()
                    }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{build_registry, ConfigDirs};

    fn llama_texts() -> RuntimeTexts {
        let registry = build_registry(&ConfigDirs::default());
        RuntimeTexts::for_engine(
            registry.engine("llamacpp").expect("the embedded engine"),
            "~/.quickai/llama-bin",
        )
    }

    /// The sentences moved out of the window's code into templates over the engine's names.
    /// `tests/golden/ui_texts.json` was extracted from the code before it was deleted, so this fails
    /// if a word of what the user reads (and the GUI suite's OCR looks for) changed on the way.
    #[test]
    fn the_managers_sentences_for_llamacpp_are_the_ones_the_window_always_had() {
        let golden: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&crate::golden::read_golden("ui_texts.json")).unwrap();
        let t = llama_texts();
        let want = |key: &str| golden[&format!("runtime.{key}")].as_str();
        assert_eq!(t.browse_help, want("browse_help"));
        assert_eq!(t.apply_url_help, want("apply_url_help"));
        assert_eq!(t.apply_path_help, want("apply_path_help"));
        assert_eq!(t.installed_label, want("installed_label"));
        assert_eq!(t.installed_help, want("installed_help"));
        assert_eq!(t.delete_build_help, want("delete_build_help"));
        assert_eq!(t.lookup_label, want("lookup_label"));
        assert_eq!(t.lookup_help, want("lookup_help"));
        assert_eq!(t.close_list_help, want("close_list_help"));
        assert_eq!(t.checking_status, want("checking_status"));
        assert_eq!(t.filter_label, want("filter_label"));
        assert_eq!(t.filter_hint, want("filter_hint"));
        assert_eq!(t.filter_help, want("filter_help"));
        assert_eq!(t.default_choice, want("default_choice"));
        assert_eq!(t.confirm_title, want("confirm_title"));
        assert_eq!(t.installed_log("{path}"), want("log_installed"));
        assert_eq!(t.install_failed_log("{msg}"), want("log_install_failed"));
        assert_eq!(t.deleted_log("{label}"), want("log_deleted"));
        assert_eq!(
            t.delete_failed_log("{label}", "{e}"),
            want("log_delete_failed")
        );
        assert_eq!(t.confirm_body("{label}"), want("confirm_body"));
    }

    #[test]
    fn another_engine_gets_its_own_names_in_the_same_sentences() {
        let registry = build_registry(&ConfigDirs::default());
        let _ = registry;
        // an engine that names itself differently
        let doc = r#"schema = 1
kind = "engine"
id = "demo"
requires = { primitives = ["spawn@1", "feed.github_release@1"] }
[engine]
name = "Demo"
server_name = "demo-daemon"
[launch]
strategies = ["spawn"]
[launch.spawn]
program = "x"
[acquire]
root = "${data}/demo"
feed = { use = "github_release", repo = "acme/demo" }
assets = { by = "names", rules = [ { when = "os == 'linux'", name = "demo.tar.gz" } ] }
install = { use = "archive", binary = { name = "demo-daemon" } }
backends = [ { match = "gpu", label = "GPU" } ]
"#;
        let source =
            quickai_core::config::MemorySource::new(&[("embedded:engines/demo.toml", doc)]);
        let registry = quickai_core::registry::Registry::build(
            quickai_core::config::load(
                &[(quickai_core::config::Layer::Embedded, &source)],
                &quickai_core::config::LoadOptions::default(),
            ),
            &crate::prims::known(),
        );
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        let t = RuntimeTexts::for_engine(registry.engine("demo").unwrap(), "~/.quickai/demo");
        assert_eq!(t.lookup_label, "Look up Demo builds");
        assert_eq!(t.browse_help, "Browse for a demo-daemon executable");
        assert_eq!(
            t.checking_status,
            "Checking github.com/acme/demo releases..."
        );
        assert!(t.lookup_help.contains("under ~/.quickai/demo and applies"));
        assert!(t.filter_help.contains("backend (GPU)"));
        assert_eq!(t.confirm_title, "Delete demo-daemon build?");
        assert_eq!(
            t.filter_hint, "",
            "no hint is better than a hint about another engine"
        );
    }

    #[test]
    fn the_root_is_shown_the_way_a_person_writes_it() {
        assert_eq!(
            display_root(Path::new("/home/u/.quickai/llama-bin"), "/home/u"),
            "~/.quickai/llama-bin"
        );
        assert_eq!(
            display_root(Path::new("/opt/llama-bin"), "/home/u"),
            "/opt/llama-bin",
            "a root outside the home directory is shown as it is"
        );
        assert_eq!(display_root(Path::new("/x/y"), ""), "/x/y");
    }
}
