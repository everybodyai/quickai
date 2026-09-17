use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use eframe::egui;

use crate::downloader::{self, DownloadEvent, DownloadHandle};
use crate::gpu::{self, GpuVendor};
use crate::hardware;
use crate::llama_install::{self, InstallEvent, InstallHandle};
use crate::llama_releases::{self, BuildLookupEvent, LlamaBuild};
use crate::models::{discover_all, ModelEntry};
use crate::paths;
use crate::proxy::TrafficEvent;
use crate::remote::{self, LookupEvent, RemoteModel};
use crate::server_manager::{ServerManager, ServerState};
use crate::settings::Settings;

const MAX_LOG_LINES: usize = 2000;

pub struct App {
    manager: ServerManager,
    models: Vec<ModelEntry>,
    selected: usize,
    port: u16,
    ctx_size: u32,
    n_gpu_layers: u32,
    log_lines: VecDeque<String>,
    bundled_models_dir: PathBuf,
    models_dir: PathBuf,
    llama_server_bin: PathBuf,
    missing_binary_warned: bool,
    /// Scratch text-edit buffer for the combined URL/path/search box — not
    /// persisted. What it means depends on its shape: a `http(s)://` URL is
    /// downloaded, an existing local file path is imported, anything else
    /// is treated as a Hugging Face search term (see the "Go" button's
    /// dynamic label right where it's used).
    model_input: String,
    download: Option<DownloadHandle>,
    show_lookup: bool,
    lookup_rx: Option<Receiver<LookupEvent>>,
    lookup_results: Vec<RemoteModel>,
    lookup_status: String,
    total_ram_bytes: u64,
    recommended_filename: Option<String>,
    dark_mode: bool,
    show_api_info: bool,
    show_settings: bool,
    show_about: bool,
    /// Appended verbatim to the llama-server command line — see ⚙ Settings.
    extra_args: String,
    /// `None` unless the user picked a custom folder/binary in ⚙ Settings;
    /// see `Settings::models_dir_override` for why this isn't always Some.
    models_dir_override: Option<PathBuf>,
    llama_bin_override: Option<PathBuf>,
    /// Scratch text-edit buffers for the ⚙ Settings window, separate from
    /// the `PathBuf`s above so a half-typed path doesn't take effect until
    /// "Apply" is clicked.
    models_dir_input: String,
    llama_bin_input: String,
    settings_error: Option<String>,
    /// Off by default — see `Settings::enable_traffic_proxy`.
    enable_traffic_proxy: bool,
    active_tab: Tab,
    traffic: VecDeque<TrafficTurn>,
    /// `None` shows every session at once; `Some(session_key)` filters the
    /// 📡 Live tab down to just that client — see `TrafficTurn::session`.
    selected_session: Option<String>,
    /// GPU vendor(s) this machine looks like it has — see `gpu::detect`.
    /// Best-effort and computed once; only used to warn about a vendor-
    /// locked llama.cpp build (CUDA/ROCm) that won't match the hardware.
    gpu_vendors: Vec<GpuVendor>,
    show_llama_lookup: bool,
    llama_lookup_rx: Option<Receiver<BuildLookupEvent>>,
    llama_lookup_results: Vec<LlamaBuild>,
    llama_lookup_status: String,
    /// Live text filter over `llama_lookup_results` — cleared whenever a
    /// fresh lookup starts, since it's a client-side narrowing of that
    /// specific result set rather than a search term of its own.
    llama_lookup_filter: String,
    llama_install: Option<InstallHandle>,
    /// A delete awaiting user confirmation — see `PendingDelete` and
    /// `show_confirm_delete_modal`.
    pending_delete: Option<PendingDelete>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Main,
    Live,
}

/// One request/response pair shown in the 📡 Live tab. `response` fills
/// in as it streams; `done`/`error` mark when there's nothing more coming.
struct TrafficTurn {
    id: u64,
    /// Which client this came from — see `TrafficEvent::Request`'s doc.
    session: String,
    method: String,
    path: String,
    prompt: Option<String>,
    /// Best-effort description of any documents/images sent alongside the
    /// prompt — see `TrafficEvent::Request::attachments`.
    attachments: Vec<String>,
    response: String,
    done: bool,
    error: Option<String>,
}

const MAX_TRAFFIC_TURNS: usize = 200;

/// A destructive delete waiting on user confirmation via a modal — see
/// `show_confirm_delete_modal`. Holding the info needed to both describe
/// and perform the delete means the modal doesn't need to re-derive it
/// (or risk acting on a stale index) once the user confirms.
enum PendingDelete {
    Model { filename: String, path: PathBuf },
    LlamaBuild { label: String, install_dir: PathBuf },
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();

        let bundled_models_dir = paths::bundled_models_dir();
        let models_dir_override = settings.models_dir_override.as_deref().map(PathBuf::from);
        let models_dir = models_dir_override
            .clone()
            .unwrap_or_else(paths::user_models_dir);
        let _ = std::fs::create_dir_all(&models_dir);
        let llama_bin_override = settings.llama_bin_override.as_deref().map(PathBuf::from);
        let llama_server_bin = llama_bin_override
            .clone()
            .unwrap_or_else(paths::llama_server_bin);
        let models = discover_all(&bundled_models_dir, &models_dir);

        let selected = settings
            .last_model_filename
            .as_deref()
            .and_then(|name| models.iter().position(|m| m.filename == name))
            .unwrap_or(0);

        let total_ram_bytes = hardware::total_ram_bytes();
        let recommended_filename =
            hardware::recommend(&models, total_ram_bytes, settings.n_gpu_layers);

        cc.egui_ctx.set_visuals(if settings.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        let models_dir_input = models_dir.display().to_string();
        let llama_bin_input = llama_bin_override
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        Self {
            manager: ServerManager::new(),
            models,
            selected,
            port: settings.port,
            ctx_size: settings.ctx_size,
            n_gpu_layers: settings.n_gpu_layers,
            log_lines: VecDeque::new(),
            bundled_models_dir,
            models_dir,
            llama_server_bin,
            missing_binary_warned: false,
            model_input: String::new(),
            download: None,
            show_lookup: false,
            lookup_rx: None,
            lookup_results: Vec::new(),
            lookup_status: String::new(),
            total_ram_bytes,
            recommended_filename,
            dark_mode: settings.dark_mode,
            show_api_info: false,
            show_settings: false,
            show_about: false,
            extra_args: settings.extra_args,
            models_dir_override,
            llama_bin_override,
            models_dir_input,
            llama_bin_input,
            settings_error: None,
            enable_traffic_proxy: settings.enable_traffic_proxy,
            active_tab: Tab::Main,
            traffic: VecDeque::new(),
            selected_session: None,
            gpu_vendors: gpu::detect(),
            show_llama_lookup: false,
            llama_lookup_rx: None,
            llama_lookup_results: Vec::new(),
            llama_lookup_status: String::new(),
            llama_lookup_filter: String::new(),
            llama_install: None,
            pending_delete: None,
        }
    }

    /// Re-scans both models folders, keeping the current selection if it's
    /// still present. Shared by the Refresh button and the ⚙ Settings
    /// "Apply"/"Reset to default" actions on the downloaded-models folder.
    fn rescan_models(&mut self) {
        let current = self.models.get(self.selected).map(|m| m.filename.clone());
        self.models = discover_all(&self.bundled_models_dir, &self.models_dir);
        self.selected = current
            .and_then(|f| self.models.iter().position(|m| m.filename == f))
            .unwrap_or(0);
        self.recommended_filename =
            hardware::recommend(&self.models, self.total_ram_bytes, self.n_gpu_layers);
    }

