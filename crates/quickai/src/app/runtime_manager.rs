//! The runtime manager (docs/architecture.md, section 9.2): the part of the settings window that
//! manages the engine's executable. A box for a path or a download address, the builds installed
//! earlier, a look-up of the builds the engine's releases offer, and the progress of an install.
//!
//! It is configured by the engine document's `[acquire]` section (which feed to ask, which file of
//! a release is for this machine, where builds are kept, what the executable is called) and knows
//! nothing of any one engine; the words it shows are the engine's name, its server's name and where
//! its builds live, put into the sentences below.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossbeam_channel::{Receiver, TryRecvError};
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
use crate::prims::install::{self, InstallSpec, InstalledBuild, Unfinished};

/// What the manager remembers between frames.
#[derive(Default)]
pub struct RuntimeManager {
    /// The binary box's text, which takes effect only when Apply or Download is clicked.
    pub input: String,
    /// The list of builds is open.
    pub show_lookup: bool,
    /// A look-up has just ended: the frames to come bring its list into view (in a window that scrolls it is
    /// otherwise under the fold, with only the sentence that says how many there are showing). More than one
    /// frame, because a list that is as tall as its rows is that tall a frame after they came.
    pub scroll_to_results: u8,
    pub lookup_rx: Option<Receiver<BuildLookupEvent>>,
    pub lookup_results: Vec<LookupRow>,
    pub lookup_status: String,
    /// A live narrowing of `lookup_results`, cleared whenever a fresh look-up starts.
    pub filter: String,
    pub install: Option<Job<PathBuf>>,
    /// What stops the install that is running (the Cancel button sets it).
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The overlays (extra archives, such as a GPU vendor's libraries) the person ticked in the
    /// list; cleared whenever a fresh look-up starts.
    pub overlays: Vec<String>,
    /// The parts of the install the person ticked, with the engine they were ticked for (ComfyUI-Manager): none
    /// until they touch a box, and then the document's own choice is not asked again.
    pub options: Option<(String, Vec<String>)>,
    /// What is on the disk, as the last scan read it.
    pub disk: DiskView,
    /// A build, or an install that stopped, being deleted.
    pub delete: Option<Deleting>,
    /// What the engine's folder holds of installs that stopped, read when the engine is chosen (and at
    /// the start), to be said in the log: the person who closed the window in the middle of one is not
    /// to have to look for it.
    pub hint: Option<Receiver<Vec<Unfinished>>>,
    /// The page's Install button was pressed: the look-up that is going ends in the install of the build it
    /// offers first for this machine (the one the document was tested with, when the feed has it). The context is the
    /// window's, which the install wakes.
    pub auto_install: Option<egui::Context>,
    /// Why the last install the page asked for did not happen: the look-up's sentence, or the install's.
    pub setup_failure: Option<String>,
}

/// What the manager knows of the disk. It is read by a thread of its own and never in a frame: a build of
/// ComfyUI is seventy thousand files, and adding up their sizes takes longer than a frame may.
#[derive(Default)]
pub struct DiskView {
    /// The builds installed, each with what its name says about its graphics backend.
    pub builds: Vec<(InstalledBuild, String)>,
    /// The installs that stopped before they were whole.
    pub unfinished: Vec<Unfinished>,
    /// A scan has finished since the manager was first shown.
    pub known: bool,
    /// Something changed on the disk, or the manager was away: the next frame it is drawn in starts a scan.
    pub stale: bool,
    /// The frame the manager was last drawn in, to tell that it was away.
    last_pass: u64,
    scan: Option<Receiver<Scan>>,
    /// What reads the disk (a test names another).
    pub(super) reader: Option<fn(&InstallSpec) -> Scan>,
}

/// What a scan of the disk found.
pub struct Scan {
    pub builds: Vec<InstalledBuild>,
    pub unfinished: Vec<Unfinished>,
}

/// Reads the builds and the installs that stopped from the engine's folder.
pub fn read_disk(spec: &InstallSpec) -> Scan {
    Scan {
        builds: install::list_installed(spec),
        unfinished: install::list_unfinished(&spec.root),
    }
}

/// A build, or an install that stopped, being deleted by a thread of its own: removing seventy thousand
/// files takes seconds, and the window must go on answering.
pub struct Deleting {
    pub job: Job<PathBuf>,
    pub label: String,
    /// The build in use is this one: the manager goes back to the engine's own when it is gone.
    pub was_in_use: bool,
    pub unfinished: bool,
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
    /// Said beside the default row when the program it stands for is not there: only for an engine that is downloaded on
    /// request, whose default is a build the person has not installed yet (`None`: the path is shown as it is).
    pub default_missing: Option<String>,
    pub confirm_title: String,
    pub unfinished_label: String,
    pub unfinished_help: String,
    pub continue_help: String,
    pub delete_unfinished_help: String,
    pub confirm_unfinished_title: String,
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
        // where the builds are published, and what a link to one is, in the words of the feed
        let from_index = matches!(
            engine.doc.acquire.as_ref().and_then(|a| a.feed.as_ref()),
            Some(quickai_core::doc::engine::Feed::Pypi { .. })
        );
        let published = if from_index {
            format!("the Python Package Index ({repo})")
        } else {
            format!("{name}'s own GitHub releases")
        };
        let link = if from_index {
            "a link to a .whl file".to_string()
        } else {
            "a release asset link copied from GitHub".to_string()
        };
        let filter_hint = engine
            .doc
            .acquire
            .as_ref()
            .and_then(|a| a.filter_hint.clone())
            .unwrap_or_default();
        // "bundled" is what the packages ship (the document says so: `bundled = true`) and no more: an engine that is
        // downloaded on request has nothing bundled for the first row of the list to be, and its row says `Default`
        let shipped = engine.doc.engine.bundled;
        let default_kind = if shipped {
            "bundled/auto-detected"
        } else {
            "default"
        };
        RuntimeTexts {
            browse_help: format!("Browse for a {server} executable"),
            apply_url_help: format!(
                "Download and install a {server} build from this direct \
                 URL (e.g. {link}), then \
                 switch to it automatically — no need to go through \"{lookup}\" first."
            ),
            apply_path_help: format!(
                "Point at an already-downloaded {server} executable on \
                 disk, or leave blank to reset to the {default_kind} one."
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
                "List recent {server} builds published on {published}, \
                 filtered to ones that match this \
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
            default_choice: if shipped {
                "Bundled/default".to_string()
            } else {
                "Default".to_string()
            },
            default_missing: (!shipped).then(|| "not installed yet".to_string()),
            confirm_title: format!("Delete {server} build?"),
            unfinished_label: "Unfinished installs".to_string(),
            unfinished_help: format!(
                "Installs of {name} that stopped before they were whole (the window was closed, the \
                 machine was switched off, a step failed). \"Continue\" does the steps that are left \
                 and none that are done; the trash can deletes what was written. They are kept under \
                 {root} until you choose."
            ),
            continue_help: "Do the steps that are left; the ones that are done are not done again.".to_string(),
            delete_unfinished_help: "Delete this unfinished install".to_string(),
            confirm_unfinished_title: "Delete unfinished install?".to_string(),
            server,
        }
    }

