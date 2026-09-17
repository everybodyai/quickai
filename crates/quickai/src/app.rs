use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use eframe::egui;

use crate::downloader::{self, DownloadEvent, DownloadHandle};
use crate::hardware;
use crate::models::{discover_all, ModelEntry};
use crate::paths;
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
    custom_url: String,
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
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();

        let bundled_models_dir = paths::bundled_models_dir();
        let models_dir_override = settings.models_dir_override.as_deref().map(PathBuf::from);
        let models_dir = models_dir_override.clone().unwrap_or_else(paths::user_models_dir);
        let _ = std::fs::create_dir_all(&models_dir);
        let llama_bin_override = settings.llama_bin_override.as_deref().map(PathBuf::from);
        let llama_server_bin = llama_bin_override.clone().unwrap_or_else(paths::llama_server_bin);
        let models = discover_all(&bundled_models_dir, &models_dir);

        let selected = settings
            .last_model_filename
            .as_deref()
            .and_then(|name| models.iter().position(|m| m.filename == name))
            .unwrap_or(0);

        let total_ram_bytes = hardware::total_ram_bytes();
        let recommended_filename = hardware::recommend(&models, total_ram_bytes);

        cc.egui_ctx.set_visuals(if settings.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        let models_dir_input = models_dir.display().to_string();
        let llama_bin_input = llama_bin_override.as_ref().map(|p| p.display().to_string()).unwrap_or_default();

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
            custom_url: String::new(),
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
        self.recommended_filename = hardware::recommend(&self.models, self.total_ram_bytes);
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
            models_dir_override: self.models_dir_override.as_ref().map(|p| p.display().to_string()),
            llama_bin_override: self.llama_bin_override.as_ref().map(|p| p.display().to_string()),
            extra_args: self.extra_args.clone(),
        }
        .save();
    }

    /// Drains the in-flight download's channel, if any. On completion,
    /// re-scans models_dir (the new file is now sitting there) and selects
    /// it; on failure, just logs — the caller already cleaned up the
    /// `.part` file.
    fn poll_download(&mut self) {
        let Some(mut handle) = self.download.take() else { return };
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
                    .or_else(|| current.and_then(|f| self.models.iter().position(|m| m.filename == f)))
                    .unwrap_or(0);
                self.recommended_filename = hardware::recommend(&self.models, self.total_ram_bytes);
                self.save_settings();
            }
            Some(Err(msg)) => self.push_log(format!("Download failed: {msg}")),
            None => self.download = Some(handle),
        }
    }

    /// Drains the online-lookup channel, if a search is running.
    fn poll_lookup(&mut self) {
        let Some(rx) = self.lookup_rx.take() else { return };
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
                    "Where models fetched via \"Model URL / Path\", \"Import\", or \"Look \
                     up online\" are saved and read back from. The models bundled with the \
                     app itself live in their own read-only folder regardless of this.",
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

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                egui::CollapsingHeader::new("Advanced: llama.cpp")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label("llama-server binary:").on_hover_text(
                            "Override the auto-detected llama-server executable — point this \
                             at a custom build (e.g. a different backend, or a newer version). \
                             Leave blank to use the bundled/auto-detected one.",
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.llama_bin_input)
                                .desired_width(ui.available_width())
                                .hint_text(paths::llama_server_bin().display().to_string()),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Apply").clicked() {
                                let trimmed = self.llama_bin_input.trim();
                                if trimmed.is_empty() {
                                    self.llama_server_bin = paths::llama_server_bin();
                                    self.llama_bin_override = None;
                                    self.missing_binary_warned = false;
                                    self.settings_error = None;
                                } else {
                                    let candidate = PathBuf::from(trimmed);
                                    if candidate.is_file() {
                                        self.llama_server_bin = candidate.clone();
                                        self.llama_bin_override = Some(candidate);
                                        self.missing_binary_warned = false;
                                        self.settings_error = None;
                                    } else {
                                        self.settings_error = Some(format!("No file at {trimmed}"));
                                    }
                                }
                                self.save_settings();
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
                egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
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
                         you feed the model never leave it, except when you explicitly use \
                         \"Model URL / Path\" or \"Look up online\", which contact the URL you \
                         gave or Hugging Face to fetch a file. Model output can be inaccurate, \
                         biased, or simply wrong; treat it as a draft, not a fact, and don't \
                         rely on it for medical, legal, financial, or other safety-critical \
                         decisions. This software is provided \"as is\", without warranty of \
                         any kind — see the license below.",
                    );
                    ui.add_space(10.0);

                    ui.label(egui::RichText::new("License").strong());
                    ui.label("QuickAI's own source code is licensed under the PolyForm Noncommercial License 1.0.0.");
                    ui.add_space(4.0);
                    ui.label("This package also bundles, unmodified and under their own licenses:");
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

}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut new_lines = Vec::new();
        self.manager.poll(&mut |line| new_lines.push(line));
        for line in new_lines {
            self.push_log(line);
        }
        self.poll_download();
        self.poll_lookup();

        let active = self.manager.is_active();
        let state = self.manager.state();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("QuickAI");
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
                                "★ Recommended for this machine (~{} RAM detected)",
                                format_bytes(self.total_ram_bytes)
                            ))
                            .color(egui::Color32::from_rgb(46, 139, 50))
                            .small(),
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
                            ui.label("Model URL / Path:").on_hover_text(
                                "Paste a direct https:// link to a .gguf file to download it, or \
                                 a local filesystem path to one you already have (Linux or \
                                 Windows style) to import it instead — either way it's copied \
                                 into your downloaded-models folder (see ⚙ Settings).",
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.custom_url)
                                    .hint_text("https://.../model.gguf  or  /path/to/model.gguf")
                                    .desired_width(320.0),
                            );
                            let trimmed = self.custom_url.trim();
                            let is_url = downloader::looks_like_url(trimmed);
                            let ready = !trimmed.is_empty()
                                && (is_url || Path::new(trimmed).is_file());
                            let button_label = if is_url || trimmed.is_empty() { "Download" } else { "Import" };
                            let button_hover = if is_url || trimmed.is_empty() {
                                "Download that URL into your models folder."
                            } else {
                                "Import that local file into your models folder — instant if \
                                 it's on the same drive, otherwise copied with a progress bar."
                            };
                            if ui
                                .add_enabled(ready, egui::Button::new(button_label))
                                .on_hover_text(button_hover)
                                .clicked()
                            {
                                let input = self.custom_url.trim().to_string();
                                self.custom_url.clear();
                                let ctx_clone = ui.ctx().clone();
                                self.download = Some(downloader::spawn_download(
                                    input,
                                    None,
                                    self.models_dir.clone(),
                                    move || ctx_clone.request_repaint(),
                                ));
                            }
                            ui.add_space(8.0);
                            if ui
                                .button("Look up online")
                                .on_hover_text(
                                    "Search Hugging Face for currently popular GGUF models you \
                                     can download with one click, without knowing a direct URL.",
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
                    // Only meaningful once the lookup section actually has
                    // models on screen to close — greyed out while idle or
                    // still searching, so it can't be clicked to "close"
                    // nothing.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let has_results = self.show_lookup && !self.lookup_results.is_empty();
                        if ui
                            .add_enabled(has_results, egui::Button::new("X").small())
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
                            format_bytes(handle.downloaded),
                            format_bytes(total)
                        ),
                        None => format!("{} {}: {}", handle.verb, handle.label, format_bytes(handle.downloaded)),
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
                                                        m.size_bytes.map(format_bytes).unwrap_or_else(|| "?".into());
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
                            let ctx_clone = ui.ctx().clone();
                            self.manager.start(
                                &self.llama_server_bin,
                                &model.path,
                                &model.display,
                                self.port,
                                self.ctx_size,
                                self.n_gpu_layers,
                                &self.extra_args,
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
    }

    fn on_exit(&mut self) {
        self.manager.stop_blocking();
    }
}

fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