    fn push_log(&mut self, line: String) {
        self.log_lines.push_back(line);
        while self.log_lines.len() > MAX_LOG_LINES {
            self.log_lines.pop_front();
        }
    }

    fn save_settings(&self) {
        Settings {
            last_model_filename: self.models.get(self.selected).map(|m| m.filename.clone()),
            port: self.port,
            ctx_size: self.ctx_size,
            n_gpu_layers: self.n_gpu_layers,
            dark_mode: self.dark_mode,
            models_dir_override: self
                .models_dir_override
                .as_ref()
                .map(|p| p.display().to_string()),
            llama_bin_override: self
                .llama_bin_override
                .as_ref()
                .map(|p| p.display().to_string()),
            extra_args: self.extra_args.clone(),
            enable_traffic_proxy: self.enable_traffic_proxy,
        }
        .save();
    }

    /// Applies one proxy `TrafficEvent` to the 📡 Live tab's turn list.
    fn apply_traffic_event(&mut self, event: TrafficEvent) {
        match event {
            TrafficEvent::Request {
                id,
                session,
                method,
                path,
                prompt,
                attachments,
            } => {
                if self.traffic.len() >= MAX_TRAFFIC_TURNS {
                    self.traffic.pop_front();
                }
                self.traffic.push_back(TrafficTurn {
                    id,
                    session,
                    method,
                    path,
                    prompt,
                    attachments,
                    response: String::new(),
                    done: false,
                    error: None,
                });
            }
            TrafficEvent::ResponseDelta { id, text } => {
                if let Some(turn) = self.traffic.iter_mut().rev().find(|t| t.id == id) {
                    turn.response.push_str(&text);
                }
            }
            TrafficEvent::ResponseDone { id, full_text } => {
                if let Some(turn) = self.traffic.iter_mut().rev().find(|t| t.id == id) {
                    if let Some(text) = full_text {
                        turn.response = text;
                    }
                    turn.done = true;
                }
            }
            TrafficEvent::ResponseError { id, message } => {
                if let Some(turn) = self.traffic.iter_mut().rev().find(|t| t.id == id) {
                    turn.error = Some(message);
                    turn.done = true;
                }
            }
        }
    }

    /// Drains the in-flight download's channel, if any. On completion,
    /// re-scans models_dir (the new file is now sitting there) and selects
    /// it; on failure, just logs — the caller already cleaned up the
    /// `.part` file.
    fn poll_download(&mut self) {
        let Some(mut handle) = self.download.take() else {
            return;
        };
        let mut result = None;
        while let Ok(event) = handle.rx.try_recv() {
            match event {
                DownloadEvent::Progress { downloaded, total } => {
                    handle.downloaded = downloaded;
                    handle.total = total;
                }
                DownloadEvent::Done(filename) => result = Some(Ok(filename)),
                DownloadEvent::Failed(msg) => result = Some(Err(msg)),
            }
        }
        match result {
            Some(Ok(filename)) => {
                self.push_log(format!("Download complete: {filename}"));
                let current = self.models.get(self.selected).map(|m| m.filename.clone());
                self.models = discover_all(&self.bundled_models_dir, &self.models_dir);
                self.selected = self
                    .models
                    .iter()
                    .position(|m| m.filename == filename)
                    .or_else(|| {
                        current.and_then(|f| self.models.iter().position(|m| m.filename == f))
                    })
                    .unwrap_or(0);
                self.recommended_filename =
                    hardware::recommend(&self.models, self.total_ram_bytes, self.n_gpu_layers);
                self.save_settings();
            }
            Some(Err(msg)) => self.push_log(format!("Download failed: {msg}")),
            None => self.download = Some(handle),
        }
    }