    pub fn confirm_unfinished_body(&self, label: &str) -> String {
        format!(
            "Delete the unfinished install \"{label}\" from disk? What it fetched and built so far is lost, and the next install of it starts from the beginning. This can't be undone."
        )
    }

    pub fn deleting_line(&self, label: &str) -> String {
        format!("Deleting {label}...")
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

    pub fn deleted_unfinished_log(&self, label: &str) -> String {
        format!("Deleted unfinished {} install: {label}", self.server)
    }

    pub fn delete_failed_log(&self, label: &str, e: &str) -> String {
        format!("Could not delete build {label}: {e}")
    }

    pub fn confirm_body(&self, label: &str) -> String {
        format!("Delete the installed build \"{label}\" from disk? This can't be undone.")
    }
}

/// What is said beside the default row: where the program the application finds by itself is, or that there is none yet.
/// A program that is not found is named by its bare name (`open-webui`), which says nothing to a person who has not
/// installed one: next to a label it looked like the thing the row stood for.
pub fn default_note(texts: &RuntimeTexts, binary: &Path) -> String {
    match &texts.default_missing {
        Some(missing) if !binary.is_file() => missing.clone(),
        _ => binary.display().to_string(),
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

    /// The parts of the install that are ticked: what the person chose for this engine, else what its document ticks.
    pub(super) fn chosen_options(&self) -> Vec<String> {
        let Some(engine) = self.runtime_engine() else {
            return Vec::new();
        };
        match &self.runtime.options {
            Some((id, chosen)) if *id == engine.id => chosen.clone(),
            _ => acquire::default_options(engine, &self.host_facts),
        }
    }

    /// The parts of the install that stopped as `label` that it was begun with, as its steps say (`None` when the window
    /// does not know of such an install).
    fn options_it_began_with(&self, label: &str) -> Option<Vec<String>> {
        let engine = self.runtime_engine()?;
        let stopped = self
            .runtime
            .disk
            .unfinished
            .iter()
            .find(|u| u.label == label)?;
        Some(acquire::options_of_steps(engine, &stopped.steps))
    }

    /// What installing the engine puts on this machine, in a sentence or two for the window: the parts ticked, and, when
    /// the person named no Python, that its Python is QuickAI's own.
    pub(super) fn install_sentence(&self, engine: &Engine, ticked: &[&str]) -> Option<String> {
        let summary = acquire::install_summary_with(engine, &self.host_facts, ticked)?;
        let named = self
            .values
            .text("python")
            .is_some_and(|p| !p.trim().is_empty());
        Some(
            match acquire::own_python_sentence(engine).filter(|_| !named) {
                Some(own) => format!("{summary} {own}"),
                None => summary,
            },
        )
    }

    pub(super) fn install_spec(&self) -> Option<InstallSpec> {
        self.install_spec_with(&self.chosen_options())
    }

    /// The install with these parts of it, and the Python the person chose.
    fn install_spec_with(&self, chosen: &[String]) -> Option<InstallSpec> {
        let chosen: Vec<&str> = chosen.iter().map(String::as_str).collect();
        let spec =
            InstallSpec::from_engine_with(self.runtime_engine()?, &self.host_facts, &chosen)?;
        // the Python the person chose for an install that makes an environment
        let python = self
            .values
            .text("python")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        Some(spec.with_python(python))
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
                self.runtime.scroll_to_results = 3;
                // a look-up that failed keeps the sentence it gave: nothing was *found* is another thing
                if !failed {
                    self.runtime.lookup_status = if self.runtime.lookup_results.is_empty() {
                        "No compatible builds found.".to_string()
                    } else {
                        format!("Found {} build(s).", self.runtime.lookup_results.len())
                    };
                }
                if let Some(ctx) = self.runtime.auto_install.take() {
                    self.install_first_build(failed, &ctx);
                }
            } else {
                self.runtime.lookup_rx = Some(rx);
            }
        }

        if let Some(rx) = self.runtime.hint.take() {
            match rx.try_recv() {
                Ok(stopped) => {
                    let name = self.server_name();
                    for u in stopped {
                        let ago = u
                            .touched
                            .and_then(|t| SystemTime::now().duration_since(t).ok())
                            .map(|d| format!(", stopped {} ago", hardware::format_duration(d)))
                            .unwrap_or_default();
                        self.push_log(format!(
                            "{name}: an install of {} stopped before it was whole ({} of {} steps done, {} on disk{ago}). Go on with it, or delete it, in Settings → Advanced → Unfinished installs.",
                            u.label,
                            u.done,
                            u.total,
                            hardware::format_bytes(u.size_bytes)
                        ));
                    }
                }
                Err(TryRecvError::Empty) => self.runtime.hint = Some(rx),
                Err(TryRecvError::Disconnected) => {}
            }
        }

        if let Some(mut deleting) = self.runtime.delete.take() {
            match deleting.job.poll() {
                Some(outcome) => {
                    self.runtime.disk.stale = true;
                    if let Some(texts) = self.runtime_texts() {
                        match &outcome {
                            Ok(_) if deleting.unfinished => {
                                self.push_log(texts.deleted_unfinished_log(&deleting.label))
                            }
                            Ok(_) => self.push_log(texts.deleted_log(&deleting.label)),
                            Err(e) => self.push_log(texts.delete_failed_log(&deleting.label, e)),
                        }
                    }
                    if outcome.is_ok() && deleting.was_in_use {
                        self.runtime.input.clear();
                        self.binary = launch::default_binary_in(
                            &self.registry,
                            &self.engine,
                            self.data_dir_override.as_deref(),
                        );
                        self.set_bin_override(None);
                        self.missing_binary_warned = false;
                        self.save_settings();
                    }
                }
                None => self.runtime.delete = Some(deleting),
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
        if outcome.is_some() {
            self.runtime.cancel = None;
            // what is on the disk is not what the last scan saw
            self.runtime.disk.stale = true;
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
            Some(Err(msg)) if msg == install::CANCELLED => {
                self.push_log(format!("{} install cancelled.", self.server_name()));
            }
            Some(Err(msg)) => {
                if let Some(texts) = self.runtime_texts() {
                    self.push_log(texts.install_failed_log(&msg));
                }
                self.runtime.setup_failure = Some(msg);
            }
            None => self.runtime.install = Some(job),
        }
    }

    /// Starts reading, on a thread of its own, which installs of the engine in use stopped before they were
    /// whole: `poll_runtime_manager` says them in the log when the answer is there.
    pub(super) fn look_for_stopped_installs(&mut self) {
        self.runtime.hint = self.install_spec().map(|spec| {
            let (tx, rx) = crossbeam_channel::bounded(1);
            std::thread::spawn(move || {
                let _ = tx.send(install::list_unfinished(&spec.root));
            });
            rx
        });
    }

    /// Takes in what the scan of the disk has read, and starts another when something has changed. Called
    /// as the manager is drawn: the scan is a thread's, and the frame only looks at its answer.
    pub(super) fn refresh_disk(&mut self, ctx: &egui::Context) {
        // the manager was not drawn in the frames in between (the window was closed, the section folded):
        // the disk may have changed meanwhile. The frames are the main window's: the Settings are a window of
        // their own, whose passes begin again from none each time it is opened, and it is drawn in the pass of
        // the main window that asked for it, so the two count the same while it is open.
        let pass = ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT);
        if pass > self.runtime.disk.last_pass.saturating_add(2) {
            self.runtime.disk.stale = true;
        }
        self.runtime.disk.last_pass = pass;
        if let Some(rx) = self.runtime.disk.scan.take() {
            match rx.try_recv() {
                Ok(scan) => self.take_scan(scan),
                Err(TryRecvError::Empty) => self.runtime.disk.scan = Some(rx),
                // the reader ended without an answer: what was known stays, and it is not asked again
                // until something changes
                Err(TryRecvError::Disconnected) => self.runtime.disk.known = true,
            }
        }
        let wanted = self.runtime.disk.stale || !self.runtime.disk.known;
        if wanted && self.runtime.disk.scan.is_none() {
            let Some(spec) = self.install_spec() else {
                self.runtime.disk.known = true;
                return;
            };
            self.runtime.disk.stale = false;
            let (tx, rx) = crossbeam_channel::bounded(1);
            let read = self.runtime.disk.reader.unwrap_or(read_disk);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(read(&spec));
                ctx.request_repaint();
            });
            self.runtime.disk.scan = Some(rx);
        }
    }