    /// Drains the online-lookup channel, if a search is running.
    fn poll_lookup(&mut self) {
        let Some(rx) = self.lookup_rx.take() else {
            return;
        };
        let mut done = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                LookupEvent::Found(m) => self.lookup_results.push(m),
                LookupEvent::Status(s) => self.lookup_status = s,
                LookupEvent::Done => done = true,
            }
        }
        if done {
            self.lookup_status = if self.lookup_results.is_empty() {
                "No compatible models found.".to_string()
            } else {
                format!("Found {} model(s).", self.lookup_results.len())
            };
        } else {
            self.lookup_rx = Some(rx);
        }
    }

    /// Drains the llama.cpp build-lookup channel, if a search is running —
    /// same shape as `poll_lookup`, for the models list.
    fn poll_llama_lookup(&mut self) {
        let Some(rx) = self.llama_lookup_rx.take() else {
            return;
        };
        let mut done = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                BuildLookupEvent::Found(b) => self.llama_lookup_results.push(b),
                BuildLookupEvent::Status(s) => self.llama_lookup_status = s,
                BuildLookupEvent::Done => done = true,
            }
        }
        if done {
            self.llama_lookup_status = if self.llama_lookup_results.is_empty() {
                "No compatible builds found.".to_string()
            } else {
                format!("Found {} build(s).", self.llama_lookup_results.len())
            };
        } else {
            self.llama_lookup_rx = Some(rx);
        }
    }

    /// Drains the in-flight llama-server install's channel, if any. On
    /// completion, points ⚙ Settings' binary override straight at the
    /// freshly-installed executable and persists it — same idea as
    /// `poll_download` selecting a newly-downloaded model.
    fn poll_llama_install(&mut self) {
        let Some(mut handle) = self.llama_install.take() else {
            return;
        };
        let mut result = None;
        while let Ok(event) = handle.rx.try_recv() {
            match event {
                InstallEvent::Progress { downloaded, total } => {
                    handle.downloaded = downloaded;
                    handle.total = total;
                }
                InstallEvent::Done(path) => result = Some(Ok(path)),
                InstallEvent::Failed(msg) => result = Some(Err(msg)),
            }
        }
        match result {
            Some(Ok(path)) => {
                self.push_log(format!("llama-server build installed: {}", path.display()));
                self.llama_bin_input = path.display().to_string();
                self.llama_server_bin = path.clone();
                self.llama_bin_override = Some(path);
                self.missing_binary_warned = false;
                self.settings_error = None;
                self.save_settings();
            }
            Some(Err(msg)) => self.push_log(format!("llama-server install failed: {msg}")),
            None => self.llama_install = Some(handle),
        }
    }

    fn state_color(state: ServerState) -> egui::Color32 {
        match state {
            ServerState::Idle => egui::Color32::GRAY,
            ServerState::Starting | ServerState::Stopping => egui::Color32::from_rgb(212, 160, 23),
            ServerState::Running => egui::Color32::from_rgb(46, 139, 50),
            ServerState::Crashed => egui::Color32::from_rgb(192, 57, 43),
        }
    }
    fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut close_clicked = false;
        egui::Window::new("⚙ Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("App").strong());
                ui.add_space(4.0);

                ui.label("Downloaded models folder:").on_hover_text(
                    "Where models fetched via the \"Model\" box (download/import/search) or \
                     \"Look up online\" are saved and read back from. The models bundled with \
                     the app itself live in their own read-only folder regardless of this.",
                );
                ui.add(egui::TextEdit::singleline(&mut self.models_dir_input).desired_width(ui.available_width()));
                ui.horizontal(|ui| {
                    if ui
                        .button("Apply")
                        .on_hover_text("Use this folder from now on and re-scan it for models.")
                        .clicked()
                    {
                        let candidate = PathBuf::from(self.models_dir_input.trim());
                        if std::fs::create_dir_all(&candidate).is_ok() {
                            self.models_dir = candidate.clone();
                            self.models_dir_override = Some(candidate);
                            self.rescan_models();
                            self.save_settings();
                            self.settings_error = None;
                        } else {
                            self.settings_error = Some(format!("Could not create/use {}", self.models_dir_input.trim()));
                        }
                    }
                    if ui.button("Reset to default").clicked() {
                        let default_dir = paths::user_models_dir();
                        let _ = std::fs::create_dir_all(&default_dir);
                        self.models_dir = default_dir.clone();
                        self.models_dir_input = default_dir.display().to_string();
                        self.models_dir_override = None;
                        self.rescan_models();
                        self.save_settings();
                        self.settings_error = None;
                    }
                });

                let mut downloaded_model_indices: Vec<usize> = self
                    .models
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| !m.path.starts_with(&self.bundled_models_dir))
                    .map(|(i, _)| i)
                    .collect();
                // Most-recently-downloaded first — a model has no other
                // notion of "latest", so this uses the file's own mtime.
                downloaded_model_indices.sort_by_key(|&i| {
                    std::cmp::Reverse(
                        std::fs::metadata(&self.models[i].path)
                            .and_then(|m| m.modified())
                            .ok(),
                    )
                });
                if !downloaded_model_indices.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Downloaded models").strong())
                        .on_hover_text(
                            "Models fetched via the \"Model\" box, \"Look up online\", or \
                             dropped into the folder above by hand, newest first. Bundled \
                             models aren't listed here — they can't be deleted.",
                        );
                    let mut to_delete: Option<PendingDelete> = None;
                    egui::ScrollArea::vertical()
                        .id_salt("downloaded_models_scroll")
                        .auto_shrink([false, true])
                        .max_height(list_scroll_height(ui, DELETE_LIST_MAX_ROWS))
                        .show(ui, |ui| {
                            for &i in &downloaded_model_indices {
                                let m = &self.models[i];
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(egui::Button::new("🗑").small())
                                        .on_hover_text("Delete this model from disk")
                                        .clicked()
                                    {
                                        to_delete = Some(PendingDelete::Model {
                                            filename: m.filename.clone(),
                                            path: m.path.clone(),
                                        });
                                    }
                                    ui.label(format!(
                                        "{} ({})",
                                        m.display,
                                        hardware::format_bytes(m.size_bytes)
                                    ));
                                });
                            }
                        });
                    if let Some(pending) = to_delete {
                        self.pending_delete = Some(pending);
                    }
                }

                ui.add_space(10.0);
                let active = self.manager.is_active();
                ui.add_enabled_ui(!active, |ui| {
                    if ui
                        .checkbox(&mut self.enable_traffic_proxy, "Show live conversation traffic")
                        .on_hover_text(
                            "Adds a small local proxy in front of llama-server so a new \"📡 \
                             Live\" button can show chat/completion requests and replies live, \
                             from any client — the built-in chat UI or an external app via the \
                             🔌 API. Off by default: clients then connect straight to \
                             llama-server, with no extra hop or overhead. Takes effect the next \
                             time you click Start.",
                        )
                        .changed()
                    {
                        self.save_settings();
                    }
                });
                if active {
                    ui.label(
                        egui::RichText::new("(Stop the server to change this — it applies on the next Start.)")
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                egui::CollapsingHeader::new("Advanced: llama.cpp")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label("llama-server binary:").on_hover_text(
                            "Override the auto-detected llama-server executable — point this \
                             at a custom build (e.g. a different backend, or a newer version), \
                             or paste a direct download URL to a release archive (.zip/.tar.gz) \
                             to fetch and install it automatically. Leave blank to use the \
                             bundled/auto-detected one.",
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.llama_bin_input)
                                    .desired_width(ui.available_width() - 30.0)
                                    .hint_text("Local binary path or a download URL..."),
                            );
                            if ui
                                .button("...")
                                .on_hover_text("Browse for a llama-server executable")
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new().pick_file() {
                                    self.llama_bin_input = path.display().to_string();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            let trimmed = self.llama_bin_input.trim().to_string();
                            let is_url = downloader::looks_like_url(&trimmed);
                            let apply_label = if is_url { "Download" } else { "Apply" };
                            let apply_hover = if is_url {
                                "Download and install a llama-server build from this direct \
                                 URL (e.g. a release asset link copied from GitHub), then \
                                 switch to it automatically — no need to go through \"Look up \
                                 llama.cpp builds\" first."
                            } else {
                                "Point at an already-downloaded llama-server executable on \
                                 disk, or leave blank to reset to the bundled/auto-detected one."
                            };
                            if ui
                                .add_enabled(
                                    self.llama_install.is_none(),
                                    egui::Button::new(apply_label),
                                )
                                .on_hover_text(apply_hover)
                                .clicked()
                            {
                                if trimmed.is_empty() {
                                    self.llama_server_bin = paths::llama_server_bin();
                                    self.llama_bin_override = None;
                                    self.missing_binary_warned = false;
                                    self.settings_error = None;
                                    self.save_settings();
                                } else if is_url {
                                    self.settings_error = None;
                                    let label = llama_install::label_from_url(&trimmed);
                                    let ctx_clone = ui.ctx().clone();
                                    self.llama_install = Some(llama_install::spawn_install(
                                        trimmed,
                                        label,
                                        move || ctx_clone.request_repaint(),
                                    ));
                                } else {
                                    let candidate = PathBuf::from(&trimmed);
                                    if candidate.is_file() {
                                        self.llama_server_bin = candidate.clone();
                                        self.llama_bin_override = Some(candidate);
                                        self.missing_binary_warned = false;
                                        self.settings_error = None;
                                    } else {
                                        self.settings_error = Some(format!("No file at {trimmed}"));
                                    }
                                    self.save_settings();
                                }
                            }
                            if ui.button("Reset to default").clicked() {
                                self.llama_bin_input.clear();
                                self.llama_server_bin = paths::llama_server_bin();
                                self.llama_bin_override = None;
                                self.missing_binary_warned = false;
                                self.settings_error = None;
                                self.save_settings();
                            }
                        });

                        let installed_builds = llama_install::list_installed();
                        if !installed_builds.is_empty() {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Installed builds").strong())
                                .on_hover_text(
                                    "llama-server builds downloaded via \"Look up llama.cpp \
                                     builds\" in an earlier run, most recently installed \
                                     first — still listed here even once that list (which only \
                                     ever shows a handful of the most recent GitHub releases) \
                                     no longer includes them. \"Use\" switches to one \
                                     instantly, with no re-download.",
                                );
                            let mut to_select: Option<PathBuf> = None;
                            let mut to_delete: Option<PendingDelete> = None;
                            egui::ScrollArea::vertical()
                                .id_salt("installed_builds_scroll")
                                .auto_shrink([false, true])
                                .max_height(list_scroll_height(ui, DELETE_LIST_MAX_ROWS))
                                .show(ui, |ui| {
                                    for build in &installed_builds {
                                        let is_current = self.llama_bin_override.as_deref()
                                            == Some(build.binary_path.as_path());
                                        ui.horizontal(|ui| {
                                            if ui.selectable_label(is_current, "Use").clicked()
                                                && !is_current
                                            {
                                                to_select = Some(build.binary_path.clone());
                                            }
                                            if ui
                                                .add(egui::Button::new("🗑").small())
                                                .on_hover_text("Delete this installed build")
                                                .clicked()
                                            {
                                                to_delete = Some(PendingDelete::LlamaBuild {
                                                    label: build.label.clone(),
                                                    install_dir: build.install_dir.clone(),
                                                });
                                            }
                                            let backend =
                                                llama_releases::backend_label(&build.label);
                                            ui.label(format!(
                                                "{} ({backend}, {})",
                                                build.label,
                                                hardware::format_bytes(build.size_bytes)
                                            ));
                                        });
                                    }
                                });
                            if let Some(path) = to_select {
                                self.llama_bin_input = path.display().to_string();
                                self.llama_server_bin = path.clone();
                                self.llama_bin_override = Some(path);
                                self.missing_binary_warned = false;
                                self.settings_error = None;
                                self.save_settings();
                            }
                            if let Some(pending) = to_delete {
                                self.pending_delete = Some(pending);
                            }
                        }

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    self.llama_install.is_none(),
                                    egui::Button::new("Look up llama.cpp builds"),
                                )
                                .on_hover_text(
                                    "List recent llama-server builds published on llama.cpp's \
                                     own GitHub releases, filtered to ones that match this \
                                     machine's OS/CPU. Picking one downloads and installs it \
                                     under ~/.quickai/llama-bin and applies it automatically — \
                                     useful when the current binary can't load a model (e.g. a \
                                     newer architecture) or doesn't match your GPU.",
                                )
                                .clicked()
                            {
                                self.show_llama_lookup = true;
                                self.llama_lookup_results.clear();
                                self.llama_lookup_filter.clear();
                                self.llama_lookup_status =
                                    "Checking github.com/ggml-org/llama.cpp releases...".into();
                                let ctx_clone = ui.ctx().clone();
                                self.llama_lookup_rx = Some(llama_releases::spawn_lookup(
                                    move || ctx_clone.request_repaint(),
                                ));
                            }
                            if self.show_llama_lookup
                                && ui
                                    .add(egui::Button::new("X").small())
                                    .on_hover_text("Close build list")
                                    .clicked()
                            {
                                self.show_llama_lookup = false;
                            }
                        });

                        if let Some(handle) = &self.llama_install {
                            ui.add_space(4.0);
                            let text = match handle.total.filter(|t| *t > 0) {
                                Some(total) => format!(
                                    "Installing {}: {} / {}",
                                    handle.label,
                                    hardware::format_bytes(handle.downloaded),
                                    hardware::format_bytes(total)
                                ),
                                None => format!(
                                    "Installing {}: {}",
                                    handle.label,
                                    hardware::format_bytes(handle.downloaded)
                                ),
                            };
                            ui.colored_label(egui::Color32::from_rgb(212, 160, 23), text);
                            if let Some(total) = handle.total.filter(|t| *t > 0) {
                                ui.add(egui::ProgressBar::new(
                                    handle.downloaded as f32 / total as f32,
                                ));
                            }
                        }

                        if self.show_llama_lookup {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(&self.llama_lookup_status)
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                            if self.llama_lookup_results.len() > 4 {
                                ui.horizontal(|ui| {
                                    ui.label("Filter:");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.llama_lookup_filter)
                                            .desired_width(160.0)
                                            .hint_text("e.g. vulkan, rocm, b110..."),
                                    )
                                    .on_hover_text(
                                        "Narrow the list below by tag, filename, or backend \
                                         (CUDA/Vulkan/SYCL/ROCm/CPU) — matched against what's \
                                         already listed, no extra lookup.",
                                    );
                                    if !self.llama_lookup_filter.is_empty()
                                        && ui.add(egui::Button::new("X").small()).clicked()
                                    {
                                        self.llama_lookup_filter.clear();
                                    }
                                });
                            }

                            let mut to_install: Option<usize> = None;
                            let mut reset_to_default = false;
                            let filter = self.llama_lookup_filter.trim().to_ascii_lowercase();

                            egui::ScrollArea::vertical()
                                .id_salt("llama_lookup_results_scroll")
                                .auto_shrink([false, true])
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    let is_default = self.llama_bin_override.is_none();
                                    if filter.is_empty() {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .selectable_label(is_default, "Bundled/default")
                                                .clicked()
                                                && !is_default
                                            {
                                                reset_to_default = true;
                                            }
                                            ui.label(
                                                egui::RichText::new(
                                                    paths::llama_server_bin().display().to_string(),
                                                )
                                                .small()
                                                .color(egui::Color32::GRAY),
                                            );
                                        });
                                    }
                                    let mut shown = 0;
                                    for (i, b) in self.llama_lookup_results.iter().enumerate() {
                                        let backend = llama_releases::backend_label(&b.asset_name);
                                        if !filter.is_empty() {
                                            let haystack = format!("{} {} {backend}", b.tag, b.asset_name)
                                                .to_ascii_lowercase();
                                            if !haystack.contains(&filter) {
                                                continue;
                                            }
                                        }
                                        shown += 1;
                                        let label = b.install_label();
                                        let is_current = self
                                            .llama_bin_override
                                            .as_ref()
                                            .is_some_and(|p| p.display().to_string().contains(&label));
                                        ui.horizontal(|ui| {
                                            if ui.selectable_label(is_current, "Use").clicked()
                                                && !is_current
                                            {
                                                to_install = Some(i);
                                            }
                                            let size = b
                                                .size_bytes
                                                .map(hardware::format_bytes)
                                                .unwrap_or_else(|| "?".into());
                                            ui.label(format!(
                                                "{} — {} ({backend}, {size})",
                                                b.tag, b.asset_name
                                            ));
                                        });
                                        if let Some(warning) =
                                            gpu::mismatch_warning(&self.gpu_vendors, backend)
                                        {
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
                                                self.llama_lookup_filter.trim()
                                            ))
                                            .small()
                                            .color(egui::Color32::GRAY),
                                        );
                                    }
                                });

                            if reset_to_default {
                                self.llama_bin_input.clear();
                                self.llama_server_bin = paths::llama_server_bin();
                                self.llama_bin_override = None;
                                self.missing_binary_warned = false;
                                self.settings_error = None;
                                self.save_settings();
                            }
                            if let Some(i) = to_install {
                                let b = &self.llama_lookup_results[i];
                                let ctx_clone = ui.ctx().clone();
                                self.llama_install = Some(llama_install::spawn_install(
                                    b.url.clone(),
                                    b.install_label(),
                                    move || ctx_clone.request_repaint(),
                                ));
                            }
                        }

                        ui.add_space(8.0);
                        ui.label("Extra command-line arguments:").on_hover_text(
                            "Appended verbatim to the llama-server command, after the built-in \
                             options (port/context size/GPU layers). Space-separated; wrap a \
                             value containing spaces in \"double quotes\". An invalid or \
                             unsupported flag will make the server fail to start — check the \
                             log pane at the bottom of the main window if Start doesn't work.\n\n\
                             Examples:\n\
                             --threads 8         number of CPU threads to use\n\
                             --flash-attn on     enable flash attention (faster; needs support)\n\
                             --batch-size 512    tokens processed per batch\n\
                             --no-mmap           disable memory-mapping the model file\n\
                             --rope-freq-base 0  override RoPE frequency base (long-context models)",
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.extra_args)
                                .desired_width(ui.available_width())
                                .hint_text("e.g. --threads 8 --flash-attn on"),
                        );
                    });

                if let Some(msg) = &self.settings_error {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(192, 57, 43), msg);
                }

                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            });
        self.show_settings = open && !close_clicked;
        if !self.show_settings {
            self.save_settings();
        }
    }

    fn show_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        let mut close_clicked = false;
        egui::Window::new("ℹ About QuickAI")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .max_height(600.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("QuickAI v{}", env!("CARGO_PKG_VERSION")))
                                .strong()
                                .size(16.0),
                        );
                        ui.label(
                        "A simple GUI for starting and stopping local GGUF language models via \
                         llama-server (llama.cpp), with GPU acceleration through Vulkan.",
                    );
                        ui.add_space(10.0);

                        ui.label(egui::RichText::new("Usage").strong());
                        ui.label(
                            "Pick a model, optionally adjust the port / context size / GPU layers \
                         (⚙ Settings has more, including advanced llama.cpp options), then click \
                         Start and wait for the status to turn green. \"Open Chat UI\" opens a \
                         browser-based chat; \"🔌 API\" shows how to connect an external app \
                         instead. Only one model runs at a time.",
                        );
                        ui.add_space(10.0);

                        ui.label(egui::RichText::new("Disclaimer").strong());
                        ui.label(
                            "Everything runs locally on this machine — your prompts and any files \
                         you feed the model never leave it, except when you explicitly use the \
                         \"Model\" box or \"Look up online\" (which contact the URL you gave or \
                         Hugging Face to fetch/search for a file) or ⚙ Settings' \"Look up \
                         llama.cpp builds\" (which contacts GitHub to list/download \
                         llama-server releases). Model output can be inaccurate, \
                         biased, or simply wrong; treat it as a draft, not a fact, and don't \
                         rely on it for medical, legal, financial, or other safety-critical \
                         decisions. This software is provided \"as is\", without warranty of \
                         any kind — see the license below.",
                        );
                        ui.add_space(10.0);

                        ui.label(egui::RichText::new("License").strong());
                        ui.label("QuickAI's own source code is licensed under the PolyForm Noncommercial License 1.0.0.");
                        ui.add_space(4.0);
                        ui.label(
                            "This package also bundles, unmodified and under their own licenses:",
                        );
                        ui.label(
                            "• llama-server, from llama.cpp — MIT License, © the ggml authors.\n\
                         • Bonsai 8B and Bonsai 1.7B model weights, from PrismML — Apache \
                         License 2.0.",
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Full license texts for all three are in LICENSE and \
                             THIRD_PARTY_LICENSES.md next to this app.",
                            )
                            .small()
                            .color(egui::Color32::GRAY),
                        );
                    });

                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            });
        self.show_about = open && !close_clicked;
    }

    /// Shows a blocking "are you sure?" modal for `self.pending_delete`, if
    /// any — the confirmation step every delete button (model or installed
    /// llama-server build) goes through before anything actually touches
    /// disk. A no-op when nothing is pending.
    fn show_confirm_delete_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_delete else {
            return;
        };
        let (title, body) = match pending {
            PendingDelete::Model { filename, .. } => (
                "Delete model?",
                format!("Delete \"{filename}\" from disk? This can't be undone."),
            ),
            PendingDelete::LlamaBuild { label, .. } => (
                "Delete llama-server build?",
                format!("Delete the installed build \"{label}\" from disk? This can't be undone."),
            ),
        };

        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::new("confirm_delete_modal")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(egui::RichText::new(title).strong());
            ui.add_space(6.0);
            ui.label(body);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
                if ui.button("Delete").clicked() {
                    confirmed = true;
                }
            });
        });
        if modal.should_close() {
            cancelled = true;
        }

        if confirmed {
            match self.pending_delete.take() {
                Some(PendingDelete::Model { filename, path }) => {
                    match std::fs::remove_file(&path) {
                        Ok(()) => self.push_log(format!("Deleted model: {filename}")),
                        Err(e) => self.push_log(format!("Could not delete {filename}: {e}")),
                    }
                    self.rescan_models();
                }
                Some(PendingDelete::LlamaBuild { label, install_dir }) => {
                    let was_active = self
                        .llama_bin_override
                        .as_deref()
                        .is_some_and(|p| p.starts_with(&install_dir));
                    match llama_install::delete_installed(&install_dir) {
                        Ok(()) => self.push_log(format!("Deleted llama-server build: {label}")),
                        Err(e) => self.push_log(format!("Could not delete build {label}: {e}")),
                    }
                    if was_active {
                        self.llama_bin_input.clear();
                        self.llama_server_bin = paths::llama_server_bin();
                        self.llama_bin_override = None;
                        self.missing_binary_warned = false;
                        self.save_settings();
                    }
                }
                None => {}
            }
        } else if cancelled {
            self.pending_delete = None;
        }
    }

    /// Every distinct client (see `TrafficTurn::session`) that has shown up
    /// in the 📡 Live Traffic feed so far, in first-seen order.
    fn session_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for t in &self.traffic {
            if !keys.iter().any(|k| k == &t.session) {
                keys.push(t.session.clone());
            }
        }
        keys
    }

    /// Content of the 📡 Live tab (see the tab strip in `ui()`): a session
    /// list down the left (like an MS Teams chat list — one row per client,
    /// click to switch, or "All" to see everything mixed together) with the
    /// picked session's conversation filling the rest of the tab on the
    /// right.
    fn show_live_traffic_tab(&mut self, ui: &mut egui::Ui) {
        // Owned, not borrowed from `self.traffic` — so nothing here keeps
        // `self` borrowed once we get to the mutable `selected_session`
        // writes below.
        let session_keys = self.session_keys();
        if let Some(sel) = &self.selected_session {
            if !session_keys.contains(sel) {
                self.selected_session = None;
            }
        }

        egui::Panel::left("live_traffic_sessions_panel")
            .resizable(true)
            .default_size(220.0)
            .size_range(160.0..=360.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Conversations").strong())
                    .on_hover_text(
                        "Click a client below to view just its conversation, or \"All\" for \
                         everything mixed together.",
                    );
                ui.add_space(6.0);
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let all_selected = self.selected_session.is_none();
                        let all_label = format!(
                            "💬 All\n{} message{}",
                            self.traffic.len(),
                            if self.traffic.len() == 1 { "" } else { "s" }
                        );
                        if ui.selectable_label(all_selected, all_label).clicked() {
                            self.selected_session = None;
                        }
                        ui.add_space(2.0);

                        for (i, key) in session_keys.iter().enumerate() {
                            let mut count = 0usize;
                            let mut live = false;
                            let mut preview = String::new();
                            for t in self.traffic.iter().filter(|t| &t.session == key) {
                                count += 1;
                                live |= !t.done;
                                preview = t.prompt.clone().unwrap_or_else(|| {
                                    if t.attachments.is_empty() {
                                        t.path.clone()
                                    } else {
                                        t.attachments.join(", ")
                                    }
                                });
                            }
                            let dot = if live { "\u{25cf} " } else { "" };
                            let label = format!(
                                "{dot}Session {}\n{} \u{b7} {count} message{}",
                                i + 1,
                                truncate_preview(&preview, 34),
                                if count == 1 { "" } else { "s" }
                            );
                            let selected = self.selected_session.as_deref() == Some(key.as_str());
                            if ui
                                .selectable_label(selected, label)
                                .on_hover_text(format!("Client at {key}"))
                                .clicked()
                            {
                                self.selected_session = Some(key.clone());
                            }
                            ui.add_space(2.0);
                        }

                        if session_keys.is_empty() {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "No traffic yet — waiting for the first request.",
                                )
                                .color(egui::Color32::GRAY),
                            );
                        }
                    });

                ui.add_space(6.0);
                if ui.button("Clear all").clicked() {
                    self.traffic.clear();
                    self.selected_session = None;
                }
            });

        // Matches the left panel's frame margin (`Frame::side_top_panel`'s
        // `Margin::symmetric(8, 2)`) instead of `CentralPanel`'s wider
        // default top margin, so the two headers line up on the same row
        // rather than the right one sitting a few pixels lower.
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Live chat/completion traffic").strong())
                .on_hover_text(
                "Every chat/completion request hitting this model, from any client, shown as \
                 it happens. Only chat/completion endpoints are shown here — other traffic \
                 (the chat UI's own page assets, health checks, etc.) is relayed to \
                 llama-server as normal but not logged.",
            );
                ui.add_space(6.0);
                ui.separator();

                let selected = self.selected_session.clone();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for turn in self
                            .traffic
                            .iter()
                            .filter(|t| selected.as_deref().is_none_or(|s| t.session == s))
                        {
                            ui.group(|ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} {}",
                                            turn.method, turn.path
                                        ))
                                        .strong()
                                        .monospace(),
                                    );
                                    if !turn.done {
                                        ui.label(
                                            egui::RichText::new("● live")
                                                .color(egui::Color32::from_rgb(46, 139, 50)),
                                        );
                                    }
                                });
                                if let Some(prompt) = &turn.prompt {
                                    ui.label(
                                        egui::RichText::new(format!("↳ {prompt}"))
                                            .color(egui::Color32::GRAY),
                                    );
                                }
                                for attachment in &turn.attachments {
                                    ui.label(
                                        egui::RichText::new(format!("↳ {attachment}"))
                                            .color(egui::Color32::GRAY),
                                    );
                                }
                                if !turn.response.is_empty() {
                                    ui.label(&turn.response);
                                } else if turn.error.is_none() && !turn.done {
                                    ui.label(egui::RichText::new("…").color(egui::Color32::GRAY));
                                }
                                if let Some(err) = &turn.error {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(192, 57, 43),
                                        format!("error: {err}"),
                                    );
                                }
                            });
                            ui.add_space(4.0);
                        }
                    });
            });
    }
}

/// How many rows the "Downloaded models"/"Installed builds" delete lists
/// (⚙ Settings) show before scrolling, so a long download history doesn't
/// push the rest of the window down.
const DELETE_LIST_MAX_ROWS: usize = 4;

/// Height for a `ScrollArea` that shows exactly `rows` list rows — each
/// row is a `ui.horizontal` containing an interactive widget, so its
/// height is governed by `interact_size.y` plus the spacing between rows.
fn list_scroll_height(ui: &egui::Ui, rows: usize) -> f32 {
    (ui.spacing().interact_size.y + ui.spacing().item_spacing.y) * rows as f32
}

/// Shortens `s` to at most `max_chars` characters (UTF-8-safe), replacing
/// the empty case with a placeholder so a session's chat-list row never
/// shows a blank preview line.
fn truncate_preview(s: &str, max_chars: usize) -> String {
    if s.is_empty() {
        return "(no prompt)".to_string();
    }
    let mut truncated: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        truncated.push('\u{2026}');
    }
    truncated
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut new_lines = Vec::new();
        let mut new_traffic = Vec::new();
        self.manager
            .poll(&mut |line| new_lines.push(line), &mut |evt| {
                new_traffic.push(evt)
            });
        for line in new_lines {
            self.push_log(line);
        }
        for evt in new_traffic {
            self.apply_traffic_event(evt);
        }
        self.poll_download();
        self.poll_lookup();
        self.poll_llama_lookup();
        self.poll_llama_install();

        let active = self.manager.is_active();
        let state = self.manager.state();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("QuickAI");

                if self.enable_traffic_proxy {
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
                    let theme_icon = if self.dark_mode { "☀" } else { "🌙" };
                    let theme_hover = if self.dark_mode {
                        "Switch to light theme"
                    } else {
                        "Switch to dark theme"
                    };
                    if ui
                        .add(egui::Button::new(theme_icon).frame(false))
                        .on_hover_text(theme_hover)
                        .clicked()
                    {
                        self.dark_mode = !self.dark_mode;
                        ui.ctx().set_visuals(if self.dark_mode {
                            egui::Visuals::dark()
                        } else {
                            egui::Visuals::light()
                        });
                        self.save_settings();
                    }

                    ui.add_space(6.0);

                    let size = self.models.get(self.selected).map(|m| m.size_bytes).unwrap_or(0);
                    let tier = hardware::speed_tier(size, self.total_ram_bytes, self.n_gpu_layers);
                    ui.label(egui::RichText::new(tier.emoji()).size(16.0)).on_hover_text(format!(
                        "Estimated speed for the selected model on this machine: {}. \
                         Rough estimate (model size vs. detected RAM, and whether GPU \
                         offload is on) — not a live benchmark.",
                        tier.label()
                    ));

                    ui.add_space(6.0);
                    if ui
                        .add(egui::Button::new("⚙").frame(false))
                        .on_hover_text("App and llama.cpp settings — downloaded-models folder, \
                                        custom llama-server binary, extra command-line arguments")
                        .clicked()
                    {
                        self.show_settings = !self.show_settings;
                    }

                    ui.add_space(6.0);
                    if ui
                        .add(egui::Button::new("ℹ").frame(false))
                        .on_hover_text("About QuickAI: usage, disclaimer, and license")
                        .clicked()
                    {
                        self.show_about = !self.show_about;
                    }
                });
            });
            ui.add_space(6.0);

            if self.enable_traffic_proxy {
                ui.separator();
                ui.add_space(6.0);
            }

            if self.active_tab == Tab::Live {
                self.show_live_traffic_tab(ui);
                return;
            }

            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Model:").on_hover_text(
                        "Which .gguf file llama-server will load on Start. Entries highlighted \
                         green are bundled with this app and always available; others were \
                         downloaded or dropped into a models folder by hand.",
                    );
                    let prev_selected = self.selected;
                    egui::ComboBox::from_id_salt("model_combo")
                        .width(320.0)
                        .selected_text(
                            self.models
                                .get(self.selected)
                                .map(|m| m.display.as_str())
                                .unwrap_or("(no models found)"),
                        )
                        .show_ui(ui, |ui| {
                            for (i, m) in self.models.iter().enumerate() {
                                let label = if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                                    format!("{} (recommended)", m.display)
                                } else {
                                    m.display.clone()
                                };
                                if m.bundled {
                                    let bg = if ui.visuals().dark_mode {
                                        egui::Color32::from_rgb(24, 54, 30)
                                    } else {
                                        egui::Color32::from_rgb(214, 245, 214)
                                    };
                                    egui::Frame::NONE
                                        .fill(bg)
                                        .inner_margin(egui::Margin::symmetric(4, 2))
                                        .show(ui, |ui| {
                                            ui.selectable_value(&mut self.selected, i, label);
                                        });
                                } else {
                                    ui.selectable_value(&mut self.selected, i, label);
                                }
                            }
                        });
                    // A fresh pick of a different model gets its own
                    // context-size default — bigger models/tighter RAM get
                    // a smaller suggestion. Still just a starting point:
                    // the DragValue below stays freely editable afterward.
                    if self.selected != prev_selected {
                        if let Some(m) = self.models.get(self.selected) {
                            self.ctx_size = hardware::recommend_ctx_size(m.size_bytes, self.total_ram_bytes);
                        }
                    }
                    ui.add_enabled_ui(!active, |ui| {
                        if ui
                            .button("Refresh")
                            .on_hover_text(
                                "Re-scan the models folders for new .gguf files — e.g. one you \
                                 just dropped in by hand — without restarting the app.",
                            )
                            .clicked()
                        {
                            self.rescan_models();
                        }
                    });
                });
                if let Some(m) = self.models.get(self.selected) {
                    if !m.note.is_empty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&m.note).color(egui::Color32::GRAY).small());
                    }
                    if self.recommended_filename.as_deref() == Some(m.filename.as_str()) {
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "★ Recommended for this machine (~{} RAM detected{})",
                                hardware::format_bytes(self.total_ram_bytes),
                                if self.n_gpu_layers == 0 { ", CPU only" } else { "" }
                            ))
                            .color(egui::Color32::from_rgb(46, 139, 50))
                            .small(),
                        )
                        .on_hover_text(
                            "A starting point, not a guarantee: based on total RAM and \
                             whether GPU offload is enabled, favoring a smaller/faster \
                             model on CPU-only setups since a big model stays slow \
                             token-by-token no matter how much RAM is free. This can't \
                             see actual VRAM, so a model with GPU offload on can still \
                             run slower than expected if it doesn't fully fit on the GPU.",
                        );
                    }
                } else {
                    ui.colored_label(
                        egui::Color32::from_rgb(192, 57, 43),
                        format!(
                            "No .gguf files found in {} or {}",
                            self.bundled_models_dir.display(),
                            self.models_dir.display()
                        ),
                    );
                }
            });

            ui.add_space(8.0);
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!active && self.download.is_none(), |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Model:").on_hover_text(
                                "One box, three things it can mean: a direct https:// link \
                                 downloads that file, an existing local path imports it, and \
                                 anything else is treated as a Hugging Face search term. Use \
                                 \"...\" to browse for a local file instead of typing a path.",
                            );
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut self.model_input)
                                    .hint_text("URL, local path, or search by name...")
                                    .desired_width(260.0),
                            );
                            if ui
                                .button("...")
                                .on_hover_text("Browse for a local .gguf file")
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("GGUF model", &["gguf"])
                                    .pick_file()
                                {
                                    self.model_input = path.display().to_string();
                                }
                            }
                            let trimmed = self.model_input.trim().to_string();
                            let is_url = downloader::looks_like_url(&trimmed);
                            let is_local_file = !is_url && Path::new(&trimmed).is_file();
                            let (button_label, button_hover) = if trimmed.is_empty() {
                                (
                                    "Search",
                                    "Type a URL, a local file path, or a model name to search \
                                     Hugging Face for.",
                                )
                            } else if is_url {
                                ("Download", "Download that URL into your models folder.")
                            } else if is_local_file {
                                (
                                    "Import",
                                    "Link that local file into your models folder — instant \
                                     and no extra disk space, falling back to a copy only if \
                                     linking isn't possible.",
                                )
                            } else {
                                (
                                    "Search",
                                    "Search Hugging Face by name, then automatically widen the \
                                     search to similar models too — all listed together below.",
                                )
                            };
                            let submitted =
                                field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let go = ui
                                .add_enabled(!trimmed.is_empty(), egui::Button::new(button_label))
                                .on_hover_text(button_hover)
                                .clicked();
                            if !trimmed.is_empty() && (go || submitted) {
                                if is_url || is_local_file {
                                    self.model_input.clear();
                                    let ctx_clone = ui.ctx().clone();
                                    self.download = Some(downloader::spawn_download(
                                        trimmed,
                                        None,
                                        self.models_dir.clone(),
                                        move || ctx_clone.request_repaint(),
                                    ));
                                } else {
                                    self.show_lookup = true;
                                    self.lookup_results.clear();
                                    self.lookup_status =
                                        format!("Searching Hugging Face for \"{trimmed}\"...");
                                    let ctx_clone = ui.ctx().clone();
                                    self.lookup_rx = Some(remote::spawn_keyword_search(
                                        trimmed,
                                        move || ctx_clone.request_repaint(),
                                    ));
                                }
                            }
                            ui.add_space(8.0);
                            if ui
                                .button("Look up online")
                                .on_hover_text(
                                    "Search Hugging Face for currently popular GGUF models you \
                                     can download with one click, without knowing a direct URL \
                                     or name.",
                                )
                                .clicked()
                            {
                                self.show_lookup = true;
                                self.lookup_results.clear();
                                self.lookup_status = "Searching Hugging Face for GGUF models...".into();
                                let ctx_clone = ui.ctx().clone();
                                self.lookup_rx = Some(remote::spawn_lookup(move || ctx_clone.request_repaint()));
                            }
                        });
                    });
                    // Only shown once the lookup section actually has models
                    // on screen to close — hidden entirely while idle or
                    // still searching, rather than shown greyed out with
                    // nothing for it to do.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let has_results = self.show_lookup && !self.lookup_results.is_empty();
                        if has_results
                            && ui
                                .add(egui::Button::new("X").small())
                                .on_hover_text("Close lookup results")
                                .clicked()
                        {
                            self.show_lookup = false;
                        }
                    });
                });

                if let Some(handle) = &self.download {
                    ui.add_space(4.0);
                    let text = match handle.total.filter(|t| *t > 0) {
                        Some(total) => format!(
                            "{} {}: {} / {}",
                            handle.verb,
                            handle.label,
                            hardware::format_bytes(handle.downloaded),
                            hardware::format_bytes(total)
                        ),
                        None => format!(
                            "{} {}: {}",
                            handle.verb,
                            handle.label,
                            hardware::format_bytes(handle.downloaded)
                        ),
                    };
                    ui.colored_label(egui::Color32::from_rgb(212, 160, 23), text);
                    if let Some(total) = handle.total.filter(|t| *t > 0) {
                        ui.add(egui::ProgressBar::new(handle.downloaded as f32 / total as f32));
                    }
                }

                if self.show_lookup {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&self.lookup_status).small().color(egui::Color32::GRAY));
                    if !self.lookup_results.is_empty() {
                        let mut to_download: Option<usize> = None;
                        const GB: u64 = 1024 * 1024 * 1024;
                        // First section is the plain top-10 (already
                        // downloads-sorted by the search). The rest slice
                        // the same pool by file size, so someone on
                        // constrained hardware has a browsable "Small"
                        // list instead of only ever seeing the biggest,
                        // most-downloaded repos.
                        let sections: [(Option<&str>, u64, u64); 4] = [
                            (None, 0, u64::MAX),
                            (Some("High (15 GB+)"), 15 * GB, u64::MAX),
                            (Some("Medium (4-15 GB)"), 4 * GB, 15 * GB),
                            (Some("Small (< 4 GB)"), 0, 4 * GB),
                        ];

                        // A fixed-height scroll area with its own collapsing
                        // header: the result list (up to 4 sections * 10
                        // entries) can get long, so it needs a hard size cap
                        // with scrollbars rather than pushing the rest of
                        // the window down, and a way to tuck it away once
                        // you've found what you wanted without losing the
                        // results if you reopen it.
                        egui::CollapsingHeader::new(format!("Results ({})", self.lookup_results.len()))
                            .default_open(true)
                            .show(ui, |ui| {
                                // `both()` (not just vertical) so a result
                                // line too wide for the panel gets its own
                                // bottom scrollbar instead of being clipped
                                // or forcing the whole panel wider. Default
                                // auto_shrink would instead shrink the box
                                // down to the widest row whenever that's
                                // narrower than the panel — so turn it off
                                // for the horizontal axis: the box always
                                // fills the panel's actual width (adapting
                                // live as the window is resized), and the
                                // horizontal scrollbar only appears once a
                                // row is wider than that. Vertical
                                // auto_shrink stays on, so a short result
                                // list doesn't reserve the full max_height.
                                egui::ScrollArea::both()
                                    .auto_shrink([false, true])
                                    .max_height(280.0)
                                    .show(ui, |ui| {
                                        for (heading, min, max) in sections {
                                            let indices: Vec<usize> = self
                                                .lookup_results
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, m)| {
                                                    heading.is_none()
                                                        || m.size_bytes.map(|s| s >= min && s < max).unwrap_or(false)
                                                })
                                                .map(|(i, _)| i)
                                                .take(10)
                                                .collect();
                                            if indices.is_empty() {
                                                continue;
                                            }
                                            if let Some(h) = heading {
                                                ui.add_space(6.0);
                                                ui.label(egui::RichText::new(h).strong().small());
                                            }
                                            for i in indices {
                                                let m = &self.lookup_results[i];
                                                ui.horizontal(|ui| {
                                                    if ui
                                                        .add_enabled(self.download.is_none(), egui::Button::new("Get"))
                                                        .clicked()
                                                    {
                                                        to_download = Some(i);
                                                    }
                                                    let size =
                                                        m.size_bytes.map(hardware::format_bytes).unwrap_or_else(|| "?".into());
                                                    ui.label(format!(
                                                        "{} — {} ({size}, {} downloads)",
                                                        m.repo_id, m.filename, m.downloads
                                                    ));
                                                });
                                            }
                                        }
                                    });
                            });
                        if let Some(i) = to_download {
                            let m = &self.lookup_results[i];
                            let ctx_clone = ui.ctx().clone();
                            self.download = Some(downloader::spawn_download(
                                m.url.clone(),
                                Some(m.filename.clone()),
                                self.models_dir.clone(),
                                move || ctx_clone.request_repaint(),
                            ));
                        }
                    }
                }
            });

            ui.add_space(8.0);
            ui.group(|ui| {
                ui.add_enabled_ui(!active, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Port:").on_hover_text(
                            "TCP port llama-server listens on, on this machine only \
                             (127.0.0.1) — never exposed to your network. The default 8080 is \
                             fine unless something else on your system is already using it; \
                             any free port from 1-65535 works.",
                        );
                        ui.add(egui::DragValue::new(&mut self.port).range(1..=65535));
                        ui.add_space(12.0);
                        ui.label("Context size:").on_hover_text(
                            "Maximum number of tokens (roughly, word-pieces) of conversation \
                             llama.cpp keeps in memory at once — prompt plus reply combined. \
                             Bigger means it remembers more of a long chat/document, but uses \
                             more RAM/VRAM and can respond slower. Typical values: 2048-4096 for \
                             short chats, 8192-16384 for longer ones, 32768+ only for large \
                             documents on machines with memory to spare.",
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.ctx_size)
                                .range(256..=131072)
                                .speed(64),
                        );
                        ui.add_space(12.0);
                        ui.label("GPU layers:").on_hover_text(
                            "How many of the model's layers to run on the GPU instead of the \
                             CPU. 999 (default) offloads every layer llama.cpp can — fastest, if \
                             the model fits in your GPU's VRAM. 0 = CPU only. If Start crashes or \
                             the log shows an out-of-memory error, the model likely doesn't fit \
                             in VRAM — try a smaller number (e.g. 20-40) to offload only part of \
                             it, or 0 to fall back to CPU entirely.",
                        );
                        ui.add(egui::DragValue::new(&mut self.n_gpu_layers).range(0..=999));
                    });
                });
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!active && !self.models.is_empty(), egui::Button::new("Start"))
                    .on_hover_text("Launch llama-server with the selected model and the settings above.")
                    .clicked()
                {
                    if let Some(model) = self.models.get(self.selected).cloned() {
                        if !self.llama_server_bin.is_file() {
                            self.missing_binary_warned = true;
                        } else {
                            self.save_settings();
                            self.log_lines.clear();
                            self.traffic.clear();
                            self.selected_session = None;
                            let ctx_clone = ui.ctx().clone();
                            self.manager.start(
                                &self.llama_server_bin,
                                &model.path,
                                &model.display,
                                self.port,
                                self.ctx_size,
                                self.n_gpu_layers,
                                &self.extra_args,
                                self.enable_traffic_proxy,
                                move || ctx_clone.request_repaint(),
                            );
                        }
                    }
                }
                if ui
                    .add_enabled(
                        matches!(state, ServerState::Starting | ServerState::Running),
                        egui::Button::new("Stop"),
                    )
                    .on_hover_text("Stop the running llama-server process.")
                    .clicked()
                {
                    self.manager.stop();
                }
                if ui
                    .add_enabled(state == ServerState::Running, egui::Button::new("Open Chat UI"))
                    .on_hover_text("Open llama-server's built-in web chat for the running model in your browser.")
                    .clicked()
                {
                    if let Some(port) = self.manager.port() {
                        let _ = webbrowser::open(&format!("http://127.0.0.1:{port}"));
                    }
                }
                if ui
                    .add_enabled(state == ServerState::Running, egui::Button::new("🔌 API"))
                    .on_hover_text("Show how to connect an external app to this running model")
                    .clicked()
                {
                    self.show_api_info = !self.show_api_info;
                }

                ui.add_space(12.0);
                let label = match (state, self.manager.model_name(), self.manager.port()) {
                    (ServerState::Running, Some(name), Some(port)) => {
                        format!("Running: {name} (port {port})")
                    }
                    _ => state.label().to_string(),
                };
                ui.colored_label(Self::state_color(state), label);
            });

            if self.show_api_info && state == ServerState::Running {
                if let Some(port) = self.manager.port() {
                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Connect an app to this model").strong());
                        let base_url = format!("http://127.0.0.1:{port}/v1");
                        let model = self.manager.model_name().unwrap_or("local-model").to_string();

                        ui.horizontal(|ui| {
                            ui.label("Base URL:");
                            ui.code(&base_url);
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(base_url.clone());
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("API key:");
                            ui.code("none");
                        });
                        ui.label(
                            egui::RichText::new(
                                "llama.cpp doesn't require one by default — leave it blank, or use \
                                 any placeholder value if your client insists on a non-empty string.",
                            )
                            .small()
                            .color(egui::Color32::GRAY),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Model:");
                            ui.code(&model);
                        });
                        ui.label(
                            egui::RichText::new(
                                "Only one model is ever loaded at a time, so most OpenAI-compatible \
                                 clients work with any value here.",
                            )
                            .small()
                            .color(egui::Color32::GRAY),
                        );

                        ui.add_space(4.0);
                        let curl = format!(
                            "curl {base_url}/chat/completions \\\n  -H \"Content-Type: application/json\" \\\n  -d '{{\"model\":\"{model}\",\"messages\":[{{\"role\":\"user\",\"content\":\"Hello\"}}]}}'"
                        );
                        ui.horizontal(|ui| {
                            ui.label("curl example:");
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(curl.clone());
                            }
                        });
                        ui.code(&curl);
                    });
                }
            }

            if self.missing_binary_warned {
                ui.colored_label(
                    egui::Color32::from_rgb(192, 57, 43),
                    format!(
                        "llama-server binary not found at {}",
                        self.llama_server_bin.display()
                    ),
                );
            }

            if state == ServerState::Crashed {
                ui.add_space(6.0);
                ui.group(|ui| {
                    ui.colored_label(
                        Self::state_color(state),
                        "The server exited before it finished starting — see the log below \
                         for the exact reason (a common one: \"unknown model architecture\", \
                         meaning this llama-server build predates support for this model).",
                    );
                    let backend = llama_releases::backend_label(
                        &self.llama_server_bin.display().to_string(),
                    );
                    if let Some(warning) = gpu::mismatch_warning(&self.gpu_vendors, backend) {
                        ui.label(
                            egui::RichText::new(warning)
                                .small()
                                .color(egui::Color32::from_rgb(212, 160, 23)),
                        );
                    }
                    ui.label(
                        egui::RichText::new(
                            "Worth trying a different llama-server build:\n\
                             1. Open ⚙ Settings → Advanced: llama.cpp.\n\
                             2. Click \"Look up llama.cpp builds\" and pick one for your \
                                platform/GPU from the list — it installs and applies \
                                automatically.\n\
                             3. Or, if you already have a llama-server build of your own \
                                (a different backend, a custom compile, etc.), type or browse \
                                to its path in the \"llama-server binary\" field above and \
                                click Apply instead.\n\
                             4. Click Start again.",
                        )
                        .small(),
                    );
                });
            }

            ui.add_space(8.0);
            ui.separator();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.style_mut().override_font_id = Some(egui::FontId::monospace(11.0));
                    for line in &self.log_lines {
                        ui.label(line);
                    }
                });
        });

        self.show_settings_window(ui.ctx());
        self.show_about_window(ui.ctx());
        self.show_confirm_delete_modal(ui.ctx());
    }

    fn on_exit(&mut self) {
        self.manager.stop_blocking();
    }
}