    fn take_scan(&mut self, scan: Scan) {
        let builds: Vec<(InstalledBuild, String)> = scan
            .builds
            .into_iter()
            .map(|build| {
                let backend = self
                    .runtime_engine()
                    .map_or("default", |e| acquire::backend_label(e, &build.label))
                    .to_string();
                (build, backend)
            })
            .collect();
        self.runtime.disk.builds = builds;
        self.runtime.disk.unfinished = scan.unfinished;
        self.runtime.disk.known = true;
    }

    /// Goes on with the install of `label` that stopped: the steps that are left.
    fn continue_install(&mut self, label: &str, ctx: &egui::Context) {
        // with the parts it was begun with, whatever the boxes say now: another plan is not gone on with
        let chosen = self
            .options_it_began_with(label)
            .unwrap_or_else(|| self.chosen_options());
        let Some(spec) = self.install_spec_with(&chosen) else {
            return;
        };
        // from the document when it can say where the source is, and otherwise from what the install that
        // stopped recorded of it (the address of a wheel is the index's to say)
        let request = self
            .runtime_engine()
            .and_then(|e| feed::request_for_label(e, &self.host_facts, label))
            .or_else(|| {
                let source = self
                    .runtime
                    .disk
                    .unfinished
                    .iter()
                    .find(|u| u.label == label)?
                    .source
                    .clone()?;
                Some(install::InstallRequest {
                    label: label.to_string(),
                    base: install::Source {
                        url: source.url,
                        host_digest: source.host_digest,
                        pin: source.pin,
                        size: source.size,
                    },
                    overlays: Vec::new(),
                })
            });
        let Some(request) = request else {
            self.push_log(format!(
                "Cannot go on with the install of {label}: this version of QuickAI does not know where its source is. Delete it and install again."
            ));
            return;
        };
        self.runtime.cancel = Some(spec.cancel.clone());
        self.runtime.setup_failure = None;
        self.runtime.install = Some(install::spawn_install_build(
            spec,
            request,
            super::calm_wake(ctx.clone()),
        ));
    }

    /// Deletes `dir`, a build or an install that stopped, on a thread of its own.
    pub(super) fn start_delete(
        &mut self,
        label: String,
        dir: PathBuf,
        unfinished: bool,
        ctx: &egui::Context,
    ) {
        let was_in_use = self
            .bin_override()
            .is_some_and(|path| path.starts_with(&dir));
        let wake = ctx.clone();
        let target = dir.clone();
        let job = Job::spawn(
            label.clone(),
            "Deleting",
            move || wake.request_repaint(),
            move |send| match install::delete_installed(&target) {
                Ok(()) => send(crate::jobs::JobEvent::Done(target)),
                Err(e) => send(crate::jobs::JobEvent::Failed(e)),
            },
        );
        self.runtime.delete = Some(Deleting {
            job,
            label,
            was_in_use,
            unfinished,
        });
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
        self.binary = launch::default_binary_in(
            &self.registry,
            &self.engine,
            self.data_dir_override.as_deref(),
        );
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

    /// The parts of the install the document offers as boxes (ComfyUI-Manager), each with its words: ticked as the
    /// document ticks it until the person says otherwise. They are changed only while no install runs: what
    /// is ticked is what the install that is begun does.
    fn show_install_options(&mut self, ui: &mut egui::Ui) {
        let Some(engine) = self.runtime_engine() else {
            return;
        };
        let engine_id = engine.id.clone();
        let offered: Vec<(String, String, Option<String>, Option<String>)> =
            acquire::install_options(engine, &self.host_facts)
                .into_iter()
                .map(|o| {
                    (
                        o.id.clone(),
                        o.label.clone(),
                        o.badge.clone(),
                        o.help.clone(),
                    )
                })
                .collect();
        if offered.is_empty() {
            return;
        }
        let idle = self.runtime.install.is_none();
        let mut chosen = self.chosen_options();
        let mut changed = false;
        ui.add_space(4.0);
        for (id, label, badge, help) in &offered {
            let mut on = chosen.contains(id);
            let mut flipped = false;
            ui.horizontal_wrapped(|ui| {
                let tick = ui.add_enabled(idle, egui::Checkbox::new(&mut on, label.as_str()));
                let tick = match help {
                    Some(help) => tick.on_hover_text(help),
                    None => tick,
                };
                if let Some(badge) = badge {
                    let chip = super::theme::chip(ui, badge, Some(super::theme::Tone::Good));
                    if let Some(help) = help {
                        chip.on_hover_text(help);
                    }
                }
                if tick.changed() {
                    flipped = true;
                }
            });
            if flipped {
                changed = true;
                chosen.retain(|o| o != id);
                if on {
                    chosen.push(id.clone());
                }
            }
        }
        if changed {
            self.runtime.options = Some((engine_id, chosen));
        }
    }

    /// The widget `runtime_manager` for the field `field` (the binary override).
    pub(super) fn show_runtime_manager(&mut self, ui: &mut egui::Ui, field: &FormField) {
        let Some(texts) = self.runtime_texts() else {
            return;
        };
        self.refresh_disk(ui.ctx());
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
        // a build that is a release's source has no file of a kind for a graphics card to be told by
        let from_source = self
            .runtime_engine()
            .is_some_and(acquire::builds_from_source);
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
                        self.runtime.cancel = Some(spec.cancel.clone());
                        self.runtime.setup_failure = None;
                        self.runtime.install = Some(install::spawn_install(
                            spec,
                            trimmed,
                            label,
                            super::calm_wake(ctx_clone),
                        ));
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

        // what the last scan of the disk read: the frame does not look at the disk itself
        let installed: Vec<(InstalledBuild, String)> = self.runtime.disk.builds.clone();
        if install_spec.is_some() && !self.runtime.disk.known {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Looking at what is installed...")
                    .small()
                    .color(super::theme::palette().muted),
            );
        }
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
                                .add_enabled(
                                    self.runtime.delete.is_none(),
                                    egui::Button::new("🗑").small(),
                                )
                                .on_hover_text(&texts.delete_build_help)
                                .clicked()
                            {
                                to_delete = Some(PendingDelete::Build {
                                    label: build.label.clone(),
                                    install_dir: build.install_dir.clone(),
                                });
                            }
                            if from_source {
                                ui.label(format!(
                                    "{} ({})",
                                    build.label,
                                    hardware::format_bytes(build.size_bytes)
                                ));
                            } else {
                                ui.label(format!(
                                    "{} ({backend}, {})",
                                    build.label,
                                    hardware::format_bytes(build.size_bytes)
                                ));
                            }
                        });
                    }
                });
            if let Some(path) = to_select {
                self.use_binary(path);
            }
            if let Some(pending) = to_delete {
                self.delete_of_companion = self.in_companion;
                self.pending_delete = Some(pending);
            }
        }

        // installs that stopped before they were whole (the one that is going now is not one of them)
        let going = self.runtime.install.as_ref().map(|job| job.label.clone());
        let stopped: Vec<Unfinished> = self
            .runtime
            .disk
            .unfinished
            .iter()
            .filter(|u| going.as_deref() != Some(u.label.as_str()))
            .cloned()
            .collect();
        if !stopped.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(&texts.unfinished_label).strong())
                .on_hover_text(&texts.unfinished_help);
            let idle = self.runtime.install.is_none() && self.runtime.delete.is_none();
            let mut to_continue: Option<String> = None;
            let mut to_remove: Option<PendingDelete> = None;
            for u in &stopped {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(idle, egui::Button::new("Continue"))
                        .on_hover_text(&texts.continue_help)
                        .clicked()
                    {
                        to_continue = Some(u.label.clone());
                    }
                    if ui
                        .add_enabled(idle, egui::Button::new("🗑").small())
                        .on_hover_text(&texts.delete_unfinished_help)
                        .clicked()
                    {
                        to_remove = Some(PendingDelete::Unfinished {
                            label: u.label.clone(),
                            dir: u.dir.clone(),
                        });
                    }
                    let ago = u
                        .touched
                        .and_then(|t| SystemTime::now().duration_since(t).ok())
                        .map(|d| format!(", stopped {} ago", hardware::format_duration(d)))
                        .unwrap_or_default();
                    // wrapped in what the buttons leave of the row: unwrapped, a long line made the whole section wider
                    // than the window, and every sentence under it was cut at the window's edge
                    let line = ui.add(
                        egui::Label::new(format!(
                            "{} ({} of {} steps done, {}{ago})",
                            u.label,
                            u.done,
                            u.total,
                            hardware::format_bytes(u.size_bytes)
                        ))
                        .wrap(),
                    );
                    if let Some(next) = &u.next {
                        line.on_hover_text(format!("Next: {next}"));
                    }
                });
            }
            if let Some(label) = to_continue {
                self.continue_install(&label, ui.ctx());
            }
            if let Some(pending) = to_remove {
                self.delete_of_companion = self.in_companion;
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

        self.show_install_options(ui);
        let ticked = self.chosen_options();
        let ticked: Vec<&str> = ticked.iter().map(String::as_str).collect();
        if let Some(summary) = self
            .runtime_engine()
            .and_then(|e| self.install_sentence(e, &ticked))
        {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(summary)
                    .small()
                    .color(super::theme::palette().muted),
            );
        }
        if let Some(job) = &self.runtime.install {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.colored_label(super::theme::palette().warn, job.progress_line());
                if let Some(cancel) = &self.runtime.cancel {
                    if ui
                        .small_button("Cancel")
                        .on_hover_text("Stop the install. What was fetched so far is thrown away.")
                        .clicked()
                    {
                        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            });
            if let Some(fraction) = job.fraction() {
                super::widgets::progress_bar(ui, Some(fraction));
            }
            // what it is doing now: the step, the file, how fast, and how long it has been quiet
            if let Some(detail) = job.detail() {
                ui.label(
                    egui::RichText::new(detail)
                        .small()
                        .color(super::theme::palette().muted),
                );
            }
        }
        // how the last install ended when it failed: said here too, for the log is in the other window
        if self.runtime.install.is_none() {
            if let Some(why) = &self.runtime.setup_failure {
                ui.add_space(4.0);
                ui.colored_label(super::theme::palette().bad, texts.install_failed_log(why));
            }
        }
        if let Some(deleting) = &self.runtime.delete {
            ui.add_space(4.0);
            ui.colored_label(
                super::theme::palette().warn,
                texts.deleting_line(&deleting.label),
            );
        }

        if !self.runtime.show_lookup {
            self.runtime.scroll_to_results = 0;
        }
        if self.runtime.show_lookup {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&self.runtime.lookup_status)
                    .small()
                    .color(super::theme::palette().muted),
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

            // (the list is as tall as its rows up to 200 px, wherever it is in the window that scrolls: a scroll area
            // inside one is given only what is left of the one outside to be seen, which was two rows low in the page)
            let list = egui::ScrollArea::vertical()
                .id_salt("llama_lookup_results_scroll")
                .auto_shrink([false, true])
                .min_scrolled_height(200.0)
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
                            let binary = launch::default_binary_in(
                                &self.registry,
                                &self.engine,
                                self.data_dir_override.as_deref(),
                            );
                            ui.label(
                                egui::RichText::new(default_note(&texts, &binary))
                                    .small()
                                    .color(super::theme::palette().muted),
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
                            if from_source {
                                ui.label(format!("{}{tested}", b.tag));
                            } else {
                                ui.label(format!(
                                    "{} — {} ({backend}, {size}){tested}",
                                    b.tag, b.asset_name
                                ));
                            }
                        });
                        if b.conflict {
                            ui.label(
                                egui::RichText::new(
                                    "This file is not the one this version was tested with (its checksum differs), so it is not installed.",
                                )
                                .small()
                                .color(super::theme::palette().warn),
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
                                    .color(super::theme::palette().warn),
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
                            .color(super::theme::palette().muted),
                        );
                    }
                });
            if self.runtime.scroll_to_results > 0 {
                self.runtime.scroll_to_results -= 1;
                ui.scroll_to_rect(list.inner_rect, Some(egui::Align::Max));
                ui.ctx().request_repaint();
            }

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
                self.runtime.cancel = Some(spec.cancel.clone());
                self.runtime.setup_failure = None;
                self.runtime.install = Some(install::spawn_install_build(
                    spec,
                    request,
                    super::calm_wake(ctx_clone),
                ));
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

    /// Every sentence of the manager, as a list.
    fn sentences(t: &RuntimeTexts) -> Vec<(&'static str, &str)> {
        vec![
            ("browse_help", &t.browse_help),
            ("apply_url_help", &t.apply_url_help),
            ("apply_path_help", &t.apply_path_help),
            ("installed_label", &t.installed_label),
            ("installed_help", &t.installed_help),
            ("delete_build_help", &t.delete_build_help),
            ("lookup_label", &t.lookup_label),
            ("lookup_help", &t.lookup_help),
            ("close_list_help", &t.close_list_help),
            ("checking_status", &t.checking_status),
            ("filter_label", &t.filter_label),
            ("filter_hint", &t.filter_hint),
            ("filter_help", &t.filter_help),
            ("default_choice", &t.default_choice),
            ("confirm_title", &t.confirm_title),
            ("unfinished_label", &t.unfinished_label),
            ("unfinished_help", &t.unfinished_help),
            ("continue_help", &t.continue_help),
            ("delete_unfinished_help", &t.delete_unfinished_help),
            ("confirm_unfinished_title", &t.confirm_unfinished_title),
        ]
    }

    /// "Bundled" is what the packages ship (`bundled = true` in the document) and nothing else. The first row of the list
    /// of builds used to say `Bundled/default` for every engine, so that Open WebUI, which is downloaded on request and never
    /// shipped (its licence is why), was offered to the person as a program that came with QuickAI.
    #[test]
    fn p3_t43_an_engine_that_is_not_shipped_is_never_called_bundled_by_its_manager() {
        let registry = build_registry(&ConfigDirs::default());
        let mut said_bundled_of_a_shipped_one = false;
        for id in ["llamacpp", "ollama", "comfyui", "openwebui"] {
            let engine = registry.engine(id).expect("the embedded engine");
            let t = RuntimeTexts::for_engine(engine, "~/.quickai/x");
            let bundled: Vec<&str> = sentences(&t)
                .into_iter()
                .filter(|(_, text)| text.to_lowercase().contains("bundled"))
                .map(|(name, _)| name)
                .collect();
            let nowhere = Path::new("/nowhere/program");
            if engine.doc.engine.bundled {
                said_bundled_of_a_shipped_one |= !bundled.is_empty();
                assert_eq!(
                    t.default_choice, "Bundled/default",
                    "{id}: what it always was"
                );
                assert_eq!(
                    default_note(&t, nowhere),
                    "/nowhere/program",
                    "{id}: the path beside the row is shown as it is, there or not"
                );
            } else {
                assert!(
                    bundled.is_empty(),
                    "{id} is downloaded on request and never shipped, and its manager says bundled in {bundled:?}"
                );
                assert_eq!(t.default_choice, "Default", "{id}");
                assert!(
                    t.apply_path_help
                        .ends_with("or leave blank to reset to the default one."),
                    "{id}: {}",
                    t.apply_path_help
                );
                // beside the row: a build that is there is shown by its place, and none that is not by a word and
                // not by the bare name of a program (`open-webui` read as what the row stood for)
                assert_eq!(default_note(&t, nowhere), "not installed yet", "{id}");
                let dir = scratch(&format!("t43-{id}"));
                let there = dir.join("program");
                std::fs::write(&there, "#!/bin/sh\n").unwrap();
                assert_eq!(
                    default_note(&t, &there),
                    there.display().to_string(),
                    "{id}"
                );
            }
        }
        assert!(
            said_bundled_of_a_shipped_one,
            "the word is kept for the engine that is shipped: llama.cpp"
        );
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

    // ---- what the window does with the disk and with an install that stopped ------------------------------------------

    use crate::prims::install::{InstallKind, Journal, StepRecord};
    use crate::settings::Settings;
    use crate::testkit::{scratch, wait_until};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const GB: u64 = 1024 * 1024 * 1024;

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
                egui::vec2(760.0, 900.0),
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

    /// The field the settings window draws the manager for.
    fn manager_field(app: &App) -> FormField {
        app.form()
            .expect("a form")
            .section("advanced")
            .expect("the advanced section")
            .fields
            .iter()
            .find(|f| f.widget.as_deref() == Some("runtime_manager"))
            .cloned()
            .expect("the manager's field")
    }

    fn said(texts: &[String], word: &str) -> bool {
        texts.iter().any(|t| t.contains(word))
    }

    /// An install of ComfyUI that stopped, in the folder an application on `dir` keeps its builds in: three
    /// of its seven steps are done, and its directory holds something.
    /// One frame of `show` with these events: where each text of it was painted.
    fn frame_with_events(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        show: impl FnMut(&mut egui::Ui),
    ) -> Vec<(String, egui::Rect)> {
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
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        // in a panel, as the window draws it: the root of a frame is not a place a click is heard
        let mut show = show;
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| show(ui));
        });
        output.textures_delta.clear();
        let mut found = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    /// The person clicks `at` in what `show` draws: the pointer goes there, presses and lets go, a frame each, and a frame
    /// after, which is the one that shows what the click did.
    fn click_in(
        ctx: &egui::Context,
        at: egui::Pos2,
        mut show: impl FnMut(&mut egui::Ui),
    ) -> Vec<(String, egui::Rect)> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut last = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
            Vec::new(),
        ] {
            last = frame_with_events(ctx, events, &mut show);
        }
        last
    }

    /// The manager drawn once what it says of the disk is there ("Looking at what is installed..." goes when the scan is
    /// read, and the rest of the page moves up), so that where a thing is is where it stays.
    fn settled_manager(
        app: &mut App,
        ctx: &egui::Context,
        field: &FormField,
    ) -> Vec<(String, egui::Rect)> {
        wait_until(Duration::from_secs(10), "the disk to be read", || {
            frame_with_events(ctx, Vec::new(), |ui| app.show_runtime_manager(ui, field));
            app.runtime.disk.known
        });
        frame_with_events(ctx, Vec::new(), |ui| app.show_runtime_manager(ui, field))
    }

    fn where_is(frame: &[(String, egui::Rect)], text: &str) -> Option<egui::Rect> {
        frame.iter().find(|(t, _)| t == text).map(|(_, r)| *r)
    }

    /// What the install the window would begin now is made of: the ids of its pip steps and what its launcher passes.
    fn made_of(app: &App) -> (Vec<String>, Vec<String>) {
        let spec = app.install_spec().expect("an install spec");
        let InstallKind::PythonApp(plan) = spec.kind else {
            panic!("a python_app install")
        };
        (
            plan.pip.iter().map(|p| p.id.clone()).collect(),
            plan.launch_args,
        )
    }

    #[test]
    fn p3_f99_the_install_options_are_boxes_ticked_as_the_document_ticks_them_and_what_is_ticked_is_what_the_install_is_made_of(
    ) {
        let dir = scratch("f99");
        let mut app = app_for(&dir, "comfyui");
        let field = manager_field(&app);
        let ctx = egui::Context::default();

        // the box says what it is for and that it is recommended, and it is ticked: the install the window begins has the
        // Manager's step, after what ComfyUI needs, and its launcher passes the word of the classic interface
        let drawn = settled_manager(&mut app, &ctx, &field);
        let texts: Vec<String> = drawn.iter().map(|(t, _)| t.clone()).collect();
        assert!(said(&texts, "Install ComfyUI-Manager"), "{texts:#?}");
        assert!(said(&texts, "highly recommended"), "{texts:#?}");
        assert!(
            texts.iter().any(|t| t.contains("ComfyUI-Manager")
                && t.contains("it needs 30GiB free")
                && t.starts_with("Installs a Python environment with")),
            "the sentence under the button says what is ticked: {texts:#?}"
        );
        assert_eq!(app.chosen_options(), ["manager"]);
        let (pip, words) = made_of(&app);
        assert_eq!(pip.last().map(String::as_str), Some("manager"), "{pip:?}");
        assert_eq!(words, ["--enable-manager-legacy-ui"]);

        // a click on the box unticks it: the install is what it was before the option, and the sentence says so
        let label = where_is(&drawn, "Install ComfyUI-Manager").expect("the box is drawn");
        let drawn = click_in(&ctx, label.center(), |ui| {
            app.show_runtime_manager(ui, &field)
        });
        assert!(
            app.chosen_options().is_empty(),
            "{:?}",
            app.chosen_options()
        );
        let (pip, words) = made_of(&app);
        assert!(!pip.iter().any(|id| id == "manager") && words.is_empty());
        let texts: Vec<String> = drawn.iter().map(|(t, _)| t.clone()).collect();
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("Installs a Python environment") && !t.contains("Manager")),
            "{texts:#?}"
        );
        assert!(
            said(&texts, "Install ComfyUI-Manager"),
            "the box is still there to be ticked"
        );

        // and a click again ticks it
        let label = where_is(&drawn, "Install ComfyUI-Manager").unwrap();
        click_in(&ctx, label.center(), |ui| {
            app.show_runtime_manager(ui, &field)
        });
        assert_eq!(app.chosen_options(), ["manager"]);

        // while an install runs the boxes are not changed: what is ticked is what that install does
        let release = std::sync::Arc::new(AtomicBool::new(false));
        let held = release.clone();
        app.runtime.install = Some(Job::spawn(
            "v1".to_string(),
            "Installing",
            || {},
            move |send| {
                while !held.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                send(crate::jobs::JobEvent::Done(PathBuf::new()));
            },
        ));
        let drawn = settled_manager(&mut app, &ctx, &field);
        let label = where_is(&drawn, "Install ComfyUI-Manager").unwrap();
        click_in(&ctx, label.center(), |ui| {
            app.show_runtime_manager(ui, &field)
        });
        assert_eq!(
            app.chosen_options(),
            ["manager"],
            "an install that is going is not changed"
        );
        release.store(true, Ordering::SeqCst);

        // an engine whose document has no option has no box
        let mut other = app_for(&scratch("f99b"), "llamacpp");
        let field = manager_field(&other);
        let drawn = painted(&ctx, |ui| other.show_runtime_manager(ui, &field));
        assert!(
            !said(&drawn, "highly recommended") && !said(&drawn, "Install ComfyUI-Manager"),
            "{drawn:#?}"
        );
        assert!(other.chosen_options().is_empty());
    }

    #[test]
    fn p3_f99a_an_install_that_stopped_is_gone_on_with_the_parts_it_was_begun_with_and_not_the_boxes(
    ) {
        let dir = scratch("f99a");
        let mut app = app_for(&dir, "comfyui");
        let stopped = |label: &str, steps: &[&str]| Unfinished {
            label: label.into(),
            dir: dir.join(format!("{label}.partial")),
            size_bytes: GB,
            done: 3,
            total: steps.len(),
            next: Some("PyTorch".into()),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            touched: None,
            source: None,
        };
        app.runtime.disk.unfinished = vec![
            stopped(
                "with",
                &[
                    "source",
                    "venv",
                    "pip:torch_cpu",
                    "pip:requirements",
                    "pip:manager",
                    "check",
                ],
            ),
            stopped(
                "without",
                &[
                    "source",
                    "venv",
                    "pip:torch_cpu",
                    "pip:requirements",
                    "check",
                ],
            ),
        ];
        // the box is ticked (as the document ticks it): the one begun without the Manager is gone on with without it
        assert_eq!(app.chosen_options(), ["manager"]);
        assert_eq!(
            app.options_it_began_with("with"),
            Some(vec!["manager".to_string()])
        );
        assert_eq!(app.options_it_began_with("without"), Some(Vec::new()));
        // the box unticked: the one begun with the Manager is gone on with it
        app.runtime.options = Some(("comfyui".into(), Vec::new()));
        assert_eq!(
            app.options_it_began_with("with"),
            Some(vec!["manager".to_string()])
        );
        // an install the window knows nothing of has no record to go by: the boxes are what is asked
        assert_eq!(app.options_it_began_with("unknown"), None);
        // and what the window asks for is what the plan is made of
        let plan_of = |chosen: &[String]| {
            let spec = app.install_spec_with(chosen).expect("a spec");
            let InstallKind::PythonApp(plan) = spec.kind else {
                panic!()
            };
            plan.pip.iter().any(|p| p.id == "manager")
        };
        assert!(plan_of(&["manager".to_string()]) && !plan_of(&[]));
        // what is ticked for one engine is not ticked for another: the choice is kept with the engine it was made for
        app.runtime.options = Some(("another".into(), Vec::new()));
        assert_eq!(
            app.chosen_options(),
            ["manager"],
            "the document's own choice"
        );
    }

    fn stopped_install(dir: &Path) -> PathBuf {
        let stopped = dir.join("data/comfyui/v0.39.0.partial");
        std::fs::create_dir_all(stopped.join("venv")).unwrap();
        std::fs::write(stopped.join("venv/x"), vec![0u8; 4096]).unwrap();
        let steps: Vec<StepRecord> = [
            ("source", "the source of v0.39.0"),
            ("node:gguf", "ComfyUI-GGUF"),
            ("venv", "the Python environment"),
            ("pip:torch_amd", "PyTorch for AMD GPUs (ROCm 7.2)"),
            ("pip:requirements", "what ComfyUI needs"),
            ("needs:gguf", "what ComfyUI-GGUF needs"),
            ("check", "the check that it runs"),
        ]
        .iter()
        .map(|(id, label)| StepRecord {
            id: id.to_string(),
            label: label.to_string(),
        })
        .collect();
        let mut journal = Journal::new("v0.39.0", "f".into(), steps);
        for id in ["source", "node:gguf", "venv"] {
            journal.mark(id);
        }
        journal.write(&stopped).unwrap();
        stopped
    }

    static RELEASE: AtomicBool = AtomicBool::new(false);
    static READS: AtomicUsize = AtomicUsize::new(0);

    /// A reader of the disk that takes as long as the test says, and finds one build and one install that stopped.
    fn slow_reader(_: &InstallSpec) -> Scan {
        READS.fetch_add(1, Ordering::SeqCst);
        let began = Instant::now();
        while !RELEASE.load(Ordering::SeqCst) && began.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(10));
        }
        Scan {
            builds: vec![InstalledBuild {
                label: "v1".into(),
                binary_path: "/x/v1/comfyui".into(),
                install_dir: "/x/v1".into(),
                size_bytes: 5 * GB,
            }],
            unfinished: vec![Unfinished {
                label: "v2".into(),
                dir: "/x/v2.partial".into(),
                size_bytes: 7 * GB,
                done: 3,
                total: 7,
                next: Some("PyTorch".into()),
                steps: Vec::new(),
                touched: None,
                source: None,
            }],
        }
    }

    #[test]
    fn p3_f65_a_frame_never_waits_for_the_disk_and_looks_again_when_something_changed() {
        let dir = scratch("f65");
        let mut app = app_for(&dir, "comfyui");
        let field = manager_field(&app);
        app.runtime.disk.reader = Some(slow_reader);
        let ctx = egui::Context::default();

        // the disk is slow: the frame is drawn at once, and says it is looking
        let began = Instant::now();
        let texts = painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the frame waited for the disk: {:?}",
            began.elapsed()
        );
        assert!(said(&texts, "Looking at what is installed"), "{texts:?}");
        assert!(!app.runtime.disk.known);
        for _ in 0..3 {
            painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
        }
        assert_eq!(
            READS.load(Ordering::SeqCst),
            1,
            "one scan at a time, however many frames wait for it"
        );

        // it answers: the next frame shows the build and the install that stopped
        RELEASE.store(true, Ordering::SeqCst);
        wait_until(Duration::from_secs(10), "the scan to be taken in", || {
            painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
            app.runtime.disk.known
        });
        let texts = painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
        assert!(said(&texts, "Installed builds"), "{texts:?}");
        assert!(said(&texts, "v1 (5.0 GB)"), "{texts:?}");
        assert!(said(&texts, "Unfinished installs"), "{texts:?}");
        assert!(said(&texts, "v2 (3 of 7 steps done, 7.0 GB)"), "{texts:?}");
        assert!(!said(&texts, "Looking at what is installed"));
        assert_eq!(
            READS.load(Ordering::SeqCst),
            1,
            "and no more scans without a reason"
        );

        // an install that ends, or a delete, changes the disk: the next frame reads it again
        app.runtime.disk.stale = true;
        wait_until(Duration::from_secs(10), "the second scan", || {
            painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
            READS.load(Ordering::SeqCst) == 2
        });

        // so does coming back to the manager after it was away (the section folded, the window closed)
        for _ in 0..5 {
            painted(&ctx, |_| {});
        }
        wait_until(Duration::from_secs(10), "the third scan", || {
            painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
            READS.load(Ordering::SeqCst) == 3
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn p3_f66_an_install_that_stopped_is_listed_and_goes_on_or_is_deleted_without_stopping_the_window(
    ) {
        let dir = scratch("f66");
        let mut app = app_for(&dir, "comfyui");
        let field = manager_field(&app);
        let ctx = egui::Context::default();

        // a stopped install on the disk: three of seven steps done, and a directory that holds something
        let stopped = stopped_install(&dir);

        let mut texts = Vec::new();
        wait_until(Duration::from_secs(10), "the scan of the disk", || {
            texts = painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
            app.runtime.disk.known
        });
        texts = painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
        assert!(said(&texts, "Unfinished installs"), "{texts:?}");
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("v0.39.0 (3 of 7 steps done, ")),
            "{texts:?}"
        );
        assert!(said(&texts, "Continue"), "{texts:?}");
        assert!(
            !said(&texts, "Installed builds"),
            "it is not a build: {texts:?}"
        );

        // going on is asked for the release of that label, from the document alone, with its pin
        let engine = app.runtime_engine().unwrap();
        let request = feed::request_for_label(engine, &app.host_facts, "v0.39.0").unwrap();
        assert_eq!(request.label, "v0.39.0");
        assert_eq!(
            request.base.url,
            "https://github.com/Comfy-Org/ComfyUI/archive/refs/tags/v0.39.0.tar.gz"
        );
        assert!(request.base.pin.is_some(), "the tested release is checked");
        assert!(feed::request_for_label(engine, &app.host_facts, "v0.40.0")
            .unwrap()
            .base
            .pin
            .is_none());
        let llama = app_for(&dir, "llamacpp");
        assert!(
            feed::request_for_label(llama.runtime_engine().unwrap(), &llama.host_facts, "b1")
                .is_none(),
            "an engine whose builds are archives has nothing to go on with"
        );

        // deleting it takes a thread of its own: the engine cannot be changed meanwhile, the window is not
        // waiting, and when it is gone the disk is read again and the log says so
        app.set_bin_override(Some(stopped.join("comfyui")));
        app.start_delete("v0.39.0".into(), stopped.clone(), true, &ctx);
        assert!(app.runtime.delete.is_some() && app.engine_locked());
        let texts = painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
        assert!(said(&texts, "Deleting v0.39.0..."), "{texts:?}");
        wait_until(Duration::from_secs(10), "the delete to end", || {
            app.poll_runtime_manager();
            app.runtime.delete.is_none()
        });
        assert!(!stopped.exists(), "the directory is gone");
        assert!(app.runtime.disk.stale, "the disk is read again");
        assert!(
            app.bin_override().is_none(),
            "the build in use was this one"
        );
        assert!(
            app.log_lines
                .iter()
                .any(|l| l == "Deleted unfinished ComfyUI install: v0.39.0"),
            "{:?}",
            app.log_lines
        );
        assert!(!app.engine_locked());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn p3_f69_an_install_that_stopped_is_said_in_the_log_when_the_application_starts() {
        let dir = scratch("f69");
        let stopped = stopped_install(&dir);
        let mut app = app_for(&dir, "comfyui");
        wait_until(Duration::from_secs(10), "the log to say it", || {
            app.poll_runtime_manager();
            app.log_lines
                .iter()
                .any(|l| l.contains("stopped before it was whole"))
        });
        let line = app
            .log_lines
            .iter()
            .find(|l| l.contains("stopped before it was whole"))
            .unwrap();
        assert!(
            line.starts_with(
                "ComfyUI: an install of v0.39.0 stopped before it was whole (3 of 7 steps done, "
            ) && line.contains("Unfinished installs"),
            "{line}"
        );
        // once, and only for the engine that has one
        app.poll_runtime_manager();
        assert_eq!(
            app.log_lines
                .iter()
                .filter(|l| l.contains("stopped before it was whole"))
                .count(),
            1
        );
        let mut llama = app_for(&dir, "llamacpp");
        std::thread::sleep(Duration::from_millis(300));
        llama.poll_runtime_manager();
        assert!(llama
            .log_lines
            .iter()
            .all(|l| !l.contains("stopped before")));

        // an install that is going on is not one that stopped: it is not said
        let held = crate::prims::install::hold_for_test(&stopped);
        let mut again = app_for(&dir, "comfyui");
        std::thread::sleep(Duration::from_millis(300));
        again.poll_runtime_manager();
        assert!(
            again
                .log_lines
                .iter()
                .all(|l| !l.contains("stopped before")),
            "{:?}",
            again.log_lines
        );
        drop(held);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn p3_f67_the_install_says_what_it_is_doing_under_its_bar_and_is_read_again_when_it_ends() {
        let dir = scratch("f67");
        let mut app = app_for(&dir, "comfyui");
        let field = manager_field(&app);
        let ctx = egui::Context::default();
        let (release, held) = crossbeam_channel::bounded::<()>(1);
        app.runtime.install = Some(Job::spawn(
            "v0.39.0".into(),
            "Installing",
            || {},
            move |send| {
                send(crate::jobs::JobEvent::Fraction(0.25));
                send(crate::jobs::JobEvent::Detail(
                    "Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2) · downloading torch.whl: 2.0 GB of 6.0 GB".into(),
                ));
                let _ = held.recv_timeout(Duration::from_secs(30));
                send(crate::jobs::JobEvent::Done(PathBuf::from(
                    "/x/v0.39.0/comfyui",
                )));
            },
        ));
        wait_until(Duration::from_secs(10), "the detail to arrive", || {
            app.poll_runtime_manager();
            app.runtime
                .install
                .as_ref()
                .is_some_and(|j| j.detail().is_some())
        });
        app.runtime.cancel = Some(Default::default());
        let texts = painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
        assert!(said(&texts, "Installing v0.39.0: 25%"), "{texts:?}");
        assert!(
            said(&texts, "Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2) · downloading torch.whl: 2.0 GB of 6.0 GB"),
            "{texts:?}"
        );
        assert!(said(&texts, "Cancel"));

        // when it ends the build is the one in use, and the disk is read again
        app.runtime.disk.stale = false;
        release.send(()).unwrap();
        wait_until(Duration::from_secs(10), "the end of the install", || {
            app.poll_runtime_manager();
            app.runtime.install.is_none()
        });
        assert!(app.runtime.disk.stale);
        assert_eq!(
            app.bin_override(),
            Some(PathBuf::from("/x/v0.39.0/comfyui"))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn p3_f68_what_an_install_says_many_times_a_second_asks_for_a_repaint_soon_and_not_at_once() {
        let ctx = egui::Context::default();
        let delays = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Duration>::new()));
        let seen = delays.clone();
        ctx.set_request_repaint_callback(move |info| seen.lock().unwrap().push(info.delay));

        ctx.request_repaint();
        let wake = crate::app::calm_wake(ctx.clone());
        for _ in 0..100 {
            wake();
        }
        let delays = delays.lock().unwrap().clone();
        assert_eq!(delays[0], Duration::ZERO, "an ordinary request is at once");
        assert!(
            delays[1..].iter().all(|d| *d >= Duration::from_millis(100)),
            "a calm one is not: {:?}",
            &delays[1..4]
        );
    }

    #[test]
    fn p3_i15_a_look_up_of_builds_that_ends_has_its_list_brought_into_view_for_a_few_frames() {
        let dir = scratch("z15");
        let mut app = app_for(&dir, "llamacpp");
        let field = manager_field(&app);
        let ctx = egui::Context::default();
        let (tx, rx) = crossbeam_channel::unbounded();
        app.runtime.lookup_rx = Some(rx);
        app.runtime.show_lookup = true;
        tx.send(BuildLookupEvent::Done).unwrap();
        app.poll_runtime_manager();
        assert_eq!(app.runtime.scroll_to_results, 3);
        // a list is as tall as its rows a frame after they came, so one frame is not enough
        for left in [2, 1, 0, 0] {
            painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
            assert_eq!(app.runtime.scroll_to_results, left);
        }
        // a list that is closed does not keep the scroll for when it is opened again
        app.runtime.scroll_to_results = 3;
        app.runtime.show_lookup = false;
        painted(&ctx, |ui| app.show_runtime_manager(ui, &field));
        assert_eq!(app.runtime.scroll_to_results, 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
